//! Core types for Nimbus.

pub mod auth;
pub mod dependency;
pub mod document;
pub mod document_history;
pub mod error;
pub mod index_history;
pub mod mutation;
pub mod mvcc;
pub mod query;
pub mod resource_path;
pub mod scheduled;
pub mod schema;
pub mod subscription;
pub mod transaction;
pub mod trigger;
pub mod typed_scalar;
pub mod types;
pub mod versioned_registry;
pub mod visibility;
pub mod write_batch;

pub use auth::{
    AccessAction, AccessOperator, AccessPredicate, AccessRule, AccessValue, CompiledReadRule,
    PrincipalClaimSource, PrincipalContext, PrincipalSnapshot, TableAccessPolicy,
    policy_revision_id,
};
pub use dependency::{
    DependencySet, IndexRangeDependency, PaginatedWindowDependency, PredicateDependency,
    commit_intersects_dependency_set, durable_record_intersects_dependency_set,
};
pub use document::Document;
pub use document_history::{DocumentVersion, DocumentVersionHistory};
pub use error::{Error, HistoricalReadErrorKind, Result, StorageErrorKind};
pub use index_history::{
    HistoricalIndexCursor, HistoricalIndexHistory, HistoricalIndexNumberKey, HistoricalIndexPage,
    HistoricalIndexQuery, HistoricalIndexScalar, HistoricalIndexTuple, HistoricalIndexVersion,
    order_preserving_number_bits,
};
pub use mutation::{
    CommitEntry, DurableMutationRecord, IndexLifecycleEvent, Mutation, SchemaChangeEvent,
    TableLifecycleEvent, TenantEventKind, TenantEventRecord, WriteOp, WriteOpType,
};
pub use mvcc::{
    CommitSequence, CommitTimestamp, HistoricalAuthorization, HistoricalCursorIdentity,
    HistoricalQueryShape, HistoricalReadSnapshot, HistoricalReadSupport,
    HistoricalVersionVisibility, HistoryWindow, PolicySnapshotId, ReadTimestamp, RetentionFloor,
};
pub use query::{
    AggregationOperator, CollectionSelector, CompositeFilter, CompositeOperator, CountAggregation,
    Cursor, DistanceMeasure, FieldFilter, FieldFilterOperator, FieldReference, Filter, FilterOp,
    FindNearest, OrderBy, OrderDirection, Page, PaginatedQuery, Projection, Query, QueryDirection,
    QueryFilter, StructuredAggregation, StructuredAggregationQuery, StructuredAggregationResult,
    StructuredCursor, StructuredOrder, StructuredQuery, UnaryFilter, UnaryFilterOperator,
};
pub use resource_path::{
    CollectionName, CollectionPath, CollectionPathSegment, DocumentLocator, DocumentPath,
    DocumentTriggerMatch, DocumentTriggerPattern, ResourcePathBinding,
};
pub use scheduled::{
    CreateCronRequest, CronJob, CronSchedule, JobId, ScheduleRequest, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult,
};
pub use schema::{FieldSchema, FieldType, IndexDefinition, IndexState, Schema, TableSchema};
pub use subscription::{
    SubscriptionCommitMetadata, SubscriptionDocumentChange, SubscriptionDocumentChangeKind,
    SubscriptionResultSnapshot, SubscriptionSnapshotDiff, diff_subscription_snapshots,
};
pub use transaction::{TransactionSession, TransactionSessionMode, TransactionSessionToken};
pub use trigger::{
    CloudEventSpecVersion, DocumentEventData, DocumentEventDocument, DocumentEventUpdateMask,
    FirestoreCloudEventType, FirestoreTriggerMetadata, TriggerCloudEvent, TriggerCommitMetadata,
    TriggerDeliveryCursor, TriggerEvent, TriggerExecutionPrincipal, TriggerInvocationAncestry,
    TriggerInvocationKey, TriggerInvocationRecord, TriggerInvocationState, TriggerWriteOrigin,
};
pub use typed_scalar::{NumericValue, SpecialDouble, StoredValue, TypedFieldMap, TypedScalarValue};
pub use types::{
    DocumentId, IndexId, ResolvedDocumentId, SequenceNumber, TableId, TableName, TableState,
    TenantId, Timestamp,
};
pub use versioned_registry::{HistoricalReadShape, VersionedRegistry};
pub use visibility::{PinnedServingSnapshot, ReadVisibility, RequiredSequence};
pub use write_batch::{
    ArrayPopSide, AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, AtomicWriteResult,
    BitwiseOperation, FieldTransform, FieldTransformOperation, WriteKey, WritePrecondition,
    WriteSetMode,
};

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::str::FromStr;

    use crate::{
        CommitEntry, Document, DocumentId, DurableMutationRecord, OrderBy, OrderDirection, Query,
        ResolvedDocumentId, SequenceNumber, TableId, TableName, TenantId, Timestamp, WriteOp,
        WriteOpType,
    };

    #[test]
    fn document_id_roundtrip() {
        let id = DocumentId::new();
        let parsed = DocumentId::from_str(&id.to_string()).expect("document id should parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn document_id_accepts_firestore_style_keys() {
        let numeric = DocumentId::from_str("1").expect("numeric id should parse");
        let dotted = DocumentId::from_str("alpha.beta").expect("dotted id should parse");
        let unicode = DocumentId::from_str("東京").expect("unicode id should parse");

        assert_eq!(numeric.to_string(), "1");
        assert_eq!(dotted.to_string(), "alpha.beta");
        assert_eq!(unicode.to_string(), "東京");
    }

    #[test]
    fn document_id_rejects_invalid_keys() {
        let empty = DocumentId::from_str("");
        let nested = DocumentId::from_str("cities/SF");
        let nul = DocumentId::from_key("fire\u{0000}store".to_string());

        assert!(matches!(empty, Err(crate::Error::InvalidInput(_))));
        assert!(matches!(nested, Err(crate::Error::InvalidInput(_))));
        assert!(matches!(nul, Err(crate::Error::InvalidInput(_))));
    }

    #[test]
    fn resolved_document_id_round_trips_table_scoped_ids() {
        let table = TableName::new("messages").expect("table should parse");
        let raw_id = DocumentId::from_key("custom:id").expect("raw id should parse");
        let scoped = ResolvedDocumentId::encode_table_scoped(&table, &raw_id)
            .expect("scoped id should encode");

        let resolved = ResolvedDocumentId::resolve_table_scoped(&table, scoped)
            .expect("scoped id should resolve");

        assert_eq!(resolved.table(), &table);
        assert_eq!(resolved.document_id(), &raw_id);
    }

    #[test]
    fn resolved_document_id_rejects_wrong_table() {
        let messages = TableName::new("messages").expect("table should parse");
        let users = TableName::new("users").expect("table should parse");
        let raw_id = DocumentId::new();
        let scoped = ResolvedDocumentId::encode_table_scoped(&messages, &raw_id)
            .expect("scoped id should encode");

        let error = ResolvedDocumentId::resolve_table_scoped(&users, scoped)
            .expect_err("wrong-table id should fail");

        assert!(
            error.to_string().contains("belongs to table messages"),
            "wrong-table error should name the encoded table: {error}"
        );
    }

    #[test]
    fn table_id_roundtrip() {
        let id = TableId::new();
        let parsed = TableId::from_str(id.as_str()).expect("table id should parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn table_state_parses_canonical_lifecycle_values() {
        assert_eq!(
            crate::TableState::from_str("active").expect("active should parse"),
            crate::TableState::Active
        );
        assert_eq!(
            crate::TableState::from_str("hidden").expect("hidden should parse"),
            crate::TableState::Hidden
        );
        assert_eq!(
            crate::TableState::from_str("deleting").expect("deleting should parse"),
            crate::TableState::Deleting
        );
        assert!(matches!(
            crate::TableState::from_str("archived"),
            Err(crate::Error::InvalidInput(_))
        ));
    }

    #[test]
    fn mutation_insert_rejects_invalid_document_key_during_deserialization() {
        let mutation = serde_json::json!({
            "op": "insert",
            "table": "tasks",
            "id": "cities/SF",
            "fields": {
                "title": "Hello"
            }
        });

        let error = serde_json::from_value::<crate::Mutation>(mutation)
            .expect_err("invalid id should fail");

        assert!(matches!(
            error.classify(),
            serde_json::error::Category::Data
        ));
    }

    #[test]
    fn document_to_json_includes_system_fields() {
        let document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        );

        let value = document.to_json();
        assert_eq!(value["title"], json!("Hello"));
        assert!(value["_id"].is_string());
        assert!(value["_creationTime"].is_u64());
        assert!(value["_updateTime"].is_u64());
    }

    #[test]
    fn document_into_json_matches_borrowed_conversion() {
        let document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Hello")),
                ("done".to_string(), json!(true)),
            ]),
        );

        assert_eq!(document.clone().into_json(), document.to_json());
    }

    #[test]
    fn query_serialization_roundtrip() {
        let query = Query {
            table: TableName::new("tasks").expect("table name should be valid"),
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "title".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: Some(10),
        };

        let serialized = serde_json::to_string(&query).expect("query should serialize");
        let deserialized: Query =
            serde_json::from_str(&serialized).expect("query should deserialize");
        assert_eq!(query, deserialized);
    }

    #[test]
    fn logical_names_reject_unsafe_characters() {
        let tenant = TenantId::new("../demo");
        let table = TableName::new("tasks/alpha");

        assert!(tenant.is_err());
        assert!(table.is_err());
    }

    #[test]
    fn commit_entry_affected_tables_deduplicates_table_names() {
        let tasks = TableName::new("tasks").expect("table name should be valid");
        let tasks_id = TableId::new();
        let users = TableName::new("users").expect("table name should be valid");
        let users_id = TableId::new();
        let entry = CommitEntry {
            sequence: SequenceNumber(1),
            timestamp: Timestamp(123),
            writes: vec![
                WriteOp {
                    table: tasks.clone(),
                    table_id: tasks_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: DocumentId::new(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: None,
                },
                WriteOp {
                    table: tasks.clone(),
                    table_id: tasks_id.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: DocumentId::new(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: None,
                },
                WriteOp {
                    table: users.clone(),
                    table_id: users_id.clone(),
                    op_type: WriteOpType::Delete,
                    doc_id: DocumentId::new(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: None,
                },
            ],
        };

        let affected = entry.affected_tables();
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&tasks));
        assert!(affected.contains(&users));

        let affected_ids = entry.affected_table_ids();
        assert_eq!(affected_ids.len(), 2);
        assert!(affected_ids.contains(&tasks_id));
        assert!(affected_ids.contains(&users_id));
    }

    #[test]
    fn durable_mutation_record_roundtrips_and_verifies_integrity() {
        let record = DurableMutationRecord::new(
            SequenceNumber(9),
            Timestamp(42),
            vec![WriteOp {
                table: TableName::new("tasks").expect("table name should be valid"),
                table_id: TableId::new(),
                op_type: WriteOpType::Insert,
                doc_id: DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(Document::new(
                    TableName::new("tasks").expect("table name should be valid"),
                    serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
                )),
            }],
            Some("scheduled:demo".to_string()),
        )
        .expect("record should build");

        let encoded = rmp_serde::to_vec(&record).expect("record should serialize");
        let decoded: DurableMutationRecord =
            rmp_serde::from_slice(&encoded).expect("record should deserialize");

        decoded
            .validate_integrity()
            .expect("record integrity should verify");
        assert_eq!(decoded.as_commit_entry().sequence, SequenceNumber(9));
    }

    #[test]
    fn durable_mutation_record_without_scheduler_id_roundtrips_and_verifies_integrity() {
        let record = DurableMutationRecord::new(
            SequenceNumber(10),
            Timestamp(43),
            vec![WriteOp {
                table: TableName::new("tasks").expect("table name should be valid"),
                table_id: TableId::new(),
                op_type: WriteOpType::Update,
                doc_id: DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(Document::new(
                    TableName::new("tasks").expect("table name should be valid"),
                    serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
                )),
                current: Some(Document::new(
                    TableName::new("tasks").expect("table name should be valid"),
                    serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
                )),
            }],
            None,
        )
        .expect("record should build");

        let encoded = rmp_serde::to_vec(&record).expect("record should serialize");
        let decoded: DurableMutationRecord =
            rmp_serde::from_slice(&encoded).expect("record should deserialize");

        decoded
            .validate_integrity()
            .expect("record integrity should verify");
        assert_eq!(decoded.as_commit_entry().sequence, SequenceNumber(10));
    }
}
