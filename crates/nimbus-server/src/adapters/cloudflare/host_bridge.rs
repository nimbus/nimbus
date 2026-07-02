#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "CFA5 proves the Cloudflare Worker KV bridge before the Worker front door constructs it in production"
    )
)]

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_runtime::{
    HostBridge, HostCallCancellation, HostCallEnvelope, HostCallPayload, HostCallRequest,
    NimbusRuntimeError, Result, RuntimeAsyncCfKvDeletePayload, RuntimeAsyncCfKvGetPayload,
    RuntimeAsyncCfKvListPayload, RuntimeAsyncCfKvPutPayload,
};
use nimbus_storage::KvPut;
use serde_json::{Value, json};

use super::{CloudflareConfig, kv};

#[derive(Clone)]
pub struct CloudflareHostBridge {
    engine: Arc<Engine>,
    config: Arc<CloudflareConfig>,
    tenant_id: TenantId,
}

impl CloudflareHostBridge {
    pub fn new(engine: Arc<Engine>, config: Arc<CloudflareConfig>, tenant_id: TenantId) -> Self {
        Self {
            engine,
            config,
            tenant_id,
        }
    }

    fn dispatch(&self, request: HostCallRequest) -> Result<Value> {
        let envelope = HostCallEnvelope::try_from(request)?;
        match envelope.payload {
            HostCallPayload::CfKvGet(payload) => self.kv_get(payload),
            HostCallPayload::CfKvPut(payload) => self.kv_put(payload),
            HostCallPayload::CfKvDelete(payload) => self.kv_delete(payload),
            HostCallPayload::CfKvList(payload) => self.kv_list(payload),
            payload => Err(NimbusRuntimeError::Contract(format!(
                "cloudflare host bridge does not own `{}` runtime compatibility",
                payload.operation()
            ))),
        }
    }

    fn validate_tenant(&self, tenant_id: &str) -> Result<()> {
        if tenant_id == self.tenant_id.as_str() {
            return Ok(());
        }
        Err(NimbusRuntimeError::Contract(format!(
            "Cloudflare KV host call tenant `{tenant_id}` does not match invocation tenant `{}`",
            self.tenant_id
        )))
    }

    fn namespace(&self, namespace: &str) -> Result<String> {
        kv::resolve_worker_namespace(&self.config, namespace).map_err(kv_runtime_error)
    }

    fn kv_get(&self, payload: RuntimeAsyncCfKvGetPayload) -> Result<Value> {
        self.validate_tenant(&payload.tenant_id)?;
        let namespace = self.namespace(&payload.namespace)?;
        let storage_key = kv::storage_key(&namespace, &payload.key).map_err(kv_runtime_error)?;
        let Some(entry) = self
            .engine
            .tenant_kv_get(&self.tenant_id, &storage_key, kv::now_ms())
            .map_err(core_runtime_error)?
        else {
            return Ok(Value::Null);
        };
        Ok(json!({
            "value_base64": STANDARD.encode(entry.value),
            "metadata": kv::decode_metadata(&entry.metadata),
        }))
    }

    fn kv_put(&self, payload: RuntimeAsyncCfKvPutPayload) -> Result<Value> {
        self.validate_tenant(&payload.tenant_id)?;
        let namespace = self.namespace(&payload.namespace)?;
        let storage_key = kv::storage_key(&namespace, &payload.key).map_err(kv_runtime_error)?;
        let value = STANDARD
            .decode(payload.value_base64.as_bytes())
            .map_err(|error| {
                kv_runtime_error(kv::KvRestError::bad_request(format!(
                    "Workers KV value_base64 is invalid: {error}"
                )))
            })?;
        let expire_at_ms =
            kv::resolve_expire_at_ms_values(payload.expiration, payload.expiration_ttl)
                .map_err(kv_runtime_error)?;
        let metadata =
            kv::encode_metadata_value(Some(&payload.metadata)).map_err(kv_runtime_error)?;
        let mut put = KvPut::new(storage_key, value);
        put.metadata = metadata;
        put.expire_at_ms = expire_at_ms;
        self.engine
            .tenant_kv_put(&self.tenant_id, put)
            .map_err(core_runtime_error)?;
        Ok(Value::Null)
    }

    fn kv_delete(&self, payload: RuntimeAsyncCfKvDeletePayload) -> Result<Value> {
        self.validate_tenant(&payload.tenant_id)?;
        let namespace = self.namespace(&payload.namespace)?;
        let storage_key = kv::storage_key(&namespace, &payload.key).map_err(kv_runtime_error)?;
        let _ = self
            .engine
            .tenant_kv_delete(&self.tenant_id, &storage_key)
            .map_err(core_runtime_error)?;
        Ok(Value::Null)
    }

