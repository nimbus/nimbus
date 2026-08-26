//! Fixture builders and read oracles shared by the three remote-provider test
//! modules.
//!
//! What lives here describes fixture *shape* -- table schemas, documents,
//! historical snapshots, expected-title oracles -- which is provider
//! independent by construction. Anything that knows how a provider connects,
//! names its namespaces, or tears itself down stays in that provider's own
//! `support.rs`.

use super::{
    Document, DocumentId, FieldSchema, FieldType, IndexDefinition, SequenceNumber, TableId,
    TableName, TableSchema, TenantEventRecord, Timestamp, WriteOp, WriteOpType,
};
use nimbus_core::{HistoricalReadShape, Mutation, Result, ScheduledJob, TableState};

pub(crate) use super::historical_fixtures::{
    historical_read_shape, indexed_rank_schema, ranked_document,
};

/// Reads a document as of a historical sequence.
///
/// Every remote provider exposes this as an inherent method with an identical
/// signature. The trait exists so the full-scan oracles below can be written
/// once instead of three times.
pub(crate) trait DocumentVersionOracle {
    fn document_version_at(
        &self,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>>;
}

/// The historical index scans a shared scenario needs to drive.
///
/// These five entry points are inherent methods on all three provider stores,
/// but they do not arrive there the same way: PostgreSQL and MySQL get them
/// from `sql_historical_index_facade!` over `SqlHistoricalIndexStore`, while
/// the libSQL replica store hand-writes its own. There is therefore no
/// existing trait that all three share, so a shared scenario cannot bound on
/// one -- hence this test-only forwarding trait over the inherent surface.
pub(crate) trait HistoricalIndexScanOracle {
    fn scan_eq(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &serde_json::Value,
    ) -> Result<Vec<Document>>;

    fn scan_prefix(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[serde_json::Value],
    ) -> Result<Vec<Document>>;

    fn scan_prefix_page(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        after: Option<&nimbus_core::HistoricalIndexCursor>,
        limit: usize,
    ) -> Result<crate::store::HistoricalIndexDocumentPage>;

    fn scan_range(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: crate::IndexRangeBound<'_>,
        end: crate::IndexRangeBound<'_>,
    ) -> Result<Vec<Document>>;

    fn scan_composite_range(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: crate::IndexRangeBound<'_>,
        end: crate::IndexRangeBound<'_>,
    ) -> Result<Vec<Document>>;
}

/// Reads a store's health diagnostic.
///
/// `storage_health_diagnostic` is generated per store by
/// `impl_storage_health_diagnostic!`, so like the scans above it is inherent on
/// all three providers and backed by no shared trait.
pub(crate) trait StorageHealthProbe {
    fn health(&self) -> Result<crate::StorageHealthDiagnostic>;
}

/// Forwards the read oracles and the health probe to `$store`'s inherent
/// methods.
///
/// Every forwarded call names `$store` explicitly rather than going through
/// `self`, so the inherent method is the one selected and these impls can
/// never recurse into themselves.
macro_rules! impl_provider_read_oracles {
    ($store:ty) => {
        impl DocumentVersionOracle for $store {
            fn document_version_at(
                &self,
                table: &TableName,
                table_id: &TableId,
                document_id: &DocumentId,
                sequence: SequenceNumber,
            ) -> Result<Option<Document>> {
                <$store>::get_document_version_at(self, table, table_id, document_id, sequence)
            }
        }

        impl StorageHealthProbe for $store {
            fn health(&self) -> Result<crate::StorageHealthDiagnostic> {
                <$store>::storage_health_diagnostic(self)
            }
        }

        impl HistoricalIndexScanOracle for $store {
            fn scan_eq(
                &self,
                read_shape: &HistoricalReadShape,
                index_name: &str,
                value: &serde_json::Value,
            ) -> Result<Vec<Document>> {
                <$store>::historical_index_scan_eq_cancellable(
                    self,
                    read_shape,
                    index_name,
                    value,
                    &mut || Ok(()),
                )
            }

            fn scan_prefix(
                &self,
                read_shape: &HistoricalReadShape,
                index_name: &str,
                prefix_values: &[serde_json::Value],
            ) -> Result<Vec<Document>> {
                <$store>::historical_index_scan_prefix_cancellable(
                    self,
                    read_shape,
                    index_name,
                    prefix_values,
                    &mut || Ok(()),
                )
            }

            fn scan_prefix_page(
                &self,
                read_shape: &HistoricalReadShape,
                index_name: &str,
                prefix_values: &[serde_json::Value],
                after: Option<&nimbus_core::HistoricalIndexCursor>,
                limit: usize,
            ) -> Result<crate::store::HistoricalIndexDocumentPage> {
                <$store>::historical_index_scan_prefix_page_cancellable(
                    self,
                    read_shape,
                    index_name,
                    prefix_values,
                    after,
                    limit,
                    &mut || Ok(()),
                )
            }

            fn scan_range(
                &self,
                read_shape: &HistoricalReadShape,
                index_name: &str,
                start: crate::IndexRangeBound<'_>,
                end: crate::IndexRangeBound<'_>,
            ) -> Result<Vec<Document>> {
                <$store>::historical_index_scan_range_cancellable(
                    self,
                    read_shape,
                    index_name,
                    start,
                    end,
                    &mut || Ok(()),
                )
            }

            fn scan_composite_range(
                &self,
                read_shape: &HistoricalReadShape,
                index_name: &str,
                exact_prefix: &[serde_json::Value],
                start: crate::IndexRangeBound<'_>,
                end: crate::IndexRangeBound<'_>,
            ) -> Result<Vec<Document>> {
                <$store>::historical_index_scan_composite_range_cancellable(
                    self,
                    read_shape,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    &mut || Ok(()),
                )
            }
        }
    };
}

#[cfg(feature = "postgres")]
impl_provider_read_oracles!(crate::PostgresTenantStore);
#[cfg(feature = "mysql")]
impl_provider_read_oracles!(crate::MySqlTenantStore);
#[cfg(feature = "libsql")]
impl_provider_read_oracles!(crate::LibsqlReplicaTenantStore);

pub(crate) fn status_rank_schema(table: &TableName) -> TableSchema {
    TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            },
        ],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

