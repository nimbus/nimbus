use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::backends::v8::embedder::{
    JsErrorBox, JsRuntime, JsonModuleEvaluationCb, ModuleName, ModuleSpecifier, RuntimeOptions,
    SharedArrayBufferStore, ValidateImportAttributesCb, v8,
};
use crate::backends::v8::{V8StartupSnapshot, create_v8_startup_snapshot};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeCompatibilityTarget;
use crate::module_loader::RestrictedModuleLoader;
use crate::runtime_capabilities::RuntimePathPolicy;

use super::super::bootstrap::{
    InstalledRuntimeWorkerBootstrapState, execution_extensions, extension_transpiler_for_target,
    finalize_bootstrap, initialize_runtime_state, install_bootstrap,
    install_missing_deno_extension_state, main_thread_worker_bootstrap_state,
    worker_threads_state_extension,
};
use super::super::{NimbusRuntime, RuntimeBundle};

impl NimbusRuntime {
    pub(crate) fn bootstrap_snapshot(&self) -> Result<&'static V8StartupSnapshot> {
        static WEB_STANDARD_BOOTSTRAP_SNAPSHOT: OnceLock<
            std::result::Result<V8StartupSnapshot, String>,
        > = OnceLock::new();
        static NODE22_BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<V8StartupSnapshot, String>> =
            OnceLock::new();
        static NODE20_BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<V8StartupSnapshot, String>> =
            OnceLock::new();
        static NODE24_BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<V8StartupSnapshot, String>> =
            OnceLock::new();
        static NODE26_BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<V8StartupSnapshot, String>> =
            OnceLock::new();
        let snapshot = match self.policy.limits().compatibility_target {
            RuntimeCompatibilityTarget::WebStandardIsolate => &WEB_STANDARD_BOOTSTRAP_SNAPSHOT,
            RuntimeCompatibilityTarget::Node20 => &NODE20_BOOTSTRAP_SNAPSHOT,
            RuntimeCompatibilityTarget::Node22 => &NODE22_BOOTSTRAP_SNAPSHOT,
            RuntimeCompatibilityTarget::Node24 => &NODE24_BOOTSTRAP_SNAPSHOT,
            RuntimeCompatibilityTarget::Node26 => &NODE26_BOOTSTRAP_SNAPSHOT,
            RuntimeCompatibilityTarget::BunJsc => {
                return Err(NimbusRuntimeError::Contract(
                    "Bun/JSC compatibility target cannot use the V8 bootstrap snapshot path"
                        .to_string(),
                ));
            }
        };
        match snapshot.get_or_init(|| {
            Self::create_bootstrap_snapshot(self.policy.limits().compatibility_target)
                .map_err(|error| error.to_string())
        }) {
            Ok(snapshot) => Ok(snapshot),
            Err(message) => Err(NimbusRuntimeError::Contract(format!(
                "failed to initialize runtime bootstrap snapshot: {message}"
            ))),
        }
    }

    pub(crate) fn create_bootstrap_snapshot(
        compatibility_target: RuntimeCompatibilityTarget,
    ) -> Result<V8StartupSnapshot> {
        create_v8_startup_snapshot(compatibility_target)
    }

    pub(crate) fn create_runtime_from_snapshot(
        &self,
        bundle: &RuntimeBundle,
        snapshot: &V8StartupSnapshot,
    ) -> Result<JsRuntime> {
        self.create_runtime_with_bootstrap_state(
            bundle,
            Some(snapshot),
            false,
            main_thread_worker_bootstrap_state(),
        )
    }

    pub(crate) fn create_runtime(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&V8StartupSnapshot>,
        use_locker: bool,
    ) -> Result<JsRuntime> {
        self.create_runtime_with_bootstrap_state(
            bundle,
            startup_snapshot,
            use_locker,
            main_thread_worker_bootstrap_state(),
        )
    }

    fn create_runtime_with_bootstrap_state(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&V8StartupSnapshot>,
        use_locker: bool,
        worker_bootstrap_state: InstalledRuntimeWorkerBootstrapState,
    ) -> Result<JsRuntime> {
        let mut runtime = JsRuntime::new(self.runtime_options(
            bundle,
            startup_snapshot,
            use_locker,
            worker_bootstrap_state,
        )?);
        install_missing_runtime_extension_state(&mut runtime);
        self.initialize_runtime_state(&mut runtime, bundle)?;
        if startup_snapshot.is_none() {
            Self::install_bootstrap(&mut runtime)?;
        }
        Self::finalize_bootstrap(&mut runtime)?;
        Ok(runtime)
    }

    pub(crate) fn create_unsnapshotted_runtime_with_worker_bootstrap(
        &self,
        bundle: &RuntimeBundle,
        worker_bootstrap_state: InstalledRuntimeWorkerBootstrapState,
    ) -> Result<JsRuntime> {
        self.create_runtime_with_bootstrap_state(bundle, None, false, worker_bootstrap_state)
    }

    pub(crate) fn runtime_options(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&V8StartupSnapshot>,
        use_locker: bool,
        worker_bootstrap_state: InstalledRuntimeWorkerBootstrapState,
    ) -> Result<RuntimeOptions> {
        let path_policy = RuntimePathPolicy::for_bundle(bundle, self.policy.limits())?;
        let loader_hook_registry = self
            .policy
            .limits()
            .compatibility_target
            .is_node()
            .then(deno_node::ops::module_hooks::LoaderHookRegistry::default);
        let mut extensions = execution_extensions(
            self.policy.limits().compatibility_target,
            &path_policy,
            loader_hook_registry.clone(),
            self.policy.limits(),
        );
        extensions.push(worker_threads_state_extension(worker_bootstrap_state));
        let startup_snapshot_bytes = startup_snapshot.map(V8StartupSnapshot::as_startup_snapshot);
        let residual_lazy_js_sources = startup_snapshot
            .map(V8StartupSnapshot::residual_lazy_js_sources)
            .unwrap_or_default();
        let residual_lazy_esm_sources = startup_snapshot
            .map(V8StartupSnapshot::residual_lazy_esm_sources)
            .unwrap_or_default();
        Ok(RuntimeOptions {
            create_params: Some(self.create_isolate_params()),
            module_loader: Some(Rc::new(RestrictedModuleLoader::new(
                path_policy.clone(),
                self.policy.limits().compatibility_target,
                self.policy.limits().node_conditions.clone(),
                bundle.module_code_cache(self.policy.limits()),
                loader_hook_registry,
            ))),
            extensions,
            extension_transpiler: extension_transpiler_for_target(
                self.policy.limits().compatibility_target,
            ),
            inspector: matches!(
                self.policy.limits().compatibility_target,
                RuntimeCompatibilityTarget::Node20
                    | RuntimeCompatibilityTarget::Node22
                    | RuntimeCompatibilityTarget::Node24
                    | RuntimeCompatibilityTarget::Node26
            ),
            startup_snapshot: startup_snapshot_bytes,
            residual_lazy_js_sources,
            residual_lazy_esm_sources,
            shared_array_buffer_store: Some(SharedArrayBufferStore::default()),
            validate_import_attributes_cb: node_import_attribute_validator(
                self.policy.limits().compatibility_target,
            ),
            json_module_evaluation_cb: node_json_module_evaluator(
                self.policy.limits().compatibility_target,
            ),
            use_locker,
            ..Default::default()
        })
    }

    pub(crate) fn create_isolate_params(&self) -> v8::CreateParams {
        let heap_megabyte = 1usize << 20;
        v8::Isolate::create_params().heap_limits(
            self.policy.limits().initial_heap_mb * heap_megabyte,
            self.policy.limits().max_heap_mb * heap_megabyte,
        )
    }

    pub(crate) fn initialize_runtime_state(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<()> {
        initialize_runtime_state(runtime, self, bundle)
    }

    pub(crate) fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
        install_bootstrap(runtime)
    }

    pub(crate) fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
        finalize_bootstrap(runtime)
    }
}

