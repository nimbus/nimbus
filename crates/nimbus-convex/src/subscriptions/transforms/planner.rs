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
            // Subscription results are delivered in the Convex wire format,
            // whose `_id` values are table-scoped — keep the validated scoped
            // id so the Get transform matches what the forwarder emits.
            resolve_convex_document_id(&table, id.clone())?;
            Ok((
                Query {
                    table,
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
                ConvexSubscriptionTransform::Get { document_id: id },
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
    // When the app has a runtime bundle, the bundle's `__nimbusInvoke` is the
    // authoritative evaluator for every named function — plan-backed ones
    // included — so subscription re-evaluation routes through it. The compiled
    // plan still provides the exact base query used for invalidation.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    fn registry_with_compiled_plan(include_runtime_bundle: bool) -> ConvexRegistry {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": "messages:compiledPlan",
                        "kind": "query",
                        "visibility": "public",
                        "plan": {
                            "table": "messages",
                            "filters": [],
                            "order": null,
                            "limit": 20
                        },
                        "runtime_handler": null
                    },
                    {
                        "name": "messages:runtimeOnly",
                        "kind": "query",
                        "visibility": "public",
                        "plan": null,
                        "runtime_handler": "async () => []"
                    }
                ]
            }))
            .expect("convex manifest json should serialize"),
        )
        .expect("convex manifest should write");
        fs::write(
            convex_dir.join("http_routes.json"),
            serde_json::to_vec_pretty(&json!({ "routes": [] }))
                .expect("convex http route manifest should serialize"),
        )
        .expect("convex http route manifest should write");
        if include_runtime_bundle {
            let bundle_path = convex_dir.join("bundle.mjs");
            fs::write(
                &bundle_path,
                "globalThis.__nimbusInvoke = async function () { return { status: \"ok\", value: [] }; }; export {};",
            )
            .expect("convex runtime bundle should write");
            let bundle_hash = nimbus_runtime::RuntimeBundle::compute_sha256_for_path(&bundle_path)
                .expect("convex runtime bundle hash should compute");
            fs::write(bundle_path.with_extension("sha256"), bundle_hash)
                .expect("convex runtime bundle hash should write");
        }

        ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load")
    }

    fn registry_with_runtime_bundle_and_compiled_plan() -> ConvexRegistry {
        registry_with_compiled_plan(true)
    }

    #[test]
    fn compiled_plan_subscription_stays_direct_without_runtime_bundle() {
        let registry = registry_with_compiled_plan(false);
        let args = json!({});
        let query = registry
            .resolve_subscription_query("messages:compiledPlan", &args)
            .expect("compiled plan query should resolve");
        let (base_query, transform) = subscription_plan_for_named_query(
            &registry,
            "messages:compiledPlan",
            &args,
            None,
            None,
            query,
        )
        .expect("compiled plan subscription should plan");

        assert_eq!(base_query.table.as_str(), "messages");
        assert!(
            matches!(transform, ConvexSubscriptionTransform::Identity),
            "without a runtime bundle, plan-backed queries re-evaluate natively, got {transform:?}"
        );
    }

    #[test]
    fn compiled_plan_subscription_reevaluates_through_runtime_bundle_when_present() {
        let registry = registry_with_runtime_bundle_and_compiled_plan();
        let args = json!({});
        let query = registry
            .resolve_subscription_query("messages:compiledPlan", &args)
            .expect("compiled plan query should resolve");
        let (base_query, transform) = subscription_plan_for_named_query(
            &registry,
            "messages:compiledPlan",
            &args,
            None,
            None,
            query,
        )
        .expect("compiled plan subscription should plan");

        assert_eq!(base_query.table.as_str(), "messages");
        assert!(
            matches!(
                transform,
                ConvexSubscriptionTransform::RuntimeNamedQuery { ref name, .. } if name == "messages:compiledPlan"
            ),
            "with a runtime bundle present, the bundle is the authoritative evaluator even for plan-backed queries, got {transform:?}"
        );
    }

    #[test]
    fn runtime_only_subscription_uses_runtime_named_query_transform() {
        let registry = registry_with_runtime_bundle_and_compiled_plan();
        let args = json!({});
        // Runtime-only functions carry no compiled plan to resolve, so the
        // planner is exercised with a synthesized base query, the way a
        // runtime-backed caller would provide one.
        let query = ConvexExecutableQuery::Query(Query {
            table: nimbus_core::TableName::new("messages".to_string())
                .expect("table name should validate"),
            filters: Vec::new(),
            order: None,
            limit: None,
        });
        let (_, transform) = subscription_plan_for_named_query(
            &registry,
            "messages:runtimeOnly",
            &args,
            None,
            None,
            query,
        )
        .expect("runtime-only subscription should plan");

        assert!(
            matches!(
                transform,
                ConvexSubscriptionTransform::RuntimeNamedQuery { ref name, .. } if name == "messages:runtimeOnly"
            ),
            "runtime-only queries must re-evaluate through the runtime bundle, got {transform:?}"
        );
    }
}