pub(crate) fn status_rank_document(
    table: &TableName,
    title: &str,
    status: &str,
    rank: u64,
) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), serde_json::json!(title)),
            ("status".to_string(), serde_json::json!(status)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

pub(crate) fn rank_full_scan_oracle_titles<S>(
    store: &S,
    table: &TableName,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    rank: u64,
) -> Vec<String>
where
    S: DocumentVersionOracle,
{
    let mut titles = corpus
        .iter()
        .filter_map(|document| {
            store
                .document_version_at(table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter(|document| {
            document.fields.get("rank").and_then(|value| value.as_u64()) == Some(rank)
        })
        .map(|document| document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

pub(crate) fn status_rank_full_scan_oracle_titles<S>(
    store: &S,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    status: &str,
    start_rank: Option<u64>,
    end_rank: Option<u64>,
) -> Vec<String>
where
    S: DocumentVersionOracle,
{
    let mut rows = corpus
        .iter()
        .filter_map(|document| {
            store
                .document_version_at(&document.table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter_map(|document| {
            let document_status = document.fields.get("status")?.as_str()?;
            let rank = document.fields.get("rank")?.as_u64()?;
            if document_status == status
                && start_rank.is_none_or(|start| rank >= start)
                && end_rank.is_none_or(|end| rank <= end)
            {
                Some((rank, document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

pub(crate) fn document_titles(documents: &[Document]) -> Vec<&str> {
    documents
        .iter()
        .map(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                .expect("document should have a string title")
        })
        .collect()
}

pub(crate) fn document_title_strings(documents: &[Document]) -> Vec<String> {
    documents.iter().map(document_title_string).collect()
}

pub(crate) fn document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

pub(crate) fn active_table_id_for_diagnostic(
    diagnostics: &[crate::TableIdentityDiagnostic],
    table: &TableName,
) -> TableId {
    diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.table_name == *table && diagnostic.state == TableState::Active
        })
        .expect("active table identity should exist")
        .table_id
        .clone()
}

// Test-only helper mirroring `WriteOp` field-by-field; call sites pass
// distinctly-typed newtypes positionally, so a wrapper struct would only add
// call-site ceremony without reducing risk of mixups.
#[allow(clippy::too_many_arguments)]
pub(crate) fn durable_write_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: DocumentId,
    previous: Option<Document>,
    current: Option<Document>,
) -> TenantEventRecord {
    TenantEventRecord::new(
        sequence,
        timestamp,
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type,
            doc_id,
            resource_path_binding: None,
            trigger_write_origin: None,
            previous,
            current,
        }],
        None,
    )
    .expect("durable record should build")
}

pub(crate) fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should build"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), serde_json::json!(title))]),
        },
        created_at: Timestamp(100),
    }
}
