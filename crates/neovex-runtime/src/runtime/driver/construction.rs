use std::rc::Rc;
use std::sync::OnceLock;

use crate::backends::v8::embedder::{JsRuntime, RuntimeOptions, v8};
use crate::backends::v8::{V8StartupSnapshot, create_v8_startup_snapshot};
use crate::error::{NeovexRuntimeError, Result};
use crate::limits::RuntimeCompatibilityTarget;
use crate::module_loader::RestrictedModuleLoader;
use crate::runtime_capabilities::RuntimePathPolicy;

use super::super::bootstrap::{
    execution_extensions, extension_transpiler_for_target, finalize_bootstrap,
    initialize_runtime_state, install_bootstrap,
};
use super::super::{NeovexRuntime, RuntimeBundle};

impl NeovexRuntime {
    pub(crate) fn bootstrap_snapshot(&self) -> Result<&'static V8StartupSnapshot> {
        static WEB_STANDARD_BOOTSTRAP_SNAPSHOT: OnceLock<
            std::result::Result<V8StartupSnapshot, String>,
        > = OnceLock::new();
        static NODE22_BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<V8StartupSnapshot, String>> =
            OnceLock::new();
        let snapshot = match self.policy.limits().compatibility_target {
            RuntimeCompatibilityTarget::WebStandardIsolate => &WEB_STANDARD_BOOTSTRAP_SNAPSHOT,
            RuntimeCompatibilityTarget::Node22 => &NODE22_BOOTSTRAP_SNAPSHOT,
        };
        match snapshot.get_or_init(|| {
            Self::create_bootstrap_snapshot(self.policy.limits().compatibility_target)
                .map_err(|error| error.to_string())
        }) {
            Ok(snapshot) => Ok(snapshot),
            Err(message) => Err(NeovexRuntimeError::Contract(format!(
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
        self.create_runtime(bundle, Some(snapshot), false)
    }

    pub(crate) fn create_runtime(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&V8StartupSnapshot>,
        use_locker: bool,
    ) -> Result<JsRuntime> {
        let mut runtime =
            JsRuntime::new(self.runtime_options(bundle, startup_snapshot, use_locker)?);
        self.initialize_runtime_state(&mut runtime, bundle)?;
        if startup_snapshot.is_none() {
            Self::install_bootstrap(&mut runtime)?;
        }
        Self::finalize_bootstrap(&mut runtime)?;
        Ok(runtime)
    }

    pub(crate) fn runtime_options(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&V8StartupSnapshot>,
        use_locker: bool,
    ) -> Result<RuntimeOptions> {
        let path_policy = RuntimePathPolicy::for_bundle(bundle, self.policy.limits())?;
        Ok(RuntimeOptions {
            create_params: Some(self.create_isolate_params()),
            module_loader: Some(Rc::new(RestrictedModuleLoader::new(
                path_policy.clone(),
                self.policy.limits().compatibility_target,
                bundle.module_code_cache(),
            ))),
            extensions: execution_extensions(
                self.policy.limits().compatibility_target,
                &path_policy,
            ),
            extension_transpiler: extension_transpiler_for_target(
                self.policy.limits().compatibility_target,
            ),
            startup_snapshot: startup_snapshot.map(V8StartupSnapshot::as_startup_snapshot),
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
