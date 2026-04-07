use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use deno_core::{CreateRealmOptions, JsRuntime, RuntimeOptions, v8};
use tracing::debug;

use crate::error::{NeovexRuntimeError, Result};
use crate::module_loader::SandboxedModuleLoader;

#[cfg(test)]
use super::super::bootstrap::bootstrap_snapshot_build_count_for_test;
use super::super::bootstrap::{
    RuntimeStartupSnapshot, finalize_bootstrap, initialize_runtime_state, install_bootstrap,
    runtime_extension,
};
use super::super::{NeovexRuntime, RuntimeBundle, RuntimeConstructionMode};

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

    pub(crate) fn reset_retained_runtime(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
        construction_mode: RuntimeConstructionMode,
    ) -> Result<()> {
        let options = CreateRealmOptions {
            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                bundle.module_root()?,
                bundle.module_code_cache(),
            ))),
        };
        if runtime.is_v8_lock_held() {
            self.reset_retained_runtime_inner(runtime, bundle, construction_mode, options)?;
        } else {
            let mut locked = runtime.acquire_v8_lock();
            self.reset_retained_runtime_inner(&mut locked, bundle, construction_mode, options)?;
        }
        Ok(())
    }

    fn reset_retained_runtime_inner(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
        construction_mode: RuntimeConstructionMode,
        options: CreateRealmOptions,
    ) -> Result<()> {
        let bundle_label = bundle.entrypoint().display().to_string();
        if construction_mode.uses_startup_snapshot() {
            debug!(
                bundle = %bundle_label,
                construction_mode = construction_mode.as_str(),
                "resetting snapshot-seeded retained runtime"
            );
        }

        let reset_started_at = Instant::now();
        runtime.reset_main_realm(options).map_err(|error| {
            NeovexRuntimeError::Contract(format!(
                "failed to reset retained JsRuntime main realm ({construction_mode:?}) for {}: {error}",
                bundle.entrypoint().display()
            ))
        })?;
        self.policy
            .metrics()
            .record_retained_runtime_main_realm_reset(reset_started_at.elapsed());

        let bootstrap_started_at = Instant::now();
        self.initialize_runtime_state(runtime);
        match construction_mode {
            RuntimeConstructionMode::Unsnapshotted => {
                Self::install_bootstrap(runtime)?;
                Self::finalize_bootstrap(runtime)?;
            }
            RuntimeConstructionMode::StartupSnapshot => {
                Self::finalize_bootstrap(runtime)?;
            }
        }
        self.policy
            .metrics()
            .record_retained_runtime_bootstrap_replay(bootstrap_started_at.elapsed());

        if construction_mode.uses_startup_snapshot() {
            debug!(
                bundle = %bundle_label,
                construction_mode = construction_mode.as_str(),
                "finished snapshot-seeded retained runtime reset"
            );
        }

        Ok(())
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
