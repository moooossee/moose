use rusqlite::{Connection, OptionalExtension, params};

use super::{ConversationRepository, ProviderRepository};
use crate::{
    error::{MooseError, Result},
    tools::web::WebConfig,
};

fn web_config(connection: &Connection) -> Result<WebConfig> {
    let json: Option<String> = connection
        .query_row(
            "SELECT configuration FROM web_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(json
        .map(|json| serde_json::from_str(&json))
        .transpose()?
        .unwrap_or_default())
}

fn set_web_permission(
    connection: &Connection,
    config: &WebConfig,
    allow_all_chats: bool,
) -> Result<()> {
    if allow_all_chats {
        connection.execute(
            "INSERT INTO web_permissions (id, destination) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET destination = excluded.destination",
            [config.destination()],
        )?;
    } else {
        connection.execute("DELETE FROM web_permissions", [])?;
    }
    Ok(())
}

impl ProviderRepository {
    pub fn web_config(&self) -> Result<WebConfig> {
        web_config(&self.connection)
    }

    pub fn web_permission(&self, config: &WebConfig) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM web_permissions WHERE id = 1 AND destination = ?1)",
            [config.destination()],
            |row| row.get(0),
        )?)
    }

    pub fn save_web_config(&self, config: &WebConfig, allow_all_chats: bool) -> Result<()> {
        config.validate().map_err(MooseError::ProviderRequest)?;
        let transaction = self.connection.unchecked_transaction()?;
        if self.web_config()?.destination() != config.destination()
            || (self.web_permission(config)? && !allow_all_chats)
        {
            transaction.execute("UPDATE conversation_tools SET web_permission = ''", [])?;
        }
        transaction.execute("INSERT INTO web_settings (id, configuration) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET configuration = excluded.configuration", params![serde_json::to_string(config)?])?;
        set_web_permission(&transaction, config, allow_all_chats)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn reset_web_permissions(&self) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "UPDATE conversation_tools SET web = 0, web_permission = ''",
            [],
        )?;
        transaction.execute("DELETE FROM web_permissions", [])?;
        transaction.commit()?;
        Ok(())
    }
}

impl ConversationRepository {
    pub fn web_permission(&self, conversation: &str, config: &WebConfig) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT web_permission = ?2 OR EXISTS(
                    SELECT 1 FROM web_permissions WHERE id = 1 AND destination = ?2
                 ) FROM conversation_tools WHERE conversation_id = ?1 AND web = 1",
                params![conversation, config.destination()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn allow_web(&self, conversation: &str, config: &WebConfig, remember: bool) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        if web_config(&transaction)?.destination() != config.destination() {
            return Err(MooseError::ProviderRequest(
                "Web settings changed. Review Web settings and send your message again.".into(),
            ));
        }
        let updated = transaction.execute(
            "UPDATE conversation_tools SET web_permission = ?2 WHERE conversation_id = ?1 AND web = 1",
            params![conversation, config.destination()],
        )?;
        if updated == 0 {
            return Err(MooseError::ProviderRequest(
                "Web was turned off in this chat. Enable it and send your message again.".into(),
            ));
        }
        if remember {
            set_web_permission(&transaction, config, true)?;
        }
        transaction.commit()?;
        Ok(())
    }
}
