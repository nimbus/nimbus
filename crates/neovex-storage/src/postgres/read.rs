use super::*;

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
            .map_err(map_join_error)??;
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
            let documents = load_documents_from_session(&transaction, &schema_name, None).await?;
            let scheduled_execution_ids =
                load_scheduled_execution_ids_from_session(&transaction, &schema_name).await?;
            transaction.commit().await.map_err(map_postgres_error)?;
            Ok(PostgresReadSnapshot {
                schema,
                progress,
                documents,
                scheduled_execution_ids,
            })
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let id = *id;
        self.block_on(async move {
            let client = provider.client().await?;
            load_document_from_session(&client, &schema_name, &table, &id).await
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
    ) -> Result<Vec<DurableMutationRecord>> {
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
    ) -> Result<Vec<DurableMutationRecord>> {
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
        let job_id = *job_id;
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

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_cron_jobs_from_session(&client, &schema_name).await
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
            None,
            None,
            true,
            true,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
            table,
            index_name,
            &[],
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
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
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

    #[allow(clippy::too_many_arguments)]
    fn load_index_documents_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let table_schema = self.load_table_schema(table)?;
        let index_fields = index_fields_for_table_schema(&table_schema, index_name)?;
        if exact_prefix.len() > index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "index prefix length {} exceeds index '{}' field count {}",
                exact_prefix.len(),
                index_name,
                index_fields.len()
            )));
        }
        if (start.is_some() || end.is_some()) && exact_prefix.len() >= index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} leaves no range field for index '{}'",
                exact_prefix.len(),
                index_name
            )));
        }

        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table_for_query = table.clone();
        let table_for_filter = table.clone();
        let table_schema_for_query = table_schema.clone();
        let exact_prefix = exact_prefix.to_vec();
        let exact_prefix_for_query = exact_prefix.clone();
        let start = start.cloned();
        let start_for_query = start.clone();
        let end = end.cloned();
        let end_for_query = end.clone();
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
                start_for_query.as_ref(),
                end_for_query.as_ref(),
                start_inclusive,
                end_inclusive,
            )
            .await
        })?;

        filter_index_documents_with_cancel(
            documents,
            &table_for_filter,
            &index_fields,
            &exact_prefix,
            start.as_ref(),
            end.as_ref(),
            start_inclusive,
            end_inclusive,
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

impl PostgresReadSnapshot {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self.schema.clone())
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.progress.durable_head)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.progress.applied_head)
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(self.progress)
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        Ok(MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: self.progress.applied_head,
            durable_head: self.progress.durable_head,
            schema: self.schema.clone(),
            documents: self.documents.clone(),
            scheduled_execution_ids: self.scheduled_execution_ids.clone(),
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        Ok(self
            .documents
            .iter()
            .find(|document| &document.table == table && &document.id == id)
            .cloned())
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
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if matches_filters(document, filters)? && include_document(document)? {
                documents.push(document.clone());
            }
        }
        Ok(documents)
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
        let index_fields = self.index_fields(table, index_name)?;
        if prefix_values.len() > index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "index prefix length {} exceeds index '{}' field count {}",
                prefix_values.len(),
                index_name,
                index_fields.len()
            )));
        }
        self.filter_index_documents(
            table,
            &index_fields,
            prefix_values,
            None,
            None,
            true,
            true,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        self.filter_index_documents(
            table,
            &index_fields,
            &[],
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
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        if exact_prefix.len() >= index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} leaves no range field for index '{}'",
                exact_prefix.len(),
                index_name
            )));
        }
        self.filter_index_documents(
            table,
            &index_fields,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_fields(&self, table: &TableName, index_name: &str) -> Result<Vec<String>> {
        let table_schema = self
            .schema
            .get_table(table)
            .ok_or_else(|| Error::SchemaNotFound(table.clone()))?;
        let index = table_schema
            .indexes
            .iter()
            .find(|index| index.name == index_name)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "index '{}' not found for table '{}'",
                    index_name,
                    table.as_str()
                ))
            })?;
        Ok(index.fields.clone())
    }

    #[allow(clippy::too_many_arguments)]
    fn filter_index_documents(
        &self,
        table: &TableName,
        index_fields: &[String],
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let range_field = index_fields.get(exact_prefix.len());
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if !document_matches_exact_prefix(document, index_fields, exact_prefix) {
                continue;
            }
            if let Some(range_field) = range_field
                && !document_matches_range_bounds(
                    document,
                    range_field,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                )?
            {
                continue;
            }
            documents.push(document.clone());
        }
        Ok(documents)
    }
}
