use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    error::{MooseError, Result},
    profiles::{ChatProfile, NewChatProfile},
};

#[derive(Clone)]
pub struct ProfileRepository {
    connection: Rc<Connection>,
}

impl ProfileRepository {
    pub fn new(connection: Rc<Connection>) -> Self {
        Self { connection }
    }

    pub fn create(&self, new_profile: NewChatProfile) -> Result<ChatProfile> {
        let profile = new_profile.into_profile()?;
        if self.get_by_name(&profile.name)?.is_some() {
            return Err(MooseError::ProfileNameAlreadyExists);
        }
        self.connection.execute(
            "INSERT INTO chat_profiles (
                id, name, description, temperature, system_prompt, is_builtin, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                profile.id,
                profile.name,
                profile.description,
                profile.temperature,
                profile.system_prompt,
                profile.is_builtin,
                profile.created_at,
                profile.updated_at,
            ],
        )?;
        self.get_required(&profile.id)
    }

    pub fn list(&self) -> Result<Vec<ChatProfile>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, description, temperature, system_prompt, is_builtin, created_at, updated_at
             FROM chat_profiles
             ORDER BY is_builtin DESC, CASE id
                 WHEN 'builtin-general' THEN 0
                 WHEN 'builtin-code' THEN 1
                 WHEN 'builtin-writing' THEN 2
                 WHEN 'builtin-translation' THEN 3
                 WHEN 'builtin-reasoning' THEN 4
                 ELSE 5
             END, name COLLATE NOCASE ASC",
        )?;
        let profiles = statement
            .query_map([], profile_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(profiles)
    }

    pub fn get(&self, id: &str) -> Result<Option<ChatProfile>> {
        self.connection
            .query_row(
                "SELECT id, name, description, temperature, system_prompt, is_builtin, created_at, updated_at
                 FROM chat_profiles
                 WHERE id = ?1",
                params![id],
                profile_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let profile = self.get_required(id)?;
        if profile.is_builtin {
            return Err(MooseError::BuiltinProfileCannotBeDeleted);
        }
        self.connection
            .execute("DELETE FROM chat_profiles WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn get_by_name(&self, name: &str) -> Result<Option<ChatProfile>> {
        self.connection
            .query_row(
                "SELECT id, name, description, temperature, system_prompt, is_builtin, created_at, updated_at
                 FROM chat_profiles
                 WHERE name = ?1 COLLATE NOCASE",
                params![name],
                profile_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_required(&self, id: &str) -> Result<ChatProfile> {
        self.get(id)?.ok_or(MooseError::ProfileNotFound)
    }
}

fn profile_from_row(row: &Row<'_>) -> rusqlite::Result<ChatProfile> {
    Ok(ChatProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        temperature: row.get(3)?,
        system_prompt: row.get(4)?,
        is_builtin: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
