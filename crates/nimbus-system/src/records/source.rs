use std::sync::Arc;

use nimbus_core::{Document, Result};
use nimbus_engine::Engine;
use serde_json::{Value, json};

use crate::identity::system_tenant_id;
use crate::keys::{module_document_id, source_package_document_id};
use crate::schema::SystemTable;
use crate::source_package::parse_source_package;
use crate::source_store::SourcePackageStore;

use super::{
    ensure_system_tenant_async, object_fields, query_system_documents_by_eq_async,
    upsert_system_document_async,
};

/// The deploy-captured source package (the read-artifact behind the console
/// Source view) and its modules. The bytes themselves are persisted separately
/// in the content-addressed source-package store; this projects the metadata
/// rows that let the console resolve `module:function` -> module -> package.
/// See the Function Source Visibility plan (FSV3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSourcePackageRecordInput<'a> {
    pub digest: &'a str,
    pub storage_key: &'a str,
    pub size_bytes: u64,
    pub unpacked_bytes: u64,
    pub modules: Vec<SystemModuleRecordInput<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemModuleRecordInput<'a> {
    pub path: &'a str,
    pub sha256: &'a str,
}

/// Project a deployed source package and its modules into the system tenant,
/// then GC any prior package (and its modules) so the console always reflects
/// the active deployment. Re-recording the same digest is idempotent.
pub async fn record_source_package_state_async(
    engine: &Arc<Engine>,
    input: &SystemSourcePackageRecordInput<'_>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;

    upsert_system_document_async(
        engine,
        SystemTable::SourcePackages,
        &source_package_document_id(input.digest),
        object_fields(json!({
            "digest": input.digest,
            "storageKey": input.storage_key,
            "sizeBytes": input.size_bytes,
            "unpackedBytes": input.unpacked_bytes,
            "status": "active",
        })),
    )
    .await?;

    let active_module_ids = input
        .modules
        .iter()
        .map(|module| module_document_id(input.digest, module.path))
        .collect::<std::collections::BTreeSet<_>>();
    for module in &input.modules {
        upsert_system_document_async(
            engine,
            SystemTable::Modules,
            &module_document_id(input.digest, module.path),
            object_fields(json!({
                "path": module.path,
                "sourcePackageId": input.digest,
                "sha256": module.sha256,
            })),
        )
        .await?;
    }

    delete_stale_source_package_documents_async(engine, input.digest, &active_module_ids).await
}

/// A module's source resolved from the active source package, for the console
/// Source view. Read path (FSV4): module path -> `sourcePackageId` -> CAS bytes
/// (hash-verified by the store) -> the module's source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSource {
    pub path: String,
    pub source: String,
    pub source_map: Option<String>,
    pub type_info: Option<Value>,
    pub digest: String,
}

/// Resolve a module's source from the content-addressed store, or `None` when
/// the module is unknown. The store verifies the bytes against the digest, so a
/// tampered package fails closed rather than serving wrong source.
pub async fn read_module_source_async(
    engine: &Arc<Engine>,
    store: &dyn SourcePackageStore,
    module_path: &str,
) -> Result<Option<ModuleSource>> {
    let modules = query_system_documents_by_eq_async(
        engine,
        SystemTable::Modules,
        [("path", json!(module_path))],
    )
    .await?;
    let Some(module) = modules.into_iter().next() else {
        return Ok(None);
    };
    let Some(digest) = module.fields.get("sourcePackageId").and_then(Value::as_str) else {
        return Ok(None);
    };
    let bytes = store.get(digest)?;
    let parsed = parse_source_package(&bytes)?;
    let Some(found) = parsed
        .modules
        .into_iter()
        .find(|candidate| candidate.path == module_path)
    else {
        return Ok(None);
    };
    Ok(Some(ModuleSource {
        path: found.path,
        source: found.source,
        source_map: found.source_map,
        type_info: found.type_info,
        digest: digest.to_owned(),
    }))
}

/// All modules (path + source) in the source package that contains
/// `module_path`. Backs the cross-module call graph ("called by"); empty when
/// the module is unknown. See the Function Source Visibility plan (FSV7).
pub async fn read_source_package_modules_async(
    engine: &Arc<Engine>,
    store: &dyn SourcePackageStore,
    module_path: &str,
) -> Result<Vec<(String, String)>> {
    let modules = query_system_documents_by_eq_async(
        engine,
        SystemTable::Modules,
        [("path", json!(module_path))],
    )
    .await?;
    let Some(module) = modules.into_iter().next() else {
        return Ok(Vec::new());
    };
    let Some(digest) = module.fields.get("sourcePackageId").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let bytes = store.get(digest)?;
    let parsed = parse_source_package(&bytes)?;
    Ok(parsed
        .modules
        .into_iter()
        .map(|module| (module.path, module.source))
        .collect())
}

/// All modules (path + source) in the active deployment's source package.
/// Backs the deployment-wide call graph; empty when nothing is deployed. FSV7.
pub async fn read_active_source_package_modules_async(
    engine: &Arc<Engine>,
    store: &dyn SourcePackageStore,
) -> Result<Vec<(String, String)>> {
    let packages = query_system_documents_by_eq_async(
        engine,
        SystemTable::SourcePackages,
        [("status", json!("active"))],
    )
    .await?;
    let Some(package) = packages.into_iter().next() else {
        return Ok(Vec::new());
    };
    let Some(digest) = package.fields.get("digest").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let bytes = store.get(digest)?;
    let parsed = parse_source_package(&bytes)?;
    Ok(parsed
        .modules
        .into_iter()
        .map(|module| (module.path, module.source))
        .collect())
}

async fn delete_stale_source_package_documents_async(
    engine: &Arc<Engine>,
    active_digest: &str,
    active_module_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let packages_table = SystemTable::SourcePackages.table_name()?;
    let packages = query_system_documents_by_eq_async(
        engine,
        SystemTable::SourcePackages,
        [("status", json!("active"))],
    )
    .await?;
    for package in packages {
        let Some(digest) = package.fields.get("digest").and_then(Value::as_str) else {
            engine
                .delete_document_async(system_tenant.clone(), packages_table.clone(), package.id)
                .await?;
            continue;
        };
        if digest == active_digest {
            continue;
        }
        delete_modules_for_source_package_async(engine, digest, |_| true).await?;
        engine
            .delete_document_async(system_tenant.clone(), packages_table.clone(), package.id)
            .await?;
    }

    delete_modules_for_source_package_async(engine, active_digest, |module| {
        !active_module_ids.contains(&module.id.to_string())
    })
    .await?;

    Ok(())
}

async fn delete_modules_for_source_package_async(
    engine: &Arc<Engine>,
    digest: &str,
    should_delete: impl Fn(&Document) -> bool,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let modules_table = SystemTable::Modules.table_name()?;
    let modules = query_system_documents_by_eq_async(
        engine,
        SystemTable::Modules,
        [("sourcePackageId", json!(digest))],
    )
    .await?;
    for module in modules {
        if should_delete(&module) {
            engine
                .delete_document_async(system_tenant.clone(), modules_table.clone(), module.id)
                .await?;
        }
    }
    Ok(())
}
