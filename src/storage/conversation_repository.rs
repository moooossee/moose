use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    conversations::{
        Conversation, ConversationSummary, ConversationTitleUpdate, GenerationSettings, Message,
        MessageRole, MessageStatus, MessageUpdate, NewConversation, NewGenerationSettings,
        NewMessage, validate_conversation_title, validate_message_content,
    },
    core::utc_now,
    error::{MooseError, Result},
};

#[derive(Clone)]
pub struct ConversationRepository {
    connection: Rc<Connection>,
}

impl ConversationRepository {
    pub fn new(connection: Rc<Connection>) -> Self {
        Self { connection }
    }

    pub fn create(&self, new_conversation: NewConversation) -> Result<Conversation> {
        let conversation = new_conversation.into_conversation()?;

        self.connection.execute(
            "INSERT INTO conversations (
                id, provider_id, model_id, title, created_at, updated_at, archived_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                conversation.id,
                conversation.provider_id,
                conversation.model_id,
                conversation.title,
                conversation.created_at,
                conversation.updated_at,
                conversation.archived_at,
            ],
        )?;

        self.get_required(&conversation.id)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<Conversation>> {
        let limit = i64::try_from(limit)?;
        let mut statement = self.connection.prepare(
            "SELECT id, provider_id, model_id, title, created_at, updated_at, archived_at
             FROM conversations
             WHERE archived_at IS NULL
             ORDER BY updated_at DESC, created_at DESC
             LIMIT ?1",
        )?;
        let conversations = statement
            .query_map(params![limit], conversation_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(conversations)
    }

    pub fn list_recent_summaries(&self, limit: usize) -> Result<Vec<ConversationSummary>> {
        let limit = i64::try_from(limit)?;
        let mut statement = self.connection.prepare(
            "SELECT
                c.id, c.provider_id, c.model_id, c.title, c.created_at, c.updated_at, c.archived_at,
                m.id, m.conversation_id, m.role, m.content, m.status, m.token_count, m.created_at, m.completed_at
             FROM conversations c
             LEFT JOIN messages m ON m.id = (
                SELECT id
                FROM messages
                WHERE conversation_id = c.id
                ORDER BY created_at DESC, id DESC
                LIMIT 1
             )
             WHERE c.archived_at IS NULL
             ORDER BY c.updated_at DESC, c.created_at DESC
             LIMIT ?1",
        )?;
        let summaries = statement
            .query_map(params![limit], conversation_summary_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(summaries)
    }

    pub fn get(&self, id: &str) -> Result<Option<Conversation>> {
        self.connection
            .query_row(
                "SELECT id, provider_id, model_id, title, created_at, updated_at, archived_at
                 FROM conversations
                 WHERE id = ?1",
                params![id],
                conversation_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn update_title(&self, update: ConversationTitleUpdate) -> Result<Conversation> {
        let title = validate_conversation_title(&update.title)?;
        let timestamp = utc_now();
        let changed = self.connection.execute(
            "UPDATE conversations
             SET title = ?1, updated_at = ?2
             WHERE id = ?3",
            params![title, timestamp, update.id],
        )?;

        if changed == 0 {
            return Err(MooseError::ConversationNotFound);
        }

        self.get_required(&update.id)
    }

    pub fn archive(&self, id: &str) -> Result<Conversation> {
        let timestamp = utc_now();
        let changed = self.connection.execute(
            "UPDATE conversations
             SET archived_at = ?1, updated_at = ?1
             WHERE id = ?2",
            params![timestamp, id],
        )?;

        if changed == 0 {
            return Err(MooseError::ConversationNotFound);
        }

        self.get_required(id)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let changed = self
            .connection
            .execute("DELETE FROM conversations WHERE id = ?1", params![id])?;

        if changed == 0 {
            return Err(MooseError::ConversationNotFound);
        }

        Ok(())
    }

    pub fn create_message(&self, new_message: NewMessage) -> Result<Message> {
        let message = new_message.into_message()?;

        self.connection.execute(
            "INSERT INTO messages (
                id, conversation_id, role, content, status, token_count, created_at, completed_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                message.id,
                message.conversation_id,
                message.role.as_str(),
                message.content,
                message.status.as_str(),
                message.token_count,
                message.created_at,
                message.completed_at,
            ],
        )?;

        self.touch_conversation(&message.conversation_id)?;
        self.get_message_required(&message.id)
    }

    pub fn list_messages(&self, conversation_id: &str) -> Result<Vec<Message>> {
        let mut statement = self.connection.prepare(
            "SELECT id, conversation_id, role, content, status, token_count, created_at, completed_at
             FROM messages
             WHERE conversation_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let messages = statement
            .query_map(params![conversation_id], message_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn list_recent_context_messages(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let limit = i64::try_from(limit)?;
        let mut statement = self.connection.prepare(
            "SELECT id, conversation_id, role, content, status, token_count, created_at, completed_at
             FROM (
                 SELECT id, conversation_id, role, content, status, token_count, created_at, completed_at
                 FROM messages
                 WHERE conversation_id = ?1
                   AND role IN ('user', 'assistant')
                   AND status IN ('complete', 'cancelled', 'failed')
                   AND trim(content, char(9) || char(10) || char(13) || ' ') <> ''
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?2
             )
             ORDER BY created_at ASC, id ASC",
        )?;
        let messages = statement
            .query_map(params![conversation_id, limit], message_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn get_message(&self, id: &str) -> Result<Option<Message>> {
        self.connection
            .query_row(
                "SELECT id, conversation_id, role, content, status, token_count, created_at, completed_at
                 FROM messages
                 WHERE id = ?1",
                params![id],
                message_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn update_message(&self, update: MessageUpdate) -> Result<Message> {
        let content = validate_message_content(&update.content, &update.status)?;
        let completed_at = update.status.is_finished().then(utc_now);
        let changed = self.connection.execute(
            "UPDATE messages
             SET content = ?1, status = ?2, token_count = ?3, completed_at = ?4
             WHERE id = ?5",
            params![
                content,
                update.status.as_str(),
                update.token_count,
                completed_at,
                update.id,
            ],
        )?;

        if changed == 0 {
            return Err(MooseError::MessageNotFound);
        }

        let message = self.get_message_required(&update.id)?;
        self.touch_conversation(&message.conversation_id)?;
        Ok(message)
    }

    pub fn create_generation_settings(
        &self,
        new_settings: NewGenerationSettings,
    ) -> Result<GenerationSettings> {
        let settings = new_settings.into_generation_settings()?;

        self.connection.execute(
            "INSERT INTO generation_settings (
                id, conversation_id, profile_id, model, temperature, top_p, top_k, seed, num_ctx, context_messages, system_prompt, created_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                settings.id,
                settings.conversation_id,
                settings.profile_id,
                settings.model,
                settings.temperature,
                settings.top_p,
                settings.top_k,
                settings.seed,
                settings.num_ctx,
                settings.context_messages,
                settings.system_prompt,
                settings.created_at,
            ],
        )?;

        if let Some(conversation_id) = &settings.conversation_id {
            self.touch_conversation(conversation_id)?;
        }

        self.get_generation_settings_required(&settings.id)
    }

    pub fn list_generation_settings(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<GenerationSettings>> {
        let mut statement = self.connection.prepare(
            "SELECT id, conversation_id, profile_id, model, temperature, top_p, top_k, seed, num_ctx, context_messages, system_prompt, created_at
             FROM generation_settings
             WHERE conversation_id = ?1
             ORDER BY created_at DESC, id DESC",
        )?;
        let settings = statement
            .query_map(params![conversation_id], generation_settings_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(settings)
    }

    pub fn latest_generation_settings(
        &self,
        conversation_id: &str,
    ) -> Result<Option<GenerationSettings>> {
        self.connection
            .query_row(
                "SELECT id, conversation_id, profile_id, model, temperature, top_p, top_k, seed, num_ctx, context_messages, system_prompt, created_at
                 FROM generation_settings
                 WHERE conversation_id = ?1
                 ORDER BY created_at DESC, id DESC
                 LIMIT 1",
                params![conversation_id],
                generation_settings_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_required(&self, id: &str) -> Result<Conversation> {
        self.get(id)?.ok_or(MooseError::ConversationNotFound)
    }

    fn get_message_required(&self, id: &str) -> Result<Message> {
        self.get_message(id)?.ok_or(MooseError::MessageNotFound)
    }

    fn get_generation_settings_required(&self, id: &str) -> Result<GenerationSettings> {
        self.connection
            .query_row(
                "SELECT id, conversation_id, profile_id, model, temperature, top_p, top_k, seed, num_ctx, context_messages, system_prompt, created_at
                 FROM generation_settings
                 WHERE id = ?1",
                params![id],
                generation_settings_from_row,
            )
            .optional()?
            .ok_or(MooseError::GenerationSettingsNotFound)
    }

    fn touch_conversation(&self, id: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![utc_now(), id],
        )?;

        if changed == 0 {
            return Err(MooseError::ConversationNotFound);
        }

        Ok(())
    }
}

fn conversation_from_row(row: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        model_id: row.get(2)?,
        title: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        archived_at: row.get(6)?,
    })
}

fn conversation_summary_from_row(row: &Row<'_>) -> rusqlite::Result<ConversationSummary> {
    let conversation = Conversation {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        model_id: row.get(2)?,
        title: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        archived_at: row.get(6)?,
    };
    let message_id: Option<String> = row.get(7)?;
    let message = message_id
        .map(|id| {
            let role: String = row.get(9)?;
            let status: String = row.get(11)?;

            Ok::<Message, rusqlite::Error>(Message {
                id,
                conversation_id: row.get(8)?,
                role: role.parse::<MessageRole>().map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        9,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                content: row.get(10)?,
                status: status.parse::<MessageStatus>().map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        11,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                token_count: row.get(12)?,
                created_at: row.get(13)?,
                completed_at: row.get(14)?,
            })
        })
        .transpose()?;
    Ok(ConversationSummary::from_latest_message(
        conversation,
        message,
    ))
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Message> {
    let role: String = row.get(2)?;
    let status: String = row.get(4)?;

    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: role.parse::<MessageRole>().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        content: row.get(3)?,
        status: status.parse::<MessageStatus>().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        token_count: row.get(5)?,
        created_at: row.get(6)?,
        completed_at: row.get(7)?,
    })
}

fn generation_settings_from_row(row: &Row<'_>) -> rusqlite::Result<GenerationSettings> {
    Ok(GenerationSettings {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        profile_id: row.get(2)?,
        model: row.get(3)?,
        temperature: row.get(4)?,
        top_p: row.get(5)?,
        top_k: row.get(6)?,
        seed: row.get(7)?,
        num_ctx: row.get(8)?,
        context_messages: row.get(9)?,
        system_prompt: row.get(10)?,
        created_at: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::{
        conversations::{MessageStatus, MessageUpdate, NewConversation, NewMessage},
        providers::NewProvider,
        storage::{ConversationRepository, ProviderRepository, open_in_memory_database},
    };

    #[test]
    fn conversation_repository_lists_recent_summaries_with_previews() {
        let connection = Rc::new(open_in_memory_database().unwrap());
        let provider_repository = ProviderRepository::new(Rc::clone(&connection));
        let conversation_repository = ConversationRepository::new(connection);
        let provider = provider_repository
            .create(NewProvider::local_ollama(true))
            .unwrap();
        let conversation = conversation_repository
            .create(NewConversation {
                provider_id: provider.id,
                model_id: None,
                title: "First prompt".to_string(),
            })
            .unwrap();
        conversation_repository
            .create_message(NewMessage::user(&conversation.id, "Hello\nfrom Moose"))
            .unwrap();

        let summaries = conversation_repository.list_recent_summaries(10).unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].conversation.id, conversation.id);
        assert_eq!(summaries[0].preview, "Hello from Moose");
        assert_eq!(
            summaries[0].last_message_status,
            Some(MessageStatus::Complete)
        );
    }

    #[test]
    fn conversation_repository_summary_uses_latest_message_state() {
        let connection = Rc::new(open_in_memory_database().unwrap());
        let provider_repository = ProviderRepository::new(Rc::clone(&connection));
        let conversation_repository = ConversationRepository::new(connection);
        let provider = provider_repository
            .create(NewProvider::local_ollama(true))
            .unwrap();
        let conversation = conversation_repository
            .create(NewConversation {
                provider_id: provider.id,
                model_id: None,
                title: "Generation".to_string(),
            })
            .unwrap();
        conversation_repository
            .create_message(NewMessage::user(&conversation.id, "Write a note"))
            .unwrap();
        let assistant = conversation_repository
            .create_message(NewMessage::assistant_streaming(&conversation.id))
            .unwrap();
        conversation_repository
            .update_message(MessageUpdate::failed(assistant.id, ""))
            .unwrap();

        let summary = conversation_repository
            .list_recent_summaries(10)
            .unwrap()
            .remove(0);

        assert_eq!(summary.preview, "Generation failed");
        assert_eq!(summary.last_message_status, Some(MessageStatus::Failed));
    }
}
