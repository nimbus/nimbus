use super::*;

impl LibsqlReplicaTenantStore {
    pub fn read_snapshot(&self) -> Result<SqliteReadSnapshot> {
        let store = self.current_query_cache_store()?;
        store.read_snapshot()
    }

    pub fn load_schema(&self) -> Result<Schema> {
        let remote_schema = self.block_on(self.load_remote_schema())?;
        let local_schema = self.active_cache_store()?.load_schema()?;
        if local_schema != remote_schema {
            self.refresh_needed.store(true, Ordering::Release);
            self.freshness_metrics
                .note_refresh_request(LibsqlReplicaRefreshCause::SchemaMismatch);
            self.schedule_background_refresh();
        }
        Ok(remote_schema)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        self.block_on(self.load_remote_latest_sequence())
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        self.active_cache_store()?.applied_sequence()
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(JournalProgress {
            durable_head: self.latest_sequence()?,
            applied_head: self.applied_sequence()?,
        })
    }

    pub fn replica_freshness_stats(&self) -> Result<LibsqlReplicaFreshnessStats> {
        let required_sequence =
            SequenceNumber(self.required_cache_sequence.load(Ordering::Acquire));
        let local_progress = self.active_cache_store()?.journal_progress()?;
        Ok(self.freshness_metrics.snapshot(
            required_sequence,
            local_progress,
            self.refresh_needed.load(Ordering::Acquire),
            self.refresh_requested.load(Ordering::Acquire),
            self.refresh_inflight.load(Ordering::Acquire),
        ))
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 < progress.durable_head.0 {
            let next_sequence = SequenceNumber(progress.applied_head.0.saturating_add(1));
            let records = self.read_durable_journal_from(next_sequence)?;
            if !records.is_empty() {
                let applied_head =
                    self.block_on(self.apply_remote_durable_records_batch(records.as_slice()))?;
                self.note_required_cache_sequence_with_cause(
                    applied_head,
                    LibsqlReplicaRefreshCause::DurableJournalReplay,
                );
            } else {
                self.note_required_cache_sequence_with_cause(
                    progress.durable_head,
                    LibsqlReplicaRefreshCause::DurableJournalReplay,
                );
            }
        }
        self.ensure_local_cache_current()?;
        self.journal_progress()
    }

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        self.block_on(self.load_remote_durable_records_from(sequence))
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;
        self.block_on(self.load_remote_durable_journal_page(after, limit))
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        self.freshness_metrics
            .note_refresh_request(LibsqlReplicaRefreshCause::BootstrapExport);
        self.refresh_local_cache()?;
        self.active_cache_store()?
            .export_durable_journal_bootstrap()
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let execution_id = execution_id.to_string();
        self.block_on(async move {
            let conn = self.remote_connection()?;
            let mut rows = conn
                .query(
                    "SELECT 1 FROM scheduled_job_executions WHERE execution_id = ?1",
                    libsql::params![execution_id],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
        })
    }

    pub fn get_scheduled_job_result(
        &self,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJobResult>> {
        let job_id = job_id.to_string();
        self.block_on(async move {
            let conn = self.remote_connection()?;
            let mut rows = conn
                .query(
                    "SELECT data_json FROM scheduled_job_results WHERE job_id = ?1",
                    libsql::params![job_id],
                )
                .await
                .map_err(map_libsql_error)?;
            let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
                return Ok(None);
            };
            let json = row.get::<String>(0).map_err(map_libsql_error)?;
            Ok(Some(deserialize_json(json.as_str())?))
        })
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        self.block_on(self.load_remote_scheduled_jobs("scheduled_jobs"))
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        self.block_on(self.load_remote_cron_jobs())
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let next_job_at = self.list_scheduled_jobs()?.first().map(|job| job.run_at);
        let next_cron_at = self
            .load_cron_jobs()?
            .into_iter()
            .filter(|cron| cron.enabled)
            .map(|cron| cron.next_run)
            .min();
        Ok(match (next_job_at, next_cron_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        })
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        self.block_on(async move {
            let conn = self.remote_connection()?;
            Ok(table_has_entries_remote(&conn, "scheduled_jobs").await?
                || table_has_entries_remote(&conn, "running_scheduled_jobs").await?
                || table_has_entries_remote(&conn, "cron_jobs").await?)
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.current_query_cache_store()?.get(table, id)
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.current_query_cache_store()?
            .scan_table_matching_cancellable(table, check_cancel, include_document)
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[neovex_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.current_query_cache_store()?
            .scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?.index_scan_eq_cancellable(
            table,
            index_name,
            value,
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            )
    }
}
