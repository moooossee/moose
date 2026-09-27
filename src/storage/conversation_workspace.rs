use super::ConversationRepository;
use crate::{chat::ThinkingValue, core::utc_now, error::Result};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct MessageDetails {
    pub model: String,
    pub reasoning: String,
    pub reasoning_ms: i64,
    pub elapsed_ms: i64,
}

impl ConversationRepository {
    pub fn save_draft(&self, conversation_id: &str, content: &str) -> Result<()> {
        if content.is_empty() {
            self.connection.execute(
                "DELETE FROM conversation_drafts WHERE conversation_id = ?1",
                [conversation_id],
            )?;
        } else {
            self.connection.execute(
                "INSERT INTO conversation_drafts VALUES (?1, ?2, ?3)
                 ON CONFLICT(conversation_id) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
                params![conversation_id, content, utc_now()],
            )?;
        }
        Ok(())
    }

    pub fn draft(&self, conversation_id: &str) -> Result<String> {
        Ok(self
            .connection
            .query_row(
                "SELECT content FROM conversation_drafts WHERE conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn remember_conversation(&self, conversation_id: &str) -> Result<()> {
        self.connection.execute("INSERT INTO workspace_state VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET conversation_id = excluded.conversation_id", [conversation_id])?;
        Ok(())
    }

    pub fn last_conversation(&self) -> Result<Option<String>> {
        Ok(self.connection.query_row("SELECT c.id FROM workspace_state w JOIN conversations c ON c.id = w.conversation_id WHERE c.archived_at IS NULL AND w.id = 1", [], |row| row.get(0)).optional()?)
    }

    pub fn thinking_choice(
        &self,
        conversation_id: &str,
        model: &str,
    ) -> Result<Option<ThinkingValue>> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM conversation_thinking WHERE conversation_id = ?1 AND model = ?2",
                params![conversation_id, model],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value
            .map(|value| serde_json::from_str(&value))
            .transpose()?)
    }

    pub fn save_thinking_choice(
        &self,
        conversation_id: &str,
        model: &str,
        value: Option<&ThinkingValue>,
    ) -> Result<()> {
        if let Some(value) = value {
            self.connection.execute("INSERT INTO conversation_thinking VALUES (?1, ?2, ?3) ON CONFLICT(conversation_id, model) DO UPDATE SET value = excluded.value", params![conversation_id, model, serde_json::to_string(value)?])?;
        } else {
            self.connection.execute(
                "DELETE FROM conversation_thinking WHERE conversation_id = ?1 AND model = ?2",
                params![conversation_id, model],
            )?;
        }
        Ok(())
    }

    pub fn message_details(&self, message_id: &str) -> Result<Option<MessageDetails>> {
        Ok(self.connection.query_row("SELECT model, reasoning, reasoning_ms, elapsed_ms FROM message_details WHERE message_id = ?1", [message_id], |row| Ok(MessageDetails {model: row.get(0)?, reasoning: row.get(1)?, reasoning_ms: row.get(2)?, elapsed_ms: row.get(3)?})).optional()?)
    }

    pub fn save_response(
        &self,
        message_id: &str,
        content: &str,
        status: &str,
        details: &MessageDetails,
    ) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("UPDATE messages SET content = ?2, status = ?3, completed_at = CASE WHEN ?3 = 'streaming' THEN NULL ELSE ?4 END WHERE id = ?1", params![message_id, content, status, utc_now()])?;
        transaction.execute("INSERT INTO message_details VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(message_id) DO UPDATE SET model = excluded.model, reasoning = excluded.reasoning, reasoning_ms = excluded.reasoning_ms, elapsed_ms = excluded.elapsed_ms", params![message_id, details.model, details.reasoning, details.reasoning_ms, details.elapsed_ms])?;
        transaction.execute("UPDATE conversations SET updated_at = ?2 WHERE id = (SELECT conversation_id FROM messages WHERE id = ?1)", params![message_id, utc_now()])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recover_interrupted_responses(&self) -> Result<()> {
        self.connection.execute("UPDATE messages SET status = 'cancelled', completed_at = ?1 WHERE status = 'streaming'", [utc_now()])?;
        Ok(())
    }
}
