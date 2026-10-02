//! Dialect-shared durable-journal and scheduler reads.
//!
//! The PostgreSQL and MySQL tenant stores read the commit log and the
//! scheduler tables with the same statements and the same retention checks, so
//! those reads live here once, generic over [`SqlReadSession`].
//!
//! Two pieces stay per provider. The journal head and cursor floor are
//! [`SqlReadSession`] hooks because the write path also reads them inside its
//! own transactions. The journal stream limit check is a [`SqlReadStore`] hook
//! because its error text differs between the providers.

use std::future::Future;

use nimbus_core::{
    CommitEntry, CronJob, DocumentId, Error, Result, ScheduledJob, ScheduledJobResult,
    SequenceNumber, TenantEventRecord, Timestamp,
};

use crate::commit_log::deserialize_tenant_event_record;
use crate::retention::{validate_contiguous_journal_page, validate_retention_after_page};
use crate::sql::dialect::{SqlRow, SqlSession, SqlSessionStore, SqlValue};
use crate::sql::row::deserialize_json;
use crate::sql::store_core::SqlStoreCore;
use crate::store::DurableJournalPage;

const COMMIT_LOG_TABLE: &str = "commit_log";
const SCHEDULED_JOBS_TABLE: &str = "scheduled_jobs";
const RUNNING_SCHEDULED_JOBS_TABLE: &str = "running_scheduled_jobs";
const SCHEDULED_JOB_EXECUTIONS_TABLE: &str = "scheduled_job_executions";
const SCHEDULED_JOB_RESULTS_TABLE: &str = "scheduled_job_results";
const CRON_JOBS_TABLE: &str = "cron_jobs";

/// Session hooks for the journal positions that the providers compute in
/// their own backend code.
pub(crate) trait SqlReadSession: SqlSession {
    /// The larger of the last commit log sequence and the applied sequence.
    fn load_latest_sequence(
        &mut self,
        tenant_schema: &str,
    ) -> impl Future<Output = Result<SequenceNumber>> + Send;

    /// The sequence that a journal cursor must not fall behind.
    fn load_durable_journal_cursor_floor(
        &mut self,
        tenant_schema: &str,
    ) -> impl Future<Output = Result<SequenceNumber>> + Send;
}

/// Store-level journal and scheduler reads.
///
/// [`sql_read_store_facade`] re-exposes these defaults as inherent methods. A
/// default that would collide with a [`SqlStoreCore`] method has a distinct
/// name here.
pub(crate) trait SqlReadStore: SqlStoreCore + SqlSessionStore {
    /// Reject a journal stream page size outside the supported range.
    fn validate_journal_stream_limit(limit: usize) -> Result<()>;

