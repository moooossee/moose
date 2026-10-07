use super::{ConversationRepository, ProviderRepository};
use crate::{
    error::{MooseError, Result},
    providers::{
        Provider,
        policy::{RemotePermissions, destination},
    },
};
use rusqlite::{Connection, OptionalExtension, params};

fn provider_permissions(connection: &Connection, provider: &Provider) -> Result<RemotePermissions> {
    Ok(connection.query_row(
        "SELECT allow_messages, allow_files FROM provider_remote_permissions WHERE provider_id = ?1 AND destination = ?2",
        params![provider.id, destination(provider)],
        |r| Ok(RemotePermissions { messages: r.get(0)?, files: r.get(1)? }),
    ).optional()?.unwrap_or_default())
}

fn save_provider_permissions(
    connection: &Connection,
    provider: &Provider,
    permissions: RemotePermissions,
) -> Result<()> {
    let updated = connection.execute(
        "INSERT INTO provider_remote_permissions (provider_id, destination, allow_messages, allow_files)
         SELECT id, ?1, ?2, ?3 FROM providers WHERE id = ?4 AND kind = ?5 AND base_url = ?6 AND is_managed = 0
         ON CONFLICT(provider_id) DO UPDATE SET destination = excluded.destination, allow_messages = excluded.allow_messages, allow_files = excluded.allow_files",
        params![destination(provider), permissions.messages, permissions.files, provider.id, provider.kind.as_str(), provider.base_url],
    )?;
    if updated == 0 {
        return Err(MooseError::ProviderRequest(
            "The provider's destination changed or is unavailable. Reopen Privacy settings to review it.".into(),
        ));
    }
    Ok(())
}

fn conversation_permissions(
    connection: &Connection,
    conversation: &str,
    provider: &Provider,
) -> Result<RemotePermissions> {
    Ok(connection.query_row(
        "SELECT allow_messages, allow_files FROM conversation_remote_permissions WHERE conversation_id = ?1 AND destination = ?2",
        params![conversation, destination(provider)],
        |r| Ok(RemotePermissions { messages: r.get(0)?, files: r.get(1)? }),
    ).optional()?.unwrap_or_default())
}

impl ProviderRepository {
    #[cfg(feature = "gui")]
    pub(crate) fn insert_remote(
        &self,
        provider: Provider,
        permissions: RemotePermissions,
    ) -> Result<Provider> {
        let transaction = self.connection.unchecked_transaction()?;
        let provider = self.insert(provider)?;
        self.set_remote_permissions(&provider, permissions)?;
        self.set_local_only(false)?;
        transaction.commit()?;
        Ok(provider)
    }

    pub fn remote_permissions(&self, provider: &Provider) -> Result<RemotePermissions> {
        provider_permissions(&self.connection, provider)
    }

    pub fn set_remote_permissions(
        &self,
        provider: &Provider,
        permissions: RemotePermissions,
    ) -> Result<()> {
        save_provider_permissions(&self.connection, provider, permissions)
    }

    pub fn reset_remote_permissions(&self, provider_id: &str) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        self.clear_remote_permissions(provider_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn clear_remote_permissions(&self, provider_id: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM provider_remote_permissions WHERE provider_id = ?1",
            [provider_id],
        )?;
        self.connection.execute(
            "DELETE FROM conversation_remote_permissions WHERE conversation_id IN (SELECT id FROM conversations WHERE provider_id = ?1)",
            [provider_id],
        )?;
        Ok(())
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
        let shared = provider_permissions(&self.connection, provider)?;
        let chat = conversation_permissions(&self.connection, conversation, provider)?;
        Ok(RemotePermissions {
            messages: shared.messages || chat.messages,
            files: shared.files || chat.files,
        })
    }

    pub fn grant_remote_permissions(
        &self,
        conversation: &str,
        provider: &Provider,
        permissions: RemotePermissions,
        remember: RemotePermissions,
    ) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let shared = provider_permissions(&self.connection, provider)?;
        let shared = RemotePermissions {
            messages: shared.messages || (permissions.messages && remember.messages),
            files: shared.files || (permissions.files && remember.files),
        };
        save_provider_permissions(&self.connection, provider, shared)?;
        let chat = conversation_permissions(&self.connection, conversation, provider)?;
        self.set_remote_permissions(
            conversation,
            provider,
            RemotePermissions {
                messages: chat.messages || (permissions.messages && !shared.messages),
                files: chat.files || (permissions.files && !shared.files),
            },
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn set_remote_permissions(
        &self,
        conversation: &str,
        provider: &Provider,
        permissions: RemotePermissions,
    ) -> Result<()> {
        let updated = self.connection.execute(
            "INSERT INTO conversation_remote_permissions (conversation_id, destination, allow_messages, allow_files)
             SELECT conversations.id, ?1, ?2, ?3 FROM conversations JOIN providers ON providers.id = conversations.provider_id
             WHERE conversations.id = ?4 AND providers.id = ?5 AND providers.kind = ?6 AND providers.base_url = ?7
             ON CONFLICT(conversation_id) DO UPDATE SET destination = excluded.destination, allow_messages = excluded.allow_messages, allow_files = excluded.allow_files",
            params![destination(provider), permissions.messages, permissions.files, conversation, provider.id, provider.kind.as_str(), provider.base_url],
        )?;
        if updated == 0 {
            return Err(MooseError::ProviderRequest(
                "The chat or provider changed. Send the message again to review it.".into(),
            ));
        }
        Ok(())
    }
}
