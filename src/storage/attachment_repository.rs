use super::*;
use crate::attachments::{
    Asset, ImportedAsset, MAX_ATTACHMENTS, Source, chunks, error, search_expression,
};
use sha2::{Digest, Sha256};

const ASSET_COLUMNS: &str =
    "a.id, a.name, a.kind, a.mime_type, a.byte_size, a.page_count, a.in_library";

fn asset_from_row(row: &Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        mime_type: row.get(3)?,
        byte_size: row.get(4)?,
        page_count: row.get(5)?,
        in_library: row.get(6)?,
    })
}

impl ConversationRepository {
    pub fn import_asset(
        &self,
        imported: ImportedAsset,
        conversation: Option<&str>,
    ) -> Result<Asset> {
        let digest = Sha256::digest(&imported.payload)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let transaction = self.connection.unchecked_transaction()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT id FROM assets WHERE digest = ?1",
                [&digest],
                |row| row.get(0),
            )
            .optional()?;
        let id = existing.clone().unwrap_or_else(crate::core::new_id);
        let library = imported.kind != "image";
        if existing.is_none() {
            transaction.execute(
                "INSERT INTO assets VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    digest,
                    imported.name,
                    imported.kind,
                    imported.mime_type,
                    imported.payload.len() as i64,
                    imported.payload,
                    imported.pages.len() as i64,
                    library,
                    utc_now()
                ],
            )?;
            for (page, text) in imported.pages.iter().enumerate() {
                if text.trim().is_empty() {
                    continue;
                }
                for (position, content) in chunks(text).into_iter().enumerate() {
                    transaction.execute("INSERT INTO document_chunks(asset_id, page, position, content) VALUES (?1, ?2, ?3, ?4)", params![id, page as i64 + 1, position as i64, content])?;
                }
            }
        } else if library {
            transaction.execute("UPDATE assets SET in_library = 1 WHERE id = ?1", [&id])?;
        }
        if let Some(conversation) = conversation {
            self.attach_to_draft(conversation, &id)?;
        }
        transaction.commit()?;
        self.asset(&id)
    }

    pub fn asset(&self, id: &str) -> Result<Asset> {
        Ok(self.connection.query_row(
            &format!("SELECT {ASSET_COLUMNS} FROM assets a WHERE a.id = ?1"),
            [id],
            asset_from_row,
        )?)
    }

    pub fn asset_bytes(&self, id: &str) -> Result<Vec<u8>> {
        Ok(self
            .connection
            .query_row("SELECT payload FROM assets WHERE id = ?1", [id], |row| {
                row.get(0)
            })?)
    }

    pub fn draft_assets(&self, conversation: &str) -> Result<Vec<Asset>> {
        let mut statement = self.connection.prepare(&format!("SELECT {ASSET_COLUMNS} FROM assets a JOIN draft_assets d ON d.asset_id = a.id WHERE d.conversation_id = ?1 ORDER BY d.position"))?;
        Ok(statement
            .query_map([conversation], asset_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn message_assets(&self, message: &str) -> Result<Vec<Asset>> {
        let mut statement = self.connection.prepare(&format!("SELECT {ASSET_COLUMNS} FROM assets a JOIN message_assets m ON m.asset_id = a.id WHERE m.message_id = ?1 ORDER BY m.position"))?;
        Ok(statement
            .query_map([message], asset_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn attach_to_draft(&self, conversation: &str, asset: &str) -> Result<()> {
        let existing = self.draft_assets(conversation)?;
        if existing.iter().any(|item| item.id == asset) {
            return Ok(());
        }
        if existing.len() >= MAX_ATTACHMENTS {
            return Err(error("Attach up to 8 files per message"));
        }
        self.connection.execute("INSERT INTO draft_assets SELECT ?1, ?2, COALESCE(MAX(position), -1) + 1 FROM draft_assets WHERE conversation_id = ?1", params![conversation, asset])?;
        self.remember_conversation(conversation)?;
        Ok(())
    }

    pub fn detach_from_draft(&self, conversation: &str, asset: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM draft_assets WHERE conversation_id = ?1 AND asset_id = ?2",
            params![conversation, asset],
        )?;
        self.collect_unused_assets()
    }

    pub fn collect_unused_assets(&self) -> Result<()> {
        self.connection.execute("DELETE FROM assets WHERE in_library = 0 AND NOT EXISTS(SELECT 1 FROM draft_assets WHERE asset_id = assets.id) AND NOT EXISTS(SELECT 1 FROM message_assets WHERE asset_id = assets.id) AND NOT EXISTS(SELECT 1 FROM response_sources WHERE asset_id = assets.id)", [])?;
        Ok(())
    }

    pub fn library_assets(&self, query: &str) -> Result<Vec<Asset>> {
        let search = search_expression(query);
        if search.is_empty() {
            let mut statement = self.connection.prepare(&format!("SELECT {ASSET_COLUMNS} FROM assets a WHERE in_library = 1 AND (?1 = '' OR instr(lower(a.name), lower(?1)) > 0) ORDER BY a.created_at DESC LIMIT 500"))?;
            return Ok(statement
                .query_map([query.trim()], asset_from_row)?
                .collect::<rusqlite::Result<_>>()?);
        }
        let mut statement = self.connection.prepare(&format!("SELECT {ASSET_COLUMNS} FROM assets a WHERE in_library = 1 AND (instr(lower(a.name), lower(?1)) > 0 OR a.id IN (SELECT c.asset_id FROM document_search f JOIN document_chunks c ON c.id = f.rowid WHERE document_search MATCH ?2)) ORDER BY a.created_at DESC LIMIT 500"))?;
        Ok(statement
            .query_map(params![query.trim(), search], asset_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn remove_from_library(&self, id: &str) -> Result<()> {
        self.connection
            .execute("UPDATE assets SET in_library = 0 WHERE id = ?1", [id])?;
        self.collect_unused_assets()
    }

    pub fn library_enabled(&self, conversation: &str) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT enabled FROM conversation_library WHERE conversation_id = ?1",
                [conversation],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn set_library_enabled(&self, conversation: &str, enabled: bool) -> Result<()> {
        self.connection.execute("INSERT INTO conversation_library VALUES (?1, ?2) ON CONFLICT(conversation_id) DO UPDATE SET enabled = excluded.enabled", params![conversation, enabled])?;
        Ok(())
    }

    pub fn retrieve_sources(
        &self,
        query: &str,
        assets: &[String],
        library: bool,
        budget: usize,
    ) -> Result<Vec<Source>> {
        let selected = serde_json::to_string(assets)?;
        let search = search_expression(query);
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut remaining = budget;
        let coverage = budget.saturating_sub(assets.len() * 160) / assets.len().max(1);
        let mut add = |source: Source, maximum: usize| {
            if maximum < 40
                || source.content.trim().is_empty()
                || remaining < 200
                || !seen.insert((source.asset_id.clone(), source.page, source.content.clone()))
            {
                return;
            }
            let content = source
                .content
                .chars()
                .take(remaining.saturating_sub(160).min(maximum).min(1600))
                .collect::<String>();
            remaining = remaining.saturating_sub(content.chars().count() + 160);
            result.push(Source { content, ..source });
        };
        for id in assets.iter().take(MAX_ATTACHMENTS) {
            let sql = if search.is_empty() {
                "SELECT a.id, a.name, c.page, c.content FROM document_chunks c JOIN assets a ON a.id = c.asset_id WHERE a.id = ?1 ORDER BY c.page, c.position LIMIT 1"
            } else {
                "SELECT a.id, a.name, c.page, c.content FROM document_search f JOIN document_chunks c ON c.id = f.rowid JOIN assets a ON a.id = c.asset_id WHERE a.id = ?1 AND document_search MATCH ?2 ORDER BY bm25(document_search) LIMIT 1"
            };
            let mut statement = self.connection.prepare(sql)?;
            let row = if search.is_empty() {
                statement.query_row([id], source_from_row).optional()?
            } else {
                statement
                    .query_row(params![id, search], source_from_row)
                    .optional()?
            };
            let row = match row {
                Some(row) => Some(row),
                None => self.connection.query_row("SELECT a.id, a.name, c.page, c.content FROM document_chunks c JOIN assets a ON a.id = c.asset_id WHERE a.id = ?1 ORDER BY c.page, c.position LIMIT 1", [id], source_from_row).optional()?,
            };
            if let Some(row) = row {
                add(row, coverage);
            }
        }
        if !search.is_empty() {
            let mut statement = self.connection.prepare("SELECT a.id, a.name, c.page, c.content FROM document_search f JOIN document_chunks c ON c.id = f.rowid JOIN assets a ON a.id = c.asset_id WHERE document_search MATCH ?1 AND (a.id IN (SELECT value FROM json_each(?2)) OR (?3 AND a.in_library = 1)) ORDER BY bm25(document_search) LIMIT 32")?;
            for row in statement.query_map(params![search, selected, library], source_from_row)? {
                add(row?, 1600);
            }
        }
        if !assets.is_empty() {
            let mut statement = self.connection.prepare("SELECT a.id, a.name, c.page, c.content FROM document_chunks c JOIN assets a ON a.id = c.asset_id WHERE a.id IN (SELECT value FROM json_each(?1)) ORDER BY c.position, c.page LIMIT 32")?;
            for row in statement.query_map([selected], source_from_row)? {
                add(row?, 1600);
            }
        }
        Ok(result)
    }

    pub fn response_sources(&self, message: &str) -> Result<Vec<Source>> {
        let mut statement = self.connection.prepare("SELECT asset_id, name, page, content FROM response_sources WHERE message_id = ?1 ORDER BY number")?;
        Ok(statement
            .query_map([message], source_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn asset_page(&self, asset: &str, page: i64) -> Result<String> {
        let mut statement = self.connection.prepare("SELECT content FROM document_chunks WHERE asset_id = ?1 AND page = ?2 ORDER BY position")?;
        let chunks = statement
            .query_map(params![asset, page], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result = String::new();
        for (index, chunk) in chunks.into_iter().enumerate() {
            if index == 0 {
                result.push_str(&chunk);
            } else {
                result.extend(chunk.chars().skip(180));
            }
        }
        Ok(result)
    }

    pub fn submit_with_sources(
        &self,
        conversation: &str,
        submission: &crate::chat::ChatSubmission,
        prompt: &str,
        sources: &[Source],
    ) -> Result<(Message, Message)> {
        let transaction = self.connection.unchecked_transaction()?;
        let exchange = match submission {
            crate::chat::ChatSubmission::New(_) => {
                let exchange = self.create_exchange_inner(conversation, prompt)?;
                transaction.execute("INSERT INTO message_assets SELECT ?1, asset_id, position FROM draft_assets WHERE conversation_id = ?2", params![exchange.0.id, conversation])?;
                transaction.execute(
                    "DELETE FROM draft_assets WHERE conversation_id = ?1",
                    [conversation],
                )?;
                self.save_draft(conversation, "")?;
                exchange
            }
            crate::chat::ChatSubmission::Edit { id, .. } => {
                let exchange = self.edit_message_inner(id, prompt)?;
                transaction.execute("INSERT INTO message_assets SELECT ?1, asset_id, position FROM message_assets WHERE message_id = ?2", params![exchange.0.id, id])?;
                exchange
            }
            crate::chat::ChatSubmission::Regenerate(id) => self.regenerate_message_inner(id)?,
        };
        if exchange.0.conversation_id != conversation {
            return Err(MooseError::MessageNotFound);
        }
        for (index, source) in sources.iter().enumerate() {
            transaction.execute(
                "INSERT INTO response_sources VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    exchange.1.id,
                    index as i64 + 1,
                    source.asset_id,
                    source.name,
                    source.page,
                    source.content
                ],
            )?;
        }
        transaction.commit()?;
        Ok(exchange)
    }
}
fn source_from_row(row: &Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        asset_id: row.get(0)?,
        name: row.get(1)?,
        page: row.get(2)?,
        content: row.get(3)?,
    })
}