    fn load_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            load_durable_records_from_session(&mut session.await?, &tenant_schema, sequence).await
        })
    }

    fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .load_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    fn durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            session
                .await?
                .load_durable_journal_cursor_floor(&tenant_schema)
                .await
        })
    }

    fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        Self::validate_journal_stream_limit(limit)?;
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let page = self.block_on_read(async move {
            stream_durable_journal_from_session(&mut session.await?, &tenant_schema, after, limit)
                .await
        })?;
        self.check_retention_read_page()?;
        let authoritative_floor = self
            .durable_journal_cursor_floor()?
            .max(self.retention_floor().published_read_floors().journal);
        finish_durable_journal_page(page, after, authoritative_floor)
    }

    fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let execution_id = execution_id.to_string();
        self.block_on_read(async move {
            let mut session = session.await?;
            let query = format!(
                "SELECT 1 FROM {} WHERE execution_id = ?",
                Self::ReadSession::table(&tenant_schema, SCHEDULED_JOB_EXECUTIONS_TABLE)
            );
            let row = session
                .fetch_optional(&query, &[SqlValue::Text(&execution_id)])
                .await?;
            Ok(row.is_some())
        })
    }

    fn get_scheduled_job_result(&self, job_id: &DocumentId) -> Result<Option<ScheduledJobResult>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let job_id = job_id.to_string();
        self.block_on_read(async move {
            let mut session = session.await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE job_id = ?",
                Self::ReadSession::table(&tenant_schema, SCHEDULED_JOB_RESULTS_TABLE)
            );
            load_optional_json(&mut session, &query, &job_id).await
        })
    }

    fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        self.list_scheduler_jobs(SCHEDULED_JOBS_TABLE)
    }

    fn get_pending_scheduled_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>> {
        self.load_scheduler_job_by_id(SCHEDULED_JOBS_TABLE, job_id)
    }

    fn list_running_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        self.list_scheduler_jobs(RUNNING_SCHEDULED_JOBS_TABLE)
    }

    fn get_running_scheduled_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>> {
        self.load_scheduler_job_by_id(RUNNING_SCHEDULED_JOBS_TABLE, job_id)
    }

    fn list_scheduler_jobs(&self, table_name: &'static str) -> Result<Vec<ScheduledJob>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            load_scheduled_jobs_from_session(&mut session.await?, &tenant_schema, table_name).await
        })
    }

    fn load_scheduler_job_by_id(
        &self,
        table_name: &'static str,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJob>> {
        debug_assert!(matches!(
            table_name,
            SCHEDULED_JOBS_TABLE | RUNNING_SCHEDULED_JOBS_TABLE
        ));
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let job_id = job_id.to_string();
        self.block_on_read(async move {
            let mut session = session.await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE id = ?",
                Self::ReadSession::table(&tenant_schema, table_name)
            );
            load_optional_json(&mut session, &query, &job_id).await
        })
    }

    fn peek_due_scheduled_jobs(
        &self,
        now: Timestamp,
        max_jobs: usize,
    ) -> Result<Vec<ScheduledJob>> {
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let max_jobs = u64::try_from(max_jobs).unwrap_or(u64::MAX);
        self.block_on_read(async move {
            let mut session = session.await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE run_at <= ? ORDER BY run_at, id LIMIT ?",
                Self::ReadSession::table(&tenant_schema, SCHEDULED_JOBS_TABLE)
            );
            session
                .fetch_all(&query, &[SqlValue::Bound(now.0), SqlValue::Bound(max_jobs)])
                .await?
                .into_iter()
                .map(|row| deserialize_json::<ScheduledJob>(&row.text(0)?))
                .collect()
        })
    }

    fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            let mut session = session.await?;
            let query = format!(
                "SELECT data_json FROM {} ORDER BY name",
                Self::ReadSession::table(&tenant_schema, CRON_JOBS_TABLE)
            );
            session
                .fetch_all(&query, &[])
                .await?
                .into_iter()
                .map(|row| deserialize_json::<CronJob>(&row.text(0)?))
                .collect()
        })
    }

    fn get_cron_job(&self, name: &str) -> Result<Option<CronJob>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let name = name.to_string();
        self.block_on_read(async move {
            let mut session = session.await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE name = ?",
                Self::ReadSession::table(&tenant_schema, CRON_JOBS_TABLE)
            );
            load_optional_json(&mut session, &query, &name).await
        })
    }

    fn has_scheduled_work(&self) -> Result<bool> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            has_scheduled_work_in_session(&mut session.await?, &tenant_schema).await
        })
    }

    fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            let mut session = session.await?;
            let scheduled_jobs_query = format!(
                "SELECT MIN(run_at) FROM {}",
                Self::ReadSession::table(&tenant_schema, SCHEDULED_JOBS_TABLE)
            );
            let cron_jobs_query = format!(
                "SELECT MIN(next_run) FROM {} WHERE enabled = TRUE",
                Self::ReadSession::table(&tenant_schema, CRON_JOBS_TABLE)
            );
            let scheduled = load_optional_u64(&mut session, &scheduled_jobs_query).await?;
            let cron = load_optional_u64(&mut session, &cron_jobs_query).await?;
            Ok(match (scheduled, cron) {
                (Some(left), Some(right)) => Some(Timestamp(left.min(right))),
                (Some(value), None) | (None, Some(value)) => Some(Timestamp(value)),
                (None, None) => None,
            })
        })
    }
}

/// The journal records from `sequence` on, after the retention floor.
pub(crate) async fn load_durable_records_from_session<S: SqlReadSession>(
    session: &mut S,
    tenant_schema: &str,
    sequence: SequenceNumber,
) -> Result<Vec<TenantEventRecord>> {
    let latest_sequence = session.load_latest_sequence(tenant_schema).await?;
    let cursor_floor = session
        .load_durable_journal_cursor_floor(tenant_schema)
        .await?;
    let suffix_after = SequenceNumber(sequence.0.saturating_sub(1)).max(cursor_floor);
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence >= ? ORDER BY sequence",
        S::table(tenant_schema, COMMIT_LOG_TABLE)
    );
    let from = SequenceNumber(suffix_after.0.saturating_add(1));
    let records = session
        .fetch_all(&query, &[SqlValue::Sequence(from)])
        .await?
        .into_iter()
        .map(|row| deserialize_tenant_event_record(&row.bytes(0)?))
        .collect::<Result<Vec<_>>>()?;
    let latest_sequence = records
        .last()
        .map(|record| record.sequence)
        .unwrap_or_default()
        .max(latest_sequence);
    let authoritative_floor = session
        .load_durable_journal_cursor_floor(tenant_schema)
        .await?;
    validate_retention_after_page(suffix_after, authoritative_floor, "durable journal suffix")?;
    validate_contiguous_journal_page(suffix_after, records.as_slice(), latest_sequence, false)?;
    Ok(records)
}

