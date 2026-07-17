use super::*;
use crate::IndexRangeBound;
use crate::index::history_scan::HistoricalIndexPageRequest;

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

    pub fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        let table = table.clone();
        self.block_on(async move {
            let conn = self.remote_connection()?;
            load_remote_table_id_from_session(&conn, &table).await
        })
    }

    pub fn table_identity_diagnostics(&self) -> Result<Vec<crate::TableIdentityDiagnostic>> {
        let snapshot = self.current_query_cache_store()?.read_snapshot()?;
        let mut diagnostics = Vec::new();
        for identity in snapshot.table_identities()? {
            let document_count = if identity.namespace
                == crate::table_identity::DEFAULT_TABLE_NAMESPACE
                && identity.state == TableState::Active
            {
                let mut check_cancel = || Ok(());
                Some(
                    snapshot
                        .scan_table_matching_with_filters_cancellable(
                            &identity.table,
                            &[],
                            &mut check_cancel,
                            |_| Ok(true),
                        )?
                        .len() as u64,
                )
            } else {
                None
            };
            diagnostics.push(crate::TableIdentityDiagnostic::from_snapshot_entry(
                &identity,
                crate::TableBackendLayout::LibsqlReplicaSharedDocumentsByTableId,
                document_count,
            ));
        }
        Ok(diagnostics)
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
        let mut observed_remote_head = progress.durable_head;
        if progress.applied_head.0 < progress.durable_head.0 {
            let next_sequence = SequenceNumber(progress.applied_head.0.saturating_add(1));
            let records = self.read_durable_journal_from(next_sequence)?;
            if !records.is_empty() {
                let applied_head =
                    self.block_on(self.apply_remote_durable_records_batch(records.as_slice()))?;
                observed_remote_head = observed_remote_head.max(applied_head);
            }
        }
        self.note_recovered_remote_progress(observed_remote_head);
        self.ensure_local_cache_current()?;
        let recovered = self.journal_progress()?;
        Ok(self.retain_recovered_progress(recovered))
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
    ) -> Result<Vec<TenantEventRecord>> {
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

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        self.freshness_metrics
            .note_refresh_request(LibsqlReplicaRefreshCause::BootstrapExport);
        self.refresh_local_cache()?;
        self.active_cache_store()?
            .export_materialized_journal_snapshot()
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
        filters: &[nimbus_core::Filter],
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

    pub fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .scan_table_id_prefix_cancellable(table, id_prefix, check_cancel)
    }

    pub fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .scan_table_id_starting_at_cancellable(table, start_id, limit, check_cancel)
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

    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_range_cancellable(table, index_name, start, end, check_cancel)
    }

    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.current_query_cache_store()?
            .index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                check_cancel,
            )
    }

    pub fn historical_index_scan_eq_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.historical_index_scan_eq_page_cancellable(
            read_shape,
            index_name,
            value,
            None,
            usize::MAX,
            check_cancel,
        )
        .map(|page| page.documents)
    }

    pub fn historical_index_scan_eq_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &serde_json::Value,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        self.current_query_cache_store()?
            .read_snapshot()?
            .historical_index_scan_eq_page_cancellable(
                read_shape,
                index_name,
                value,
                after,
                limit,
                check_cancel,
            )
    }

    pub fn historical_index_scan_prefix_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.historical_index_scan_prefix_page_cancellable(
            read_shape,
            index_name,
            prefix_values,
            None,
            usize::MAX,
            check_cancel,
        )
        .map(|page| page.documents)
    }

    pub fn historical_index_scan_prefix_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        self.current_query_cache_store()?
            .read_snapshot()?
            .historical_index_scan_prefix_page_cancellable(
                read_shape,
                index_name,
                prefix_values,
                after,
                limit,
                check_cancel,
            )
    }

    pub fn historical_index_scan_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.historical_index_scan_range_page_cancellable(
            read_shape,
            index_name,
            start,
            end,
            HistoricalIndexPageRequest {
                after: None,
                limit: usize::MAX,
                check_cancel,
            },
        )
        .map(|page| page.documents)
    }

    pub(crate) fn historical_index_scan_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        self.current_query_cache_store()?
            .read_snapshot()?
            .historical_index_scan_range_page_cancellable(read_shape, index_name, start, end, page)
    }

    pub fn historical_index_scan_composite_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.historical_index_scan_composite_range_page_cancellable(
            read_shape,
            index_name,
            exact_prefix,
            start,
            end,
            HistoricalIndexPageRequest {
                after: None,
                limit: usize::MAX,
                check_cancel,
            },
        )
        .map(|page| page.documents)
    }

    pub(crate) fn historical_index_scan_composite_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        self.current_query_cache_store()?
            .read_snapshot()?
            .historical_index_scan_composite_range_page_cancellable(
                read_shape,
                index_name,
                exact_prefix,
                start,
                end,
                page,
            )
    }
}
