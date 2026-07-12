use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::backends::v8::embedder::{
    JsRuntime, RuntimeOptions, SharedArrayBufferStore, ValidateImportAttributesCb, v8,
};
use crate::backends::v8::{
    RuntimeStartupSnapshotKey, V8RuntimeConstructionMode, V8StartupSnapshot,
    create_v8_startup_snapshot,
};
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
        static WEB_STANDARD_SERVICE_BOOTSTRAP_SNAPSHOT: OnceLock<
            std::result::Result<V8StartupSnapshot, String>,
        > = OnceLock::new();
        static NODE_FULL_BOOTSTRAP_SNAPSHOT: OnceLock<
            std::result::Result<V8StartupSnapshot, String>,
        > = OnceLock::new();
        static NODE_FULL_SERVICE_BOOTSTRAP_SNAPSHOT: OnceLock<
            std::result::Result<V8StartupSnapshot, String>,
        > = OnceLock::new();

        let snapshot_key =
            RuntimeStartupSnapshotKey::for_limits(self.policy.limits()).ok_or_else(|| {
                NimbusRuntimeError::Contract(
                    "Bun/JSC compatibility target cannot use the V8 bootstrap snapshot path"
                        .to_string(),
                )
            })?;
        let snapshot = match snapshot_key {
            RuntimeStartupSnapshotKey::WebLean => &WEB_STANDARD_BOOTSTRAP_SNAPSHOT,
            RuntimeStartupSnapshotKey::WebLeanService => &WEB_STANDARD_SERVICE_BOOTSTRAP_SNAPSHOT,
            RuntimeStartupSnapshotKey::NodeFull => &NODE_FULL_BOOTSTRAP_SNAPSHOT,
            RuntimeStartupSnapshotKey::NodeFullService => &NODE_FULL_SERVICE_BOOTSTRAP_SNAPSHOT,
        };
        match snapshot.get_or_init(|| {
            // Embedded fast path for the NodeFull anchor snapshot: deserialize the committed blob
            // (~19ms) instead of building it (~4.18s, which — armed lazily — lands inside the first
            // request and blows per-request timeouts). Active in BOTH pointer-compression configs
            // (release/cage `.pc.bin` and dev/test `.bin`), each with its own committed blob.
            // Provenance-guarded and FAIL-SAFE: a stale, empty, or corrupt blob returns None and we
            // fall back to a runtime build (slow-but-correct); the embedded snapshot is NEVER
            // installed on any doubt. The cage's first-installer ORDERING is independently guarded by
            // `anchor::assert_cage_install_ordering` below.
            if matches!(snapshot_key, RuntimeStartupSnapshotKey::NodeFull)
                && let Some(embedded) = crate::backends::v8::try_embedded_node22_anchor_snapshot(
                    crate::backends::v8::EMBEDDED_NODE22_ANCHOR_SNAPSHOT,
                )
            {
                return Ok(embedded);
            }
            Self::create_bootstrap_snapshot(
                snapshot_key.snapshot_build_target(),
                snapshot_key.service_extension_enabled(),
            )
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
        service_extension_enabled: bool,
    ) -> Result<V8StartupSnapshot> {
        create_v8_startup_snapshot(compatibility_target, service_extension_enabled)
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

    /// The SINGLE source of truth mapping a `V8RuntimeConstructionMode` to whether a startup
    /// snapshot is deserialized: `StartupSnapshot` rides `bootstrap_snapshot`, `Unsnapshotted`
    /// builds with no snapshot. Every construction path that selects a mode — the warm pool AND
    /// the pool-less direct invocation path — routes through here, so the mapping cannot drift. A
    /// divergent copy on the direct path that hardcoded the snapshot mode was the second
    /// cross-profile cage-crash hole (efd891a8a).
    pub(crate) fn create_runtime_for_mode(
        &self,
        bundle: &RuntimeBundle,
        use_locker: bool,
        construction_mode: V8RuntimeConstructionMode,
    ) -> Result<JsRuntime> {
        let startup_snapshot = match construction_mode {
            V8RuntimeConstructionMode::StartupSnapshot => Some(self.bootstrap_snapshot()?),
            V8RuntimeConstructionMode::Unsnapshotted => None::<&V8StartupSnapshot>,
        };
        self.create_runtime(bundle, startup_snapshot, use_locker)
    }

    fn create_runtime_with_bootstrap_state(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&V8StartupSnapshot>,
        use_locker: bool,
        worker_bootstrap_state: InstalledRuntimeWorkerBootstrapState,
    ) -> Result<JsRuntime> {
        // Serialize cold isolate CREATION (snapshot restore deserializers in
        // Isolate::Init) against snapshot BUILD and isolate DISPOSAL on one
        // process-global re-entrant lock owned by deno_core. On a single (shared)
        // pointer-compression cage every isolate aliases the group's shared
        // read-only heap; create/build/dispose are its only unguarded writers and
        // must be mutually exclusive across threads (else `shared_heap_object_cache
        // ->at()` OOB / vector abort). Held across the whole body since
        // bootstrap/snapshot-restore also touch the shared RO heap. Re-entrant so a
        // failed construction dropping a partial runtime (deno Drop) doesn't
        // self-deadlock. A multi-cage build (private RO heap per isolate) can drop
        // this lock entirely.
        // Anchor floor (Option A): fail closed if any isolate is built before the NodeFull
        // RO-heap anchor is installed. No-op unless the anchor system is in use.
        super::anchor::assert_anchor_floor();
        let _shared_heap_guard = deno_core::shared_ro_heap_serialize_lock().lock();
        // SHIPPED cage invariant guard (Option A): UNDER the shared-RO-heap lock (so the recorded
        // first installer matches the actual install order), abort LOUD if a NodeFull superset
        // snapshot would deserialize against a cage first fixed by a non-superset isolate — the
        // cross-profile crash, otherwise a rare, silent V8_Fatal in ReadOnlyDeserializer. Covers the
        // pre-anchor-arm window assert_anchor_floor does not. No-op feature-off.
        super::anchor::assert_cage_install_ordering(
            startup_snapshot.is_some(),
            self.policy.limits().compatibility_target,
        );
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
        let construction_mode = if startup_snapshot.is_some() {
            V8RuntimeConstructionMode::StartupSnapshot
        } else {
            V8RuntimeConstructionMode::Unsnapshotted
        };
        let mut extensions = execution_extensions(
            self.policy.limits().compatibility_target,
            &path_policy,
            loader_hook_registry.clone(),
            self.policy.limits(),
            self.policy.file_system(),
        );
        extensions.push(worker_threads_state_extension(worker_bootstrap_state));
        let startup_snapshot_bytes = startup_snapshot.map(V8StartupSnapshot::as_startup_snapshot);
        let residual_lazy_js_sources = startup_snapshot
            .map(V8StartupSnapshot::residual_lazy_js_sources)
            .unwrap_or_default();
        let residual_lazy_esm_sources = startup_snapshot
            .map(V8StartupSnapshot::residual_lazy_esm_sources)
            .unwrap_or_default();
        let extension_replay_js_sources = startup_snapshot
            .map(V8StartupSnapshot::extension_replay_js_sources)
            .unwrap_or_default();
        let extension_replay_esm_sources = startup_snapshot
            .map(V8StartupSnapshot::extension_replay_esm_sources)
            .unwrap_or_default();
        Ok(RuntimeOptions {
            create_params: Some(self.create_isolate_params()),
            module_loader: Some(Rc::new(RestrictedModuleLoader::new(
                path_policy.clone(),
                self.policy.limits().compatibility_target,
                self.policy.limits().guest_semantics,
                self.policy.limits().node_conditions.clone(),
                bundle.module_code_cache(self.policy.limits(), construction_mode),
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
            extension_replay_js_sources,
            extension_replay_esm_sources,
            shared_array_buffer_store: Some(SharedArrayBufferStore::default()),
            validate_import_attributes_cb: node_import_attribute_validator(
                self.policy.limits().compatibility_target,
            ),
            use_locker,
            ..Default::default()
        })
    }

    pub(crate) fn create_isolate_params(&self) -> v8::CreateParams {
        let heap_megabyte = 1usize << 20;
        crate::backends::v8::attach_cppgc_heap(v8::Isolate::create_params())
            .heap_limits(
                self.policy.limits().initial_heap_mb * heap_megabyte,
                self.policy.limits().max_heap_mb * heap_megabyte,
            )
            .allow_atomics_wait(false)
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

fn validate_node_import_attributes(
    scope: &mut v8::PinScope,
    attributes: &HashMap<String, String>,
    _context: &deno_core::ImportAttributesContext,
) {
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

fn install_missing_runtime_extension_state(runtime: &mut JsRuntime) {
    install_missing_uv_loop(runtime);
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    install_missing_deno_extension_state(&mut state);
}

/// Re-install the per-runtime libuv-compat loop on snapshot-restored runtimes.
///
/// `deno_core::JsRuntime::new` installs the `Box<uv_loop_t>` (and its sibling
/// `uv_compat::AsyncId`) that ext/node TTY and stream ops borrow from `OpState`
/// only on the fresh, non-snapshot construction path; the startup-snapshot
/// restore path skips it. Without the loop, the first Node op that needs it --
/// e.g. `process.stdout` materializing a `TTY` via `TTY.newTty` -- panics
/// inside a non-unwinding V8 callback ("required type Box<uv_loop_t> is not
/// present in GothamState container") and aborts the whole process.
/// `install_missing_deno_extension_state` already backfills the sibling
/// `AsyncId`; this backfills the loop itself, mirroring `deno_core`'s own setup.
/// `deno_core`'s runtime teardown destroys the realm and then drops the
/// `Box<uv_loop_t>` from `OpState` generically, so no extra cleanup is required.
fn install_missing_uv_loop(runtime: &mut JsRuntime) {
    {
        let op_state = runtime.op_state();
        let already_installed = op_state
            .borrow()
            .has::<Box<deno_core::uv_compat::uv_loop_t>>();
        if already_installed {
            return;
        }
    }
    // SAFETY: zeroed memory is a valid `uv_loop_t` before `uv_loop_init`; the
    // pointer is valid and initialized before `register_uv_loop`; the loop is
    // owned by `OpState` for the runtime's lifetime and torn down with it.
    let mut uv_loop = Box::new(unsafe { std::mem::zeroed::<deno_core::uv_compat::uv_loop_t>() });
    unsafe { deno_core::uv_compat::uv_loop_init(&mut *uv_loop) };
    let loop_ptr: *mut deno_core::uv_compat::uv_loop_t = &mut *uv_loop;
    unsafe { runtime.register_uv_loop(loop_ptr) };
    runtime.op_state().borrow_mut().put(uv_loop);
}
