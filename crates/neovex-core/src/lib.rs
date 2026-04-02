//! Core types for Neovex.

pub mod auth;
pub mod dependency;
pub mod document;
pub mod error;
pub mod mutation;
pub mod query;
pub mod scheduled;
pub mod schema;
pub mod types;

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
pub use error::{Error, Result};
pub use mutation::{CommitEntry, DurableMutationRecord, Mutation, WriteOp, WriteOpType};
pub use query::{Cursor, Filter, FilterOp, OrderBy, OrderDirection, Page, PaginatedQuery, Query};
pub use scheduled::{
    CreateCronRequest, CronJob, CronSchedule, JobId, ScheduleRequest, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult,
};
pub use schema::{FieldSchema, FieldType, IndexDefinition, Schema, TableSchema};
pub use types::{DocumentId, SequenceNumber, TableName, TenantId, Timestamp};

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
    fn document_to_json_includes_system_fields() {
        let document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        );

        let value = document.to_json();
        assert_eq!(value["title"], json!("Hello"));
        assert!(value["_id"].is_string());
        assert!(value["_creationTime"].is_u64());
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
    fn document_msgpack_roundtrip_preserves_all_fields() {
        let document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Hello")),
                ("rank".to_string(), json!(2)),
                ("active".to_string(), json!(true)),
            ]),
        );

        let bytes = document.to_msgpack().expect("document should serialize");
        let decoded = Document::from_msgpack(&bytes).expect("document should deserialize");

        assert_eq!(decoded, document);
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
                    previous: None,
                    current: None,
                },
                WriteOp {
                    table: tasks.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: DocumentId::new(),
                    previous: None,
                    current: None,
                },
                WriteOp {
                    table: users.clone(),
                    op_type: WriteOpType::Delete,
                    doc_id: DocumentId::new(),
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