fn node_import_attribute_validator(
    compatibility_target: RuntimeCompatibilityTarget,
) -> Option<ValidateImportAttributesCb> {
    compatibility_target
        .is_node()
        .then(|| Box::new(validate_node_import_attributes) as ValidateImportAttributesCb)
}

fn node_json_module_evaluator(
    compatibility_target: RuntimeCompatibilityTarget,
) -> Option<JsonModuleEvaluationCb> {
    compatibility_target
        .is_node()
        .then(|| Box::new(evaluate_node_json_module_default_export) as JsonModuleEvaluationCb)
}

fn validate_node_import_attributes(scope: &mut v8::PinScope, attributes: &HashMap<String, String>) {
    for (key, value) in attributes {
        if key == "type" {
            continue;
        }
        let message = format!("Import attribute \"{key}\" with value \"{value}\" is not supported");
        let message = v8::String::new(scope, &message).unwrap();
        let exception = v8::Exception::type_error(scope, message);
        let exception_obj = exception.to_object(scope).unwrap();
        let code_key = v8::String::new(scope, "code").unwrap();
        let code_value = v8::String::new(scope, "ERR_IMPORT_ATTRIBUTE_UNSUPPORTED").unwrap();
        exception_obj.set(scope, code_key.into(), code_value.into());
        scope.throw_exception(exception);
        return;
    }
}

