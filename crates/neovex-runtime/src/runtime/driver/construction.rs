use std::rc::Rc;
use std::sync::OnceLock;

use deno_core::{JsRuntime, RuntimeOptions, v8};

use crate::error::{NeovexRuntimeError, Result};
use crate::module_loader::SandboxedModuleLoader;

#[cfg(test)]
use super::super::bootstrap::bootstrap_snapshot_build_count_for_test;
use super::super::bootstrap::{
    RuntimeStartupSnapshot, finalize_bootstrap, initialize_runtime_state, install_bootstrap,
    runtime_extension,
};
use super::super::{NeovexRuntime, RuntimeBundle};

impl NeovexRuntime {
    pub(crate) fn bootstrap_snapshot(&self) -> Result<&'static RuntimeStartupSnapshot> {
        static BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<RuntimeStartupSnapshot, String>> =
            OnceLock::new();
        match BOOTSTRAP_SNAPSHOT
            .get_or_init(|| Self::create_bootstrap_snapshot().map_err(|error| error.to_string()))
        {
            Ok(snapshot) => Ok(snapshot),
            Err(message) => Err(NeovexRuntimeError::Contract(format!(
                "failed to initialize runtime bootstrap snapshot: {message}"
            ))),
        }
    }

    pub(crate) fn create_bootstrap_snapshot() -> Result<RuntimeStartupSnapshot> {
        super::super::bootstrap::create_bootstrap_snapshot()
    }

    pub(crate) fn create_runtime_from_snapshot(
        &self,
        bundle: &RuntimeBundle,
        snapshot: &RuntimeStartupSnapshot,
    ) -> Result<JsRuntime> {
        self.create_runtime(bundle, Some(snapshot), false)
    }

    pub(crate) fn create_runtime(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&RuntimeStartupSnapshot>,
        use_locker: bool,
    ) -> Result<JsRuntime> {
        let mut runtime =
            JsRuntime::new(self.runtime_options(bundle, startup_snapshot, use_locker)?);
        self.initialize_runtime_state(&mut runtime);
        if startup_snapshot.is_none() {
            Self::install_bootstrap(&mut runtime)?;
        }
        Self::finalize_bootstrap(&mut runtime)?;
        Ok(runtime)
    }

    pub(crate) fn runtime_options(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&RuntimeStartupSnapshot>,
        use_locker: bool,
    ) -> Result<RuntimeOptions> {
        Ok(RuntimeOptions {
            create_params: Some(self.create_isolate_params()),
            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                bundle.module_root()?,
                bundle.module_code_cache(),
            ))),
            extensions: vec![runtime_extension()],
            startup_snapshot: startup_snapshot.map(RuntimeStartupSnapshot::as_startup_snapshot),
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

    pub(crate) fn initialize_runtime_state(&self, runtime: &mut JsRuntime) {
        initialize_runtime_state(runtime, self);
    }

    pub(crate) fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
        install_bootstrap(runtime)
    }

    pub(crate) fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
        finalize_bootstrap(runtime)
    }
}

#[cfg(test)]
pub(crate) fn snapshot_build_count_for_test() -> usize {
    bootstrap_snapshot_build_count_for_test()
}
