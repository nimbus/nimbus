use super::*;

impl SqliteTenantStore {
    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.read_snapshot()?.get(table, id)
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
        self.read_snapshot()?
            .scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
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

    pub fn index_scan_eq(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
    ) -> Result<Vec<Document>> {
        self.index_scan_eq_cancellable(table, index_name, value, &mut || Ok(()))
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?
            .index_scan_eq_cancellable(table, index_name, value, check_cancel)
    }

    pub fn index_scan_prefix(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(table, index_name, prefix_values, &mut || Ok(()))
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?.index_scan_prefix_cancellable(
            table,
            index_name,
            prefix_values,
            check_cancel,
        )
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
        self.read_snapshot()?.index_scan_range_cancellable(
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
        self.read_snapshot()?
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

    pub fn scan_table(&self, table: &TableName) -> Result<Vec<Document>> {
        self.scan_table_cancellable(table, &mut || Ok(()))
    }

    pub fn scan_table_cancellable(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.scan_table_matching_with_filters_cancellable(table, &[], check_cancel, |_| Ok(true))
    }
}

impl SqliteReadSnapshot {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self
            .schema_cache
            .read()
            .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))?
            .clone())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT record_blob
                 FROM commit_log
                 WHERE sequence >= ?1
                 ORDER BY sequence",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query(params![sequence.0]).map_err(map_sqlite_error)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let payload: Vec<u8> = row.get(0).map_err(map_sqlite_error)?;
            records.push(deserialize_durable_record(payload.as_slice())?);
        }
        Ok(records)
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;

        let latest_sequence = self.latest_sequence()?;
        let cursor_floor = self.durable_journal_cursor_floor()?;
        if after.0 < cursor_floor.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is behind the retention floor {}",
                after.0, cursor_floor.0
            )));
        }
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT record_blob
                 FROM commit_log
                 WHERE sequence > ?1
                 ORDER BY sequence
                 LIMIT ?2",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(params![
                after.0,
                u64::try_from(limit.saturating_add(1)).unwrap_or(u64::MAX)
            ])
            .map_err(map_sqlite_error)?;
        let mut records = Vec::with_capacity(limit);
        let mut has_more = false;
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let payload: Vec<u8> = row.get(0).map_err(map_sqlite_error)?;
            if records.len() == limit {
                has_more = true;
                break;
            }
            records.push(deserialize_durable_record(payload.as_slice())?);
        }

        let next_cursor = records
            .last()
            .map(|record| record.sequence)
            .unwrap_or(after);
        Ok(DurableJournalPage {
            records,
            next_cursor,
            latest_sequence,
            cursor_floor,
            has_more,
        })
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let snapshot = self.export_materialized_journal_snapshot()?;
        let cursor_floor = self.durable_journal_cursor_floor()?;
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor,
        })
    }

    pub fn metadata_blob(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sqlite_error)
    }

    pub fn journal_mode(&self) -> Result<String> {
        let mode = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .map_err(map_sqlite_error)?;
        Ok(mode.to_ascii_lowercase())
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(SequenceNumber(
            next_sequence_in_conn(&self.conn)?.saturating_sub(1),
        ))
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(SequenceNumber(
            self.metadata_blob(APPLIED_SEQUENCE_KEY)?
                .map(|bytes| decode_u64(bytes.as_slice()))
                .transpose()?
                .unwrap_or(0),
        ))
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(JournalProgress {
            durable_head: self.latest_sequence()?,
            applied_head: self.applied_sequence()?,
        })
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        let progress = self.journal_progress()?;
        Ok(MaterializedJournalSnapshot {
            version: 1,
            applied_sequence: progress.applied_head,
            durable_head: progress.durable_head,
            schema: self.load_schema()?,
            documents: self.documents()?,
            scheduled_execution_ids: self.scheduled_execution_ids()?,
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT creation_time, update_time, data_json, typed_fields_json
                 FROM documents
                 WHERE table_name = ?1 AND id = ?2",
                params![table.as_str(), id.to_string()],
                |row| {
                    Ok(row_to_document(
                        table,
                        id,
                        row.get(0)?,
                        row.get(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?
            .transpose()
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM scheduled_job_executions WHERE execution_id = ?1",
                params![execution_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .is_some())
    }

    pub fn documents(&self) -> Result<Vec<Document>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json
                 FROM documents
                 ORDER BY table_name, id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            let table_name = row.get::<_, String>(0).map_err(map_sqlite_error)?;
            let table = TableName::new(table_name)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let id =
                DocumentId::from_str(row.get::<_, String>(1).map_err(map_sqlite_error)?.as_str())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
            documents.push(row_to_document(
                &table,
                &id,
                row.get(2).map_err(map_sqlite_error)?,
                row.get(3).map_err(map_sqlite_error)?,
                row.get::<_, String>(4).map_err(map_sqlite_error)?,
                row.get::<_, String>(5).map_err(map_sqlite_error)?,
            )?);
        }
        Ok(documents)
    }

    pub fn scheduled_execution_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT execution_id
                 FROM scheduled_job_executions
                 ORDER BY execution_id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut execution_ids = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            execution_ids.push(row.get::<_, String>(0).map_err(map_sqlite_error)?);
        }
        Ok(execution_ids)
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        _filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, creation_time, update_time, data_json, typed_fields_json
                 FROM documents
                 WHERE table_name = ?1
                 ORDER BY id",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(params![table.as_str()])
            .map_err(map_sqlite_error)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            check_cancel()?;
            let id = row.get::<_, String>(0).map_err(map_sqlite_error)?;
            let document = row_to_document(
                table,
                &DocumentId::from_str(id.as_str())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
                row.get(1).map_err(map_sqlite_error)?,
                row.get(2).map_err(map_sqlite_error)?,
                row.get::<_, String>(3).map_err(map_sqlite_error)?,
                row.get::<_, String>(4).map_err(map_sqlite_error)?,
            )?;
            if include_document(&document)? {
                documents.push(document);
            }
        }
        Ok(documents)
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
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
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let fields = index_fields_for_cached_schema(&self.schema_cache, table, index_name)?;
        if prefix_values.len() > fields.len() {
            return Err(Error::InvalidInput(format!(
                "index prefix length {} exceeds field count {} for index {}",
                prefix_values.len(),
                fields.len(),
                index_name
            )));
        }
        let sql = sqlite_index_scan_prefix_query_sql(&fields, prefix_values.len())?;
        let mut params = vec![SqlValue::Text(table.as_str().to_string())];
        params.extend(
            prefix_values
                .iter()
                .map(sql_value_from_json)
                .collect::<Result<Vec<_>>>()?,
        );
        self.query_documents(&sql, params, table, check_cancel)
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
        self.index_scan_composite_range_cancellable(
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
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let fields = index_fields_for_cached_schema(&self.schema_cache, table, index_name)?;
        if exact_prefix.len() >= fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} must be smaller than field count {} for index {}",
                exact_prefix.len(),
                fields.len(),
                index_name
            )));
        }
        let mut params = vec![SqlValue::Text(table.as_str().to_string())];
        params.extend(
            exact_prefix
                .iter()
                .map(sql_value_from_json)
                .collect::<Result<Vec<_>>>()?,
        );
        if let Some(start) = start {
            params.push(sql_value_from_json(start)?);
        }
        if let Some(end) = end {
            params.push(sql_value_from_json(end)?);
        }
        let sql = sqlite_index_scan_composite_range_query_sql(
            &fields,
            exact_prefix.len(),
            start.is_some(),
            end.is_some(),
            start_inclusive,
            end_inclusive,
        )?;
        self.query_documents(&sql, params, table, check_cancel)
    }

    fn query_documents(
        &self,
        sql: &str,
        params: Vec<SqlValue>,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let mut stmt = self.conn.prepare_cached(sql).map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(rusqlite::params_from_iter(params))
            .map_err(map_sqlite_error)?;
        let mut documents = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            check_cancel()?;
            let id =
                DocumentId::from_str(row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
            documents.push(row_to_document(
                table,
                &id,
                row.get(1).map_err(map_sqlite_error)?,
                row.get(2).map_err(map_sqlite_error)?,
                row.get::<_, String>(3).map_err(map_sqlite_error)?,
                row.get::<_, String>(4).map_err(map_sqlite_error)?,
            )?);
        }
        Ok(documents)
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        load_scheduled_jobs_from_conn(&self.conn, "scheduled_jobs")
    }

    pub fn get_scheduled_job_result(&self, job_id: &JobId) -> Result<Option<ScheduledJobResult>> {
        self.conn
            .query_row(
                "SELECT data_json FROM scheduled_job_results WHERE job_id = ?1",
                params![job_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .map(|json| deserialize_json::<ScheduledJobResult>(json.as_str()))
            .transpose()
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT data_json
                 FROM cron_jobs
                 ORDER BY name",
            )
            .map_err(map_sqlite_error)?;
        let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
        let mut crons = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            crons.push(deserialize_json::<CronJob>(
                row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str(),
            )?);
        }
        Ok(crons)
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
        Ok(table_has_entries(&self.conn, "scheduled_jobs")?
            || table_has_entries(&self.conn, "running_scheduled_jobs")?
            || table_has_entries(&self.conn, "cron_jobs")?)
    }

    pub fn durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let sequence = self
            .conn
            .query_row(
                "SELECT sequence FROM commit_log ORDER BY sequence ASC LIMIT 1",
                [],
                |row| row.get::<_, u64>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        Ok(SequenceNumber(
            sequence.map(|value| value.saturating_sub(1)).unwrap_or(0),
        ))
    }
}
