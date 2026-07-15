use std::sync::Arc;

use nimbus_bridge::admission::RuntimeExecutionAdmission;
use nimbus_bridge::{RuntimeHostInvocation, RuntimeHostScope};
use nimbus_core::{Error, Result, StorageErrorKind, TenantId, TriggerInvocationRecord};
use nimbus_engine::{Engine, TriggerInvocationExecution, TriggerInvocationExecutor};
use nimbus_runtime::{InvocationKind, InvocationRequest};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
    admit_runtime_invocation_decision,
};

use crate::retry::execute_mutation_with_occ_retries;
use crate::{
    CloudFunctionsHostBridge, CloudFunctionsRegistry, CloudFunctionsRuntimeInvocation,
    CloudFunctionsRuntimeInvoker,
};

pub struct CloudFunctionsTriggerExecutor {
    engine: Arc<Engine>,
    registry: Arc<CloudFunctionsRegistry>,
    deployment_generation: u64,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    tenant_isolation_mode: TenantIsolationMode,
    runtime_invoker: Arc<dyn CloudFunctionsRuntimeInvoker>,
}

impl CloudFunctionsTriggerExecutor {
    pub fn new(
        engine: Arc<Engine>,
        registry: Arc<CloudFunctionsRegistry>,
        deployment_generation: u64,
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
        tenant_isolation_mode: TenantIsolationMode,
        runtime_invoker: Arc<dyn CloudFunctionsRuntimeInvoker>,
    ) -> Self {
        Self {
            engine,
            registry,
            deployment_generation,
            runtime_service_registry,
            tenant_isolation_mode,
            runtime_invoker,
        }
    }
}

impl TriggerInvocationExecutor for CloudFunctionsTriggerExecutor {
    fn execute_invocation(
        &self,
        tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> TriggerInvocationExecution {
        match self.execute_invocation_once(tenant_id, record) {
            Ok(()) => TriggerInvocationExecution::completed(),
            Err(error) => classify_cloud_functions_trigger_error(error),
        }
    }
}

impl CloudFunctionsTriggerExecutor {
    fn execute_invocation_once(
        &self,
        tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> Result<()> {
        let target = self
            .registry
            .required_firestore_trigger_target(&record.key.registration_id)?;
        let args = serde_json::to_value(&record.event)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let server_request_id = format!(
            "cloud-functions-trigger-{}-{}",
            record.key.registration_id, record.key.event_id
        );
        let isolation =
            TenantIsolationContext::system(tenant_id.clone(), "cloud_functions.trigger_runtime")
                .with_deployment_generation(self.deployment_generation);
        isolation.ensure_deployment_generation_matches(
            self.deployment_generation,
            "cloud functions trigger runtime deployment",
        )?;
        let bundle = self.registry.runtime_bundle();
        isolation
            .ensure_runtime_bundle_matches(&bundle, "cloud functions trigger runtime bundle")?;
        let services = self
            .runtime_service_registry
            .snapshot_for_tenant(isolation.tenant_id());
        let runtime_policy = self.registry.runtime_policy();
        let decision = admit_runtime_invocation_decision(
            &isolation,
            &target.entrypoint,
            Some(server_request_id.as_str()),
            &runtime_policy,
            RuntimeIsolationTier::InProcessUntrusted,
            self.tenant_isolation_mode,
            services.keys().cloned(),
        )?;
        decision
            .ensure_runtime_bundle_matches(&bundle, "cloud functions trigger runtime bundle")?;
        RuntimeExecutionAdmission::for_decision(&decision)
            .ensure_in_process_available("cloud functions trigger runtime invocation")?;
        let request = InvocationRequest {
            kind: InvocationKind::Mutation,
            function_name: target.entrypoint.clone(),
            args,
            page_size: None,
            cursor: None,
            auth: None,
            services: services.clone(),
        };
        execute_mutation_with_occ_retries(self.engine.as_ref(), tenant_id, || {
            let bridge = Arc::new(CloudFunctionsHostBridge::build(
                RuntimeHostScope::new(
                    self.engine.clone(),
                    self.registry.runtime_policy(),
                    decision.clone(),
                ),
                RuntimeHostInvocation::new(
                    record.event.execution.principal().clone(),
                    Some(server_request_id.clone()),
                    InvocationKind::Mutation,
                    target.entrypoint.clone(),
                )
                .with_trigger_write_origin(nimbus_core::TriggerWriteOrigin::new(
                    record.key.clone(),
                    record.depth(),
                )),
            )?);

            self.runtime_invoker
                .invoke_runtime_bundle(CloudFunctionsRuntimeInvocation {
                    runtime_executor: self.registry.runtime_executor(),
                    runtime_policy: self.registry.runtime_policy(),
                    host_bridge: bridge.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    tenant_id: decision.tenant_id().clone(),
                    server_request_id: Some(server_request_id.clone()),
                    provenance_gate: self.registry.runtime_bundle_provenance().cloned(),
                })?;
            bridge.commit_mutation_execution_unit()?;
            Ok(())
        })
    }
}

fn classify_cloud_functions_trigger_error(error: Error) -> TriggerInvocationExecution {
    let message = error.to_string();
    match error {
        Error::Cancelled | Error::ResourceExhausted(_) => {
            TriggerInvocationExecution::retryable(message)
        }
        Error::Storage {
            kind:
                StorageErrorKind::Busy
                | StorageErrorKind::Io
                | StorageErrorKind::Transient
                | StorageErrorKind::Unavailable,
            ..
        } => TriggerInvocationExecution::retryable(message),
        _ => TriggerInvocationExecution::terminal(message),
    }
}
