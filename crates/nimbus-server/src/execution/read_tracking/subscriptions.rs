use nimbus_core::{Error, Query, TableName};

use super::intersection::filters_from_runtime_index_read;
use super::read_set::RuntimeReadSet;

pub(crate) fn synthesize_runtime_subscription_base_queries(
    read_set: &RuntimeReadSet,
) -> Result<Vec<Query>, Error> {
    let mut tables = read_set.tables().into_iter().collect::<Vec<_>>();
    tables.sort();

    if tables.is_empty() {
        return Err(Error::InvalidInput(
            "runtime-backed live subscriptions require at least one table-backed read".to_string(),
        ));
    }

    let mut queries = Vec::new();
    for table in tables {
        for query in synthesize_runtime_subscription_base_queries_for_table(read_set, &table) {
            if !queries.contains(&query) {
                queries.push(query);
            }
        }
    }

    Ok(queries)
}

fn synthesize_runtime_subscription_base_queries_for_table(
    read_set: &RuntimeReadSet,
    table: &TableName,
) -> Vec<Query> {
    if read_set.tables.contains(table) {
        return vec![broad_runtime_subscription_query(table.clone())];
    }

    let predicates = read_set
        .predicates
        .iter()
        .filter(|predicate| &predicate.table == table)
        .collect::<Vec<_>>();
    let index_ranges = read_set
        .index_ranges
        .iter()
        .filter(|range| &range.table == table)
        .collect::<Vec<_>>();
    let paginated_windows = read_set
        .paginated_windows
        .iter()
        .filter(|read| &read.table == table)
        .collect::<Vec<_>>();

    let mut queries = Vec::new();

    for predicate in predicates {
        queries.push(Query {
            table: table.clone(),
            filters: predicate.filters.clone(),
            order: None,
            limit: None,
        });
    }

    for index_range in index_ranges {
        queries.push(Query {
            table: table.clone(),
            filters: filters_from_runtime_index_read(index_range),
            order: None,
            limit: None,
        });
    }

    for paginated_window in paginated_windows {
        queries.push(Query {
            table: table.clone(),
            filters: paginated_window.filters.clone(),
            order: None,
            limit: None,
        });
    }

    if queries.is_empty()
        && read_set
            .documents
            .iter()
            .any(|(document_table, _)| document_table == table)
    {
        queries.push(broad_runtime_subscription_query(table.clone()));
    }

    if queries.is_empty() {
        queries.push(broad_runtime_subscription_query(table.clone()));
    }

    queries
}

fn broad_runtime_subscription_query(table: TableName) -> Query {
    Query {
        table,
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}
