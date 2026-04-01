use super::*;
use neovex_core::{Filter, FilterOp, Query, TableName};
use serde_json::Value;

#[test]
fn synthesize_runtime_subscription_base_queries_keeps_disjoint_same_table_predicates() {
    let table = TableName::new("messages").expect("table should be valid");
    let mut read_set = RuntimeReadSet::default();
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }],
    );
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Bob".to_string()),
        }],
    );

    let queries =
        synthesize_runtime_subscription_base_queries(&read_set).expect("queries should synthesize");

    assert_eq!(queries.len(), 2);
    assert!(queries.iter().all(|query| query.table == table));
    assert!(queries.iter().any(|query| query.filters
        == vec![Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }]));
    assert!(queries.iter().any(|query| query.filters
        == vec![Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Bob".to_string()),
        }]));
}

#[test]
fn synthesize_runtime_subscription_base_queries_prefers_broad_query_for_full_table_reads() {
    let table = TableName::new("messages").expect("table should be valid");
    let mut read_set = RuntimeReadSet::default();
    read_set.record_table(&table);
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }],
    );

    let queries =
        synthesize_runtime_subscription_base_queries(&read_set).expect("queries should synthesize");

    assert_eq!(
        queries,
        vec![Query {
            table,
            filters: Vec::new(),
            order: None,
            limit: None,
        }]
    );
}
