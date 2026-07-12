use std::rc::Rc;
use std::time::Instant;

use crate::RuntimeInvocationContext;
use crate::backends::v8::V8RuntimeConstructionMode;
use crate::backends::v8::embedder::{
    CreateRealmOptions, JsRealm, JsRuntime, ModuleLoader, PollEventLoopOptions, v8,
};
use crate::error::{NimbusRuntimeError, Result};
use crate::execution_plan::RuntimeExecutionPlan;
use crate::module_loader::RestrictedModuleLoader;
use crate::runtime_capabilities::RuntimePathPolicy;

use super::super::bootstrap::{
    clear_runtime_wait_until_pending, finalize_bootstrap_in_realm, install_bootstrap_in_realm,
    reset_bootstrap_invocation_state_in_realm, reset_runtime_contract,
    runtime_resource_table_delta,
};
use super::super::classify::{
    deserialize_json_value, ensure_wait_until_drain_succeeded, runtime_js_error,
};
use super::super::realm_lease::{
    RuntimeRealmLease, RuntimeRealmLeaseCondemnationReason, RuntimeRealmLeaseController,
    RuntimeRealmLeaseOwner,
};
use super::super::realm_lifecycle::destroy_fresh_realm;
use super::super::{
    InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeBundleEntrypointKind,
};
use super::tracing::{
    trace_snapshot_seeded_runtime_error, trace_snapshot_seeded_runtime_error_with_optional_bundle,
    trace_snapshot_seeded_runtime_phase, trace_snapshot_seeded_runtime_phase_with_optional_bundle,
};

#[derive(Clone, Copy)]
pub(crate) struct FreshRealmInvocationTrace<'a> {
    pub(crate) bundle: &'a RuntimeBundle,
    pub(crate) request: &'a InvocationRequest,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    pub(crate) context: Option<&'a RuntimeInvocationContext>,
}

pub(crate) struct FreshRealmInvocationResponse<'a> {
    pub(crate) realm: &'a JsRealm,
    pub(crate) value: v8::Global<v8::Value>,
    pub(crate) trace: FreshRealmInvocationTrace<'a>,
}

