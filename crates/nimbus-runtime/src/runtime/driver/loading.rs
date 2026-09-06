use std::time::Instant;

use crate::RuntimeInvocationContext;
use crate::backends::v8::V8RuntimeConstructionMode;
use crate::backends::v8::embedder::{JsRuntime, PollEventLoopOptions};
use crate::error::{NimbusRuntimeError, Result};

use super::super::bootstrap::clear_runtime_wait_until_pending;
use super::super::classify::{
    deserialize_json_value, ensure_wait_until_drain_succeeded, runtime_js_error,
};
use super::super::{
    InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeBundleEntrypointKind,
};
use super::tracing::{
    trace_snapshot_seeded_runtime_error, trace_snapshot_seeded_runtime_error_with_optional_bundle,
    trace_snapshot_seeded_runtime_phase, trace_snapshot_seeded_runtime_phase_with_optional_bundle,
};

impl NimbusRuntime {
    /// Builds the guest-semantics import-phase entry script for `bundle`, or
    /// `None` on Host-semantics lanes. Executed immediately before module
    /// evaluation so module-scope code observes the deploy-stamped clock and
    /// deploy-seeded PRNG (Convex default-runtime import-time contract).
    fn guest_import_phase_script(&self, bundle: &RuntimeBundle) -> Option<String> {
        if !matches!(
            self.policy.limits().guest_semantics,
            crate::limits::RuntimeGuestSemantics::ConvexDefault
        ) {
            return None;
        }
        let stamp = bundle.deploy_stamp();
        let seed_json =
            serde_json::to_string(&stamp.seed_hex).unwrap_or_else(|_| "\"\"".to_string());
        Some(format!(
            "globalThis.__nimbusEnterGuestImportPhase?.({{ deploy_ts_ms: {}, deploy_seed_hex: {} }});",
            stamp.timestamp_ms, seed_json
        ))
    }

