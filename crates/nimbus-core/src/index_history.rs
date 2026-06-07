use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    CommitSequence, CommitTimestamp, Document, DocumentId, Error, HistoricalReadErrorKind,
    HistoricalReadShape, HistoricalReadSnapshot, IndexDefinition, IndexId, PolicySnapshotId,
    ReadTimestamp, Result, TableId, TenantEventKind, TenantEventRecord, VersionedRegistry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HistoricalIndexNumberKey(u64);

impl HistoricalIndexNumberKey {
    pub fn from_json_number(number: &serde_json::Number) -> Result<Self> {
        let value = number.as_f64().ok_or_else(|| {
            Error::InvalidInput("unsupported numeric historical index value".to_string())
        })?;
        let mut bits = value.to_bits();
        if value.is_sign_positive() || value == 0.0 {
            bits ^= 0x8000_0000_0000_0000;
        } else {
            bits = !bits;
        }
        Ok(Self(bits))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HistoricalIndexScalar {
    Null,
    Bool(bool),
    Number(HistoricalIndexNumberKey),
    String(String),
}

impl HistoricalIndexScalar {
    pub fn from_json(value: &Value) -> Result<Self> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(*value)),
            Value::Number(number) => Ok(Self::Number(HistoricalIndexNumberKey::from_json_number(
                number,
            )?)),
            Value::String(value) => Ok(Self::String(value.clone())),
            _ => Err(Error::InvalidInput(
                "historical index values must be null, boolean, number, or string".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HistoricalIndexTuple(Vec<HistoricalIndexScalar>);

impl HistoricalIndexTuple {
    pub fn from_values(values: &[Value]) -> Result<Self> {
        values
            .iter()
            .map(HistoricalIndexScalar::from_json)
            .collect::<Result<Vec<_>>>()
            .map(Self)
    }

    pub fn values(&self) -> &[HistoricalIndexScalar] {
        self.0.as_slice()
    }

    pub fn from_document(document: &Document, index: &IndexDefinition) -> Result<Option<Self>> {
        let mut values = Vec::with_capacity(index.fields.len());
        for field in &index.fields {
            let Some(value) = document.get_field(field) else {
                return Ok(None);
            };
            values.push(HistoricalIndexScalar::from_json(value)?);
        }
        Ok(Some(Self(values)))
    }

    fn starts_with(&self, prefix: &[HistoricalIndexScalar]) -> bool {
        self.0.starts_with(prefix)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoricalIndexQuery {
    All,
    Equal(HistoricalIndexTuple),
    Prefix(Vec<HistoricalIndexScalar>),
    Range {
        start: Option<HistoricalIndexTuple>,
        start_inclusive: bool,
        end: Option<HistoricalIndexTuple>,
        end_inclusive: bool,
    },
}

impl HistoricalIndexQuery {
    fn matches(&self, tuple: &HistoricalIndexTuple) -> bool {
        match self {
            Self::All => true,
            Self::Equal(expected) => tuple == expected,
            Self::Prefix(prefix) => tuple.starts_with(prefix),
            Self::Range {
                start,
                start_inclusive,
                end,
                end_inclusive,
            } => {
                let lower_ok = start.as_ref().is_none_or(|start| {
                    let ordering = tuple.cmp(start);
                    ordering == Ordering::Greater
                        || (ordering == Ordering::Equal && *start_inclusive)
                });
                let upper_ok = end.as_ref().is_none_or(|end| {
                    let ordering = tuple.cmp(end);
                    ordering == Ordering::Less || (ordering == Ordering::Equal && *end_inclusive)
                });
                lower_ok && upper_ok
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalIndexCursor {
    read_snapshot: HistoricalReadSnapshot,
    table_id: TableId,
    index_id: IndexId,
    query: HistoricalIndexQuery,
    policy_snapshot: PolicySnapshotId,
    storage_format_generation: u16,
    last_tuple: HistoricalIndexTuple,
    last_document_id: DocumentId,
}

impl HistoricalIndexCursor {
    pub fn new(
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        query: HistoricalIndexQuery,
        last_tuple: HistoricalIndexTuple,
        last_document_id: DocumentId,
    ) -> Self {
        Self {
            read_snapshot: read_shape.read_snapshot(),
            table_id: read_shape.table_id().clone(),
            index_id: index.id.clone(),
            query,
            policy_snapshot: read_shape.policy_snapshot().clone(),
            storage_format_generation: read_shape.storage_format_generation(),
            last_tuple,
            last_document_id,
        }
    }

    pub fn validate_context(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        query: &HistoricalIndexQuery,
    ) -> Result<()> {
        validate_cursor(read_shape, index, query, self)
    }

    pub fn last_tuple(&self) -> &HistoricalIndexTuple {
        &self.last_tuple
    }

    pub fn last_document_id(&self) -> &DocumentId {
        &self.last_document_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalIndexPage {
    pub document_ids: Vec<DocumentId>,
    pub next_cursor: Option<HistoricalIndexCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalIndexVersion {
    table_id: TableId,
    index_id: IndexId,
    tuple: HistoricalIndexTuple,
    document_id: DocumentId,
    visible_from: CommitSequence,
    visible_until: Option<CommitSequence>,
}

/// Canonical event-derived index-version model for historical index scans.
#[derive(Debug, Clone)]
pub struct HistoricalIndexHistory {
    versions: Vec<HistoricalIndexVersion>,
    storage_format_generation: u16,
}

impl HistoricalIndexHistory {
    pub fn from_records(records: impl IntoIterator<Item = TenantEventRecord>) -> Result<Self> {
        Self::from_records_with_format_generation(records, 1)
    }

    pub fn from_records_with_format_generation(
        records: impl IntoIterator<Item = TenantEventRecord>,
        storage_format_generation: u16,
    ) -> Result<Self> {
        if storage_format_generation == 0 {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                "historical index storage format generation cannot be zero",
            ));
        }

        let mut records = records.into_iter().collect::<Vec<_>>();
        records.sort_by_key(|record| record.sequence);
        let registry = VersionedRegistry::from_records_with_format_generation(
            records.clone(),
            storage_format_generation,
        )?;
        let mut versions = Vec::new();
        let mut open_versions = BTreeMap::new();

        for record in &records {
            let sequence = CommitSequence::new(record.sequence);
            for event in record.events() {
                if let TenantEventKind::DocumentWrite { writes } = event {
                    for write in writes {
                        let snapshot = snapshot_for_record(record);
                        let Some(shape) = registry.read_shape_at(&write.table, snapshot)? else {
                            continue;
                        };
                        let Some(schema) = shape.schema() else {
                            continue;
                        };
                        for index in schema.maintained_indexes() {
                            if let Some(previous) = write.previous.as_ref() {
                                close_index_version(
                                    &mut versions,
                                    &mut open_versions,
                                    &write.table_id,
                                    index,
                                    previous,
                                    sequence,
                                )?;
                            }
                            if let Some(current) = write.current.as_ref() {
                                open_index_version(
                                    &mut versions,
                                    &mut open_versions,
                                    &write.table_id,
                                    index,
                                    current,
                                    sequence,
                                )?;
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            versions,
            storage_format_generation,
        })
    }

    pub fn scan_at(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        query: HistoricalIndexQuery,
    ) -> Result<Vec<DocumentId>> {
        Ok(self
            .matching_entries(read_shape, index_name, &query)?
            .into_iter()
            .map(|entry| entry.document_id.clone())
            .collect())
    }

    pub fn scan_page_at(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        query: HistoricalIndexQuery,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
    ) -> Result<HistoricalIndexPage> {
        if limit == 0 {
            return Err(Error::InvalidInput(
                "historical index page limit must be greater than zero".to_string(),
            ));
        }

        let index = queryable_index(read_shape, index_name)?;
        if let Some(cursor) = after {
            validate_cursor(read_shape, &index, &query, cursor)?;
        }

        let entries = self.matching_entries(read_shape, index_name, &query)?;
        let start = after
            .and_then(|cursor| {
                entries.iter().position(|entry| {
                    entry.tuple == cursor.last_tuple && entry.document_id == cursor.last_document_id
                })
            })
            .map_or(0, |position| position.saturating_add(1));
        let selected = entries
            .into_iter()
            .skip(start)
            .take(limit)
            .collect::<Vec<_>>();
        let next_cursor = if selected.len() == limit {
            selected.last().map(|entry| {
                HistoricalIndexCursor::new(
                    read_shape,
                    &index,
                    query.clone(),
                    entry.tuple.clone(),
                    entry.document_id.clone(),
                )
            })
        } else {
            None
        };

        Ok(HistoricalIndexPage {
            document_ids: selected
                .into_iter()
                .map(|entry| entry.document_id.clone())
                .collect(),
            next_cursor,
        })
    }

    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    pub fn storage_format_generation(&self) -> u16 {
        self.storage_format_generation
    }

    fn matching_entries(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        query: &HistoricalIndexQuery,
    ) -> Result<Vec<&HistoricalIndexVersion>> {
        let index = queryable_index(read_shape, index_name)?;
        let read_sequence = read_shape.read_snapshot().sequence();
        let mut entries = self
            .versions
            .iter()
            .filter(|entry| {
                entry.table_id == *read_shape.table_id()
                    && entry.index_id == index.id
                    && entry.visible_at(read_sequence)
                    && query.matches(&entry.tuple)
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.tuple
                .cmp(&right.tuple)
                .then_with(|| left.document_id.cmp(&right.document_id))
        });
        Ok(entries)
    }
}

impl HistoricalIndexVersion {
    fn visible_at(&self, sequence: CommitSequence) -> bool {
        self.visible_from <= sequence && self.visible_until.is_none_or(|until| sequence < until)
    }
}

type OpenIndexKey = (TableId, IndexId, HistoricalIndexTuple, DocumentId);

fn close_index_version(
    versions: &mut [HistoricalIndexVersion],
    open_versions: &mut BTreeMap<OpenIndexKey, usize>,
    table_id: &TableId,
    index: &IndexDefinition,
    document: &Document,
    sequence: CommitSequence,
) -> Result<()> {
    let Some(tuple) = HistoricalIndexTuple::from_document(document, index)? else {
        return Ok(());
    };
    let key = (
        table_id.clone(),
        index.id.clone(),
        tuple,
        document.id.clone(),
    );
    if let Some(version_index) = open_versions.remove(&key)
        && let Some(version) = versions.get_mut(version_index)
    {
        version.visible_until = Some(sequence);
    }
    Ok(())
}

fn open_index_version(
    versions: &mut Vec<HistoricalIndexVersion>,
    open_versions: &mut BTreeMap<OpenIndexKey, usize>,
    table_id: &TableId,
    index: &IndexDefinition,
    document: &Document,
    sequence: CommitSequence,
) -> Result<()> {
    let Some(tuple) = HistoricalIndexTuple::from_document(document, index)? else {
        return Ok(());
    };
    let key = (
        table_id.clone(),
        index.id.clone(),
        tuple.clone(),
        document.id.clone(),
    );
    let version = HistoricalIndexVersion {
        table_id: table_id.clone(),
        index_id: index.id.clone(),
        tuple,
        document_id: document.id.clone(),
        visible_from: sequence,
        visible_until: None,
    };
    versions.push(version);
    open_versions.insert(key, versions.len().saturating_sub(1));
    Ok(())
}

fn queryable_index(read_shape: &HistoricalReadShape, index_name: &str) -> Result<IndexDefinition> {
    read_shape
        .queryable_indexes()
        .iter()
        .find(|index| index.name == index_name)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "enabled historical index not found for table {}: {}",
                read_shape.table(),
                index_name
            ))
        })
}

fn validate_cursor(
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    query: &HistoricalIndexQuery,
    cursor: &HistoricalIndexCursor,
) -> Result<()> {
    if cursor.read_snapshot != read_shape.read_snapshot()
        || cursor.table_id != *read_shape.table_id()
        || cursor.index_id != index.id
        || cursor.query != *query
        || cursor.policy_snapshot != *read_shape.policy_snapshot()
        || cursor.storage_format_generation != read_shape.storage_format_generation()
    {
        return Err(Error::historical_read(
            HistoricalReadErrorKind::CursorMismatch,
            "historical index cursor does not match read snapshot, table, index, query, policy snapshot, or storage format generation",
        ));
    }
    Ok(())
}

fn snapshot_for_record(record: &TenantEventRecord) -> HistoricalReadSnapshot {
    HistoricalReadSnapshot::new(
        ReadTimestamp::new(record.timestamp),
        CommitSequence::new(record.sequence),
        CommitTimestamp::new(record.timestamp),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use crate::{
        AccessRule, FieldSchema, FieldType, IndexState, SchemaChangeEvent, SequenceNumber,
        TableAccessPolicy, TableName, TableSchema, Timestamp, WriteOp, WriteOpType,
    };

    use super::*;

    fn table(name: &str) -> TableName {
        TableName::new(name).expect("table should build")
    }

    fn ranked_schema(table: &TableName, index: IndexDefinition) -> TableSchema {
        TableSchema {
            table: table.clone(),
            fields: vec![
                FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: true,
                },
                FieldSchema {
                    name: "group".to_string(),
                    field_type: FieldType::String,
                    required: true,
                },
            ],
            indexes: vec![index],
            access_policy: None,
        }
    }

    fn authenticated_ranked_schema(table: &TableName, index: IndexDefinition) -> TableSchema {
        TableSchema {
            access_policy: Some(TableAccessPolicy {
                read: AccessRule {
                    require_authenticated: true,
                    ..AccessRule::default()
                },
                ..TableAccessPolicy::default()
            }),
            ..ranked_schema(table, index)
        }
    }

    fn document(table: &TableName, id: &DocumentId, rank: u64, group: &str) -> Document {
        let mut fields = Map::new();
        fields.insert("rank".to_string(), json!(rank));
        fields.insert("group".to_string(), json!(group));
        Document::with_id(id.clone(), table.clone(), fields)
    }

    fn shape_at(
        registry: &VersionedRegistry,
        table: &TableName,
        sequence: u64,
    ) -> HistoricalReadShape {
        registry
            .read_shape_at(
                table,
                HistoricalReadSnapshot::new(
                    ReadTimestamp::new(Timestamp(sequence)),
                    CommitSequence::new(SequenceNumber(sequence)),
                    CommitTimestamp::new(Timestamp(sequence)),
                ),
            )
            .expect("shape should resolve")
            .expect("shape should exist")
    }

    fn schema_record(
        sequence: u64,
        table: &TableName,
        table_id: &TableId,
        schema: TableSchema,
    ) -> TenantEventRecord {
        TenantEventRecord::from_events(
            SequenceNumber(sequence),
            Timestamp(sequence),
            vec![TenantEventKind::SchemaChange {
                change: Box::new(SchemaChangeEvent::SetTable {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    previous: None,
                    current: schema,
                }),
            }],
        )
        .expect("schema event should build")
    }

    fn write_record(
        sequence: u64,
        table: &TableName,
        table_id: &TableId,
        previous: Option<Document>,
        current: Option<Document>,
    ) -> TenantEventRecord {
        let doc_id = previous
            .as_ref()
            .or(current.as_ref())
            .expect("write needs a document")
            .id
            .clone();
        let op_type = match (&previous, &current) {
            (None, Some(_)) => WriteOpType::Insert,
            (Some(_), Some(_)) => WriteOpType::Update,
            (Some(_), None) => WriteOpType::Delete,
            (None, None) => unreachable!("write needs previous or current"),
        };
        TenantEventRecord::from_events(
            SequenceNumber(sequence),
            Timestamp(sequence),
            vec![TenantEventKind::DocumentWrite {
                writes: vec![WriteOp {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    op_type,
                    doc_id,
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous,
                    current,
                }],
            }],
        )
        .expect("write event should build")
    }

    #[test]
    fn historical_index_scan_tracks_update_and_delete_visibility() {
        let table = table("tasks");
        let table_id = TableId::new();
        let index = IndexDefinition::new("by_rank", ["rank"]);
        let first_id = DocumentId::new();
        let second_id = DocumentId::new();
        let first_v1 = document(&table, &first_id, 1, "a");
        let first_v2 = document(&table, &first_id, 3, "a");
        let second = document(&table, &second_id, 2, "a");
        let records = vec![
            schema_record(1, &table, &table_id, ranked_schema(&table, index.clone())),
            write_record(2, &table, &table_id, None, Some(first_v1.clone())),
            write_record(3, &table, &table_id, None, Some(second.clone())),
            write_record(4, &table, &table_id, Some(first_v1), Some(first_v2.clone())),
            write_record(5, &table, &table_id, Some(second.clone()), None),
        ];
        let registry = VersionedRegistry::from_records(records.clone()).expect("registry builds");
        let history = HistoricalIndexHistory::from_records(records).expect("history builds");

        let at_three = shape_at(&registry, &table, 3);
        let at_five = shape_at(&registry, &table, 5);

        assert_eq!(
            history
                .scan_at(&at_three, "by_rank", HistoricalIndexQuery::All)
                .expect("scan should succeed"),
            vec![first_id.clone(), second_id.clone()]
        );
        assert_eq!(
            history
                .scan_at(&at_five, "by_rank", HistoricalIndexQuery::All)
                .expect("scan should succeed"),
            vec![first_id]
        );
        assert_eq!(history.version_count(), 3);
    }

    #[test]
    fn historical_index_prefix_range_and_cursor_identity_are_stable() {
        let table = table("tasks");
        let table_id = TableId::new();
        let index = IndexDefinition::new("by_group_rank", ["group", "rank"]);
        let first_id = DocumentId::new();
        let second_id = DocumentId::new();
        let third_id = DocumentId::new();
        let records = vec![
            schema_record(1, &table, &table_id, ranked_schema(&table, index.clone())),
            write_record(
                2,
                &table,
                &table_id,
                None,
                Some(document(&table, &first_id, 1, "a")),
            ),
            write_record(
                3,
                &table,
                &table_id,
                None,
                Some(document(&table, &second_id, 2, "a")),
            ),
            write_record(
                4,
                &table,
                &table_id,
                None,
                Some(document(&table, &third_id, 1, "b")),
            ),
        ];
        let registry = VersionedRegistry::from_records(records.clone()).expect("registry builds");
        let history = HistoricalIndexHistory::from_records(records).expect("history builds");
        let shape = shape_at(&registry, &table, 4);
        let prefix = vec![HistoricalIndexScalar::String("a".to_string())];
        let page = history
            .scan_page_at(
                &shape,
                "by_group_rank",
                HistoricalIndexQuery::Prefix(prefix),
                None,
                1,
            )
            .expect("first page should load");

        assert_eq!(page.document_ids, vec![first_id.clone()]);
        let next = history
            .scan_page_at(
                &shape,
                "by_group_rank",
                HistoricalIndexQuery::Prefix(vec![HistoricalIndexScalar::String("a".to_string())]),
                page.next_cursor.as_ref(),
                10,
            )
            .expect("second page should load");
        assert_eq!(next.document_ids, vec![second_id.clone()]);

        let range = HistoricalIndexQuery::Range {
            start: Some(
                HistoricalIndexTuple::from_values(&[json!("a"), json!(2)]).expect("start tuple"),
            ),
            start_inclusive: true,
            end: Some(
                HistoricalIndexTuple::from_values(&[json!("b"), json!(1)]).expect("end tuple"),
            ),
            end_inclusive: true,
        };
        assert_eq!(
            history
                .scan_at(&shape, "by_group_rank", range)
                .expect("range scan should load"),
            vec![second_id, third_id]
        );

        let wrong_query = HistoricalIndexQuery::All;
        let err = history
            .scan_page_at(
                &shape,
                "by_group_rank",
                wrong_query,
                page.next_cursor.as_ref(),
                1,
            )
            .expect_err("cursor query mismatch must fail closed");
        assert!(err.to_string().contains("cursor_mismatch"));
    }

    #[test]
    fn historical_index_cursor_rejects_policy_snapshot_drift() {
        let table = table("tasks");
        let table_id = TableId::new();
        let index = IndexDefinition::new("by_group_rank", ["group", "rank"]);
        let first_id = DocumentId::new();
        let second_id = DocumentId::new();
        let records = vec![
            schema_record(1, &table, &table_id, ranked_schema(&table, index.clone())),
            write_record(
                2,
                &table,
                &table_id,
                None,
                Some(document(&table, &first_id, 1, "a")),
            ),
            write_record(
                3,
                &table,
                &table_id,
                None,
                Some(document(&table, &second_id, 2, "a")),
            ),
            schema_record(
                4,
                &table,
                &table_id,
                authenticated_ranked_schema(&table, index),
            ),
        ];
        let registry = VersionedRegistry::from_records(records.clone()).expect("registry builds");
        let history = HistoricalIndexHistory::from_records(records).expect("history builds");
        let before_policy = shape_at(&registry, &table, 3);
        let after_policy = shape_at(&registry, &table, 4);
        let query =
            HistoricalIndexQuery::Prefix(vec![HistoricalIndexScalar::String("a".to_string())]);
        let page = history
            .scan_page_at(&before_policy, "by_group_rank", query.clone(), None, 1)
            .expect("first page should load");

        let err = history
            .scan_page_at(
                &after_policy,
                "by_group_rank",
                query,
                page.next_cursor.as_ref(),
                1,
            )
            .expect_err("policy drift must fail closed");

        assert!(err.to_string().contains("cursor_mismatch"));
    }

    #[test]
    fn historical_index_cursor_rejects_storage_format_drift() {
        let table = table("tasks");
        let table_id = TableId::new();
        let index = IndexDefinition::new("by_group_rank", ["group", "rank"]);
        let first_id = DocumentId::new();
        let second_id = DocumentId::new();
        let records = vec![
            schema_record(1, &table, &table_id, ranked_schema(&table, index)),
            write_record(
                2,
                &table,
                &table_id,
                None,
                Some(document(&table, &first_id, 1, "a")),
            ),
            write_record(
                3,
                &table,
                &table_id,
                None,
                Some(document(&table, &second_id, 2, "a")),
            ),
        ];
        let registry_v1 =
            VersionedRegistry::from_records_with_format_generation(records.clone(), 1)
                .expect("registry builds");
        let registry_v2 =
            VersionedRegistry::from_records_with_format_generation(records.clone(), 2)
                .expect("registry builds");
        let history = HistoricalIndexHistory::from_records(records).expect("history builds");
        let shape_v1 = shape_at(&registry_v1, &table, 3);
        let shape_v2 = shape_at(&registry_v2, &table, 3);
        let query =
            HistoricalIndexQuery::Prefix(vec![HistoricalIndexScalar::String("a".to_string())]);
        let page = history
            .scan_page_at(&shape_v1, "by_group_rank", query.clone(), None, 1)
            .expect("first page should load");

        let err = history
            .scan_page_at(
                &shape_v2,
                "by_group_rank",
                query,
                page.next_cursor.as_ref(),
                1,
            )
            .expect_err("format drift must fail closed");

        assert!(err.to_string().contains("cursor_mismatch"));
    }

    #[test]
    fn historical_index_history_rejects_zero_format_generation() {
        let err = HistoricalIndexHistory::from_records_with_format_generation(Vec::new(), 0)
            .expect_err("zero format generation must fail closed");
        assert!(err.to_string().contains("format_mismatch"));
    }

    #[test]
    fn historical_index_ignores_non_queryable_index_at_read_time() {
        let table = table("tasks");
        let table_id = TableId::new();
        let pending = IndexDefinition::with_state("by_rank", ["rank"], IndexState::Pending);
        let doc = document(&table, &DocumentId::new(), 1, "a");
        let records = vec![
            schema_record(1, &table, &table_id, ranked_schema(&table, pending)),
            write_record(2, &table, &table_id, None, Some(doc)),
        ];
        let registry = VersionedRegistry::from_records(records.clone()).expect("registry builds");
        let history = HistoricalIndexHistory::from_records(records).expect("history builds");
        let shape = shape_at(&registry, &table, 2);

        let err = history
            .scan_at(&shape, "by_rank", HistoricalIndexQuery::All)
            .expect_err("pending index must not be queryable");
        assert!(
            err.to_string()
                .contains("enabled historical index not found")
        );
    }
}
