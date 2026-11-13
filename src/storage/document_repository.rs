use crate::i18n::gettext;

use super::*;
use crate::documents::{
    CreatedDocument, DocumentFormat, DocumentJob, DocumentRequest, MAX_CONTENT_BYTES,
    MAX_FILE_BYTES,
};

impl ConversationRepository {
    pub fn document_job(&self, message: &str) -> Result<Option<DocumentJob>> {
        let value: Option<(String, String, String, String, String)> = self.connection.query_row(
            "SELECT title, format, content, state, error FROM document_requests WHERE message_id = ?1", [message],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?;
        value
            .map(|(title, format, content, state, error)| {
                let format = DocumentFormat::parse(&format).ok_or_else(|| {
                    crate::attachments::error(gettext("This document format is not supported."))
                })?;
                Ok(DocumentJob {
                    request: DocumentRequest { title, format },
                    content,
                    state,
                    error,
                })
            })
            .transpose()
    }

    pub fn save_document_request(
        &self,
        message: &str,
        request: &DocumentRequest,
        content: &str,
    ) -> Result<()> {
        if content.len() > MAX_CONTENT_BYTES {
            return Err(crate::attachments::error(gettext(
                "The document content is too large.",
            )));
        }
        self.connection.execute(
            "INSERT INTO document_requests (message_id, title, format, content) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(message_id) DO UPDATE SET title = excluded.title, format = excluded.format, content = excluded.content, state = 'waiting', error = ''",
            params![message, request.title, request.format.extension(), content],
        )?;
        Ok(())
    }

    pub fn save_document_content(&self, message: &str, content: &str) -> Result<()> {
        if content.len() > MAX_CONTENT_BYTES {
            return Err(crate::attachments::error(gettext(
                "The document content is too large.",
            )));
        }
        self.connection.execute(
            "UPDATE document_requests SET content = ?2 WHERE message_id = ?1",
            params![message, content],
        )?;
        Ok(())
    }

    pub fn set_document_state(&self, message: &str, state: &str, error: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE document_requests SET state = ?2, error = ?3 WHERE message_id = ?1",
            params![message, state, error],
        )?;
        Ok(())
    }

    pub fn create_document(
        &self,
        message: &str,
        title: &str,
        format: DocumentFormat,
        content: &str,
        payload: &[u8],
    ) -> Result<CreatedDocument> {
        if payload.is_empty() || payload.len() > MAX_FILE_BYTES || content.len() > MAX_CONTENT_BYTES
        {
            return Err(crate::attachments::error(gettext(
                "The generated document is empty or exceeds the size limit.",
            )));
        }
        let id = crate::core::new_id();
        let created_at = crate::core::utc_now();
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO created_documents (id, message_id, title, format, content, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(message_id, format) DO UPDATE SET id = excluded.id, title = excluded.title,
             content = excluded.content, payload = excluded.payload, created_at = excluded.created_at",
            params![id, message, title, format.extension(), content, payload, created_at],
        )?;
        transaction.execute(
            "UPDATE document_requests SET title = ?2 WHERE message_id = ?1 AND format = ?3",
            params![message, title, format.extension()],
        )?;
        transaction.commit()?;
        Ok(CreatedDocument {
            id,
            message_id: message.into(),
            title: title.into(),
            format,
            byte_size: payload.len() as i64,
            created_at,
        })
    }

    pub fn message_documents(&self, message: &str) -> Result<Vec<CreatedDocument>> {
        let mut statement = self.connection.prepare("SELECT id, message_id, title, format, length(payload), created_at FROM created_documents WHERE message_id = ?1 ORDER BY created_at, id")?;
        let rows = statement
            .query_map([message], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, message_id, title, format, byte_size, created_at)| {
                let format = DocumentFormat::parse(&format).ok_or_else(|| {
                    crate::attachments::error(gettext("This document format is not supported."))
                })?;
                Ok(CreatedDocument {
                    id,
                    message_id,
                    title,
                    format,
                    byte_size,
                    created_at,
                })
            })
            .collect()
    }

    pub fn document_bytes(&self, id: &str) -> Result<Vec<u8>> {
        Ok(self.connection.query_row(
            "SELECT payload FROM created_documents WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn created_document(&self, id: &str) -> Result<Option<CreatedDocument>> {
        let message: Option<String> = self
            .connection
            .query_row(
                "SELECT message_id FROM created_documents WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        match message {
            Some(message) => Ok(self
                .message_documents(&message)?
                .into_iter()
                .find(|document| document.id == id)),
            None => Ok(None),
        }
    }

    pub fn document_content(&self, id: &str) -> Result<String> {
        Ok(self.connection.query_row(
            "SELECT content FROM created_documents WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?)
    }

    pub(super) fn copy_documents(&self, original: &str, copied: &str) -> Result<()> {
        self.connection.execute("INSERT INTO document_requests SELECT ?1, title, format, content, CASE WHEN state = 'creating' THEN 'waiting' ELSE state END, error FROM document_requests WHERE message_id = ?2", params![copied, original])?;
        for document in self.message_documents(original)? {
            self.connection.execute(
                "INSERT INTO created_documents SELECT ?1, ?2, title, format, content, payload, created_at FROM created_documents WHERE id = ?3",
                params![crate::core::new_id(), copied, document.id],
            )?;
        }
        Ok(())
    }
}
