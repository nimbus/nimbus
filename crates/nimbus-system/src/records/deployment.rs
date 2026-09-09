use std::sync::Arc;

use nimbus_core::{Document, Result};
use nimbus_engine::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::identity::system_tenant_id;
use crate::keys::{bundle_document_id, deploy_document_id, function_document_id};
use crate::schema::SystemTable;

use super::{
    ensure_system_tenant_async, object_fields, query_system_documents_by_eq_async,
    unix_time_millis, upsert_system_document_async,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentRecordInput<'a> {
    pub source_ref: &'a str,
    /// Who activated the bundle: `server` at startup, `deploy-admin` for a
    /// deploy, the operator principal for a rollback.
    pub actor: &'a str,
    /// `startup`, `deploy`, or `rollback`.
    pub kind: &'a str,
    pub generation: u64,
    /// The Convex silo the bundle's auth verifier was bound to, when any.
    pub silo: Option<&'a str>,
    pub functions: Vec<SystemDeploymentFunctionRecordInput<'a>>,
    pub http_routes: Vec<SystemDeploymentHttpRouteRecordInput<'a>>,
    pub schema_fingerprint: Option<&'a str>,
    pub index_fingerprint: Option<&'a str>,
    pub runtime_bundle_fingerprint: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentFunctionRecordInput<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub fingerprint: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentHttpRouteRecordInput<'a> {
    pub key: &'a str,
    pub fingerprint: &'a str,
}

/// One recorded bundle activation, as the `deploys` table holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentActivation {
    pub sha256: String,
    pub generation: u64,
    pub activated_at_ms: u64,
    pub actor: String,
    pub source_ref: String,
    pub kind: String,
    pub silo: Option<String>,
    pub functions: Vec<SystemDeploymentActivationFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentActivationFunction {
    pub path: String,
    pub kind: String,
}

/// Projects an activation: the active `bundles` and `functions` inventory is
/// replaced, and one `deploys` row is appended. Returns the bundle SHA-256
/// the activation was recorded under.
pub async fn record_deployment_state_async(
    engine: &Arc<Engine>,
    input: &SystemDeploymentRecordInput<'_>,
) -> Result<String> {
    ensure_system_tenant_async(engine).await?;
    let bundle_sha256 = deployment_bundle_sha256(input);
    let activated_at_ms = unix_time_millis()?;
    let mut activation = object_fields(json!({
        "sha256": bundle_sha256.as_str(),
        "generation": input.generation,
        "activatedAt": activated_at_ms,
        "actor": input.actor,
        "sourceRef": input.source_ref,
        "kind": input.kind,
        "functions": input
            .functions
            .iter()
            .map(|function| json!({ "path": function.name, "kind": function.kind }))
            .collect::<Vec<_>>(),
        "functionCount": input.functions.len(),
    }));
    // An optional system field is absent, never null.
    if let Some(silo) = input.silo {
        activation.insert("silo".to_owned(), json!(silo));
    }
    upsert_system_document_async(
        engine,
        SystemTable::Deploys,
        &deploy_document_id(activated_at_ms, &bundle_sha256),
        activation,
    )
    .await?;
    upsert_system_document_async(
        engine,
        SystemTable::Bundles,
        &bundle_document_id(&bundle_sha256),
        object_fields(json!({
            "sha256": bundle_sha256.as_str(),
            "sourceRef": input.source_ref,
            "status": "active",
        })),
    )
    .await?;

    let active_function_ids = input
        .functions
        .iter()
        .map(|function| function_document_id(&bundle_sha256, function.name))
        .collect::<std::collections::BTreeSet<_>>();
    for function in &input.functions {
        upsert_system_document_async(
            engine,
            SystemTable::Functions,
            &function_document_id(&bundle_sha256, function.name),
            object_fields(json!({
                "bundleId": bundle_sha256.as_str(),
                "path": function.name,
                "kind": function.kind,
            })),
        )
        .await?;
    }
    delete_stale_deployment_documents_async(engine, &bundle_sha256, &active_function_ids).await?;
    Ok(bundle_sha256)
}

