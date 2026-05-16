use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    core::utc_now,
    error::{MooseError, Result},
    models::{
        DownloadJob, DownloadJobStatus, NewDownloadJob, validate_download_byte_count,
        validate_download_error_message,
    },
};

#[derive(Clone)]
pub struct DownloadJobRepository {
    connection: Rc<Connection>,
}

impl DownloadJobRepository {
    pub fn new(connection: Rc<Connection>) -> Self {
        Self { connection }
    }

    pub fn create(&self, new_job: NewDownloadJob) -> Result<DownloadJob> {
        let job = new_job.into_download_job()?;

        self.connection.execute(
            "INSERT INTO download_jobs (
                id, provider_id, model_name, status, total_bytes, completed_bytes, error_message, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                job.id,
                job.provider_id,
                job.model_name,
                job.status.as_str(),
                job.total_bytes,
                job.completed_bytes,
                job.error_message,
                job.created_at,
                job.updated_at,
            ],
        )?;

        self.get_required(&job.id)
    }

    pub fn get(&self, id: &str) -> Result<Option<DownloadJob>> {
        self.connection
            .query_row(
                "SELECT id, provider_id, model_name, status, total_bytes, completed_bytes, error_message, created_at, updated_at
                 FROM download_jobs
                 WHERE id = ?1",
                params![id],
                download_job_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn latest_for_provider(&self, provider_id: &str) -> Result<Option<DownloadJob>> {
        self.connection
            .query_row(
                "SELECT id, provider_id, model_name, status, total_bytes, completed_bytes, error_message, created_at, updated_at
                 FROM download_jobs
                 WHERE provider_id = ?1
                 ORDER BY updated_at DESC, created_at DESC, id DESC
                 LIMIT 1",
                params![provider_id],
                download_job_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_recent_for_provider(
        &self,
        provider_id: &str,
        limit: usize,
    ) -> Result<Vec<DownloadJob>> {
        let limit = i64::try_from(limit)?;
        let mut statement = self.connection.prepare(
            "SELECT id, provider_id, model_name, status, total_bytes, completed_bytes, error_message, created_at, updated_at
             FROM download_jobs
             WHERE provider_id = ?1
             ORDER BY updated_at DESC, created_at DESC, id DESC
             LIMIT ?2",
        )?;
        let jobs = statement
            .query_map(params![provider_id, limit], download_job_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn update_progress(
        &self,
        id: &str,
        total_bytes: Option<u64>,
        completed_bytes: Option<u64>,
    ) -> Result<DownloadJob> {
        let total_bytes = validate_download_byte_count(optional_u64_to_i64(total_bytes)?)?;
        let completed_bytes = validate_download_byte_count(optional_u64_to_i64(completed_bytes)?)?;
        let timestamp = utc_now();
        let changed = self.connection.execute(
            "UPDATE download_jobs
             SET status = ?1,
                 total_bytes = COALESCE(?2, total_bytes),
                 completed_bytes = COALESCE(?3, completed_bytes),
                 error_message = NULL,
                 updated_at = ?4
             WHERE id = ?5",
            params![
                DownloadJobStatus::Running.as_str(),
                total_bytes,
                completed_bytes,
                timestamp,
                id,
            ],
        )?;

        if changed == 0 {
            return Err(MooseError::DownloadJobNotFound);
        }

        self.get_required(id)
    }

    pub fn complete(&self, id: &str) -> Result<DownloadJob> {
        let timestamp = utc_now();
        let changed = self.connection.execute(
            "UPDATE download_jobs
             SET status = ?1,
                 completed_bytes = COALESCE(completed_bytes, total_bytes),
                 error_message = NULL,
                 updated_at = ?2
             WHERE id = ?3",
            params![DownloadJobStatus::Complete.as_str(), timestamp, id],
        )?;

        if changed == 0 {
            return Err(MooseError::DownloadJobNotFound);
        }

        self.get_required(id)
    }

    pub fn cancel(&self, id: &str) -> Result<DownloadJob> {
        self.finish(id, DownloadJobStatus::Cancelled, None)
    }

    pub fn fail(&self, id: &str, error_message: &str) -> Result<DownloadJob> {
        self.finish(id, DownloadJobStatus::Failed, Some(error_message))
    }

    pub fn fail_active_jobs(&self, error_message: &str) -> Result<()> {
        let error_message = validate_download_error_message(Some(error_message))?;
        let timestamp = utc_now();
        self.connection.execute(
            "UPDATE download_jobs
             SET status = ?1, error_message = ?2, updated_at = ?3
             WHERE status IN ('queued', 'running')",
            params![DownloadJobStatus::Failed.as_str(), error_message, timestamp],
        )?;
        Ok(())
    }

    fn finish(
        &self,
        id: &str,
        status: DownloadJobStatus,
        error_message: Option<&str>,
    ) -> Result<DownloadJob> {
        let error_message = validate_download_error_message(error_message)?;
        let timestamp = utc_now();
        let changed = self.connection.execute(
            "UPDATE download_jobs
             SET status = ?1, error_message = ?2, updated_at = ?3
             WHERE id = ?4",
            params![status.as_str(), error_message, timestamp, id],
        )?;

        if changed == 0 {
            return Err(MooseError::DownloadJobNotFound);
        }

        self.get_required(id)
    }

    fn get_required(&self, id: &str) -> Result<DownloadJob> {
        self.get(id)?.ok_or(MooseError::DownloadJobNotFound)
    }
}

fn download_job_from_row(row: &Row<'_>) -> rusqlite::Result<DownloadJob> {
    let status: String = row.get(3)?;

    Ok(DownloadJob {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        model_name: row.get(2)?,
        status: status.parse::<DownloadJobStatus>().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        total_bytes: row.get(4)?,
        completed_bytes: row.get(5)?,
        error_message: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn optional_u64_to_i64(value: Option<u64>) -> Result<Option<i64>> {
    value.map(i64::try_from).transpose().map_err(Into::into)
}
