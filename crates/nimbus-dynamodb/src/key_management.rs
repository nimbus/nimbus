//! Persisted, Nimbus-native access-key management (D7.3).
//!
//! Access keys are global — they map an AWS access-key id to a Nimbus tenant —
//! so they live outside any user tenant, in a reserved system tenant
//! (`_nimbus_ddb_system`), table `_ddb_access_keys`, one doc per access-key id.
//!
//! This is the runtime surface for configuring and rotating keys without a
//! restart. `dispatch::authenticate` consults it when an access key is not found
//! in the static in-memory [`crate::AccessKeyRegistry`], so operators can add or
//! rotate credentials live. The static registry stays the fast path; the store
//! is read only on a registry miss.
//!
//! **At-rest protection** rides the platform envelope encryption, not a bespoke
//! scheme: the `_ddb_access_keys` documents are ordinary tenant storage, so when
//! `nimbus-engine`'s `LocalEncryptionConfig` is enabled (master-key-file /
//! key-directory / AWS-KMS wrapped-DEK) the secrets are encrypted at rest like
//! all other data. Production deployments must enable it (or use an external
//! database with its own at-rest encryption). Secrets are **never** returned
//! over the management/listing API — see [`RedactedAccessKey`].

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use nimbus_core::{DocumentId, StructuredQuery, TableName, TenantId};
use nimbus_engine::{Engine, MutationActor};
use nimbus_tenant::TenantIsolationContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::map_core_error;
use crate::tenant::{adapter_principal, ensure_tenant, ensure_tenant_async, maintenance_context};

/// Reserved system tenant that owns the global access-key store.
const KEY_STORE_TENANT: &str = "_nimbus_ddb_system";
/// Table holding one access-key doc per access-key id.
const KEY_STORE_TABLE: &str = "_ddb_access_keys";
/// Surface label recorded on the key-store tenant context.
const KEY_STORE_SURFACE: &str = "DynamoDB key store";

/// A persisted access-key binding: the tenant it scopes to, plus the optional
/// secret (required for strict SigV4) and region (informational).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccessKey {
    /// The Nimbus tenant id this access key resolves to.
    pub tenant: String,
    /// The secret access key, used for strict signature verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    /// The region this key is configured for (informational; verification uses
    /// the region from the request's credential scope).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// A **secret-free** view of a stored access key for the management/listing API.
/// The secret access key exists only to verify inbound SigV4 signatures and is
/// never read back over a listing surface, so it is omitted entirely here (F6b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedAccessKey {
    /// The Nimbus tenant id this access key resolves to.
    pub tenant: String,
    /// The region this key is configured for (informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

impl RedactedAccessKey {
    /// Drop the secret from a stored record, keeping only the listable fields.
    fn from_stored(record: StoredAccessKey) -> Self {
        Self {
            tenant: record.tenant,
            region: record.region,
        }
    }
}

/// The key store is Nimbus's own, in a reserved tenant no access key can bind
/// to, and it is read on the authentication path *before* any caller identity
/// exists. It is therefore system-authority work, never a caller's.
fn store_context() -> Result<TenantIsolationContext, DynamoDbError> {
    Ok(maintenance_context(
        TenantId::new(KEY_STORE_TENANT).map_err(map_core_error)?,
        KEY_STORE_SURFACE,
    ))
}

fn store_table() -> Result<TableName, DynamoDbError> {
    TableName::new(KEY_STORE_TABLE).map_err(map_core_error)
}

fn key_id(access_key_id: &str) -> Result<DocumentId, DynamoDbError> {
    DocumentId::from_key(access_key_id).map_err(map_core_error)
}