    #[cfg(test)]
    pub(crate) async fn load_bundle(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<()> {
        self.load_bundle_with_trace(
            runtime,
            bundle,
            V8RuntimeConstructionMode::Unsnapshotted,
            None,
            None,
        )
        .await
    }

    pub(crate) async fn load_bundle_with_trace(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
        construction_mode: V8RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
        request: Option<&InvocationRequest>,
    ) -> Result<()> {
        self.load_bundle_without_post_return_settle_with_trace(
            runtime,
            bundle,
            construction_mode,
            context,
            request,
        )
        .await?;
        self.settle_post_bundle_load_with_trace(
            runtime,
            bundle,
            construction_mode,
            context,
            request,
        )
        .await
    }

    async fn load_bundle_without_post_return_settle_with_trace(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
        construction_mode: V8RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
        request: Option<&InvocationRequest>,
    ) -> Result<()> {
        let started_at = Instant::now();
        let module_specifier = bundle.module_specifier()?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:start",
        );
        let module_load_started_at = Instant::now();
        let (load_start_phase, load_error_phase, load_complete_phase) =
            match bundle.entrypoint_kind() {
                RuntimeBundleEntrypointKind::Main => (
                    "load_bundle:load_main_es_module:start",
                    "load_bundle:load_main_es_module:error",
                    "load_bundle:load_main_es_module:complete",
                ),
                RuntimeBundleEntrypointKind::Side => (
                    "load_bundle:load_side_es_module:start",
                    "load_bundle:load_side_es_module:error",
                    "load_bundle:load_side_es_module:complete",
                ),
            };
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            load_start_phase,
        );
        if let Some(script) = self.guest_import_phase_script(bundle) {
            runtime
                .execute_script("<nimbus-runtime:guest-semantics:import-phase>", script)
                .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
        }
        let module_id = match bundle.entrypoint_kind() {
            RuntimeBundleEntrypointKind::Main => {
                runtime.load_main_es_module(&module_specifier).await
            }
            RuntimeBundleEntrypointKind::Side => {
                runtime.load_side_es_module(&module_specifier).await
            }
        }
        .map_err(|error| {
            trace_snapshot_seeded_runtime_error(
                construction_mode,
                bundle,
                context,
                request,
                load_error_phase,
                &error,
            );
            runtime_js_error(error)
        })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            load_complete_phase,
        );
        self.policy
            .metrics()
            .record_bundle_module_load(module_load_started_at.elapsed());
        let evaluation = runtime.mod_evaluate(module_id);
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:mod_evaluate:scheduled",
        );
        let evaluation_started_at = Instant::now();
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:run_event_loop:start",
        );
        runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    request,
                    "load_bundle:run_event_loop:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:run_event_loop:complete",
        );
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:evaluation_await:start",
        );
        evaluation.await.map_err(|error| {
            trace_snapshot_seeded_runtime_error(
                construction_mode,
                bundle,
                context,
                request,
                "load_bundle:evaluation_await:error",
                &error,
            );
            runtime_js_error(error)
        })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:evaluation_await:complete",
        );
        tokio::task::yield_now().await;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:post_evaluation_run_event_loop:start",
        );
        runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    request,
                    "load_bundle:post_evaluation_run_event_loop:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:post_evaluation_run_event_loop:complete",
        );
        self.policy
            .metrics()
            .record_bundle_evaluation(evaluation_started_at.elapsed());
        self.policy
            .metrics()
            .record_bundle_load(started_at.elapsed());
        // Capture the invocation entrypoints (globalThis.__nimbusInvoke and, on
        // their lanes, __nimbusInvokeCloudflareWorkerFetch / __nimbusBeginGuestInvocation)
        // off the guest-reachable graph now that the bundle has fully evaluated
        // and before any guest handler body runs. Warm-pool reuse invokes the
        // captured reference (HG0/HG5), so a guest reassignment in one invocation
        // cannot redirect the trusted path of a later same-tenant invocation on
        // the same isolate.
        crate::runtime::captured_dispatch::capture_invocation_targets(
            runtime,
            self.policy.limits().guest_semantics,
        )?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:complete",
        );
        Ok(())
    }

    async fn settle_post_bundle_load_with_trace(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
        construction_mode: V8RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
        request: Option<&InvocationRequest>,
    ) -> Result<()> {
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:post_return_settle:start",
        );
        tokio::task::yield_now().await;
        runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    request,
                    "load_bundle:post_return_settle:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:post_return_settle:complete",
        );
        Ok(())
    }

    pub(crate) async fn drain_wait_until_with_trace(
        &self,
        runtime: &mut JsRuntime,
        bundle: Option<&RuntimeBundle>,
        request: &InvocationRequest,
        construction_mode: V8RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<()> {
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "wait_until:drain:start",
        );
        let value = runtime
            .execute_script(
                "<nimbus-runtime:wait-until>",
                "globalThis.__nimbusDrainWaitUntil()",
            )
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error_with_optional_bundle(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "wait_until:drain:execute_script:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        let resolve = runtime.resolve(value);
        let drain = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error_with_optional_bundle(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "wait_until:drain:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        ensure_wait_until_drain_succeeded(runtime, drain)?;
        clear_runtime_wait_until_pending(runtime);
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "wait_until:drain:complete",
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn invoke_loaded_bundle(
        &self,
        runtime: &mut JsRuntime,
        request: &InvocationRequest,
    ) -> Result<serde_json::Value> {
        self.invoke_loaded_bundle_with_trace(
            runtime,
            request,
            None,
            V8RuntimeConstructionMode::Unsnapshotted,
            None,
        )
        .await
    }

    pub(crate) async fn invoke_loaded_bundle_with_trace(
        &self,
        runtime: &mut JsRuntime,
        request: &InvocationRequest,
        bundle: Option<&RuntimeBundle>,
        construction_mode: V8RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<serde_json::Value> {
        let module_specifier = match (
            matches!(request.kind, InvocationKind::CloudflareWorkerFetch),
            bundle,
        ) {
            (true, Some(bundle)) => Some(bundle.module_specifier()?.to_string()),
            (true, None) | (false, _) => None,
        };
        let request_json = serde_json::to_string(request)?;
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:start",
        );
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:execute_script:start",
        );
        // Call the invocation entrypoint captured off the guest-reachable graph
        // at bundle load (HG0/HG5), never `globalThis.__nimbusInvoke` by name.
        let value = crate::runtime::captured_dispatch::call_captured_invocation(
            runtime,
            &request_json,
            self.policy.limits().guest_semantics,
            module_specifier.as_deref(),
        )
        .inspect_err(|error| {
            trace_snapshot_seeded_runtime_error_with_optional_bundle(
                construction_mode,
                bundle,
                context,
                Some(request),
                "invoke_loaded_bundle:execute_script:error",
                error,
            );
        })?;
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:execute_script:complete",
        );
        let resolve = runtime.resolve(value);
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:with_event_loop_promise:start",
        );
        let value = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error_with_optional_bundle(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_loaded_bundle:with_event_loop_promise:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:with_event_loop_promise:complete",
        );
        let value = deserialize_json_value(runtime, value).inspect_err(|error| {
            trace_snapshot_seeded_runtime_error_with_optional_bundle(
                construction_mode,
                bundle,
                context,
                Some(request),
                "invoke_loaded_bundle:deserialize:error",
                error,
            );
        })?;
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:response_ready",
        );
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:deserialize:complete",
        );
        trace_snapshot_seeded_runtime_phase_with_optional_bundle(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_loaded_bundle:complete",
        );
        Ok(value)
    }
}
