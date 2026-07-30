use super::resource_paths::load_resource_path_bindings_from_session;
use super::*;
use crate::IndexRangeBound;
use crate::range_bound::{borrow_index_range_bound, clone_index_range_bound};

pub(super) async fn has_scheduled_work_from_provider(
    provider: PostgresProvider,
    schema_name: String,
) -> Result<bool> {
    let client = provider.client().await?;
    Ok(
        table_has_rows_in_session(&client, &schema_name, "scheduled_jobs").await?
            || table_has_rows_in_session(&client, &schema_name, "running_scheduled_jobs").await?
            || table_has_rows_in_session(&client, &schema_name, "cron_jobs").await?,
    )
}

impl PostgresTenantStore {
    pub fn load_schema(&self) -> Result<Schema> {
        if let Some(schema) = cached_schema(&self.schema_cache) {
            return Ok(schema);
        }
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let schema = self.block_on(async move {
            let client = provider.client().await?;
            load_schema_from_session(&client, &schema_name).await
        })?;
        publish_schema_cache(&self.schema_cache, &schema);
        Ok(schema)
    }

    pub async fn load_schema_async(&self) -> Result<Schema> {
        if let Some(schema) = cached_schema(&self.schema_cache) {
            return Ok(schema);
        }
        let client = self.provider.client().await?;
        let schema = load_schema_from_session(&client, &self.schema_name).await?;
        publish_schema_cache(&self.schema_cache, &schema);
        Ok(schema)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.journal_progress()?.durable_head)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.journal_progress()?.applied_head)
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_journal_progress_from_session(&client, &schema_name).await
        })
    }

    pub async fn journal_progress_async(&self) -> Result<JournalProgress> {
        let client = self.provider.client().await?;
        load_journal_progress_from_session(&client, &self.schema_name).await
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from(from)?;
        self.apply_durable_records_batch(&pending)?;
        self.journal_progress()
    }

    pub async fn recover_durable_journal_async(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress_async().await?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from_async(from).await?;
        let store = self.clone();
        self.provider
            .runtime_handle
            .spawn_blocking(move || store.apply_durable_records_batch(&pending))
            .await
            .map_err(|error| map_executor_join_error(POSTGRES_EXECUTOR_CONTEXT, error))??;
        self.journal_progress_async().await
    }

    pub fn read_snapshot(&self) -> Result<PostgresReadSnapshot> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let mut client = provider.client().await?;
            let transaction = client
                .build_transaction()
                .read_only(true)
                .isolation_level(IsolationLevel::RepeatableRead)
                .start()
                .await
                .map_err(map_postgres_error)?;
            let schema = load_schema_from_session(&transaction, &schema_name).await?;
            let progress = load_journal_progress_from_session(&transaction, &schema_name).await?;
            let table_identities =
                load_table_identities_from_session(&transaction, &schema_name).await?;
            let documents = load_documents_from_session(&transaction, &schema_name, None).await?;
            let resource_path_bindings =
                load_resource_path_bindings_from_session(&transaction, &schema_name).await?;
            let scheduled_execution_ids =
                load_scheduled_execution_ids_from_session(&transaction, &schema_name).await?;
            transaction.commit().await.map_err(map_postgres_error)?;
            Ok(PostgresReadSnapshot {
                schema,
                progress,
                table_identities,
                documents,
                resource_path_bindings,
                scheduled_execution_ids,
            })
        })
    }

    pub fn table_identity_diagnostics(&self) -> Result<Vec<crate::TableIdentityDiagnostic>> {
        self.read_snapshot()?
            .table_identity_diagnostics(crate::TableBackendLayout::SharedDocumentsByTableId)
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let id = id.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_document_from_session(&client, &schema_name, &table, &id).await
        })
    }

    pub fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_table_id_from_session(&client, &schema_name, &table).await
        })
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
        self.scan_table_matching_with_filters_cancellable(
            table,
            &[],
            check_cancel,
            include_document,
        )
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let documents = self.load_table_documents(table)?;
        filter_documents_with_predicate(documents, filters, check_cancel, include_document)
    }

    pub fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let id_prefix = id_prefix.to_owned();
        let documents = self.block_on(async move {
            let client = provider.client().await?;
            load_documents_by_id_prefix_from_session(&client, &schema_name, &table, &id_prefix)
                .await
        })?;
        filter_documents_with_predicate(documents, &[], check_cancel, |_| Ok(true))
    }

    pub fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let start_id = start_id.to_owned();
        let documents = self.block_on(async move {
            let client = provider.client().await?;
            load_documents_starting_at_id_from_session(
                &client,
                &schema_name,
                &table,
                &start_id,
                limit,
            )
            .await
        })?;
        filter_documents_with_predicate(documents, &[], check_cancel, |_| Ok(true))
    }

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub async fn read_commit_log_from_async(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from_async(sequence)
            .await?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_durable_records_from_session(&client, &schema_name, sequence).await
        })
    }

    pub async fn read_durable_journal_from_async(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        let client = self.provider.client().await?;
        load_durable_records_from_session(&client, &self.schema_name, sequence).await
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;

        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            stream_durable_journal_from_session(&client, &schema_name, after, limit).await
        })
    }

    pub async fn stream_durable_journal_async(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;

        let client = self.provider.client().await?;
        stream_durable_journal_from_session(&client, &self.schema_name, after, limit).await
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let snapshot = self
            .read_snapshot()?
            .export_materialized_journal_snapshot()?;
        let cursor_floor = self.durable_journal_cursor_floor()?;
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor,
        })
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        self.read_snapshot()?.export_materialized_journal_snapshot()
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let execution_id = execution_id.to_string();
        self.block_on(async move {
            let client = provider.client().await?;
            let query = format!(
                "SELECT 1 FROM {} WHERE execution_id = $1",
                qualified_table(&schema_name, "scheduled_job_executions")
            );
            client
                .query_opt(query.as_str(), &[&execution_id])
                .await
                .map(|row| row.is_some())
                .map_err(map_postgres_error)
        })
    }

    pub fn get_scheduled_job_result(
        &self,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJobResult>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let job_id = job_id.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_scheduled_job_result_from_session(&client, &schema_name, &job_id).await
        })
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_scheduled_jobs_from_session(&client, &schema_name, "scheduled_jobs").await
        })
    }

    pub fn get_pending_scheduled_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>> {
        self.load_scheduler_job_by_id("scheduled_jobs", job_id)
    }

    pub fn list_running_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_scheduled_jobs_from_session(&client, &schema_name, "running_scheduled_jobs").await
        })
    }

    pub fn get_running_scheduled_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>> {
        self.load_scheduler_job_by_id("running_scheduled_jobs", job_id)
    }

    fn load_scheduler_job_by_id(
        &self,
        table_name: &str,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJob>> {
        debug_assert!(matches!(
            table_name,
            "scheduled_jobs" | "running_scheduled_jobs"
        ));
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table_name = table_name.to_string();
        let job_id = job_id.to_string();
        self.block_on(async move {
            let client = provider.client().await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE id = $1",
                qualified_table(&schema_name, &table_name)
            );
            client
                .query_opt(query.as_str(), &[&job_id])
                .await
                .map_err(map_postgres_error)?
                .map(|row| deserialize_json::<ScheduledJob>(row.get::<_, String>(0).as_str()))
                .transpose()
        })
    }

    pub fn peek_due_scheduled_jobs(
        &self,
        now: Timestamp,
        max_jobs: usize,
    ) -> Result<Vec<ScheduledJob>> {
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let max_jobs = i64::try_from(max_jobs).unwrap_or(i64::MAX);
        self.block_on(async move {
            let client = provider.client().await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE run_at <= $1 ORDER BY run_at, id LIMIT $2",
                qualified_table(&schema_name, "scheduled_jobs")
            );
            let run_at = claim_due_jobs_upper_bound(now);
            client
                .query(query.as_str(), &[&run_at, &max_jobs])
                .await
                .map_err(map_postgres_error)?
                .into_iter()
                .map(|row| deserialize_json::<ScheduledJob>(row.get::<_, String>(0).as_str()))
                .collect()
        })
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_cron_jobs_from_session(&client, &schema_name).await
        })
    }

    pub fn get_cron_job(&self, name: &str) -> Result<Option<CronJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let name = name.to_string();
        self.block_on(async move {
            let client = provider.client().await?;
            let query = format!(
                "SELECT data_json FROM {} WHERE name = $1",
                qualified_table(&schema_name, "cron_jobs")
            );
            client
                .query_opt(query.as_str(), &[&name])
                .await
                .map_err(map_postgres_error)?
                .map(|row| deserialize_json::<CronJob>(row.get::<_, String>(0).as_str()))
                .transpose()
        })
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
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move { has_scheduled_work_from_provider(provider, schema_name).await })
    }

    pub async fn has_scheduled_work_async(&self) -> Result<bool> {
        has_scheduled_work_from_provider(self.provider.clone(), self.schema_name.clone()).await
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(
            table,
            index_name,
            std::slice::from_ref(value),
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
            table,
            index_name,
            prefix_values,
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            check_cancel,
        )
    }

    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(table, index_name, &[], start, end, check_cancel)
    }

    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            check_cancel,
        )
    }

    fn load_table_documents(&self, table: &TableName) -> Result<Vec<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_documents_from_session(&client, &schema_name, Some(&table)).await
        })
    }

    fn load_table_schema(&self, table: &TableName) -> Result<TableSchema> {
        self.load_schema()?
            .get_table(table)
            .cloned()
            .ok_or(Error::SchemaNotFound(table.clone()))
    }

    fn load_index_documents_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let table_schema = self.load_table_schema(table)?;
        let index_fields = index_fields_for_table_schema(&table_schema, index_name)?;
        validate_index_prefix_len(index_name, exact_prefix.len(), index_fields.len())?;
        validate_index_range_prefix(
            index_name,
            exact_prefix.len(),
            index_fields.len(),
            start,
            end,
        )?;

        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table_for_query = table.clone();
        let table_for_filter = table.clone();
        let table_schema_for_query = table_schema.clone();
        let exact_prefix = exact_prefix.to_vec();
        let exact_prefix_for_query = exact_prefix.clone();
        let start = clone_index_range_bound(start);
        let end = clone_index_range_bound(end);
        let bounds_for_query = crate::range_bound::OwnedIndexRangeBounds {
            start: start.clone(),
            end: end.clone(),
        };
        let index_name = index_name.to_string();
        let documents = self.block_on(async move {
            let client = provider.client().await?;
            load_index_candidate_documents_from_session(
                &client,
                &schema_name,
                &table_for_query,
                &table_schema_for_query,
                index_name.as_str(),
                &exact_prefix_for_query,
                bounds_for_query,
            )
            .await
        })?;

        filter_index_documents_with_cancel(
            documents,
            &table_for_filter,
            &index_fields,
            &exact_prefix,
            borrow_index_range_bound(&start),
            borrow_index_range_bound(&end),
            check_cancel,
        )
    }

    fn durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_durable_journal_cursor_floor_from_session(&client, &schema_name).await
        })
    }
}
