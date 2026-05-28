use nimbus_core::{Error, Query};
use serde_json::Value;

use super::super::types::ConvexSubscriptionTransform;
use crate::{
    ConvexExecutableQuery, ConvexFunctionKind, ConvexReadCommand, ConvexRegistry,
    resolve_convex_document_id,
};

fn subscription_plan_for_query(
    query: ConvexExecutableQuery,
) -> Result<(Query, ConvexSubscriptionTransform), Error> {
    match query {
        ConvexExecutableQuery::Query(query) => Ok((query, ConvexSubscriptionTransform::Identity)),
        ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => {
            let document_id = resolve_convex_document_id(&table, id)?.into_document_id();
            Ok((
                Query {
                    table,
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
                ConvexSubscriptionTransform::Get { document_id },
            ))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::First { query }) => {
            Ok((query, ConvexSubscriptionTransform::First))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
            Ok((query, ConvexSubscriptionTransform::Unique))
        }
    }
}

pub fn subscription_plan_for_named_query(
    registry: &ConvexRegistry,
    name: &str,
    args: &Value,
    page_size: Option<usize>,
    cursor: Option<String>,
    query: ConvexExecutableQuery,
) -> Result<(Query, ConvexSubscriptionTransform), Error> {
    let (base_query, transform) = subscription_plan_for_query(query)?;
    let Some(definition) = registry.functions.get(name) else {
        return Ok((base_query, transform));
    };
    if registry.runtime_bundle().is_none() {
        return Ok((base_query, transform));
    }

    match definition.kind {
        ConvexFunctionKind::Query => Ok((
            base_query,
            ConvexSubscriptionTransform::RuntimeNamedQuery {
                name: name.to_string(),
                args: args.clone(),
                auth: None,
                services: Default::default(),
                read_set: None,
                last_value: None,
            },
        )),
        ConvexFunctionKind::PaginatedQuery => {
            if let Some(page_size) = page_size {
                Ok((
                    base_query,
                    ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                        name: name.to_string(),
                        args: args.clone(),
                        page_size,
                        cursor,
                        auth: None,
                        services: Default::default(),
                        read_set: None,
                        last_value: None,
                    },
                ))
            } else {
                Ok((base_query, transform))
            }
        }
        _ => Ok((base_query, transform)),
    }
}
