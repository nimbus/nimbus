//! Core types for Neovex.

pub mod auth;
pub mod dependency;
pub mod document;
pub mod error;
pub mod mutation;
pub mod query;
pub mod resource_path;
pub mod scheduled;
pub mod schema;
pub mod subscription;
pub mod transaction;
pub mod trigger;
pub mod typed_scalar;
pub mod types;
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
pub use error::{Error, Result, StorageErrorKind};
pub use mutation::{CommitEntry, DurableMutationRecord, Mutation, WriteOp, WriteOpType};
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
pub use schema::{FieldSchema, FieldType, IndexDefinition, Schema, TableSchema};
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
pub use types::{DocumentId, SequenceNumber, TableName, TenantId, Timestamp};
pub use write_batch::{
    AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, AtomicWriteResult, FieldTransform,
    FieldTransformOperation, WriteKey, WritePrecondition, WriteSetMode,
};

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::str::FromStr;

    use crate::{
        CommitEntry, Document, DocumentId, DurableMutationRecord, OrderBy, OrderDirection, Query,
        SequenceNumber, TableName, TenantId, Timestamp, WriteOp, WriteOpType,
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
        let users = TableName::new("users").expect("table name should be valid");
        let entry = CommitEntry {
            sequence: SequenceNumber(1),
            timestamp: Timestamp(123),
            writes: vec![
                WriteOp {
                    table: tasks.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: DocumentId::new(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: None,
                },
                WriteOp {
                    table: tasks.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: DocumentId::new(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: None,
                },
                WriteOp {
                    table: users.clone(),
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
    }

    #[test]
    fn durable_mutation_record_roundtrips_and_verifies_integrity() {
        let record = DurableMutationRecord::new(
            SequenceNumber(9),
            Timestamp(42),
            vec![WriteOp {
                table: TableName::new("tasks").expect("table name should be valid"),
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