/// Full-replace write of a record under `access_key_id` (delete-then-insert so a
/// re-put with fewer fields, e.g. dropping the region, actually clears them).
fn write_record(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    access_key_id: &str,
    record: &StoredAccessKey,
) -> Result<(), DynamoDbError> {
    let table = store_table()?;
    let id = key_id(access_key_id)?;
    let fields = match serde_json::to_value(record) {
        Ok(Value::Object(map)) => map,
        _ => {
            return Err(DynamoDbError::InternalServerError(
                "failed to serialize access key".to_owned(),
            ));
        }
    };
    let principal = adapter_principal();
    if engine
        .get_document_with_principal(context.tenant_id(), &table, id.clone(), &principal)
        .is_ok()
    {
        engine
            .delete_document_with(
                context.tenant_id(),
                table.clone(),
                id.clone(),
                MutationActor::with_principal(&principal),
            )
            .map_err(map_core_error)?;
    }
    engine
        .insert_document_with(
            context.tenant_id(),
            table,
            Some(id),
            fields,
            MutationActor::with_principal(&principal),
        )
        .map_err(map_core_error)?;
    Ok(())
}

/// Configure an access key after synchronous system-tenant admission.
///
/// Provider-capable callers must use [`put_access_key_async`].
///
/// # Errors
/// A mapped engine error if the system tenant or the record cannot be written.
pub fn put_access_key(
    engine: &Arc<Engine>,
    access_key_id: &str,
    tenant: &TenantId,
    secret: Option<String>,
    region: Option<String>,
) -> Result<(), DynamoDbError> {
    // A key must never be bound to a reserved Nimbus-internal tenant (e.g. the
    // key-store's own `_nimbus_ddb_system`) — that would let a request scoped to
    // it read every stored credential (F6a).
    if crate::tenant::is_reserved_tenant(tenant) {
        return Err(DynamoDbError::ValidationException(format!(
            "Access keys cannot be bound to the reserved Nimbus-internal tenant '{}'",
            tenant.as_str()
        )));
    }
    let context = store_context()?;
    ensure_tenant(engine, &context)?;
    let record = StoredAccessKey {
        tenant: tenant.as_str().to_owned(),
        secret,
        region,
    };
    write_record(engine, &context, access_key_id, &record)
}

/// Configure an access key through canonical async system-tenant admission.
///
/// This is the provider-capable management entrypoint. Only `AlreadyExists`
/// is treated as idempotent by the shared async lifecycle.
pub async fn put_access_key_async(
    engine: &Arc<Engine>,
    access_key_id: &str,
    tenant: &TenantId,
    secret: Option<String>,
    region: Option<String>,
) -> Result<(), DynamoDbError> {
    if crate::tenant::is_reserved_tenant(tenant) {
        return Err(DynamoDbError::ValidationException(format!(
            "Access keys cannot be bound to the reserved Nimbus-internal tenant '{}'",
            tenant.as_str()
        )));
    }
    let context = store_context()?;
    ensure_tenant_async(engine, &context).await?;
    let record = StoredAccessKey {
        tenant: tenant.as_str().to_owned(),
        secret,
        region,
    };
    write_record(engine, &context, access_key_id, &record)
}

/// Rotate an existing access key's secret, preserving its tenant + region.
///
/// # Errors
/// `ResourceNotFoundException` if the access key is not configured; a mapped
/// engine error if the write fails.
pub fn rotate_secret(
    engine: &Arc<Engine>,
    access_key_id: &str,
    new_secret: impl Into<String>,
) -> Result<(), DynamoDbError> {
    let context = store_context()?;
    let mut record = lookup(engine, access_key_id)?.ok_or_else(|| {
        DynamoDbError::ResourceNotFoundException(format!("Access key not found: {access_key_id}"))
    })?;
    record.secret = Some(new_secret.into());
    write_record(engine, &context, access_key_id, &record)
}

