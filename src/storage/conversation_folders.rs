use rusqlite::{OptionalExtension, params};

use super::ConversationRepository;
use crate::{
    conversations::{Conversation, ConversationFolder, NewConversation, validate_folder_name},
    core::{new_id, utc_now},
    error::{MooseError, Result},
};

impl ConversationRepository {
    pub fn create_in_folder(
        &self,
        conversation: NewConversation,
        folder_id: Option<&str>,
    ) -> Result<Conversation> {
        let transaction = self.connection.unchecked_transaction()?;
        let conversation = self.create(conversation)?;
        self.move_to_folder(&conversation.id, folder_id)?;
        let conversation = self.get_required(&conversation.id)?;
        transaction.commit()?;
        Ok(conversation)
    }

    pub fn list_folders(&self, include_archived: bool) -> Result<Vec<ConversationFolder>> {
        let mut statement = self.connection.prepare(
            "SELECT f.id, f.name, COUNT(c.id)
             FROM conversation_folders f
             LEFT JOIN conversations c ON c.folder_id = f.id
                 AND (?1 OR c.archived_at IS NULL)
             GROUP BY f.id
             ORDER BY f.name_key, f.id",
        )?;
        Ok(statement
            .query_map([include_archived], |row| {
                Ok(ConversationFolder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    conversation_count: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_folder(&self, name: &str, conversation_id: Option<&str>) -> Result<String> {
        let name = validate_folder_name(name)?;
        let transaction = self.connection.unchecked_transaction()?;
        self.require_available_folder_name(name, None)?;
        let id = new_id();
        transaction.execute(
            "INSERT INTO conversation_folders (id, name, name_key, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, name, name.to_lowercase(), utc_now()],
        )?;
        if let Some(conversation_id) = conversation_id {
            self.move_to_folder(conversation_id, Some(&id))?;
        }
        transaction.commit()?;
        Ok(id)
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<()> {
        let name = validate_folder_name(name)?;
        let transaction = self.connection.unchecked_transaction()?;
        self.require_folder(id)?;
        self.require_available_folder_name(name, Some(id))?;
        transaction.execute(
            "UPDATE conversation_folders SET name = ?2, name_key = ?3 WHERE id = ?1",
            params![id, name, name.to_lowercase()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn delete_folder(&self, id: &str) -> Result<()> {
        let changed = self
            .connection
            .execute("DELETE FROM conversation_folders WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(MooseError::FolderNotFound);
        }
        Ok(())
    }

    pub fn move_to_folder(&self, conversation_id: &str, folder_id: Option<&str>) -> Result<()> {
        if let Some(id) = folder_id {
            self.require_folder(id)?;
        }
        let changed = self.connection.execute(
            "UPDATE conversations SET folder_id = ?2 WHERE id = ?1",
            params![conversation_id, folder_id],
        )?;
        if changed == 0 {
            return Err(MooseError::ConversationNotFound);
        }
        Ok(())
    }

    fn require_folder(&self, id: &str) -> Result<()> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_folders WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(MooseError::FolderNotFound);
        }
        Ok(())
    }

    fn require_available_folder_name(&self, name: &str, except_id: Option<&str>) -> Result<()> {
        let existing: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM conversation_folders WHERE name_key = ?1",
                [name.to_lowercase()],
                |row| row.get(0),
            )
            .optional()?;
        if existing.as_deref().is_some_and(|id| Some(id) != except_id) {
            return Err(MooseError::FolderNameAlreadyExists);
        }
        Ok(())
    }
}
