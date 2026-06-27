use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use wasmtime::component::Component;

use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeBundleContentKind;
use crate::runtime::RuntimeBundle;

use super::host_linker::{
    WasmtimeHostLinker, build_nimbus_host_linker, create_wasmtime_component_engine,
};

const WASMTIME_ENGINE_CONFIG_HASH: &str =
    "wasmtime-46.0.1|component-model|component-async|fuel|epoch";
const WASMTIME_RUN_TO_COMPLETION_FUEL: u64 = 10_000_000;

#[derive(Clone)]
pub(crate) struct WasmtimeBackendFactory {
    shared: &'static WasmtimeSharedBackend,
}

struct WasmtimeSharedBackend {
    engine: wasmtime::Engine,
    linker: WasmtimeHostLinker,
    module_cache: Arc<WasmtimeModuleCache>,
}

impl WasmtimeBackendFactory {
    pub(crate) fn new() -> Self {
        static SHARED: OnceLock<WasmtimeSharedBackend> = OnceLock::new();
        Self {
            shared: SHARED.get_or_init(|| {
                let engine = create_wasmtime_component_engine()
                    .expect("Wasmtime component engine should initialize");
                let linker =
                    build_nimbus_host_linker(&engine).expect("Wasmtime host linker should build");
                WasmtimeSharedBackend {
                    module_cache: Arc::new(WasmtimeModuleCache::new(engine.clone())),
                    engine,
                    linker,
                }
            }),
        }
    }
}

impl RuntimeBackendFactory for WasmtimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        Box::new(WasmtimeBackend {
            engine: self.shared.engine.clone(),
            linker: self.shared.linker.clone(),
            module_cache: self.shared.module_cache.clone(),
        })
    }
}

struct WasmtimeBackend {
    engine: wasmtime::Engine,
    linker: WasmtimeHostLinker,
    module_cache: Arc<WasmtimeModuleCache>,
}

impl RuntimeBackend for WasmtimeBackend {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        Box::pin(async move { self.invoke_component(invocation).await })
    }
}

impl WasmtimeBackend {
    async fn invoke_component(&mut self, invocation: RuntimeBackendInvocation) -> Result<Value> {
        let RuntimeBackendInvocation {
            watchdog: _,
            host,
            policy,
            bundle,
            request,
            context,
            cancellation,
            permit: _,
        } = invocation;

        if cancellation
            .as_ref()
            .is_some_and(crate::host::HostCallCancellation::is_cancelled)
        {
            return Err(NimbusRuntimeError::Cancelled);
        }
        bundle.verify_integrity()?;
        policy.validate_bundle_content_kind(bundle.content_kind())?;
        if !matches!(
            bundle.content_kind(),
            RuntimeBundleContentKind::WasmComponent
        ) {
            return Err(NimbusRuntimeError::Contract(format!(
                "Wasmtime backend requires a WASM component bundle, got {:?}",
                bundle.content_kind()
            )));
        }

        let component = self.module_cache.get_or_compile(&bundle)?;
        let mut store = wasmtime::Store::new(
            &self.engine,
            super::host_linker::InvocationHostState::new(
                host.bridge(),
                context.clone(),
                cancellation,
            ),
        );
        store
            .set_fuel(WASMTIME_RUN_TO_COMPLETION_FUEL)
            .map_err(wasmtime_error)?;

        let instance = self
            .linker
            .instantiate_async(&mut store, component.as_ref())
            .await
            .map_err(wasmtime_error)?;
        let args = serde_json::to_string(&request.args)?;
        call_nimbus_function_handler(&instance, &mut store, args).await
    }
}

async fn call_nimbus_function_handler(
    instance: &wasmtime::component::Instance,
    store: &mut wasmtime::Store<super::host_linker::InvocationHostState>,
    args: String,
) -> Result<Value> {
    if let Ok(handler) = instance
        .get_typed_func::<(String,), (std::result::Result<String, String>,)>(&mut *store, "handler")
    {
        let (response,) = handler
            .call_async(&mut *store, (args,))
            .await
            .map_err(wasmtime_error)?;
        return parse_handler_result(response);
    }

    if let Ok(handler) = instance.get_typed_func::<(String,), (String,)>(&mut *store, "handler") {
        let (response,) = handler
            .call_async(&mut *store, (args,))
            .await
            .map_err(wasmtime_error)?;
        return parse_component_response(&response);
    }

    Err(NimbusRuntimeError::Contract(
        "WASM component does not export a supported nimbus-function handler".to_string(),
    ))
}