fn evaluate_node_json_module_default_export(
    scope: &mut v8::PinScope,
    module_name: &ModuleName,
    parsed_json: v8::Global<v8::Value>,
) -> std::result::Result<v8::Global<v8::Value>, JsErrorBox> {
    let Some((filename, dirname)) = node_json_cjs_cache_path(module_name.as_str()) else {
        return Ok(parsed_json);
    };
    let Some(module_cache) = node_module_cache(scope)? else {
        return Ok(parsed_json);
    };

    let filename_key = v8_string(scope, &filename)?;
    if let Some(cached_module) = module_cache.get(scope, filename_key.into())
        && let Ok(cached_module) = cached_module.try_cast::<v8::Object>()
        && v8_object_bool(scope, cached_module, "loaded")?
        && let Some(exports) = v8_object_get(scope, cached_module, "exports")?
    {
        return Ok(v8::Global::new(scope, exports));
    }

    let parsed_json_value = v8::Local::new(scope, parsed_json.clone());
    let cache_entry = v8::Object::new(scope);
    let id_value = v8_string(scope, &filename)?;
    let path_value = v8_string(scope, &dirname)?;
    let filename_value = v8_string(scope, &filename)?;
    let loaded_value = v8::Boolean::new(scope, true);
    let children_value = v8::Array::new(scope, 0);

    v8_object_set(scope, cache_entry, "id", id_value.into())?;
    v8_object_set(scope, cache_entry, "path", path_value.into())?;
    v8_object_set(scope, cache_entry, "exports", parsed_json_value)?;
    v8_object_set(scope, cache_entry, "filename", filename_value.into())?;
    v8_object_set(scope, cache_entry, "loaded", loaded_value.into())?;
    v8_object_set(scope, cache_entry, "children", children_value.into())?;
    if !module_cache
        .set(scope, filename_key.into(), cache_entry.into())
        .unwrap_or(false)
    {
        return Err(JsErrorBox::generic(
            "failed to populate CommonJS cache for JSON module",
        ));
    }

    Ok(parsed_json)
}

fn node_json_cjs_cache_path(module_name: &str) -> Option<(String, String)> {
    let module_specifier = ModuleSpecifier::parse(module_name).ok()?;
    if module_specifier.scheme() != "file"
        || module_specifier.query().is_some()
        || module_specifier.fragment().is_some()
    {
        return None;
    }
    let path = module_specifier.to_file_path().ok()?;
    let filename = path.to_string_lossy().into_owned();
    let dirname = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some((filename, dirname))
}

fn node_module_cache<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> std::result::Result<Option<v8::Local<'s, v8::Object>>, JsErrorBox> {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let Some(process_value) = v8_object_get(scope, global, "process")? else {
        return Ok(None);
    };
    let Ok(process_object) = process_value.try_cast::<v8::Object>() else {
        return Ok(None);
    };
    let Some(get_builtin_value) = v8_object_get(scope, process_object, "getBuiltinModule")? else {
        return Ok(None);
    };
    let Ok(get_builtin_module) = get_builtin_value.try_cast::<v8::Function>() else {
        return Ok(None);
    };
    let module_name = v8_string(scope, "module")?;
    let Some(module_value) =
        get_builtin_module.call(scope, process_object.into(), &[module_name.into()])
    else {
        return Ok(None);
    };
    let Ok(module_object) = module_value.try_cast::<v8::Object>() else {
        return Ok(None);
    };
    let Some(cache_value) = v8_object_get(scope, module_object, "_cache")? else {
        return Ok(None);
    };
    Ok(cache_value.try_cast::<v8::Object>().ok())
}

fn v8_object_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
) -> std::result::Result<Option<v8::Local<'s, v8::Value>>, JsErrorBox> {
    let key = v8_string(scope, key)?;
    Ok(object.get(scope, key.into()))
}

fn v8_object_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
    value: v8::Local<'s, v8::Value>,
) -> std::result::Result<(), JsErrorBox> {
    let key_name = key;
    let key_value = v8_string(scope, key_name)?;
    if object.set(scope, key_value.into(), value).unwrap_or(false) {
        Ok(())
    } else {
        Err(JsErrorBox::generic(format!(
            "failed to set CommonJS cache property {key_name}"
        )))
    }
}

fn v8_object_bool<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
) -> std::result::Result<bool, JsErrorBox> {
    Ok(v8_object_get(scope, object, key)?.is_some_and(|value| value.is_true()))
}

fn v8_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: &str,
) -> std::result::Result<v8::Local<'s, v8::String>, JsErrorBox> {
    v8::String::new(scope, value).ok_or_else(|| {
        JsErrorBox::generic("failed to allocate runtime module loader string".to_string())
    })
}

fn install_missing_runtime_extension_state(runtime: &mut JsRuntime) {
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    install_missing_deno_extension_state(&mut state);
}