/// Delete a configured access key (idempotent — a no-op if absent).
///
/// # Errors
/// A mapped engine error if the delete fails.
pub fn delete_access_key(engine: &Arc<Engine>, access_key_id: &str) -> Result<(), DynamoDbError> {
    let context = store_context()?;
    let table = store_table()?;
    let id = key_id(access_key_id)?;
    let principal = adapter_principal();
    match engine.get_document_with_principal(context.tenant_id(), &table, id.clone(), &principal) {
        Ok(_) => engine
            .delete_document_with(
                context.tenant_id(),
                table,
                id,
                MutationActor::with_principal(&principal),
            )
            .map_err(map_core_error),
        Err(
            nimbus_core::Error::NotFound(_)
            | nimbus_core::Error::DocumentNotFound(_)
            | nimbus_core::Error::TenantNotFound(_),
        ) => Ok(()),
        Err(error) => Err(map_core_error(error)),
    }
}

/// Look up a persisted access key, returning `None` if it is not configured (or
/// the store does not exist yet).
///
/// # Errors
/// A mapped engine error for a storage failure other than "not found".
pub fn lookup(
    engine: &Arc<Engine>,
    access_key_id: &str,
) -> Result<Option<StoredAccessKey>, DynamoDbError> {
    let context = store_context()?;
    match engine.get_document_with_principal(
        context.tenant_id(),
        &store_table()?,
        key_id(access_key_id)?,
        &adapter_principal(),
    ) {
        Ok(document) => {
            let record = serde_json::from_value(Value::Object(document.fields.clone())).map_err(
                |error| DynamoDbError::InternalServerError(format!("corrupt access key: {error}")),
            )?;
            Ok(Some(record))
        }
        // The store does not exist yet (no key ever configured → no system
        // tenant / table / doc): treat as "not configured", not an error.
        Err(
            nimbus_core::Error::NotFound(_)
            | nimbus_core::Error::DocumentNotFound(_)
            | nimbus_core::Error::TenantNotFound(_),
        ) => Ok(None),
        Err(error) => Err(map_core_error(error)),
    }
}

/// Look up a persisted access key through the provider-capable async engine
/// lifecycle.
///
/// This is the authentication-path counterpart to [`lookup`]. Provider-backed
/// transports must use it because the key-store tenant may need to be opened
/// asynchronously before Nimbus can determine that a key is absent.
///
/// # Errors
/// A mapped engine error for a storage failure other than "not found".
pub async fn lookup_async(
    engine: &Arc<Engine>,
    access_key_id: &str,
) -> Result<Option<StoredAccessKey>, DynamoDbError> {
    let context = store_context()?;
    match engine
        .get_document_async(
            context.tenant_id().clone(),
            store_table()?,
            key_id(access_key_id)?,
        )
        .await
    {
        Ok(document) => {
            let record = serde_json::from_value(Value::Object(document.fields.clone())).map_err(
                |error| DynamoDbError::InternalServerError(format!("corrupt access key: {error}")),
            )?;
            Ok(Some(record))
        }
        Err(
            nimbus_core::Error::NotFound(_)
            | nimbus_core::Error::DocumentNotFound(_)
            | nimbus_core::Error::TenantNotFound(_),
        ) => Ok(None),
        Err(error) => Err(map_core_error(error)),
    }
}

