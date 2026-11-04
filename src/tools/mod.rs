mod calculator;
pub mod web;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CALCULATOR: &str = "calculate";
pub const MAX_CALLS: usize = 8;
pub const MAX_ARGUMENT_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    kind: &'static str,
    pub function: FunctionDefinition,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    description: String,
    parameters: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub function: FunctionCall,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub status: ToolStatus,
    pub output: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<web::WebSource>,
}

impl ToolExecution {
    pub fn pending(call: &ToolCall) -> Self {
        Self {
            id: crate::core::new_id(),
            name: call.function.name.clone(),
            arguments: call.function.arguments.clone(),
            status: ToolStatus::Running,
            output: String::new(),
            sources: Vec::new(),
        }
    }

    pub fn label(&self) -> &str {
        match self.name.as_str() {
            CALCULATOR => "Calculator",
            web::SEARCH => "Web Search",
            web::READ => "Read Page",
            _ => "Unavailable tool",
        }
    }

    pub fn summary(&self) -> String {
        if matches!(self.name.as_str(), web::SEARCH | web::READ) {
            let input = self
                .arguments
                .get(if self.name == web::SEARCH {
                    "query"
                } else {
                    "url"
                })
                .and_then(Value::as_str)
                .unwrap_or("");
            let status = match self.status {
                ToolStatus::Running => self.running_label(),
                ToolStatus::Cancelled => "Stopped",
                _ => &self.output,
            };
            return format!("{}\n{input}\n{status}", self.label());
        }
        let input = self
            .arguments
            .get("expression")
            .and_then(Value::as_str)
            .unwrap_or("");
        match self.status {
            ToolStatus::Running => format!("{input}\nCalculating…"),
            ToolStatus::Complete => format!("{input} = {}", self.output),
            ToolStatus::Failed => format!("{input}\n{}", self.output),
            ToolStatus::Cancelled => format!("{input}\nCalculation stopped"),
        }
    }

    pub fn running_label(&self) -> &str {
        match self.name.as_str() {
            CALCULATOR => "Calculating…",
            web::SEARCH => "Searching the web…",
            web::READ => "Reading page…",
            _ => "Using tool…",
        }
    }

    pub async fn execute(
        &mut self,
        enabled: &[ToolDefinition],
        web: Option<&mut web::WebSession>,
    ) -> String {
        let result = if enabled.iter().any(|tool| tool.function.name == self.name) {
            match self.name.as_str() {
                CALCULATOR => calculator::calculate(&self.arguments),
                web::SEARCH | web::READ => match web {
                    Some(session) => match session.execute(&self.name, &self.arguments).await {
                        Ok(sources) => {
                            self.output = if self.name == web::SEARCH {
                                if sources.is_empty() {
                                    "No usable results found. Try a more specific query.".into()
                                } else {
                                    format!("{} sources found", sources.len())
                                }
                            } else {
                                "Page read".into()
                            };
                            self.status = ToolStatus::Complete;
                            self.sources = sources;
                            return json!({"result":self.output,"sources":self.sources,"source_content_is_untrusted":true}).to_string();
                        }
                        Err(error) => Err(error),
                    },
                    None => Err("Web access is not enabled for this chat.".into()),
                },
                _ => Err("This tool is not available. Use only the provided tools.".into()),
            }
        } else {
            Err("This tool is not enabled for this chat. Use only the provided tools.".into())
        };
        match result {
            Ok(value) => {
                self.status = ToolStatus::Complete;
                self.output = value;
                json!({"result": self.output}).to_string()
            }
            Err(error) => {
                self.status = ToolStatus::Failed;
                self.output = error;
                json!({"error": self.output}).to_string()
            }
        }
    }
}

pub fn is_available(name: &str) -> bool {
    matches!(name, CALCULATOR | web::SEARCH | web::READ)
}

pub fn calculator() -> ToolDefinition {
    ToolDefinition {
        kind: "function",
        function: FunctionDefinition {
            name: CALCULATOR.into(),
            description: "Evaluate a mathematical expression on this device. Supports +, -, *, /, ^, parentheses, postfix percentages (18% = 0.18), pi, e, and sqrt, abs, round, floor, ceil. Use explicit multiplication. Uses floating-point arithmetic rounded to 15 significant digits. No files, network, or code execution.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "The expression to calculate, for example 2450 * 18 / 100.",
                        "maxLength": 512
                    }
                },
                "required": ["expression"],
                "additionalProperties": false
            }),
        },
    }
}