/// One page of at most `limit` journal records after `after`.
pub(crate) async fn stream_durable_journal_from_session<S: SqlReadSession>(
    session: &mut S,
    tenant_schema: &str,
    after: SequenceNumber,
    limit: usize,
) -> Result<DurableJournalPage> {
    let latest_sequence = session.load_latest_sequence(tenant_schema).await?;
    let cursor_floor = session
        .load_durable_journal_cursor_floor(tenant_schema)
        .await?;
    validate_retention_after_page(after, cursor_floor, "durable journal cursor")?;
    if after.0 > latest_sequence.0 {
        return Err(Error::InvalidInput(format!(
            "journal cursor {} is ahead of the latest durable sequence {}",
            after.0, latest_sequence.0
        )));
    }

    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence > ? ORDER BY sequence LIMIT ?",
        S::table(tenant_schema, COMMIT_LOG_TABLE)
    );
    let fetch_limit = u64::try_from(limit.saturating_add(1)).unwrap_or(u64::MAX);
    let rows = session
        .fetch_all(
            &query,
            &[SqlValue::Sequence(after), SqlValue::Bound(fetch_limit)],
        )
        .await?;
    let mut records = Vec::with_capacity(limit);
    let mut has_more = false;
    let mut observed_latest_sequence = latest_sequence;
    for row in rows {
        let record = deserialize_tenant_event_record(&row.bytes(0)?)?;
        observed_latest_sequence = observed_latest_sequence.max(record.sequence);
        if records.len() == limit {
            has_more = true;
            break;
        }
        records.push(record);
    }
    let latest_sequence = observed_latest_sequence;

    let next_cursor = records
        .last()
        .map(|record| record.sequence)
        .unwrap_or(after);
    let authoritative_floor = session
        .load_durable_journal_cursor_floor(tenant_schema)
        .await?;
    validate_retention_after_page(after, authoritative_floor, "durable journal page")?;
    validate_contiguous_journal_page(after, records.as_slice(), latest_sequence, has_more)?;
    Ok(DurableJournalPage {
        records,
        next_cursor,
        latest_sequence,
        cursor_floor: authoritative_floor,
        has_more,
    })
}

/// Check a streamed page again after the read, against the floor that
/// retention may have advanced meanwhile.
pub(crate) fn finish_durable_journal_page(
    mut page: DurableJournalPage,
    after: SequenceNumber,
    authoritative_floor: SequenceNumber,
) -> Result<DurableJournalPage> {
    validate_retention_after_page(after, authoritative_floor, "durable journal page")?;
    page.cursor_floor = page.cursor_floor.max(authoritative_floor);
    Ok(page)
}

/// Every job in `table_name`. Pending jobs come in due order.
pub(crate) async fn load_scheduled_jobs_from_session<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    table_name: &str,
) -> Result<Vec<ScheduledJob>> {
    let order_by = if table_name == SCHEDULED_JOBS_TABLE {
        "run_at, id"
    } else {
        "id"
    };
    let query = format!(
        "SELECT data_json FROM {} ORDER BY {order_by}",
        S::table(tenant_schema, table_name)
    );
    session
        .fetch_all(&query, &[])
        .await?
        .into_iter()
        .map(|row| deserialize_json::<ScheduledJob>(&row.text(0)?))
        .collect()
}