/// List every configured access key as `(access_key_id, redacted_record)` pairs,
/// sorted by id. The returned [`RedactedAccessKey`] omits the secret — it is
/// never exposed over the listing surface (F6b).
///
/// # Errors
/// A mapped engine error for a storage failure.
pub fn list_access_keys(
    engine: &Arc<Engine>,
) -> Result<Vec<(String, RedactedAccessKey)>, DynamoDbError> {
    let context = store_context()?;
    let documents = match engine.query_documents_structured_with_principal(
        context.tenant_id(),
        &store_table()?,
        &StructuredQuery::default(),
        &adapter_principal(),
    ) {
        Ok(documents) => documents,
        Err(
            nimbus_core::Error::NotFound(_)
            | nimbus_core::Error::DocumentNotFound(_)
            | nimbus_core::Error::TenantNotFound(_),
        ) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(map_core_error(error)),
    };
    let mut keys = documents
        .iter()
        .map(|document| {
            let record: StoredAccessKey = serde_json::from_value(Value::Object(
                document.fields.clone(),
            ))
            .map_err(|error| {
                DynamoDbError::InternalServerError(format!("corrupt access key: {error}"))
            })?;
            Ok((
                document.id.as_str().to_owned(),
                RedactedAccessKey::from_stored(record),
            ))
        })
        .collect::<Result<Vec<_>, DynamoDbError>>()?;
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> (Arc<Engine>, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        (engine, temp)
    }

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant")
    }

    #[test]
    fn lookup_is_none_before_any_key_is_configured() {
        let (engine, _t) = engine();
        assert_eq!(lookup(&engine, "AKIANOPE").unwrap(), None);
        assert!(list_access_keys(&engine).unwrap().is_empty());
    }

    #[tokio::test]
    async fn async_lookup_uses_memory_provider_lifecycle_for_absent_and_present_keys() {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(
            Engine::new_with_memory_persistence(temp.path()).expect("memory provider engine"),
        );

        assert_eq!(lookup_async(&engine, "AKIANOPE").await.unwrap(), None);

        put_access_key_async(
            &engine,
            "AKIAACME",
            &tenant("acme"),
            Some("secret-1".to_owned()),
            Some("us-east-1".to_owned()),
        )
        .await
        .expect("async put");
        let stored = lookup_async(&engine, "AKIAACME")
            .await
            .expect("async lookup")
            .expect("present");
        assert_eq!(stored.tenant, "acme");
        assert_eq!(stored.secret.as_deref(), Some("secret-1"));
        assert_eq!(stored.region.as_deref(), Some("us-east-1"));
    }

    #[tokio::test]
    async fn async_lookup_reopens_durable_key_store_after_engine_restart() {
        let data_dir = tempfile::tempdir().expect("temporary data dir should create");
        let tenant_id = tenant("acme");
        let engine = Arc::new(Engine::new(data_dir.path()).expect("embedded engine should create"));

        put_access_key_async(
            &engine,
            "AKIARESTART",
            &tenant_id,
            Some("fixture-dynamodb".to_owned()),
            Some("us-east-1".to_owned()),
        )
        .await
        .expect("async put should create the durable key-store tenant");
        engine.quiesce().await;
        drop(engine);

        let reopened = Arc::new(
            Engine::new(data_dir.path()).expect("engine should reopen without a loaded runtime"),
        );
        let stored = lookup_async(&reopened, "AKIARESTART")
            .await
            .expect("async lookup should open the durable key-store tenant")
            .expect("persisted key should survive the engine restart");
        assert_eq!(stored.tenant, tenant_id.as_str());
        assert_eq!(stored.secret.as_deref(), Some("fixture-dynamodb"));
        assert_eq!(stored.region.as_deref(), Some("us-east-1"));
        reopened.quiesce().await;
    }

    #[test]
    fn put_then_lookup_roundtrips_all_fields() {
        let (engine, _t) = engine();
        put_access_key(
            &engine,
            "AKIAACME",
            &tenant("acme"),
            Some("secret-1".to_owned()),
            Some("us-east-1".to_owned()),
        )
        .expect("put");
        let stored = lookup(&engine, "AKIAACME")
            .expect("lookup")
            .expect("present");
        assert_eq!(stored.tenant, "acme");
        assert_eq!(stored.secret.as_deref(), Some("secret-1"));
        assert_eq!(stored.region.as_deref(), Some("us-east-1"));
    }

    #[test]
    fn rotate_secret_replaces_only_the_secret() {
        let (engine, _t) = engine();
        put_access_key(
            &engine,
            "AKIAACME",
            &tenant("acme"),
            Some("secret-1".to_owned()),
            Some("us-east-1".to_owned()),
        )
        .expect("put");
        rotate_secret(&engine, "AKIAACME", "secret-2").expect("rotate");
        let stored = lookup(&engine, "AKIAACME")
            .expect("lookup")
            .expect("present");
        assert_eq!(stored.secret.as_deref(), Some("secret-2"), "secret rotated");
        assert_eq!(stored.tenant, "acme", "tenant preserved");
        assert_eq!(
            stored.region.as_deref(),
            Some("us-east-1"),
            "region preserved"
        );
    }

    #[test]
    fn rotate_unknown_key_is_resource_not_found() {
        let (engine, _t) = engine();
        let err = rotate_secret(&engine, "AKIAGHOST", "x").expect_err("missing");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn put_replaces_and_can_clear_region() {
        let (engine, _t) = engine();
        put_access_key(
            &engine,
            "AKIAACME",
            &tenant("acme"),
            Some("s".to_owned()),
            Some("us-east-1".to_owned()),
        )
        .expect("put");
        // Re-put without a region clears it (full replace, not merge).
        put_access_key(
            &engine,
            "AKIAACME",
            &tenant("acme"),
            Some("s".to_owned()),
            None,
        )
        .expect("re-put");
        let stored = lookup(&engine, "AKIAACME")
            .expect("lookup")
            .expect("present");
        assert_eq!(stored.region, None, "region cleared by full-replace put");
    }

    #[test]
    fn delete_removes_the_key() {
        let (engine, _t) = engine();
        put_access_key(&engine, "AKIAACME", &tenant("acme"), None, None).expect("put");
        delete_access_key(&engine, "AKIAACME").expect("delete");
        assert_eq!(lookup(&engine, "AKIAACME").unwrap(), None);
        // Idempotent.
        delete_access_key(&engine, "AKIAACME").expect("delete again");
    }

    #[test]
    fn put_access_key_rejects_a_reserved_tenant() {
        // F6a: binding a key to the key-store's own reserved tenant (or any
        // `_nimbus`-prefixed tenant) would expose every stored credential.
        let (engine, _t) = engine();
        assert!(matches!(
            put_access_key(
                &engine,
                "AKIAEVIL",
                &tenant("_nimbus_ddb_system"),
                Some("s".to_owned()),
                None,
            ),
            Err(DynamoDbError::ValidationException(_))
        ));
        assert!(
            put_access_key(&engine, "AKIAEVIL2", &tenant("_nimbus_other"), None, None).is_err(),
            "any _nimbus-prefixed tenant is reserved"
        );
        // A non-reserved tenant still works.
        put_access_key(&engine, "AKIAOK", &tenant("acme"), None, None).expect("normal put");
    }

    #[test]
    fn list_access_keys_redacts_secrets() {
        // F6b: the secret must never be returned over the listing surface.
        let (engine, _t) = engine();
        put_access_key(
            &engine,
            "AKIAACME",
            &tenant("acme"),
            Some("top-secret-value".to_owned()),
            Some("us-east-1".to_owned()),
        )
        .expect("put");
        let keys = list_access_keys(&engine).expect("list");
        assert_eq!(keys.len(), 1);
        let (id, redacted) = &keys[0];
        assert_eq!(id, "AKIAACME");
        assert_eq!(redacted.tenant, "acme");
        assert_eq!(redacted.region.as_deref(), Some("us-east-1"));
        // Structurally secret-free: serializing the listed view leaks nothing.
        let json = serde_json::to_string(redacted).expect("serialize");
        assert!(
            !json.contains("top-secret-value") && !json.contains("secret"),
            "redacted view must not carry the secret: {json}"
        );
    }

    #[test]
    fn list_returns_all_keys_sorted() {
        let (engine, _t) = engine();
        put_access_key(&engine, "AKIAB", &tenant("two"), None, None).expect("put");
        put_access_key(&engine, "AKIAA", &tenant("one"), None, None).expect("put");
        let keys = list_access_keys(&engine).expect("list");
        let ids: Vec<&str> = keys.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["AKIAA", "AKIAB"], "sorted by id");
        assert_eq!(keys[0].1.tenant, "one");
    }
}
