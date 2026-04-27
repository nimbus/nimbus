use super::*;

impl SqliteTenantStore {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self.read_schema_cache()?.clone())
    }

    pub fn save_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_table_schema(table_schema))?;
        Ok(())
    }

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.replace_table_schema(table_schema))?;
        Ok(())
    }

    pub fn replace_schema(&self, schema: &Schema) -> Result<()> {
        let current = self.load_schema()?;
        if current == *schema {
            return Ok(());
        }

        let mut tables_to_remove = current
            .tables
            .keys()
            .filter(|table| !schema.tables.contains_key(*table))
            .cloned()
            .collect::<Vec<_>>();
        tables_to_remove.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));

        let mut tables_to_replace = schema
            .tables
            .iter()
            .filter_map(|(table, table_schema)| {
                (current.tables.get(table) != Some(table_schema)).then_some(table_schema.clone())
            })
            .collect::<Vec<_>>();
        tables_to_replace
            .sort_unstable_by(|left, right| left.table.as_str().cmp(right.table.as_str()));

        self.execute_write(move |transaction| {
            for table in &tables_to_remove {
                transaction.delete_table_schema(table)?;
            }
            for table_schema in &tables_to_replace {
                transaction.replace_table_schema(table_schema)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn delete_table_schema_entry(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema_entry(table))?;
        Ok(())
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema(table))?;
        Ok(())
    }

    fn read_schema_cache(&self) -> Result<RwLockReadGuard<'_, Schema>> {
        self.schema_cache
            .read()
            .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))
    }

    fn write_schema_cache(&self) -> Result<RwLockWriteGuard<'_, Schema>> {
        self.schema_cache
            .write()
            .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))
    }

    pub(super) fn replace_cached_schema(&self, schema: Schema) -> Result<()> {
        *self.write_schema_cache()? = schema;
        Ok(())
    }
}

pub(crate) fn rebuild_sqlite_indexes_from_loaded_schema(conn: &Connection) -> Result<()> {
    let schema = load_schema_from_conn(conn)?;
    for table_schema in schema.tables.values() {
        create_sqlite_indexes_for_table_schema(conn, table_schema)?;
    }
    Ok(())
}

pub(super) fn load_schema_from_conn(conn: &Connection) -> Result<Schema> {
    let mut stmt = conn
        .prepare_cached("SELECT schema_json FROM schemas ORDER BY table_name")
        .map_err(map_sqlite_error)?;
    let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
    let mut schema = Schema::default();
    while let Some(row) = rows.next().map_err(map_sqlite_error)? {
        let table_schema: TableSchema =
            serde_json::from_str(row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str())
                .map_err(|error| Error::Serialization(error.to_string()))?;
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
    }
    Ok(schema)
}

pub(super) fn load_table_schema_from_conn(
    conn: &Connection,
    table: &TableName,
) -> Result<Option<TableSchema>> {
    conn.query_row(
        "SELECT schema_json FROM schemas WHERE table_name = ?1",
        params![table.as_str()],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(map_sqlite_error)?
    .map(|json| deserialize_json::<TableSchema>(json.as_str()))
    .transpose()
}

pub(super) fn create_sqlite_indexes_for_table_schema(
    conn: &Connection,
    table_schema: &TableSchema,
) -> Result<()> {
    for index in &table_schema.indexes {
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS \"{}\" ON documents ({})",
            sqlite_index_name(&table_schema.table, &index.name),
            sqlite_index_columns(&index.fields)
        );
        conn.execute_batch(&sql).map_err(map_sqlite_error)?;
    }
    Ok(())
}

pub(super) fn drop_sqlite_indexes_for_table_schema(
    conn: &Connection,
    table_schema: &TableSchema,
) -> Result<()> {
    for index in &table_schema.indexes {
        let sql = format!(
            "DROP INDEX IF EXISTS \"{}\"",
            sqlite_index_name(&table_schema.table, &index.name)
        );
        conn.execute_batch(&sql).map_err(map_sqlite_error)?;
    }
    Ok(())
}

