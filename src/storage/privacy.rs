use super::{ConversationRepository, ProviderRepository};
use crate::{
    error::Result,
    providers::{
        Provider,
        policy::{RemotePermissions, destination},
    },
};
use rusqlite::{OptionalExtension, params};

impl ProviderRepository {
    #[cfg(feature = "gui")]
    pub(crate) fn insert_remote(&self, provider: Provider) -> Result<Provider> {
        let transaction = self.connection.unchecked_transaction()?;
        let provider = self.insert(provider)?;
        self.set_local_only(false)?;
        transaction.commit()?;
        Ok(provider)
    }
    pub fn local_only(&self) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT local_only FROM privacy_settings WHERE id = 1",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn set_local_only(&self, enabled: bool) -> Result<()> {
        self.connection.execute(
            "UPDATE privacy_settings SET local_only = ?1 WHERE id = 1",
            [enabled],
        )?;
        Ok(())
    }
    pub fn has_conversations(&self, id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE provider_id = ?1)",
            [id],
            |r| r.get(0),
        )?)
    }
}

impl ConversationRepository {
    pub fn remote_permissions(
        &self,
        conversation: &str,
        provider: &Provider,
    ) -> Result<RemotePermissions> {
        Ok(self.connection.query_row(
            "SELECT allow_messages, allow_files FROM conversation_remote_permissions WHERE conversation_id = ?1 AND destination = ?2",
            params![conversation, destination(provider)],
            |r| Ok(RemotePermissions { messages: r.get(0)?, files: r.get(1)? }),
        ).optional()?.unwrap_or_default())
    }
    pub fn set_remote_permissions(
        &self,
        conversation: &str,
        provider: &Provider,
        permissions: RemotePermissions,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO conversation_remote_permissions (conversation_id, destination, allow_messages, allow_files) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(conversation_id) DO UPDATE SET destination = excluded.destination, allow_messages = excluded.allow_messages, allow_files = excluded.allow_files",
            params![conversation, destination(provider), permissions.messages, permissions.files],
        )?;
        Ok(())
    }
}
