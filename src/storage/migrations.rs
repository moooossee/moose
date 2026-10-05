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

        let transaction = connection.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![version, utc_now()],
        )?;
        transaction.commit()?;
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
