use std::sync::Arc;

use nimbus_core::{Document, Result};
use nimbus_engine::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::identity::system_tenant_id;
use crate::keys::{bundle_document_id, function_document_id};
use crate::schema::SystemTable;

use super::{
    ensure_system_tenant_async, object_fields, query_system_documents_by_eq_async,
    upsert_system_document_async,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentRecordInput<'a> {
    pub source_ref: &'a str,
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

pub async fn record_deployment_state_async(
    engine: &Arc<Engine>,
    input: &SystemDeploymentRecordInput<'_>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let bundle_sha256 = deployment_bundle_sha256(input);
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
    delete_stale_deployment_documents_async(engine, &bundle_sha256, &active_function_ids).await
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

fn deployment_bundle_sha256(input: &SystemDeploymentRecordInput<'_>) -> String {
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
