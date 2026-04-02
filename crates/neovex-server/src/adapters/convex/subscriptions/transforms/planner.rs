use neovex_core::Query;
use serde_json::Value;

use super::super::types::ConvexSubscriptionTransform;
use crate::adapters::convex::{
    ConvexExecutableQuery, ConvexFunctionKind, ConvexReadCommand, ConvexRegistry,
};

fn subscription_plan_for_query(
    query: ConvexExecutableQuery,
) -> (Query, ConvexSubscriptionTransform) {
    match query {
        ConvexExecutableQuery::Query(query) => (query, ConvexSubscriptionTransform::Identity),
        ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => (
            Query {
                table,
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            ConvexSubscriptionTransform::Get { document_id: id },
        ),
        ConvexExecutableQuery::Read(ConvexReadCommand::First { query }) => {
            (query, ConvexSubscriptionTransform::First)
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
            (query, ConvexSubscriptionTransform::Unique)
        }
    }
}

pub(in crate::adapters::convex::subscriptions) fn subscription_plan_for_named_query(
    registry: &ConvexRegistry,
    name: &str,
    args: &Value,
    page_size: Option<usize>,
    cursor: Option<String>,
    query: ConvexExecutableQuery,
) -> (Query, ConvexSubscriptionTransform) {
    let (base_query, transform) = subscription_plan_for_query(query);
    let Some(definition) = registry.functions.get(name) else {
        return (base_query, transform);
    };
    if registry.runtime_bundle().is_none() {
        return (base_query, transform);
    }

    match definition.kind {
        ConvexFunctionKind::Query => (
            base_query,
            ConvexSubscriptionTransform::RuntimeNamedQuery {
                name: name.to_string(),
                args: args.clone(),
                auth: None,
                read_set: None,
                last_value: None,
            },
        ),
        ConvexFunctionKind::PaginatedQuery => {
            if let Some(page_size) = page_size {
                (
                    base_query,
                    ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                        name: name.to_string(),
                        args: args.clone(),
                        page_size,
                        cursor,
                        auth: None,
                        read_set: None,
                        last_value: None,
                    },
                )
            } else {
                (base_query, transform)
            }
        }
        _ => (base_query, transform),
    }
}
