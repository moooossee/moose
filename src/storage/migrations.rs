use rusqlite::{Connection, OptionalExtension, params};

use crate::{core::utc_now, error::Result};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_initial.sql")),
    (
        2,
        include_str!("../../migrations/0002_generation_context_messages.sql"),
    ),
    (3, include_str!("../../migrations/0003_chat_profiles.sql")),
    (
        4,
        include_str!("../../migrations/0004_history_organization.sql"),
    ),
    (
        5,
        include_str!("../../migrations/0005_message_versions.sql"),
    ),
    (6, include_str!("../../migrations/0006_chat_workspace.sql")),
    (
        7,
        include_str!("../../migrations/0007_attachments_library.sql"),
    ),
    (8, include_str!("../../migrations/0008_asset_locations.sql")),
    (9, include_str!("../../migrations/0009_cloud_providers.sql")),
    (
        10,
        "CREATE TABLE provider_remote_permissions (
            provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
            destination TEXT NOT NULL,
            allow_messages INTEGER NOT NULL DEFAULT 0 CHECK (allow_messages IN (0, 1)),
            allow_files INTEGER NOT NULL DEFAULT 0 CHECK (allow_files IN (0, 1))
        );",
    ),
    (
        11,
        include_str!("../../migrations/0011_mistral_deepseek_providers.sql"),
    ),
    (
        13,
        include_str!("../../migrations/0013_custom_providers.sql"),
    ),
    (
        14,
        include_str!("../../migrations/0014_nvidia_cloudflare_providers.sql"),
    ),
];

pub fn run_migrations(connection: &mut Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    for (version, sql) in MIGRATIONS {
        let applied = connection
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                params![version],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();

        if applied {
            continue;
        }

        let rebuilds_parent = matches!(*version, 9 | 11 | 13 | 14);
        if rebuilds_parent {
            connection.pragma_update(None, "foreign_keys", "OFF")?;
        }
        let result = (|| -> Result<()> {
            let transaction = connection.transaction()?;
            transaction.execute_batch(sql)?;
            let violations: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check",
                [],
                |row| row.get(0),
            )?;
            if violations != 0 {
                return Err(crate::error::MooseError::ProviderResponse(
                    "Database migration would break existing references".into(),
                ));
            }
            transaction.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![version, utc_now()],
            )?;
            transaction.commit()?;
            Ok(())
        })();
        if rebuilds_parent {
            connection.pragma_update(None, "foreign_keys", "ON")?;
        }
        result?;
    }

    Ok(())
}

#[cfg(test)]
fn require_migration(connection: &Connection, version: i64) -> Result<()> {
    let applied = connection
        .query_row(
            "SELECT version FROM schema_migrations WHERE version = ?1",
            params![version],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();

    if applied {
        Ok(())
    } else {
        Err(crate::error::MooseError::InvalidOllamaResponse(format!(
            "missing migration {version}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{require_migration, run_migrations};

    #[test]
    fn new_cloud_providers_migration_preserves_settings_and_history() {
        let mut connection = Connection::open_in_memory().unwrap();
        for (version, sql) in super::MIGRATIONS
            .iter()
            .filter(|(version, _)| *version < 14)
        {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1, '2026-01-01')",
                    [version],
                )
                .unwrap();
        }
        let config = r#"{"protocol":"chat-completions","authentication":"none","models":[],"discover_models":true}"#;
        connection.execute(
            "INSERT INTO providers VALUES ('custom', 'custom', 'Custom', 'https://example.com/v1', 0, 1, '2026-01-01', '2026-01-01', ?1)",
            [config],
        ).unwrap();
        connection.execute_batch(
            "INSERT INTO conversations (id, provider_id, title, created_at, updated_at) VALUES ('chat', 'custom', 'Saved chat', '2026-01-01', '2026-01-01');
             INSERT INTO messages (id, conversation_id, role, content, status, created_at) VALUES ('message', 'chat', 'user', 'Hello', 'complete', '2026-01-01');
             INSERT INTO provider_remote_permissions VALUES ('custom', 'saved-destination', 1, 0);"
        ).unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        run_migrations(&mut connection).unwrap();
        run_migrations(&mut connection).unwrap();
        let saved: (String, bool) = connection
            .query_row(
                "SELECT custom_config, is_default FROM providers WHERE id = 'custom'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(saved, (config.into(), true));
        let content: String = connection
            .query_row(
                "SELECT content FROM messages WHERE id = 'message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content, "Hello");
        let destination: String = connection
            .query_row(
                "SELECT destination FROM provider_remote_permissions WHERE provider_id = 'custom'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(destination, "saved-destination");
        for kind in ["nvidia", "cloudflare"] {
            connection.execute(
                "INSERT INTO providers VALUES (?1, ?1, ?1, 'https://example.com', 0, 0, '2026-01-01', '2026-01-01', NULL)",
                [kind],
            ).unwrap();
        }
        let violations: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
        let foreign_keys: bool = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert!(foreign_keys);
    }

    #[test]
    fn message_versions_migration_preserves_existing_history() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        for (version, sql) in super::MIGRATIONS.iter().filter(|(version, _)| *version < 5) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations VALUES (?1, '2026-01-01')",
                    [version],
                )
                .unwrap();
        }
        connection.execute_batch(
            "INSERT INTO providers VALUES ('provider', 'ollama', 'Local', 'http://127.0.0.1:11434/api', 0, 1, '2026-01-01', '2026-01-01');
             INSERT INTO conversations (id, provider_id, title, created_at, updated_at) VALUES
                ('chat', 'provider', 'Existing chat', '2026-01-01', '2026-01-01'),
                ('other', 'provider', 'Other chat', '2026-01-01', '2026-01-01');
             INSERT INTO messages (id, conversation_id, role, content, status, created_at) VALUES
                ('b', 'chat', 'assistant', 'Answer', 'complete', '2026-01-01'),
                ('a', 'chat', 'user', 'Question', 'complete', '2026-01-01'),
                ('c', 'chat', 'user', 'Follow-up', 'complete', '2026-01-02'),
                ('d', 'other', 'user', 'Independent', 'complete', '2026-01-01');"
        ).unwrap();
        run_migrations(&mut connection).unwrap();
        run_migrations(&mut connection).unwrap();
        let mut statement = connection.prepare("SELECT id, parent_id FROM active_messages WHERE conversation_id = 'chat' ORDER BY depth").unwrap();
        let messages = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            messages,
            vec![
                ("a".into(), None),
                ("b".into(), Some("a".into())),
                ("c".into(), Some("b".into()))
            ]
        );
        let other_parent: Option<String> = connection
            .query_row("SELECT parent_id FROM messages WHERE id = 'd'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(other_parent.is_none());
        let violations: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
    }

    #[test]
    fn initial_migration_creates_schema_once() {
        let mut connection = Connection::open_in_memory().unwrap();

        run_migrations(&mut connection).unwrap();
        run_migrations(&mut connection).unwrap();
        for (version, _) in super::MIGRATIONS {
            require_migration(&connection, *version).unwrap();
        }

        let provider_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'providers'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(provider_table_count, 1);
        assert_eq!(
            usize::try_from(migration_count).unwrap(),
            super::MIGRATIONS.len()
        );
    }
}
