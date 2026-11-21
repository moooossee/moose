use crate::i18n::gettext;

use super::*;
use crate::attachments::{
    Asset, ExtractedPage, FileEntry, ImportedAsset, MAX_ATTACHMENTS, MAX_TEXT_BYTES, Source,
    chunks, error, search_expression,
};
use crate::documents::{CreatedDocument, DocumentFormat};
use sha2::{Digest, Sha256};

const ASSET_COLUMNS: &str =
    "a.id, a.name, a.kind, a.mime_type, a.byte_size, a.page_count, a.in_library,
     EXISTS(SELECT 1 FROM document_chunks c WHERE c.asset_id = a.id)";

fn asset_from_row(row: &Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        mime_type: row.get(3)?,
        byte_size: row.get(4)?,
        page_count: row.get(5)?,
        in_library: row.get(6)?,
        has_text: row.get(7)?,
        text_only: false,
    })
}

fn attached_asset_from_row(row: &Row<'_>) -> rusqlite::Result<Asset> {
    let mut asset = asset_from_row(row)?;
    asset.text_only = row.get(8)?;
    Ok(asset)
}

fn asset_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl ConversationRepository {
    pub fn import_asset(
        &self,
        imported: ImportedAsset,
        conversation: Option<&str>,
    ) -> Result<Asset> {
        self.import_asset_with_location(imported, conversation, None)
    }

    pub fn import_asset_from_uri(
        &self,
        imported: ImportedAsset,
        conversation: Option<&str>,
        source_uri: &str,
    ) -> Result<Asset> {
        self.import_asset_with_location(imported, conversation, Some(source_uri))
    }

    fn import_asset_with_location(
        &self,
        imported: ImportedAsset,
        conversation: Option<&str>,
        source_uri: Option<&str>,
    ) -> Result<Asset> {
        let digest = asset_digest(imported.original.as_deref().unwrap_or(&imported.payload));
        let legacy_digest = imported
            .original
            .as_ref()
            .map(|_| asset_digest(&imported.payload));
        let transaction = self.connection.unchecked_transaction()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT id FROM assets WHERE digest = ?1 OR (digest = ?2 AND NOT EXISTS(SELECT 1 FROM asset_originals WHERE asset_id = assets.id)) ORDER BY digest = ?1 DESC LIMIT 1",
                params![digest, legacy_digest],
                |row| row.get(0),
            )
            .optional()?;
        let id = existing.clone().unwrap_or_else(crate::core::new_id);
        let library = imported.kind != "image" || conversation.is_none();
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
        } else {
            transaction.execute(
                "UPDATE assets SET digest = ?2 WHERE id = ?1",
                params![id, digest],
            )?;
        }
        if let Some(original) = imported.original {
            transaction.execute(
                "INSERT OR IGNORE INTO asset_originals VALUES (?1, ?2)",
                params![id, original],
            )?;
        }
        let indexed: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM asset_pages WHERE asset_id = ?1)",
            [&id],
            |row| row.get(0),
        )?;
        if !imported.pages.is_empty() && !indexed {
            self.store_asset_pages(&id, &imported.pages, None)?;
        }
        self.link_asset(&id, conversation, source_uri)?;
        transaction.commit()?;
        self.asset(&id)
    }

    pub fn reuse_asset(
        &self,
        bytes: &[u8],
        conversation: Option<&str>,
        source_uri: &str,
    ) -> Result<bool> {
        let id = self
            .connection
            .query_row(
                "SELECT id FROM assets WHERE digest = ?1",
                [asset_digest(bytes)],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(false) };
        let transaction = self.connection.unchecked_transaction()?;
        self.link_asset(&id, conversation, Some(source_uri))?;
        transaction.commit()?;
        Ok(true)
    }

    fn link_asset(
        &self,
        id: &str,
        conversation: Option<&str>,
        source_uri: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE assets SET in_library = 1 WHERE id = ?1 AND (kind != 'image' OR ?2 IS NULL)",
            params![id, conversation],
        )?;
        if let Some(conversation) = conversation {
            self.attach_to_draft(conversation, id)?;
        }
        if let Some(source_uri) = source_uri {
            self.connection.execute(
                "INSERT INTO asset_locations (asset_id, source_uri) VALUES (?1, ?2)
                 ON CONFLICT(asset_id) DO UPDATE SET source_uri = excluded.source_uri",
                params![id, source_uri],
            )?;
        }
        Ok(())
    }

    pub fn asset(&self, id: &str) -> Result<Asset> {
        Ok(self.connection.query_row(
            &format!("SELECT {ASSET_COLUMNS} FROM assets a WHERE a.id = ?1"),
            [id],
            asset_from_row,
        )?)
    }

    pub fn asset_source_uri(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT source_uri FROM asset_locations WHERE asset_id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn files(&self, query: &str, filter: &str, limit: usize) -> Result<Vec<FileEntry>> {
        let search = search_expression(query);
        let content_match = if search.is_empty() {
            "?2 != ''"
        } else {
            "a.id IN (SELECT c.asset_id FROM document_search f
                JOIN document_chunks c ON c.id = f.rowid
                WHERE document_search MATCH ?2)"
        };
        let mut statement = self.connection.prepare(&format!(
            "SELECT {ASSET_COLUMNS}, a.created_at, '' AS message_id,
                    '' AS format, '' AS conversation_title FROM assets a
             WHERE (?1 = '' OR instr(lower(a.name), lower(?1)) > 0
                OR ({content_match}))
             AND (?3 = 'all' OR (?3 = 'library' AND a.in_library = 1)
                OR (?3 = 'documents' AND a.kind != 'image')
                OR (?3 = 'images' AND a.kind = 'image'))
             UNION ALL
             SELECT d.id, d.title, 'generated', '', length(d.payload), 0, 0, 0,
                    d.created_at, d.message_id, d.format, c.title
             FROM created_documents d
             JOIN messages m ON m.id = d.message_id
             JOIN conversations c ON c.id = m.conversation_id
             WHERE ?3 IN ('all', 'generated')
                AND (?1 = '' OR instr(lower(d.title || '.' || d.format), lower(?1)) > 0
                    OR instr(lower(c.title), lower(?1)) > 0
                    OR instr(lower(d.content), lower(?1)) > 0)
             ORDER BY created_at DESC, id DESC LIMIT ?4"
        ))?;
        let rows = statement
            .query_map(
                params![query.trim(), search, filter, i64::try_from(limit)?],
                |row| {
                    Ok((
                        asset_from_row(row)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(
                |(asset, created_at, message_id, format, conversation_title)| {
                    if message_id.is_empty() {
                        return Ok(FileEntry::Uploaded(asset));
                    }
                    let format = DocumentFormat::parse(&format)
                        .ok_or_else(|| error(gettext("This document format is not supported.")))?;
                    Ok(FileEntry::Generated {
                        document: CreatedDocument {
                            id: asset.id,
                            message_id,
                            title: asset.name,
                            format,
                            byte_size: asset.byte_size,
                            created_at,
                        },
                        conversation_title,
                    })
                },
            )
            .collect()
    }

    pub fn asset_conversations(&self, asset: &str) -> Result<Vec<Conversation>> {
        let mut statement = self.connection.prepare(
            "SELECT c.id, c.provider_id, c.model_id, c.title, c.created_at,
                    c.updated_at, c.archived_at, c.pinned_at, c.folder_id
             FROM conversations c WHERE c.id IN (
                SELECT d.conversation_id FROM draft_assets d WHERE d.asset_id = ?1
                UNION
                SELECT m.conversation_id FROM messages m
                JOIN message_assets a ON a.message_id = m.id WHERE a.asset_id = ?1
                UNION
                SELECT m.conversation_id FROM messages m
                JOIN response_sources s ON s.message_id = m.id WHERE s.asset_id = ?1
             ) ORDER BY c.updated_at DESC, c.id DESC",
        )?;
        Ok(statement
            .query_map([asset], conversation_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn asset_bytes(&self, id: &str) -> Result<Vec<u8>> {
        Ok(self
            .connection
            .query_row("SELECT payload FROM assets WHERE id = ?1", [id], |row| {
                row.get(0)
            })?)
    }

    pub fn asset_original_bytes(&self, id: &str) -> Result<Vec<u8>> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(o.payload, a.payload) FROM assets a LEFT JOIN asset_originals o ON o.asset_id = a.id WHERE a.id = ?1",
            [id], |row| row.get(0),
        )?)
    }

    pub fn replace_asset_pages(
        &self,
        id: &str,
        pages: &[ExtractedPage],
        selected: Option<usize>,
    ) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        self.store_asset_pages(id, pages, selected)?;
        transaction.commit()?;
        Ok(())
    }

    fn store_asset_pages(
        &self,
        id: &str,
        pages: &[ExtractedPage],
        selected: Option<usize>,
    ) -> Result<()> {
        if selected.is_some_and(|number| number == 0 || number > pages.len()) {
            return Err(error(gettext("This page is not available in the PDF")));
        }
        let mut text_bytes = pages
            .iter()
            .enumerate()
            .filter(|(index, _)| selected.is_none_or(|number| number == index + 1))
            .map(|(_, page)| page.text.len())
            .sum::<usize>();
        if let Some(selected) = selected {
            let mut statement = self.connection.prepare(
                "SELECT DISTINCT page FROM document_chunks WHERE asset_id = ?1 AND page != ?2",
            )?;
            let retained = statement
                .query_map(params![id, selected as i64], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for number in retained {
                text_bytes += self.asset_page(id, number)?.len();
            }
        }
        if pages.len() > 1000 || text_bytes > MAX_TEXT_BYTES {
            return Err(error(gettext(
                "Documents can contain up to 4 MB of extracted text",
            )));
        }
        let selected = selected.map(i64::try_from).transpose()?;
        self.connection.execute(
            "DELETE FROM document_chunks WHERE asset_id = ?1 AND (?2 IS NULL OR page = ?2)",
            params![id, selected],
        )?;
        self.connection.execute(
            "DELETE FROM asset_pages WHERE asset_id = ?1 AND (?2 IS NULL OR page = ?2)",
            params![id, selected],
        )?;
        for (index, page) in pages.iter().enumerate() {
            let number = i64::try_from(index + 1)?;
            if selected.is_some_and(|selected| selected != number) {
                continue;
            }
            self.connection.execute(
                "INSERT INTO asset_pages VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    number,
                    page.method,
                    page.language,
                    page.confidence,
                    page.error
                ],
            )?;
            if !page.text.trim().is_empty() {
                for (position, content) in chunks(&page.text).into_iter().enumerate() {
                    self.connection.execute("INSERT INTO document_chunks(asset_id, page, position, content) VALUES (?1, ?2, ?3, ?4)", params![id, number, position as i64, content])?;
                }
            }
        }
        self.connection.execute(
            "UPDATE assets SET page_count = ?2 WHERE id = ?1",
            params![id, pages.len() as i64],
        )?;
        Ok(())
    }

    pub fn asset_page_info(&self, asset: &str, page: i64) -> Result<Option<ExtractedPage>> {
        Ok(self.connection.query_row(
            "SELECT method, language, confidence, error FROM asset_pages WHERE asset_id = ?1 AND page = ?2",
            params![asset, page],
            |row| Ok(ExtractedPage { method: row.get(0)?, language: row.get(1)?, confidence: row.get(2)?, error: row.get(3)?, ..ExtractedPage::default() }),
        ).optional()?)
    }

    pub fn set_draft_text_only(
        &self,
        conversation: &str,
        asset: &str,
        enabled: bool,
    ) -> Result<()> {
        if enabled && !self.asset(asset)?.has_text {
            return Err(error(gettext(
                "Recognize text in this image before using text-only mode",
            )));
        }
        self.connection.execute("UPDATE draft_assets SET text_only = ?3 WHERE conversation_id = ?1 AND asset_id = ?2 AND asset_id IN (SELECT id FROM assets WHERE kind = 'image')", params![conversation, asset, enabled])?;
        Ok(())
    }

    pub fn draft_assets(&self, conversation: &str) -> Result<Vec<Asset>> {
        let mut statement = self.connection.prepare(&format!("SELECT {ASSET_COLUMNS}, d.text_only FROM assets a JOIN draft_assets d ON d.asset_id = a.id WHERE d.conversation_id = ?1 ORDER BY d.position"))?;
        Ok(statement
            .query_map([conversation], attached_asset_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn message_assets(&self, message: &str) -> Result<Vec<Asset>> {
        let mut statement = self.connection.prepare(&format!("SELECT {ASSET_COLUMNS}, m.text_only FROM assets a JOIN message_assets m ON m.asset_id = a.id WHERE m.message_id = ?1 ORDER BY m.position"))?;
        Ok(statement
            .query_map([message], attached_asset_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn attach_to_draft(&self, conversation: &str, asset: &str) -> Result<()> {
        let existing = self.draft_assets(conversation)?;
        if existing.iter().any(|item| item.id == asset) {
            return Ok(());
        }
        if existing.len() >= MAX_ATTACHMENTS {
            return Err(error(gettext("Attach up to 8 files per message")));
        }
        self.connection.execute("INSERT INTO draft_assets (conversation_id, asset_id, position) SELECT ?1, ?2, COALESCE(MAX(position), -1) + 1 FROM draft_assets WHERE conversation_id = ?1", params![conversation, asset])?;
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
        self.submit_with_document(conversation, submission, prompt, sources, None)
    }

    pub fn submit_with_document(
        &self,
        conversation: &str,
        submission: &crate::chat::ChatSubmission,
        prompt: &str,
        sources: &[Source],
        document: Option<&crate::documents::DocumentJob>,
    ) -> Result<(Message, Message)> {
        let transaction = self.connection.unchecked_transaction()?;
        let exchange = match submission {
            crate::chat::ChatSubmission::New(_) => {
                let exchange = self.create_exchange_inner(conversation, prompt)?;
                transaction.execute("INSERT INTO message_assets SELECT ?1, asset_id, position, text_only FROM draft_assets WHERE conversation_id = ?2", params![exchange.0.id, conversation])?;
                transaction.execute(
                    "DELETE FROM draft_assets WHERE conversation_id = ?1",
                    [conversation],
                )?;
                self.save_draft(conversation, "")?;
                exchange
            }
            crate::chat::ChatSubmission::Edit { id, .. } => {
                let exchange = self.edit_message_inner(id, prompt)?;
                transaction.execute("INSERT INTO message_assets SELECT ?1, asset_id, position, text_only FROM message_assets WHERE message_id = ?2", params![exchange.0.id, id])?;
                exchange
            }
            crate::chat::ChatSubmission::Regenerate(id) => self.regenerate_message_inner(id)?,
            crate::chat::ChatSubmission::Continue(id) => self.continue_message_inner(id, prompt)?,
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
        if let Some(document) = document {
            self.save_document_request(&exchange.1.id, &document.request, &document.content)?;
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
