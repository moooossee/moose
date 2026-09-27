use super::*;

impl ConversationRepository {
    pub fn create_exchange(
        &self,
        conversation_id: &str,
        content: &str,
    ) -> Result<(Message, Message)> {
        self.submit_with_sources(
            conversation_id,
            &crate::chat::ChatSubmission::New(content.into()),
            content,
            &[],
        )
    }

    pub fn edit_message(&self, id: &str, content: &str) -> Result<(Message, Message)> {
        let message = self.get_message_required(id)?;
        self.submit_with_sources(
            &message.conversation_id,
            &crate::chat::ChatSubmission::Edit {
                id: id.into(),
                content: content.into(),
            },
            content,
            &[],
        )
    }

    pub fn regenerate_message(&self, id: &str) -> Result<(Message, Message)> {
        let message = self.get_message_required(id)?;
        self.submit_with_sources(
            &message.conversation_id,
            &crate::chat::ChatSubmission::Regenerate(id.into()),
            "",
            &[],
        )
    }

    pub(super) fn create_exchange_inner(
        &self,
        conversation_id: &str,
        content: &str,
    ) -> Result<(Message, Message)> {
        let parent_id = self.active_leaf(conversation_id)?;
        let user = self.insert_message(
            NewMessage::user(conversation_id, content).into_message()?,
            parent_id.as_deref(),
        )?;
        let assistant = self.insert_message(
            NewMessage::assistant_streaming(conversation_id).into_message()?,
            Some(&user.id),
        )?;
        Ok((user, assistant))
    }

    pub(super) fn edit_message_inner(&self, id: &str, content: &str) -> Result<(Message, Message)> {
        let original = self.require_active_message(id)?;
        if original.role != MessageRole::User {
            return Err(MooseError::InvalidMessageRole);
        }
        let user = NewMessage::user(&original.conversation_id, content).into_message()?;
        let parent_id = self.parent_id(id)?;
        self.deselect_siblings(&original.conversation_id, parent_id.as_deref())?;
        let user = self.insert_message(user, parent_id.as_deref())?;
        let assistant = self.insert_message(
            NewMessage::assistant_streaming(&original.conversation_id).into_message()?,
            Some(&user.id),
        )?;
        Ok((user, assistant))
    }

    pub(super) fn regenerate_message_inner(&self, id: &str) -> Result<(Message, Message)> {
        let original = self.require_active_message(id)?;
        if original.role != MessageRole::Assistant || !original.status.is_finished() {
            return Err(MooseError::InvalidMessageRole);
        }
        let parent_id = self.parent_id(id)?.ok_or(MooseError::MessageNotFound)?;
        let user = self.get_message_required(&parent_id)?;
        if user.role != MessageRole::User {
            return Err(MooseError::InvalidMessageRole);
        }
        self.deselect_siblings(&original.conversation_id, Some(&user.id))?;
        let assistant = self.insert_message(
            NewMessage::assistant_streaming(&original.conversation_id).into_message()?,
            Some(&user.id),
        )?;
        Ok((user, assistant))
    }