/// Every recorded activation, newest first.
pub async fn deployment_history_async(
    engine: &Arc<Engine>,
) -> Result<Vec<SystemDeploymentActivation>> {
    let documents = engine
        .list_documents_async(system_tenant_id()?, SystemTable::Deploys.table_name()?)
        .await?;
    let mut activations = documents
        .iter()
        .filter_map(|document| activation_from_fields(&document.fields))
        .collect::<Vec<_>>();
    activations.sort_by(|a, b| {
        b.activated_at_ms
            .cmp(&a.activated_at_ms)
            .then(b.generation.cmp(&a.generation))
    });
    Ok(activations)
}

fn activation_from_fields(
    fields: &serde_json::Map<String, Value>,
) -> Option<SystemDeploymentActivation> {
    let string = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
    let functions = fields
        .get("functions")
        .and_then(Value::as_array)
        .map(|functions| {
            functions
                .iter()
                .filter_map(|function| {
                    Some(SystemDeploymentActivationFunction {
                        path: function.get("path")?.as_str()?.to_owned(),
                        kind: function.get("kind")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SystemDeploymentActivation {
        sha256: string("sha256")?,
        generation: fields.get("generation").and_then(Value::as_u64)?,
        activated_at_ms: fields.get("activatedAt").and_then(Value::as_u64)?,
        actor: string("actor")?,
        source_ref: string("sourceRef")?,
        kind: string("kind")?,
        silo: string("silo"),
        functions,
    })
}

async fn delete_stale_deployment_documents_async(
    engine: &Arc<Engine>,
    active_bundle_sha256: &str,
    active_function_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let bundles_table = SystemTable::Bundles.table_name()?;
    let bundles = query_system_documents_by_eq_async(
        engine,
        SystemTable::Bundles,
        [("status", json!("active"))],
    )
    .await?;
    for bundle in bundles {
        let Some(bundle_sha256) = bundle.fields.get("sha256").and_then(Value::as_str) else {
            engine
                .delete_document_async(system_tenant.clone(), bundles_table.clone(), bundle.id)
                .await?;
            continue;
        };
        if bundle_sha256 == active_bundle_sha256 {
            continue;
        }
        delete_functions_for_bundle_async(engine, bundle_sha256, |_| true).await?;
        engine
            .delete_document_async(system_tenant.clone(), bundles_table.clone(), bundle.id)
            .await?;
    }

    delete_functions_for_bundle_async(engine, active_bundle_sha256, |function| {
        !active_function_ids.contains(&function.id.to_string())
    })
    .await?;

    Ok(())
}

async fn delete_functions_for_bundle_async(
    engine: &Arc<Engine>,
    bundle_sha256: &str,
    should_delete: impl Fn(&Document) -> bool,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let functions_table = SystemTable::Functions.table_name()?;
    let functions = query_system_documents_by_eq_async(
        engine,
        SystemTable::Functions,
        [("bundleId", json!(bundle_sha256))],
    )
    .await?;
    for function in functions {
        if should_delete(&function) {
            engine
                .delete_document_async(system_tenant.clone(), functions_table.clone(), function.id)
                .await?;
        }
    }
    Ok(())
}

/// The SHA-256 an activation is recorded under: the runtime bundle's own
/// provenance hash when the deploy carried one, otherwise a stable digest of
/// the function, route, schema and index fingerprints.
pub fn deployment_bundle_sha256(input: &SystemDeploymentRecordInput<'_>) -> String {
    if let Some(fingerprint) = input.runtime_bundle_fingerprint {
        return fingerprint.to_owned();
    }

    let mut hasher = Sha256::new();
    hasher.update(b"nimbus-system-deployment-record-v1");
    for function in &input.functions {
        hasher.update(function.name.as_bytes());
        hasher.update([0]);
        hasher.update(function.kind.as_bytes());
        hasher.update([0]);
        hasher.update(function.fingerprint.as_bytes());
        hasher.update([0]);
    }
    for route in &input.http_routes {
        hasher.update(route.key.as_bytes());
        hasher.update([0]);
        hasher.update(route.fingerprint.as_bytes());
        hasher.update([0]);
    }
    if let Some(fingerprint) = input.schema_fingerprint {
        hasher.update(fingerprint.as_bytes());
    }
    hasher.update([0]);
    if let Some(fingerprint) = input.index_fingerprint {
        hasher.update(fingerprint.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}