fn parse_handler_result(response: std::result::Result<String, String>) -> Result<Value> {
    match response {
        Ok(response) => parse_component_response(&response),
        Err(error) => Err(NimbusRuntimeError::JavaScript(error)),
    }
}

fn parse_component_response(response: &str) -> Result<Value> {
    serde_json::from_str(response).or_else(|_| Ok(Value::String(response.to_string())))
}

fn wasmtime_error(error: wasmtime::Error) -> NimbusRuntimeError {
    NimbusRuntimeError::Contract(format!("wasmtime component execution error: {error}"))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct WasmtimeModuleCacheKey {
    bundle_sha256: String,
    engine_config_hash: &'static str,
}

struct WasmtimeModuleCache {
    engine: wasmtime::Engine,
    compiled: Mutex<HashMap<WasmtimeModuleCacheKey, Arc<Component>>>,
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl WasmtimeModuleCache {
    fn new(engine: wasmtime::Engine) -> Self {
        Self {
            engine,
            compiled: Mutex::new(HashMap::new()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    fn get_or_compile(&self, bundle: &RuntimeBundle) -> Result<Arc<Component>> {
        let key = WasmtimeModuleCacheKey {
            bundle_sha256: RuntimeBundle::compute_sha256_for_path(bundle.entrypoint())?,
            engine_config_hash: WASMTIME_ENGINE_CONFIG_HASH,
        };
        let mut compiled = self
            .compiled
            .lock()
            .expect("Wasmtime module cache lock should not be poisoned");
        if let Some(component) = compiled.get(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(component.clone());
        }

        let bytes = std::fs::read(bundle.entrypoint())?;
        let component = Arc::new(Component::new(&self.engine, bytes).map_err(wasmtime_error)?);
        compiled.insert(key, component.clone());
        self.misses.fetch_add(1, Ordering::Relaxed);
        Ok(component)
    }

    #[cfg(test)]
    fn hit_count_for_test(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn miss_count_for_test(&self) -> usize {
        self.misses.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::host::{HostBridge, HostCallRequest};
    use crate::limits::{RuntimeLimits, RuntimePolicy};
    use crate::runtime::{InvocationKind, InvocationRequest, NimbusRuntime};

    use super::*;

    struct NoopHost;

    impl HostBridge for NoopHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NimbusRuntimeError::Contract(
                "run-to-completion test component should not call host imports".to_string(),
            ))
        }
    }

    fn write_component_fixture() -> (tempfile::TempDir, RuntimeBundle) {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let component_path = tempdir.path().join("nimbus-function.component.wat");
        std::fs::write(&component_path, nimbus_function_component_wat())
            .expect("component fixture should be written");
        let expected_sha256 =
            RuntimeBundle::compute_sha256_for_path(&component_path).expect("fixture should hash");
        let bundle =
            RuntimeBundle::wasm_component_with_expected_sha256(&component_path, expected_sha256)
                .expect("WASM component bundle should record provenance hash");
        (tempdir, bundle)
    }

    fn request() -> InvocationRequest {
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "wasm:handler".to_string(),
            args: json!({ "subject": "world" }),
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        }
    }

    #[tokio::test]
    async fn wasmtime_run_to_completion_invokes_component_through_worker_loop() {
        let (_tempdir, bundle) = write_component_fixture();
        let mut limits = RuntimeLimits::application_wasm_component();
        limits.worker_threads = 1;
        limits.max_concurrent_runtime_instances = 1;
        limits.max_active_top_level_invocations_per_tenant = 1;
        limits.max_in_flight_top_level_invocations_per_tenant = 1;
        let runtime =
            NimbusRuntime::with_policy(Arc::new(NoopHost), Arc::new(RuntimePolicy::new(limits)));

        let response = runtime
            .invoke_bundle(&bundle, &request())
            .await
            .expect("Wasmtime run-to-completion invocation should succeed");

        assert_eq!(response, json!({ "ok": true }));
    }

    #[test]
    fn wasmtime_run_to_completion_module_cache_records_hit_and_miss() {
        let (_tempdir, bundle) = write_component_fixture();
        let engine = create_wasmtime_component_engine().expect("engine should initialize");
        let cache = WasmtimeModuleCache::new(engine);

        let first = cache
            .get_or_compile(&bundle)
            .expect("first component compile should succeed");
        let second = cache
            .get_or_compile(&bundle)
            .expect("second component lookup should hit cache");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.miss_count_for_test(), 1);
        assert_eq!(cache.hit_count_for_test(), 1);
    }

    #[test]
    fn wasmtime_run_to_completion_rejects_javascript_bundle_content() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let path = tempdir.path().join("handler.js");
        std::fs::write(&path, "export default {}").expect("fixture should be written");
        let bundle = RuntimeBundle::new(&path);
        let mut backend = WasmtimeBackendFactory::new().create();
        let request = request();
        let policy = Arc::new(RuntimePolicy::new(
            RuntimeLimits::application_wasm_component(),
        ));
        let context = crate::RuntimeInvocationContext::top_level(&request);
        let permit =
            crate::executor::SharedInvocationPermit::new(policy.clone(), None, None, true, None);

        let result = tokio::runtime::Runtime::new()
            .expect("tokio runtime should build")
            .block_on(backend.invoke(RuntimeBackendInvocation {
                watchdog: crate::watchdog::WatchdogTimer::new(),
                host: crate::runtime::RuntimeHost::new(Arc::new(NoopHost)),
                policy,
                bundle,
                request,
                context,
                cancellation: None,
                permit,
            }));

        assert!(
            result
                .expect_err("JavaScript bundle must be rejected by Wasmtime backend")
                .to_string()
                .contains("runtime bundle content kind JavaScript does not match")
        );
    }

    fn nimbus_function_component_wat() -> &'static str {
        r#"
            (component
              (core module $main
                (type $realloc_t (func (param i32 i32 i32 i32) (result i32)))
                (type $handler_t (func (param i32 i32) (result i32)))
                (type $post_t (func (param i32)))
                (memory $memory 1)
                (global $heap (mut i32) (i32.const 64))
                (data (i32.const 0) "\10\00\00\00\0b\00\00\00")
                (data (i32.const 16) "{\"ok\":true}")
                (func $cabi_realloc (type $realloc_t)
                  (param $old_ptr i32)
                  (param $old_size i32)
                  (param $align i32)
                  (param $new_size i32)
                  (result i32)
                  (local $ptr i32)
                  global.get $heap
                  local.set $ptr
                  global.get $heap
                  local.get $new_size
                  i32.add
                  global.set $heap
                  local.get $ptr
                )
                (func $handler (type $handler_t)
                  (param $args_ptr i32)
                  (param $args_len i32)
                  (result i32)
                  i32.const 0
                )
                (func $cabi_post_handler (type $post_t) (param $result_ptr i32))
                (export "memory" (memory $memory))
                (export "cabi_realloc" (func $cabi_realloc))
                (export "handler" (func $handler))
                (export "cabi_post_handler" (func $cabi_post_handler))
              )
              (core instance $main (instantiate $main))
              (alias core export $main "memory" (core memory $memory))
              (alias core export $main "cabi_realloc" (core func $cabi_realloc))
              (alias core export $main "handler" (core func $handler-core))
              (alias core export $main "cabi_post_handler" (core func $cabi_post_handler))
              (type $handler-ty (func (param "args" string) (result string)))
              (func $handler (type $handler-ty)
                (canon lift
                  (core func $handler-core)
                  (memory $memory)
                  (realloc $cabi_realloc)
                  string-encoding=utf8
                  (post-return $cabi_post_handler)
                )
              )
              (export "handler" (func $handler))
            )
        "#
    }
}