    pub fn select_message_version(&self, id: &str) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let message = self.get_message_required(id)?;
        let parent_id = self.parent_id(id)?;
        if let Some(parent_id) = &parent_id {
            self.require_active_message(parent_id)?;
        }
        self.deselect_siblings(&message.conversation_id, parent_id.as_deref())?;
        self.connection
            .execute("UPDATE messages SET is_selected = 1 WHERE id = ?1", [id])?;
        self.touch_conversation(&message.conversation_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn message_versions(&self, conversation_id: &str) -> Result<Vec<Vec<String>>> {
        let mut statement = self.connection.prepare(
            "SELECT id, parent_id FROM messages WHERE conversation_id = ?1 ORDER BY created_at, id",
        )?;
        let mut groups = std::collections::HashMap::<Option<String>, Vec<String>>::new();
        for row in statement.query_map([conversation_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })? {
            let (id, parent_id) = row?;
            groups.entry(parent_id).or_default().push(id);
        }
        Ok(groups.into_values().filter(|ids| ids.len() > 1).collect())
    }

    pub fn fork_at_message(&self, id: &str) -> Result<Conversation> {
        let transaction = self.connection.unchecked_transaction()?;
        let message = self.require_active_message(id)?;
        let original = self.get_required(&message.conversation_id)?;
        let title = original.title.chars().take(32).collect::<String>();
        let conversation = self.create(NewConversation {
            provider_id: original.provider_id,
            model_id: original.model_id,
            title: format!("{title} (branch)"),
        })?;
        let mut parent_id = None;
        for mut source in self.list_messages(&message.conversation_id)? {
            let is_last = source.id == id;
            let original_id = source.id.clone();
            source.id = crate::core::new_id();
            source.conversation_id = conversation.id.clone();
            let copied = self.insert_message(source, parent_id.as_deref())?;
            self.connection.execute("INSERT INTO message_details SELECT ?1, model, reasoning, reasoning_ms, elapsed_ms FROM message_details WHERE message_id = ?2", params![copied.id, original_id])?;
            self.connection.execute("INSERT INTO message_assets SELECT ?1, asset_id, position FROM message_assets WHERE message_id = ?2", params![copied.id, original_id])?;
            self.connection.execute("INSERT INTO response_sources SELECT ?1, number, asset_id, name, page, content FROM response_sources WHERE message_id = ?2", params![copied.id, original_id])?;
            parent_id = Some(copied.id);
            if is_last {
                break;
            }
        }
        self.connection.execute("INSERT INTO conversation_thinking SELECT ?1, model, value FROM conversation_thinking WHERE conversation_id = ?2", params![conversation.id, message.conversation_id])?;
        self.connection.execute("INSERT INTO conversation_library SELECT ?1, enabled FROM conversation_library WHERE conversation_id = ?2", params![conversation.id, message.conversation_id])?;
        if let Some(settings) = self.latest_generation_settings(&message.conversation_id)? {
            self.create_generation_settings(NewGenerationSettings {
                conversation_id: Some(conversation.id.clone()),
                profile_id: settings.profile_id,
                model: settings.model,
                temperature: settings.temperature,
                top_p: settings.top_p,
                top_k: settings.top_k,
                seed: settings.seed,
                num_ctx: settings.num_ctx,
                context_messages: settings.context_messages,
                system_prompt: settings.system_prompt,
            })?;
        }
        transaction.commit()?;
        Ok(conversation)
    }

    fn require_active_message(&self, id: &str) -> Result<Message> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM active_messages WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(MooseError::MessageNotFound);
        }
        self.get_message_required(id)
    }

    pub(super) fn active_leaf(&self, conversation_id: &str) -> Result<Option<String>> {
        self.connection.query_row(
            "SELECT id FROM active_messages WHERE conversation_id = ?1 ORDER BY depth DESC LIMIT 1",
            [conversation_id], |row| row.get(0),
        ).optional().map_err(Into::into)
    }

    fn parent_id(&self, id: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT parent_id FROM messages WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    fn deselect_siblings(&self, conversation_id: &str, parent_id: Option<&str>) -> Result<()> {
        self.connection.execute(
            "UPDATE messages SET is_selected = 0 WHERE conversation_id = ?1 AND parent_id IS ?2",
            params![conversation_id, parent_id],
        )?;
        Ok(())
    }

    pub(super) fn insert_message(
        &self,
        message: Message,
        parent_id: Option<&str>,
    ) -> Result<Message> {
        if let Some(parent_id) = parent_id {
            let parent = self.get_message_required(parent_id)?;
            if parent.conversation_id != message.conversation_id {
                return Err(MooseError::MessageNotFound);
            }
        }
        self.connection.execute(
            "INSERT INTO messages (
                id, conversation_id, role, content, status, token_count, created_at, completed_at, parent_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![message.id, message.conversation_id, message.role.as_str(), message.content,
                message.status.as_str(), message.token_count, message.created_at, message.completed_at, parent_id],
        )?;
        self.touch_conversation(&message.conversation_id)?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chat::{ChatSubmission, build_conversation_context},
        providers::NewProvider,
        storage::{ProviderRepository, open_in_memory_database},
    };

    fn setup() -> (ConversationRepository, String) {
        let connection = Rc::new(open_in_memory_database().unwrap());
        let provider = ProviderRepository::new(Rc::clone(&connection))
            .create(NewProvider::local_ollama(true))
            .unwrap();
        let repository = ConversationRepository::new(connection);
        let conversation = repository
            .create(NewConversation {
                provider_id: provider.id,
                model_id: None,
                title: "Version test".into(),
            })
            .unwrap();
        (repository, conversation.id)
    }

    fn exchange(
        repository: &ConversationRepository,
        conversation_id: &str,
        prompt: &str,
        answer: &str,
    ) -> (Message, Message) {
        let (user, assistant) = repository.create_exchange(conversation_id, prompt).unwrap();
        let assistant = repository
            .update_message(MessageUpdate::completed(assistant.id, answer))
            .unwrap();
        (user, assistant)
    }

    fn contents(repository: &ConversationRepository, id: &str) -> Vec<String> {
        repository
            .list_messages(id)
            .unwrap()
            .into_iter()
            .map(|message| message.content)
            .collect()
    }

    #[test]
    fn editing_earlier_messages_preserves_both_continuations() {
        let (repository, id) = setup();
        let (original, _) = exchange(&repository, &id, "Original", "Original answer");
        exchange(&repository, &id, "Follow-up", "Later answer");
        let (edited, assistant) = repository.edit_message(&original.id, "Edited").unwrap();
        repository
            .update_message(MessageUpdate::completed(assistant.id, "Edited answer"))
            .unwrap();
        exchange(&repository, &id, "New follow-up", "New later answer");
        assert_eq!(
            contents(&repository, &id),
            [
                "Edited",
                "Edited answer",
                "New follow-up",
                "New later answer"
            ]
        );
        repository.select_message_version(&original.id).unwrap();
        assert_eq!(
            contents(&repository, &id),
            ["Original", "Original answer", "Follow-up", "Later answer"]
        );
        repository.select_message_version(&edited.id).unwrap();
        assert_eq!(
            contents(&repository, &id),
            [
                "Edited",
                "Edited answer",
                "New follow-up",
                "New later answer"
            ]
        );
        assert_eq!(
            repository.message_versions(&id).unwrap(),
            vec![vec![original.id, edited.id]]
        );
    }

    #[test]
    fn regeneration_reuses_the_prompt_and_restores_old_follow_ups() {
        let (repository, id) = setup();
        let (user, original) = exchange(&repository, &id, "Question", "First answer");
        exchange(&repository, &id, "Follow-up", "Later answer");
        let (reused_user, replacement) = repository.regenerate_message(&original.id).unwrap();
        assert_eq!(user.id, reused_user.id);
        repository
            .update_message(MessageUpdate::completed(&replacement.id, "Alternative"))
            .unwrap();
        assert_eq!(contents(&repository, &id), ["Question", "Alternative"]);
        repository.select_message_version(&original.id).unwrap();
        assert_eq!(
            contents(&repository, &id),
            ["Question", "First answer", "Follow-up", "Later answer"]
        );
        repository.select_message_version(&replacement.id).unwrap();
        assert_eq!(contents(&repository, &id), ["Question", "Alternative"]);
    }

    #[test]
    fn retry_preserves_failed_and_cancelled_attempts() {
        for status in [MessageStatus::Failed, MessageStatus::Cancelled] {
            let (repository, id) = setup();
            let (_, original) = repository.create_exchange(&id, "Question").unwrap();
            repository
                .update_message(MessageUpdate {
                    id: original.id.clone(),
                    content: "Partial".into(),
                    status: status.clone(),
                    token_count: None,
                })
                .unwrap();
            let (_, retry) = repository.regenerate_message(&original.id).unwrap();
            assert_ne!(retry.id, original.id);
            assert_eq!(
                repository
                    .get_message(&original.id)
                    .unwrap()
                    .unwrap()
                    .status,
                status
            );
            assert_eq!(contents(&repository, &id), ["Question", ""]);
        }
    }

    #[test]
    fn context_search_and_export_history_follow_the_selected_version() {
        let (repository, id) = setup();
        let (original, _) = exchange(&repository, &id, "obsoleteword", "oldreply");
        let (_, assistant) = repository
            .edit_message(&original.id, "replacementword")
            .unwrap();
        repository
            .update_message(MessageUpdate::completed(assistant.id, "newreply"))
            .unwrap();
        let context = repository.list_recent_context_messages(&id, 20).unwrap();
        assert_eq!(
            context
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            ["replacementword", "newreply"]
        );
        assert!(
            repository
                .search_summaries("obsoleteword", false, 60)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            repository
                .search_summaries("replacementword", false, 60)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            repository.list_recent_summaries(60).unwrap()[0].preview,
            "newreply"
        );
        repository.select_message_version(&original.id).unwrap();
        assert_eq!(
            repository.list_recent_summaries(60).unwrap()[0].preview,
            "oldreply"
        );
        assert_eq!(contents(&repository, &id), ["obsoleteword", "oldreply"]);
    }

    #[test]
    fn generation_context_excludes_replaced_messages_and_all_future_messages() {
        let (repository, id) = setup();
        exchange(&repository, &id, "First", "First answer");
        let (user, assistant) = exchange(&repository, &id, "Second", "Second answer");
        exchange(&repository, &id, "Future", "Future answer");
        let history = repository.list_messages(&id).unwrap();
        for (submission, expected) in [
            (ChatSubmission::Regenerate(assistant.id), "Second"),
            (
                ChatSubmission::Edit {
                    id: user.id,
                    content: "Changed".into(),
                },
                "Changed",
            ),
        ] {
            let (prompt, previous) = submission.prepare(&history).unwrap();
            let context = build_conversation_context(&previous, &prompt, 20, Some("Instructions"));
            assert_eq!(
                context
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>(),
                ["Instructions", "First", "First answer", expected]
            );
            assert_eq!(
                build_conversation_context(&previous, &prompt, 1, None).len(),
                1
            );
        }
    }

    #[test]
    fn invalid_edits_leave_the_original_branch_selected() {
        let (repository, id) = setup();
        let (user, assistant) = exchange(&repository, &id, "Original", "Answer");
        assert!(repository.edit_message(&user.id, " ").is_err());
        assert!(
            repository
                .edit_message(&assistant.id, "Invalid role")
                .is_err()
        );
        assert!(repository.regenerate_message(&user.id).is_err());
        assert!(repository.select_message_version("missing").is_err());
        assert_eq!(contents(&repository, &id), ["Original", "Answer"]);
        assert!(repository.message_versions(&id).unwrap().is_empty());
    }

    #[test]
    fn failed_exchange_rolls_back_the_user_message() {
        let (repository, id) = setup();
        repository.connection.execute_batch("CREATE TRIGGER reject_assistant BEFORE INSERT ON messages WHEN NEW.role = 'assistant' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(repository.create_exchange(&id, "Question").is_err());
        assert!(repository.list_messages(&id).unwrap().is_empty());
    }

    #[test]
    fn failed_regeneration_does_not_hide_the_original_answer() {
        let (repository, id) = setup();
        let (_, assistant) = exchange(&repository, &id, "Question", "Answer");
        repository.connection.execute_batch("CREATE TRIGGER reject_assistant BEFORE INSERT ON messages WHEN NEW.role = 'assistant' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(repository.regenerate_message(&assistant.id).is_err());
        assert_eq!(contents(&repository, &id), ["Question", "Answer"]);
    }

    #[test]
    fn hidden_descendants_cannot_change_the_active_conversation() {
        let (repository, id) = setup();
        let (first, _) = exchange(&repository, &id, "First", "Answer");
        let (later, later_answer) = exchange(&repository, &id, "Later", "Later answer");
        repository.edit_message(&first.id, "Changed").unwrap();
        assert!(repository.select_message_version(&later.id).is_err());
        assert!(repository.regenerate_message(&later_answer.id).is_err());
        assert!(repository.fork_at_message(&later.id).is_err());
    }

    #[test]
    fn fork_copies_only_the_selected_prefix_and_settings() {
        let (repository, id) = setup();
        let (user, _) = exchange(&repository, &id, "Original", "Original answer");
        let (_, assistant) = repository.edit_message(&user.id, "Edited").unwrap();
        repository
            .update_message(MessageUpdate::completed(&assistant.id, "Edited answer"))
            .unwrap();
        exchange(&repository, &id, "Later", "Later answer");
        repository
            .create_generation_settings(NewGenerationSettings {
                conversation_id: Some(id.clone()),
                profile_id: Some("builtin-code".into()),
                model: Some("test:latest".into()),
                temperature: Some(0.2),
                top_p: Some(0.8),
                top_k: Some(10),
                seed: Some(42),
                num_ctx: Some(4096),
                context_messages: Some(10),
                system_prompt: Some("Instructions".into()),
            })
            .unwrap();
        let fork = repository.fork_at_message(&assistant.id).unwrap();
        assert_eq!(contents(&repository, &fork.id), ["Edited", "Edited answer"]);
        assert_eq!(
            contents(&repository, &id),
            ["Edited", "Edited answer", "Later", "Later answer"]
        );
        let settings = repository
            .latest_generation_settings(&fork.id)
            .unwrap()
            .unwrap();
        assert_eq!(settings.model.as_deref(), Some("test:latest"));
        assert_eq!(settings.system_prompt.as_deref(), Some("Instructions"));
        assert_eq!(settings.seed, Some(42));
        assert_eq!(settings.profile_id.as_deref(), Some("builtin-code"));
        assert!(repository.message_versions(&fork.id).unwrap().is_empty());
        repository.delete(&id).unwrap();
        assert_eq!(contents(&repository, &fork.id), ["Edited", "Edited answer"]);
    }

    #[test]
    fn selection_survives_reopening_the_repository() {
        let (repository, id) = setup();
        let (_, original) = exchange(&repository, &id, "Question", "Original");
        let (_, alternative) = repository.regenerate_message(&original.id).unwrap();
        repository
            .update_message(MessageUpdate::completed(&alternative.id, "Alternative"))
            .unwrap();
        let reopened = ConversationRepository::new(Rc::clone(&repository.connection));
        assert_eq!(contents(&reopened, &id), ["Question", "Alternative"]);
        reopened.select_message_version(&original.id).unwrap();
        assert_eq!(contents(&repository, &id), ["Question", "Original"]);
    }
}
