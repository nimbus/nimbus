use std::time::Instant;

use deno_core::JsRuntime;

use crate::RuntimeInvocationContext;
use crate::error::Result;

use super::super::helpers::{deserialize_json_value, runtime_js_error};
use super::super::{InvocationRequest, NeovexRuntime, RuntimeBundle, RuntimeConstructionMode};
use super::tracing::{
    trace_snapshot_seeded_runtime_error, trace_snapshot_seeded_runtime_error_with_optional_bundle,
    trace_snapshot_seeded_runtime_phase, trace_snapshot_seeded_runtime_phase_with_optional_bundle,
};

impl NeovexRuntime {
    #[cfg(test)]
    pub(crate) async fn load_bundle(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<()> {
        self.load_bundle_with_trace(
            runtime,
            bundle,
            RuntimeConstructionMode::Unsnapshotted,
            None,
            None,
        )
        .await
    }

    pub(crate) async fn load_bundle_with_trace(
        &self,
        runtime: &mut JsRuntime,
        bundle: &RuntimeBundle,
        construction_mode: RuntimeConstructionMode,
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
        construction_mode: RuntimeConstructionMode,
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
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:load_main_es_module:start",
        );
        let module_id = runtime
            .load_main_es_module(&module_specifier)
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    request,
                    "load_bundle:load_main_es_module:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            request,
            "load_bundle:load_main_es_module:complete",
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
        construction_mode: RuntimeConstructionMode,
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
            RuntimeConstructionMode::Unsnapshotted,
            None,
        )
        .await
    }

    pub(crate) async fn invoke_loaded_bundle_with_trace(
        &self,
        runtime: &mut JsRuntime,
        request: &InvocationRequest,
        bundle: Option<&RuntimeBundle>,
        construction_mode: RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<serde_json::Value> {
        let request_json = serde_json::to_string(request)?;
        let expression = format!("globalThis.__neovexInvoke({request_json})");
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
        let value = runtime
            .execute_script("<neovex-runtime:invoke>", expression)
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error_with_optional_bundle(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_loaded_bundle:execute_script:error",
                    &error,
                );
                runtime_js_error(error)
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
            .with_event_loop_promise(resolve, deno_core::PollEventLoopOptions::default())
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
