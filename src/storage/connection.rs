use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;

use super::migrations::run_migrations;

pub fn open_database(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut connection = Connection::open(path)?;
    prepare_connection(&mut connection)?;
    Ok(connection)
}

pub fn open_in_memory_database() -> Result<Connection> {
    let mut connection = Connection::open_in_memory()?;
    prepare_connection(&mut connection)?;
    Ok(connection)
}

fn prepare_connection(connection: &mut Connection) -> Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    run_migrations(connection)
}