pub fn sqlite_index_scan_prefix_query_sql<S>(fields: &[S], prefix_len: usize) -> Result<String>
where
    S: AsRef<str>,
{
    if prefix_len > fields.len() {
        return Err(Error::InvalidInput(format!(
            "index prefix length {} exceeds field count {}",
            prefix_len,
            fields.len()
        )));
    }

    let where_clauses = exact_prefix_clauses(&fields[..prefix_len]);
    let order_by = sqlite_order_by_fields_after_exact_prefix(fields, prefix_len);
    Ok(format!(
        "SELECT id, creation_time, update_time, data_json, typed_fields_json
         FROM documents
         WHERE table_name = ?1 AND {}
         ORDER BY {}",
        where_clauses.join(" AND "),
        order_by
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn sqlite_index_scan_composite_range_query_sql<S>(
    fields: &[S],
    exact_prefix_len: usize,
    has_start: bool,
    has_end: bool,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<String>
where
    S: AsRef<str>,
{
    if exact_prefix_len >= fields.len() {
        return Err(Error::InvalidInput(format!(
            "composite range prefix length {} must be smaller than field count {}",
            exact_prefix_len,
            fields.len()
        )));
    }

    let range_field = fields[exact_prefix_len].as_ref();
    let mut clauses = exact_prefix_clauses(&fields[..exact_prefix_len]);
    let mut next_param = exact_prefix_len + 2;
    if has_start {
        clauses.push(format!(
            "{} {} ?{}",
            json_extract_expr(range_field),
            if start_inclusive { ">=" } else { ">" },
            next_param
        ));
        next_param += 1;
    }
    if has_end {
        clauses.push(format!(
            "{} {} ?{}",
            json_extract_expr(range_field),
            if end_inclusive { "<=" } else { "<" },
            next_param
        ));
    }

    Ok(format!(
        "SELECT id, creation_time, update_time, data_json, typed_fields_json
         FROM documents
         WHERE table_name = ?1 AND {}
         ORDER BY {}",
        clauses.join(" AND "),
        sqlite_order_by_fields_after_exact_prefix(fields, exact_prefix_len)
    ))
}

pub(super) fn index_fields_for_cached_schema(
    schema_cache: &Arc<RwLock<Schema>>,
    table: &TableName,
    index_name: &str,
) -> Result<Vec<String>> {
    let schema = schema_cache
        .read()
        .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))?;
    index_fields_for_schema(&schema, table, index_name)
}

fn index_fields_for_schema(
    schema: &Schema,
    table: &TableName,
    index_name: &str,
) -> Result<Vec<String>> {
    let Some(table_schema) = schema.get_table(table) else {
        return Err(Error::SchemaNotFound(table.clone()));
    };
    table_schema
        .indexes
        .iter()
        .find(|definition| definition.name == index_name)
        .map(|definition| definition.fields.clone())
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index not found for table {}: {}",
                table, index_name
            ))
        })
}

fn sqlite_index_name(table: &TableName, index_name: &str) -> String {
    format!(
        "idx_{}_{}",
        sanitize_identifier_component(table.as_str()),
        sanitize_identifier_component(index_name)
    )
}

fn sqlite_index_columns<S>(fields: &[S]) -> String
where
    S: AsRef<str>,
{
    let mut columns = Vec::with_capacity(fields.len() + 2);
    columns.push("table_name".to_string());
    columns.extend(fields.iter().map(|field| json_extract_expr(field.as_ref())));
    columns.push("id".to_string());
    columns.join(", ")
}

fn sqlite_order_by_fields_after_exact_prefix<S>(fields: &[S], exact_prefix_len: usize) -> String
where
    S: AsRef<str>,
{
    let mut columns = fields
        .iter()
        .skip(exact_prefix_len)
        .map(|field| json_extract_expr(field.as_ref()))
        .collect::<Vec<_>>();
    columns.push("id".to_string());
    columns.join(", ")
}

fn exact_prefix_clauses<S>(fields: &[S]) -> Vec<String>
where
    S: AsRef<str>,
{
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| format!("{} = ?{}", json_extract_expr(field.as_ref()), index + 2))
        .collect()
}

fn json_extract_expr(field: &str) -> String {
    format!(
        "json_extract(data_json, '$.\"{}\"')",
        field.replace('"', "\\\"")
    )
}

fn sanitize_identifier_component(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