    fn kv_list(&self, payload: RuntimeAsyncCfKvListPayload) -> Result<Value> {
        self.validate_tenant(&payload.tenant_id)?;
        let namespace = self.namespace(&payload.namespace)?;
        let limit = payload.limit.unwrap_or(kv::DEFAULT_LIST_LIMIT);
        if limit > kv::MAX_LIST_LIMIT {
            return Err(kv_runtime_error(kv::KvRestError::bad_request(format!(
                "Workers KV list limit must be at most {}",
                kv::MAX_LIST_LIMIT
            ))));
        }
        let prefix = kv::storage_prefix(&namespace, payload.prefix.as_deref().unwrap_or_default())
            .map_err(kv_runtime_error)?;
        let cursor = payload
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
            .map(kv::decode_cursor)
            .transpose()
            .map_err(kv_runtime_error)?;
        let page = self
            .engine
            .tenant_kv_scan(
                &self.tenant_id,
                &prefix,
                cursor.as_deref(),
                limit,
                kv::now_ms(),
            )
            .map_err(core_runtime_error)?;
        let keys = page
            .entries
            .into_iter()
            .map(|entry| {
                let metadata = kv::decode_metadata(&entry.metadata);
                kv::display_key(&namespace, &entry.key).map(|name| {
                    json!({
                        "name": name,
                        "expiration": entry.expire_at_ms.map(|value| value / 1000),
                        "metadata": if metadata.is_null() { Value::Null } else { metadata },
                    })
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(kv_runtime_error)?;
        let cursor = page
            .next_cursor
            .map(|cursor| URL_SAFE_NO_PAD.encode(cursor));
        Ok(json!({
            "keys": keys,
            "list_complete": cursor.is_none(),
            "cursor": cursor.unwrap_or_default(),
        }))
    }
}

impl HostBridge for CloudflareHostBridge {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        self.dispatch(request)
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> Result<Value> {
        if cancellation.is_cancelled() {
            return Err(NimbusRuntimeError::Cancelled);
        }
        self.dispatch(request)
    }
}

fn kv_runtime_error(error: kv::KvRestError) -> NimbusRuntimeError {
    NimbusRuntimeError::Contract(format!("Cloudflare KV host call failed: {error}"))
}

fn core_runtime_error(error: nimbus_core::Error) -> NimbusRuntimeError {
    NimbusRuntimeError::Contract(format!("Cloudflare KV host call failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use nimbus_engine::EmbeddedProviderKind;
    use nimbus_runtime::{
        InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeExecutionModel,
        RuntimeLimits, RuntimePolicy, RuntimePoolKind,
    };
    use nimbus_testing::EngineFixture;
    use serde_json::Value;

    use crate::adapters::cloudflare::{CloudflareBindingRegistry, KvNamespaceBinding};

    #[tokio::test]
    async fn cloudflare_worker_env_ns_e2e_round_trips_kv() {
        let fixture = EngineFixture::new(|path| {
            Engine::new_with_embedded_provider(path, EmbeddedProviderKind::Redb)
        });
        let tenant = TenantId::new("tenant-a").expect("tenant id should build");
        fixture
            .engine()
            .create_tenant(tenant.clone())
            .expect("tenant should create");
        let config = Arc::new(CloudflareConfig::new(CloudflareBindingRegistry::new(
            vec![KvNamespaceBinding {
                binding: "NS".to_string(),
                id: Some("namespace-prod".to_string()),
                preview_id: None,
            }],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )));
        let runtime = NimbusRuntime::with_policy(
            Arc::new(CloudflareHostBridge::new(
                fixture.engine(),
                config,
                tenant.clone(),
            )),
            Arc::new(RuntimePolicy::new(RuntimeLimits {
                execution_model: RuntimeExecutionModel::RunToCompletion,
                runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
                ..RuntimeLimits::default()
            })),
        );
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("worker.mjs");
        std::fs::write(
            &bundle_path,
            r#"
export default {
  async fetch(_request, env) {
    await env.NS.put("greeting", JSON.stringify({ text: "hello" }), {
      metadata: { lang: "en" },
      expirationTtl: 120,
    });
    const value = await env.NS.get("greeting", "json");
    const withMetadata = await env.NS.getWithMetadata("greeting", "json");
    const list = await env.NS.list({ prefix: "g", limit: 10 });
    return new Response(JSON.stringify({ value, withMetadata, list }), {
      headers: { "content-type": "application/json" },
    });
  },
};
"#,
        )
        .expect("worker bundle should write");

        let request = InvocationRequest {
            kind: InvocationKind::CloudflareWorkerFetch,
            function_name: "worker:fetch".to_string(),
            args: json!({
                "request": {
                    "url": "https://example.com/kv",
                    "method": "GET",
                },
                "env": {
                    "NS": {
                        "type": "kv_namespace",
                        "tenant_id": tenant.as_str(),
                        "namespace": "namespace-prod",
                    },
                },
            }),
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let result = runtime
            .invoke_bundle_for_tenant(&RuntimeBundle::new(&bundle_path), &request, tenant.as_str())
            .await
            .expect("Worker env.NS flow should execute");

        assert_eq!(result["status"], json!(200));
        let body: Value = serde_json::from_str(
            result["body"]
                .as_str()
                .expect("Worker response body should be text"),
        )
        .expect("Worker response body should be JSON");
        assert_eq!(body["value"], json!({ "text": "hello" }));
        assert_eq!(
            body["withMetadata"],
            json!({
                "value": { "text": "hello" },
                "metadata": { "lang": "en" },
            })
        );
        assert_eq!(body["list"]["list_complete"], json!(true));
        assert_eq!(body["list"]["keys"][0]["name"], json!("greeting"));
        assert_eq!(body["list"]["keys"][0]["metadata"], json!({ "lang": "en" }));
    }
}
