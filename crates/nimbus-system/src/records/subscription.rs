use std::sync::Arc;

use nimbus_core::{Result, TenantId};
use nimbus_engine::Engine;
use serde_json::json;

use crate::identity::is_system_tenant_id;
use crate::keys::subscription_document_id;
use crate::schema::SystemTable;

use super::{
    delete_system_document_if_exists_async, ensure_system_tenant_async, object_fields,
    unix_time_millis, upsert_system_document_async,
};

pub async fn record_subscription_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
) -> Result<()> {
    if should_skip_subscription_projection(tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::Subscriptions,
        &subscription_document_id(adapter, tenant_id, subscription_id),
        object_fields(json!({
            "tenantId": tenant_id.as_str(),
            "adapter": adapter,
            "queryKey": query_key,
            "clientCount": 1,
            "lastDeliveryAt": unix_time_millis()?,
        })),
    )
    .await
}

pub async fn record_subscription_delivery_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
) -> Result<()> {
    record_subscription_state_async(engine, tenant_id, adapter, subscription_id, query_key).await
}

pub async fn record_subscription_error_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
    error: &str,
) -> Result<()> {
    if should_skip_subscription_projection(tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::Subscriptions,
        &subscription_document_id(adapter, tenant_id, subscription_id),
        object_fields(json!({
            "tenantId": tenant_id.as_str(),
            "adapter": adapter,
            "queryKey": query_key,
            "clientCount": 1,
            "lastDeliveryAt": unix_time_millis()?,
            "error": error,
        })),
    )
    .await
}

fn should_skip_subscription_projection(tenant_id: &TenantId) -> bool {
    is_system_tenant_id(tenant_id)
}

pub async fn delete_subscription_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::Subscriptions,
        &subscription_document_id(adapter, tenant_id, subscription_id),
    )
    .await
}
