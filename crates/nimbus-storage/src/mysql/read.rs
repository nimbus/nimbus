use super::dialect::{MySqlHandle, MySqlSession};
use super::index_entries::load_index_candidate_documents_from_session;
use super::*;
use crate::IndexRangeBound;
use crate::range_bound::{borrow_index_range_bound, clone_index_range_bound};
use crate::retention::validate_retention_after_page;
use crate::sql::read_store::{SqlReadSession, SqlReadStore, sql_read_store_facade};
use crate::sql::resource_paths::load_resource_path_bindings_from_session;

impl<H: MySqlHandle> SqlReadSession for MySqlSession<H> {
    async fn load_latest_sequence(&mut self, database_name: &str) -> Result<SequenceNumber> {
        load_latest_sequence_from_session(self.driver(), database_name).await
    }

    async fn load_durable_journal_cursor_floor(
        &mut self,
        database_name: &str,
    ) -> Result<SequenceNumber> {
        load_durable_journal_cursor_floor_from_session(self.driver(), database_name).await
    }
}

impl SqlReadStore for MySqlTenantStore {
    fn validate_journal_stream_limit(limit: usize) -> Result<()> {
        validate_durable_journal_stream_limit(limit)
    }
}

sql_read_store_facade!(MySqlTenantStore);

impl MySqlTenantStore {
    pub fn load_schema(&self) -> Result<Schema> {
        if let Some(schema) = cached_schema(&self.schema_cache) {
            return Ok(schema);
        }
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let schema = self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_schema_from_session(&mut conn, &database_name).await
        })?;
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
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_journal_progress_from_session(&mut conn, &database_name).await
        })
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from(from)?;
        self.replay_durable_records_batch(&pending)?;
        self.journal_progress()
    }

    pub fn read_snapshot(&self) -> Result<MySqlReadSnapshot> {
        Ok(self.read_snapshot_with_journal_floor()?.0)
    }

    /// Reads the snapshot together with the durable journal cursor floor,
    /// captured inside the same `REPEATABLE READ` transaction so the pair is
    /// consistent. Journal bootstrap needs both; every other read drops the
    /// floor through [`Self::read_snapshot`].
    fn read_snapshot_with_journal_floor(&self) -> Result<(MySqlReadSnapshot, SequenceNumber)> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .await
                .map_err(map_mysql_error)?;
            let mut transaction = conn
                .start_transaction(mysql_async::TxOpts::default())
                .await
                .map_err(map_mysql_error)?;
            let schema = load_schema_from_session(&mut transaction, &database_name).await?;
            let progress =
                load_journal_progress_from_session(&mut transaction, &database_name).await?;
            let journal_cursor_floor =
                load_durable_journal_cursor_floor_from_session(&mut transaction, &database_name)
                    .await?;
            let table_identities =
                load_table_identities_from_session(&mut transaction, &database_name).await?;
            let documents =
                load_documents_from_session(&mut transaction, &database_name, None).await?;
            let resource_path_bindings = load_resource_path_bindings_from_session(
                &mut MySqlSession(&mut transaction),
                &database_name,
            )
            .await?;
            let scheduled_execution_ids =
                load_scheduled_execution_ids_from_session(&mut transaction, &database_name).await?;
            let trigger_delivery_cursor = load_metadata_u64_from_session(
                &mut transaction,
                &database_name,
                TRIGGER_DELIVERY_CURSOR_KEY,
            )
            .await?
            .map(SequenceNumber)
            .map(TriggerDeliveryCursor::new)
            .unwrap_or_default();
            transaction.commit().await.map_err(map_mysql_error)?;
            Ok((
                MySqlReadSnapshot {
                    schema,
                    progress,
                    table_identities,
                    documents,
                    resource_path_bindings,
                    scheduled_execution_ids,
                    trigger_delivery_cursor,
                },
                journal_cursor_floor,
            ))
        })
    }

    pub fn table_identity_diagnostics(&self) -> Result<Vec<crate::TableIdentityDiagnostic>> {
        self.read_snapshot()?
            .table_identity_diagnostics(crate::TableBackendLayout::SharedDocumentsByTableId)
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let id = id.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_document_from_session(&mut conn, &database_name, &table, &id).await
        })
    }

    pub fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_table_id_from_session(&mut conn, &database_name, &table).await
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
        let database_name = self.database_name.clone();
        let table = table.clone();
        let id_prefix = id_prefix.to_owned();
        let documents = self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_documents_by_id_prefix_from_session(&mut conn, &database_name, &table, &id_prefix)
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
        let database_name = self.database_name.clone();
        let table = table.clone();
        let start_id = start_id.to_owned();
        let documents = self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_documents_starting_at_id_from_session(
                &mut conn,
                &database_name,
                &table,
                &start_id,
                limit,
            )
            .await
        })?;
        filter_documents_with_predicate(documents, &[], check_cancel, |_| Ok(true))
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
        let database_name = self.database_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_documents_from_session(&mut conn, &database_name, Some(&table)).await
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
        let database_name = self.database_name.clone();
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
            let mut conn = provider.conn().await?;
            load_index_candidate_documents_from_session(
                &mut conn,
                &database_name,
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

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let (read_snapshot, initial_floor) = self.read_snapshot_with_journal_floor()?;
        let snapshot = read_snapshot.export_materialized_journal_snapshot()?;
        drop(read_snapshot);
        let authoritative_floor = self.durable_journal_cursor_floor()?;
        let cursor_floor = initial_floor
            .max(authoritative_floor)
            .max(self.retention_floor.published_read_floors().journal);
        validate_retention_after_page(
            snapshot.applied_sequence,
            cursor_floor,
            "durable journal bootstrap",
        )?;
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
}
