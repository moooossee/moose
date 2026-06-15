use rusqlite::{Connection, OptionalExtension, params};

use crate::{core::utc_now, error::Result};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_initial.sql")),
    (
        2,
        include_str!("../../migrations/0002_generation_context_messages.sql"),
    ),
    (3, include_str!("../../migrations/0003_chat_profiles.sql")),
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
    fn initial_migration_creates_schema_once() {
        let mut connection = Connection::open_in_memory().unwrap();

        run_migrations(&mut connection).unwrap();
        run_migrations(&mut connection).unwrap();
        require_migration(&connection, 1).unwrap();
        require_migration(&connection, 2).unwrap();
        require_migration(&connection, 3).unwrap();

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
        assert_eq!(migration_count, 3);
    }
}