impl NimbusRuntime {
    pub(crate) fn checkout_fresh_realm_lease(
        &self,
        controller: &RuntimeRealmLeaseController,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        construction_mode: V8RuntimeConstructionMode,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<RuntimeRealmLease> {
        let context = context.ok_or_else(|| {
            NimbusRuntimeError::Contract(
                "NodeFull realm lease requires an invocation context".to_string(),
            )
        })?;
        let tenant = context.tenant_label.clone().ok_or_else(|| {
            NimbusRuntimeError::Contract("NodeFull realm lease requires a tenant label".to_string())
        })?;
        let execution_plan = RuntimeExecutionPlan::for_realm_lease_invocation(
            &self.policy,
            bundle,
            request,
            context,
            construction_mode,
        )?;
        controller
            .checkout(
                RuntimeRealmLeaseOwner::tenant(tenant),
                execution_plan.pool_authority_key().clone(),
            )
            .map_err(realm_lease_error)
    }

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

    pub(crate) async fn start_fresh_realm_bundle_invocation_with_trace(
        &self,
        runtime: &mut JsRuntime,
        trace: FreshRealmInvocationTrace<'_>,
    ) -> Result<(v8::Global<v8::Value>, JsRealm)> {
        let lease_error_reason = || RuntimeRealmLeaseCondemnationReason::Dirty;
        self.start_fresh_realm_bundle_invocation_with_optional_lease_and_trace(
            runtime,
            trace,
            None,
            &lease_error_reason,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn start_fresh_realm_bundle_invocation_with_lease_and_trace(
        &self,
        controller: &RuntimeRealmLeaseController,
        runtime: &mut JsRuntime,
        trace: FreshRealmInvocationTrace<'_>,
    ) -> Result<(v8::Global<v8::Value>, JsRealm, RuntimeRealmLease)> {
        self.start_fresh_realm_bundle_invocation_with_lease_and_reason_trace(
            controller,
            runtime,
            trace,
            || RuntimeRealmLeaseCondemnationReason::Dirty,
        )
        .await
    }

    pub(crate) async fn start_fresh_realm_bundle_invocation_with_lease_and_reason_trace(
        &self,
        controller: &RuntimeRealmLeaseController,
        runtime: &mut JsRuntime,
        trace: FreshRealmInvocationTrace<'_>,
        lease_error_reason: impl Fn() -> RuntimeRealmLeaseCondemnationReason,
    ) -> Result<(v8::Global<v8::Value>, JsRealm, RuntimeRealmLease)> {
        let mut lease = self.checkout_fresh_realm_lease(
            controller,
            trace.bundle,
            trace.request,
            trace.construction_mode,
            trace.context,
        )?;
        reset_runtime_contract(runtime, self, trace.bundle)?;
        match self
            .start_fresh_realm_bundle_invocation_with_optional_lease_and_trace(
                runtime,
                trace,
                Some(&mut lease),
                &lease_error_reason,
            )
            .await
        {
            Ok((value, realm)) => Ok((value, realm, lease)),
            Err(error) => {
                condemn_fresh_realm_lease(&mut lease, lease_error_reason());
                Err(error)
            }
        }
    }

    async fn start_fresh_realm_bundle_invocation_with_optional_lease_and_trace(
        &self,
        runtime: &mut JsRuntime,
        trace: FreshRealmInvocationTrace<'_>,
        lease: Option<&mut RuntimeRealmLease>,
        lease_error_reason: &dyn Fn() -> RuntimeRealmLeaseCondemnationReason,
    ) -> Result<(v8::Global<v8::Value>, JsRealm)> {
        let FreshRealmInvocationTrace {
            bundle,
            request,
            construction_mode,
            context,
        } = trace;
        let mut lease = lease;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:create_realm:start",
        );
        let create_started_at = Instant::now();
        let module_loader = self.fresh_realm_module_loader(bundle, construction_mode)?;
        let realm = runtime
            .create_realm(CreateRealmOptions {
                module_loader: Some(module_loader),
            })
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_recycled_context:create_realm:error",
                    &error,
                );
                if let Some(lease) = &mut lease {
                    condemn_fresh_realm_lease(lease, lease_error_reason());
                }
                runtime_js_error(error)
            })?;
        self.policy
            .metrics()
            .record_fresh_realm_create(create_started_at.elapsed());
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:create_realm:complete",
        );

        match self
            .start_existing_fresh_realm_bundle_invocation_with_trace(runtime, &realm, trace, lease)
            .await
        {
            Ok(value) => Ok((value, realm)),
            Err(error) => {
                let destroy_started_at = Instant::now();
                destroy_fresh_realm(runtime, realm);
                self.policy
                    .metrics()
                    .record_fresh_realm_destroy(destroy_started_at.elapsed());
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn resolve_fresh_realm_invocation_response_with_trace(
        &self,
        runtime: &mut JsRuntime,
        response: FreshRealmInvocationResponse<'_>,
    ) -> Result<serde_json::Value> {
        self.resolve_fresh_realm_invocation_response_with_optional_lease_and_trace(
            runtime, response, None,
        )
        .await
    }

    pub(crate) async fn resolve_fresh_realm_invocation_response_with_lease_and_trace(
        &self,
        runtime: &mut JsRuntime,
        response: FreshRealmInvocationResponse<'_>,
        lease: &mut RuntimeRealmLease,
    ) -> Result<serde_json::Value> {
        self.resolve_fresh_realm_invocation_response_with_optional_lease_and_trace(
            runtime,
            response,
            Some(lease),
        )
        .await
    }

    async fn resolve_fresh_realm_invocation_response_with_optional_lease_and_trace(
        &self,
        runtime: &mut JsRuntime,
        response: FreshRealmInvocationResponse<'_>,
        lease: Option<&mut RuntimeRealmLease>,
    ) -> Result<serde_json::Value> {
        let FreshRealmInvocationResponse {
            realm,
            value,
            trace:
                FreshRealmInvocationTrace {
                    bundle,
                    request,
                    construction_mode,
                    context,
                },
        } = response;
        let mut lease = lease;
        if let Some(lease) = &mut lease {
            lease.mark_draining().map_err(realm_lease_error)?;
        }
        let resolve = runtime.resolve_in_realm(realm, value);
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:with_event_loop_promise:start",
        );
        let promise_started_at = Instant::now();
        let value = runtime
            .with_event_loop_promise_in_realm(realm, resolve, PollEventLoopOptions::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_recycled_context:with_event_loop_promise:error",
                    &error,
                );
                runtime_js_error(error)
            });
        self.policy
            .metrics()
            .record_fresh_realm_promise_resolve(promise_started_at.elapsed());
        value.and_then(|value| {
            trace_snapshot_seeded_runtime_phase(
                construction_mode,
                bundle,
                context,
                Some(request),
                "invoke_recycled_context:with_event_loop_promise:complete",
            );
            let deserialize_started_at = Instant::now();
            let result = deserialize_json_value(runtime, value);
            self.policy
                .metrics()
                .record_fresh_realm_deserialization(deserialize_started_at.elapsed());
            result
                .inspect(|_| {
                    trace_snapshot_seeded_runtime_phase(
                        construction_mode,
                        bundle,
                        context,
                        Some(request),
                        "invoke_recycled_context:response_ready",
                    );
                })
                .inspect_err(|error| {
                    trace_snapshot_seeded_runtime_error(
                        construction_mode,
                        bundle,
                        context,
                        Some(request),
                        "invoke_recycled_context:deserialize:error",
                        error,
                    );
                })
        })
    }

    pub(crate) async fn drain_wait_until_with_trace(
        &self,
        runtime: &mut JsRuntime,
        realm: Option<&JsRealm>,
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
        let value = match realm {
            Some(realm) => realm.execute_script(
                runtime.v8_isolate(),
                "<nimbus-runtime:wait-until>",
                "globalThis.__nimbusDrainWaitUntil()",
            ),
            None => runtime.execute_script(
                "<nimbus-runtime:wait-until>",
                "globalThis.__nimbusDrainWaitUntil()",
            ),
        }
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
        let drain = match realm {
            Some(realm) => {
                let resolve = runtime.resolve_in_realm(realm, value);
                runtime
                    .with_event_loop_promise_in_realm(
                        realm,
                        resolve,
                        PollEventLoopOptions::default(),
                    )
                    .await
            }
            None => {
                let resolve = runtime.resolve(value);
                runtime
                    .with_event_loop_promise(resolve, PollEventLoopOptions::default())
                    .await
            }
        }
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

    async fn start_existing_fresh_realm_bundle_invocation_with_trace(
        &self,
        runtime: &mut JsRuntime,
        realm: &JsRealm,
        trace: FreshRealmInvocationTrace<'_>,
        lease: Option<&mut RuntimeRealmLease>,
    ) -> Result<v8::Global<v8::Value>> {
        let FreshRealmInvocationTrace {
            bundle,
            request,
            construction_mode,
            context,
        } = trace;
        let mut lease = lease;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:init_extension_js_in_realm:start",
        );
        runtime.init_extension_js_in_realm(realm).map_err(|error| {
            trace_snapshot_seeded_runtime_error(
                construction_mode,
                bundle,
                context,
                Some(request),
                "invoke_recycled_context:init_extension_js_in_realm:error",
                &error,
            );
            runtime_js_error(error)
        })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:init_extension_js_in_realm:complete",
        );

        let bootstrap_install_started_at = Instant::now();
        install_bootstrap_in_realm(runtime, realm)?;
        self.policy
            .metrics()
            .record_fresh_realm_bootstrap_install(bootstrap_install_started_at.elapsed());
        let bootstrap_finalize_started_at = Instant::now();
        finalize_bootstrap_in_realm(runtime, realm)?;
        self.policy
            .metrics()
            .record_fresh_realm_bootstrap_finalize(bootstrap_finalize_started_at.elapsed());
        let bootstrap_reset_started_at = Instant::now();
        reset_bootstrap_invocation_state_in_realm(runtime, realm)?;
        self.policy
            .metrics()
            .record_fresh_realm_bootstrap_reset(bootstrap_reset_started_at.elapsed());
        if let Some(lease) = &mut lease {
            lease.mark_realm_ready().map_err(realm_lease_error)?;
        }

        let started_at = Instant::now();
        let module_specifier = bundle.module_specifier()?;
        let module_load_started_at = Instant::now();
        let (load_start_phase, load_error_phase, load_complete_phase) =
            match bundle.entrypoint_kind() {
                RuntimeBundleEntrypointKind::Main => (
                    "invoke_recycled_context:load_main_es_module_in_realm:start",
                    "invoke_recycled_context:load_main_es_module_in_realm:error",
                    "invoke_recycled_context:load_main_es_module_in_realm:complete",
                ),
                RuntimeBundleEntrypointKind::Side => (
                    "invoke_recycled_context:load_side_es_module_in_realm:start",
                    "invoke_recycled_context:load_side_es_module_in_realm:error",
                    "invoke_recycled_context:load_side_es_module_in_realm:complete",
                ),
            };
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            load_start_phase,
        );
        if let Some(script) = self.guest_import_phase_script(bundle) {
            realm
                .execute_script(
                    runtime.v8_isolate(),
                    "<nimbus-runtime:guest-semantics:import-phase>",
                    script,
                )
                .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
        }
        let module_id = match bundle.entrypoint_kind() {
            RuntimeBundleEntrypointKind::Main => {
                runtime
                    .load_main_es_module_in_realm(realm, &module_specifier)
                    .await
            }
            RuntimeBundleEntrypointKind::Side => {
                runtime
                    .load_side_es_module_in_realm(realm, &module_specifier)
                    .await
            }
        }
        .map_err(|error| {
            trace_snapshot_seeded_runtime_error(
                construction_mode,
                bundle,
                context,
                Some(request),
                load_error_phase,
                &error,
            );
            runtime_js_error(error)
        })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            load_complete_phase,
        );
        self.policy
            .metrics()
            .record_bundle_module_load(module_load_started_at.elapsed());

        let evaluation = runtime.mod_evaluate_in_realm(realm, module_id);
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:mod_evaluate_in_realm:scheduled",
        );
        let evaluation_started_at = Instant::now();
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:run_event_loop:start",
        );
        runtime
            .run_event_loop_in_realm(realm, Default::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_recycled_context:run_event_loop:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:run_event_loop:complete",
        );
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:evaluation_await:start",
        );
        evaluation.await.map_err(|error| {
            trace_snapshot_seeded_runtime_error(
                construction_mode,
                bundle,
                context,
                Some(request),
                "invoke_recycled_context:evaluation_await:error",
                &error,
            );
            runtime_js_error(error)
        })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:evaluation_await:complete",
        );
        tokio::task::yield_now().await;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:post_evaluation_run_event_loop:start",
        );
        runtime
            .run_event_loop_in_realm(realm, Default::default())
            .await
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_recycled_context:post_evaluation_run_event_loop:error",
                    &error,
                );
                runtime_js_error(error)
            })?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:post_evaluation_run_event_loop:complete",
        );
        self.policy
            .metrics()
            .record_bundle_evaluation(evaluation_started_at.elapsed());
        self.policy
            .metrics()
            .record_bundle_load(started_at.elapsed());
        if let Some(lease) = &mut lease {
            lease.mark_bundle_loaded().map_err(realm_lease_error)?;
        }

        let module_specifier = if matches!(request.kind, InvocationKind::CloudflareWorkerFetch) {
            Some(bundle.module_specifier()?.to_string())
        } else {
            None
        };
        let expression = request.runtime_invoke_expression(
            module_specifier.as_deref(),
            self.policy.limits().guest_semantics,
        )?;
        trace_snapshot_seeded_runtime_phase(
            construction_mode,
            bundle,
            context,
            Some(request),
            "invoke_recycled_context:execute_script:start",
        );
        if let Some(lease) = &mut lease {
            lease.mark_invoking().map_err(realm_lease_error)?;
        }
        let invocation_script_started_at = Instant::now();
        realm
            .execute_script(
                runtime.v8_isolate(),
                "<nimbus-runtime:invoke-recycled-context>",
                expression,
            )
            .map_err(|error| {
                trace_snapshot_seeded_runtime_error(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_recycled_context:execute_script:error",
                    &error,
                );
                runtime_js_error(error)
            })
            .inspect(|_| {
                self.policy
                    .metrics()
                    .record_fresh_realm_invocation_script(invocation_script_started_at.elapsed());
                trace_snapshot_seeded_runtime_phase(
                    construction_mode,
                    bundle,
                    context,
                    Some(request),
                    "invoke_recycled_context:execute_script:complete",
                );
            })
    }

    fn fresh_realm_module_loader(
        &self,
        bundle: &RuntimeBundle,
        construction_mode: V8RuntimeConstructionMode,
    ) -> Result<Rc<dyn ModuleLoader>> {
        let limits = self.policy.limits();
        let path_policy = RuntimePathPolicy::for_bundle(bundle, limits)?;
        Ok(Rc::new(RestrictedModuleLoader::new(
            path_policy,
            limits.compatibility_target,
            limits.guest_semantics,
            limits.node_conditions.clone(),
            bundle.module_code_cache(limits, construction_mode),
            None,
        )))
    }

    pub(crate) fn return_clean_fresh_realm_lease(
        &self,
        runtime: &mut JsRuntime,
        lease: &mut RuntimeRealmLease,
    ) -> Result<()> {
        if let Some((baseline, current)) = runtime_resource_table_delta(runtime) {
            condemn_fresh_realm_lease(lease, RuntimeRealmLeaseCondemnationReason::Dirty);
            return Err(NimbusRuntimeError::Contract(format!(
                "NodeFull realm lease returned with changed Deno resource table entries; substrate condemned; {}",
                format_resource_table_delta(&baseline, &current)
            )));
        }
        let observed_contract = lease.contract().clone();
        lease
            .return_clean(&observed_contract)
            .map(|_| ())
            .map_err(realm_lease_error)
    }

    #[cfg(test)]
    pub(crate) fn condemn_dirty_fresh_realm_lease(&self, lease: &mut RuntimeRealmLease) {
        condemn_fresh_realm_lease(lease, RuntimeRealmLeaseCondemnationReason::Dirty);
    }

    pub(crate) fn condemn_fresh_realm_lease_with_reason(
        &self,
        lease: &mut RuntimeRealmLease,
        reason: RuntimeRealmLeaseCondemnationReason,
    ) {
        condemn_fresh_realm_lease(lease, reason);
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
        let expression = request.runtime_invoke_expression(
            module_specifier.as_deref(),
            self.policy.limits().guest_semantics,
        )?;
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
            .execute_script("<nimbus-runtime:invoke>", expression)
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

fn realm_lease_error(error: impl std::fmt::Display) -> NimbusRuntimeError {
    NimbusRuntimeError::Contract(format!("NodeFull realm lease contract failed: {error}"))
}

fn format_resource_table_delta(
    baseline: &super::super::bootstrap::RuntimeResourceTableSnapshot,
    current: &super::super::bootstrap::RuntimeResourceTableSnapshot,
) -> String {
    let added = current
        .entries()
        .iter()
        .filter(|(rid, name)| baseline.entries().get(rid) != Some(name))
        .map(|(rid, name)| format!("{rid}:{name}"))
        .collect::<Vec<_>>();
    let removed = baseline
        .entries()
        .iter()
        .filter(|(rid, name)| current.entries().get(rid) != Some(name))
        .map(|(rid, name)| format!("{rid}:{name}"))
        .collect::<Vec<_>>();
    format!(
        "added=[{}], removed=[{}]",
        added.join(", "),
        removed.join(", ")
    )
}

fn condemn_fresh_realm_lease(
    lease: &mut RuntimeRealmLease,
    reason: RuntimeRealmLeaseCondemnationReason,
) {
    let observed_contract = lease.contract().clone();
    let _ = lease.condemn(&observed_contract, reason);
}
