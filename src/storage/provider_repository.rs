use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    core::utc_now,
    error::Result,
    providers::{
        NewProvider, Provider, ProviderKind, ProviderUpdate, validate_base_url,
        validate_provider_name,
    },
};

#[derive(Clone)]
pub struct ProviderRepository {
    connection: Rc<Connection>,
}

impl ProviderRepository {
    pub fn new(connection: Rc<Connection>) -> Self {
        Self { connection }
    }

    pub fn list(&self) -> Result<Vec<Provider>> {
        let mut statement = self.connection.prepare(
            "SELECT id, kind, name, base_url, is_managed, is_default, created_at, updated_at
             FROM providers
             ORDER BY is_default DESC, updated_at DESC, name ASC",
        )?;
        let providers = statement
            .query_map([], provider_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(providers)
    }

    pub fn get(&self, id: &str) -> Result<Option<Provider>> {
        self.connection
            .query_row(
                "SELECT id, kind, name, base_url, is_managed, is_default, created_at, updated_at
                 FROM providers
                 WHERE id = ?1",
                params![id],
                provider_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn default_provider(&self) -> Result<Option<Provider>> {
        self.connection
            .query_row(
                "SELECT id, kind, name, base_url, is_managed, is_default, created_at, updated_at
                 FROM providers
                 WHERE is_default = 1
                 LIMIT 1",
                [],
                provider_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn ensure_default_provider(&self) -> Result<Provider> {
        if let Some(provider) = self.default_provider()? {
            return Ok(provider);
        }

        if let Some(provider) = self.list()?.into_iter().next() {
            self.set_default(&provider.id)?;
            return self.get(&provider.id).map(|provider| {
                provider.expect("provider exists immediately after setting default")
            });
        }

        self.create(NewProvider::local_ollama(true))
    }

    pub fn create(&self, new_provider: NewProvider) -> Result<Provider> {
        let provider = new_provider.into_provider()?;

        if provider.is_default {
            self.connection
                .execute("UPDATE providers SET is_default = 0", [])?;
        }

        self.connection.execute(
            "INSERT INTO providers (
                id, kind, name, base_url, is_managed, is_default, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                provider.id,
                provider.kind.as_str(),
                provider.name,
                provider.base_url,
                provider.is_managed,
                provider.is_default,
                provider.created_at,
                provider.updated_at,
            ],
        )?;

        self.get(&provider.id)
            .map(|provider| provider.expect("provider exists immediately after insert"))
    }

    pub fn update(&self, update: ProviderUpdate) -> Result<Provider> {
        let name = validate_provider_name(&update.name)?;
        let base_url = validate_base_url(&update.base_url)?;
        let timestamp = utc_now();

        if update.is_default {
            self.connection
                .execute("UPDATE providers SET is_default = 0", [])?;
        }

        self.connection.execute(
            "UPDATE providers
             SET name = ?1, base_url = ?2, is_default = ?3, updated_at = ?4
             WHERE id = ?5",
            params![name, base_url, update.is_default, timestamp, update.id],
        )?;

        self.get(&update.id)
            .map(|provider| provider.expect("provider exists immediately after update"))
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM providers WHERE id = ?1", params![id])?;

        if self.default_provider()?.is_none()
            && let Some(provider) = self.list()?.into_iter().next()
        {
            self.set_default(&provider.id)?;
        }

        Ok(())
    }

    pub fn set_default(&self, id: &str) -> Result<()> {
        let timestamp = utc_now();
        self.connection
            .execute("UPDATE providers SET is_default = 0", [])?;
        self.connection.execute(
            "UPDATE providers SET is_default = 1, updated_at = ?1 WHERE id = ?2",
            params![timestamp, id],
        )?;
        Ok(())
    }
}

fn provider_from_row(row: &Row<'_>) -> rusqlite::Result<Provider> {
    let kind: String = row.get(1)?;
    let is_managed: bool = row.get(4)?;
    let is_default: bool = row.get(5)?;

    Ok(Provider {
        id: row.get(0)?,
        kind: kind.parse::<ProviderKind>().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        name: row.get(2)?,
        base_url: row.get(3)?,
        is_managed,
        is_default,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::{
        providers::{NewProvider, ProviderKind, ProviderUpdate},
        storage::{ProviderRepository, open_in_memory_database},
    };

    #[test]
    fn provider_repository_creates_default_provider() {
        let connection = Rc::new(open_in_memory_database().unwrap());
        let repository = ProviderRepository::new(connection);

        let provider = repository.ensure_default_provider().unwrap();

        assert_eq!(provider.name, "Local Ollama");
        assert!(provider.is_default);
        assert_eq!(repository.list().unwrap().len(), 1);
    }

    #[test]
    fn provider_repository_updates_default_provider() {
        let connection = Rc::new(open_in_memory_database().unwrap());
        let repository = ProviderRepository::new(connection);
        let first = repository.ensure_default_provider().unwrap();
        let second = repository
            .create(NewProvider {
                kind: ProviderKind::Ollama,
                name: "Remote Ollama".to_string(),
                base_url: "http://192.168.1.20:11434/api".to_string(),
                is_managed: false,
                is_default: false,
            })
            .unwrap();

        let updated = repository
            .update(ProviderUpdate {
                id: second.id.clone(),
                name: "Workstation Ollama".to_string(),
                base_url: "http://192.168.1.21:11434/api".to_string(),
                is_default: true,
            })
            .unwrap();
        let first = repository.get(&first.id).unwrap().unwrap();

        assert_eq!(updated.name, "Workstation Ollama");
        assert!(updated.is_default);
        assert!(!first.is_default);
    }

    #[test]
    fn provider_repository_deletes_provider() {
        let connection = Rc::new(open_in_memory_database().unwrap());
        let repository = ProviderRepository::new(connection);
        let provider = repository.ensure_default_provider().unwrap();

        repository.delete(&provider.id).unwrap();

        assert!(repository.list().unwrap().is_empty());
    }
}