pub(crate) async fn has_scheduled_work_in_session<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
) -> Result<bool> {
    // A disabled cron job still counts: the scheduler gates tenant
    // load on this answer, so filtering on `enabled` would leave a
    // tenant whose only cron job is disabled permanently unloaded and
    // unable to wake when the job is re-enabled. `enabled` belongs to
    // `next_scheduled_work_at`, which computes the next due instant.
    for table_name in [
        SCHEDULED_JOBS_TABLE,
        RUNNING_SCHEDULED_JOBS_TABLE,
        CRON_JOBS_TABLE,
    ] {
        let query = format!(
            "SELECT 1 FROM {} LIMIT 1",
            S::table(tenant_schema, table_name)
        );
        if session.fetch_optional(&query, &[]).await?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn load_optional_json<S: SqlSession, T: serde::de::DeserializeOwned>(
    session: &mut S,
    query: &str,
    key: &str,
) -> Result<Option<T>> {
    session
        .fetch_optional(query, &[SqlValue::Text(key)])
        .await?
        .map(|row| deserialize_json::<T>(&row.text(0)?))
        .transpose()
}

async fn load_optional_u64<S: SqlSession>(session: &mut S, query: &str) -> Result<Option<u64>> {
    Ok(session
        .fetch_optional(query, &[])
        .await?
        .map(|row| row.optional_u64(0))
        .transpose()?
        .flatten())
}

/// Re-exposes the [`SqlReadStore`] entry points as inherent methods, keeping
/// the public API of each store unchanged.
macro_rules! sql_read_store_facade {
    ($store:ty) => {
        impl $store {
            pub fn read_commit_log_from(
                &self,
                sequence: nimbus_core::SequenceNumber,
            ) -> nimbus_core::Result<Vec<nimbus_core::CommitEntry>> {
                <Self as crate::sql::read_store::SqlReadStore>::read_commit_log_from(self, sequence)
            }

            pub fn read_durable_journal_from(
                &self,
                sequence: nimbus_core::SequenceNumber,
            ) -> nimbus_core::Result<Vec<nimbus_core::TenantEventRecord>> {
                <Self as crate::sql::read_store::SqlReadStore>::load_durable_journal_from(
                    self, sequence,
                )
            }

            pub fn stream_durable_journal(
                &self,
                after: nimbus_core::SequenceNumber,
                limit: usize,
            ) -> nimbus_core::Result<crate::store::DurableJournalPage> {
                <Self as crate::sql::read_store::SqlReadStore>::stream_durable_journal(
                    self, after, limit,
                )
            }

            pub fn scheduled_execution_exists(
                &self,
                execution_id: &str,
            ) -> nimbus_core::Result<bool> {
                <Self as crate::sql::read_store::SqlReadStore>::scheduled_execution_exists(
                    self,
                    execution_id,
                )
            }

            pub fn get_scheduled_job_result(
                &self,
                job_id: &nimbus_core::DocumentId,
            ) -> nimbus_core::Result<Option<nimbus_core::ScheduledJobResult>> {
                <Self as crate::sql::read_store::SqlReadStore>::get_scheduled_job_result(
                    self, job_id,
                )
            }

            pub fn list_scheduled_jobs(
                &self,
            ) -> nimbus_core::Result<Vec<nimbus_core::ScheduledJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::list_scheduled_jobs(self)
            }

            pub fn get_pending_scheduled_job(
                &self,
                job_id: &nimbus_core::DocumentId,
            ) -> nimbus_core::Result<Option<nimbus_core::ScheduledJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::get_pending_scheduled_job(
                    self, job_id,
                )
            }

            pub fn list_running_scheduled_jobs(
                &self,
            ) -> nimbus_core::Result<Vec<nimbus_core::ScheduledJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::list_running_scheduled_jobs(self)
            }

            pub fn get_running_scheduled_job(
                &self,
                job_id: &nimbus_core::DocumentId,
            ) -> nimbus_core::Result<Option<nimbus_core::ScheduledJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::get_running_scheduled_job(
                    self, job_id,
                )
            }

            pub fn peek_due_scheduled_jobs(
                &self,
                now: nimbus_core::Timestamp,
                max_jobs: usize,
            ) -> nimbus_core::Result<Vec<nimbus_core::ScheduledJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::peek_due_scheduled_jobs(
                    self, now, max_jobs,
                )
            }

            pub fn load_cron_jobs(&self) -> nimbus_core::Result<Vec<nimbus_core::CronJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::load_cron_jobs(self)
            }

            pub fn get_cron_job(
                &self,
                name: &str,
            ) -> nimbus_core::Result<Option<nimbus_core::CronJob>> {
                <Self as crate::sql::read_store::SqlReadStore>::get_cron_job(self, name)
            }

            pub fn has_scheduled_work(&self) -> nimbus_core::Result<bool> {
                <Self as crate::sql::read_store::SqlReadStore>::has_scheduled_work(self)
            }

            pub fn next_scheduled_work_at(
                &self,
            ) -> nimbus_core::Result<Option<nimbus_core::Timestamp>> {
                <Self as crate::sql::read_store::SqlReadStore>::next_scheduled_work_at(self)
            }
        }
    };
}

pub(crate) use sql_read_store_facade;
