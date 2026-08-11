use std::collections::BTreeMap;

use nimbus_core::{Error, TenantId, WorkloadId};
use nimbus_sandbox::{SandboxSpec, validate_sandbox_mounts};
use nimbus_tenant::{
    TenantIsolationDecision, TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision,
    TenantVolumePolicyDecision, WorkloadAttributes, WorkloadKind,
};

use crate::{SandboxResourceSource, ServiceBackend, ServiceDefinition, ServiceDefinitionSource};

use super::ServiceManager;
use super::clock::now_millis;
use super::sandboxes::{same_sandbox_resource_desire, validate_sandbox_resource_spec};
use super::types::{TenantSandboxResourceKey, TenantServiceKey, WorkloadSourceRetirementKey};

/// Exact desired standalone source plus the policy input that must admit it.
///
/// Fields are private so an admitted preparation cannot be changed before the
/// source owner reserves it. Callers may inspect cloned source bytes and clone
/// the policy input, but reservation consumes this exact preparation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandaloneSandboxProvisionSource {
    source: SandboxResourceSource,
    policy_input: TenantIsolationPolicyInput,
}

impl StandaloneSandboxProvisionSource {
    pub fn source(&self) -> &SandboxResourceSource {
        &self.source
    }

    pub fn policy_input(&self) -> &TenantIsolationPolicyInput {
        &self.policy_input
    }
}

/// Exact sandbox-backed service source plus its complete admission facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxServiceProvisionSource {
    definition: ServiceDefinition,
    volume_policy: TenantVolumePolicyDecision,
    policy_input: TenantIsolationPolicyInput,
}

impl SandboxServiceProvisionSource {
    pub fn definition(&self) -> &ServiceDefinition {
        &self.definition
    }

    pub fn sandbox_spec(&self) -> &SandboxSpec {
        self.definition
            .backend
            .sandbox_spec()
            .expect("sandbox service preparation must retain a sandbox backend")
    }

    pub fn volume_policy(&self) -> &TenantVolumePolicyDecision {
        &self.volume_policy
    }

    pub fn policy_input(&self) -> &TenantIsolationPolicyInput {
        &self.policy_input
    }
}

impl ServiceManager {
    /// Prepare one exact standalone desired source without persisting it.
    ///
    /// The caller supplies stable identity, while services owns source
    /// generation. A new source starts at generation one and exact replay
    /// adopts the retained generation and resource version.
    pub fn prepare_standalone_sandbox_provision_source(
        &self,
        tenant_id: &TenantId,
        stable_resource_id: impl Into<String>,
        profile: impl Into<String>,
        spec: SandboxSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<StandaloneSandboxProvisionSource, Error> {
        let stable_resource_id = stable_resource_id.into();
        let profile = profile.into();
        WorkloadId::new(stable_resource_id.clone())?;
        validate_sandbox_resource_spec(tenant_id, &spec)?;
        let source = {
            let state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            let key = TenantSandboxResourceKey::new(tenant_id, &stable_resource_id);
            match state.sandbox_resource_sources.get(&key) {
                Some(existing) => {
                    let candidate = SandboxResourceSource::new(
                        tenant_id.clone(),
                        stable_resource_id.clone(),
                        profile.clone(),
                        spec.clone(),
                        existing.generation,
                        now_millis(),
                        labels.clone(),
                    );
                    if same_sandbox_resource_desire(existing, &candidate) {
                        existing.clone()
                    } else {
                        return Err(Error::conflict(format!(
                            "sandbox resource `{stable_resource_id}` already has different desired source"
                        )));
                    }
                }
                None => SandboxResourceSource::new(
                    tenant_id.clone(),
                    stable_resource_id.clone(),
                    profile.clone(),
                    spec.clone(),
                    1,
                    now_millis(),
                    labels,
                ),
            }
        };
        let policy_input = TenantIsolationPolicyInput::new(
            WorkloadAttributes::sandbox(profile)
                .with_sandbox_backend(spec.backend)
                .with_sandbox_id(stable_resource_id),
        )
        .with_image(self.manager_image_policy());
        Ok(StandaloneSandboxProvisionSource {
            source,
            policy_input,
        })
    }

    /// Reserve the exact desired bytes admitted by compute.
    ///
    /// Exact replay adopts the retained source. Any crossed decision, source
    /// content, or generation rejects before state mutation or provider work.
    pub fn reserve_standalone_sandbox_provision_source(
        &self,
        decision: &TenantIsolationDecision,
        prepared: StandaloneSandboxProvisionSource,
    ) -> Result<SandboxResourceSource, Error> {
        self.validate_standalone_sandbox_provision_decision(decision, &prepared)?;
        let source = prepared.source;

        let key = TenantSandboxResourceKey::new(&source.tenant_id, &source.id);
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        if Self::source_retirement_claim_exists(
            &state,
            &WorkloadSourceRetirementKey::Sandbox(key.clone()),
        ) {
            return Err(Error::conflict(format!(
                "sandbox resource `{}` has a retirement claim in progress",
                source.id
            )));
        }
        if let Some(existing) = state.sandbox_resource_sources.get(&key) {
            if same_sandbox_resource_desire(existing, &source) {
                return Ok(existing.clone());
            }
            return Err(Error::conflict(format!(
                "sandbox resource `{}` already has different desired source",
                source.id
            )));
        }
        state.sandbox_resource_sources.insert(key, source.clone());
        Ok(source)
    }

    /// Validate one admitted decision against an exact prepared standalone
    /// source before provider-plan composition.
    ///
    /// Reservation repeats this pure check at its linearization boundary so
    /// preflight cannot substitute for source-owner authority.
    pub fn validate_standalone_sandbox_provision_decision(
        &self,
        decision: &TenantIsolationDecision,
        prepared: &StandaloneSandboxProvisionSource,
    ) -> Result<(), Error> {
        let source = prepared.source();
        validate_standalone_decision(decision, source, self.sandbox_backend.kind())?;
        self.admit_sandbox_root(decision, &source.spec)
    }

    /// Resolve a complete sandbox-backed service source without admitting or
    /// mutating it. Built-in and external definitions reject here.
    pub fn prepare_sandbox_service_provision_source(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<SandboxServiceProvisionSource, Error> {
        let definition = self
            .service_definition_for_tenant(tenant_id, service_name)
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "service `{service_name}` for tenant `{tenant_id}` was not found"
                ))
            })?;
        if definition.tenant_id != *tenant_id || definition.name != service_name {
            return Err(Error::InvalidInput(format!(
                "service source for `{service_name}` in tenant `{tenant_id}` returned crossed identity"
            )));
        }
        let ServiceBackend::Sandbox(spec) = &definition.backend else {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` is declared but is not sandbox-backed"
            )));
        };
        let volume_policy = match definition.source {
            ServiceDefinitionSource::Dynamic => TenantVolumePolicyDecision::default(),
            ServiceDefinitionSource::StaticCatalog => self
                .service_definitions
                .service_volume_policy_for_tenant(tenant_id, service_name),
        };
        validate_sandbox_mounts(&spec.mounts).map_err(|message| {
            Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` has invalid sandbox mounts: {message}"
            ))
        })?;
        let policy_input = TenantIsolationPolicyInput::new(
            WorkloadAttributes::service(service_name).with_sandbox_backend(spec.backend),
        )
        .with_services(TenantServiceGrantPolicyDecision::new([service_name]))
        .with_volumes(volume_policy.clone())
        .with_image(self.manager_image_policy());
        Ok(SandboxServiceProvisionSource {
            definition,
            volume_policy,
            policy_input,
        })
    }

    /// Validate one admitted decision against an exact prepared service source.
    pub fn validate_sandbox_service_provision_decision(
        &self,
        decision: &TenantIsolationDecision,
        prepared: &SandboxServiceProvisionSource,
    ) -> Result<(), Error> {
        let definition = prepared.definition();
        let spec = prepared.sandbox_spec();
        validate_exact_decision_workload(
            decision,
            WorkloadKind::Service,
            &definition.name,
            None,
            spec,
        )?;
        decision
            .service_access(&definition.name, "sandbox-backed service provision")?
            .ensure_sandbox_spec_matches(spec, self.sandbox_backend.kind())?;
        decision
            .network()
            .ensure_sandbox_egress_matches(spec, "sandbox-backed service provision")?;
        prepared
            .volume_policy
            .ensure_sandbox_mounts_match(spec, "sandbox-backed service provision")?;
        self.admit_sandbox_root(decision, spec)
    }

    /// Revalidate and fence an exact service source at provision insertion.
    /// This callback performs no source mutation; it shares the manager lock
    /// with retirement claims so start and stop have one local linearization.
    pub fn reserve_sandbox_service_provision_source(
        &self,
        decision: &TenantIsolationDecision,
        prepared: SandboxServiceProvisionSource,
    ) -> Result<(), Error> {
        self.validate_sandbox_service_provision_decision(decision, &prepared)?;
        let definition = prepared.definition;
        let catalog_definition = self
            .service_definitions
            .service_definition_for_tenant(&definition.tenant_id, &definition.name);
        let key = TenantServiceKey::new(&definition.tenant_id, &definition.name);
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        if Self::source_retirement_claim_exists(
            &state,
            &WorkloadSourceRetirementKey::Service(key.clone()),
        ) {
            return Err(Error::conflict(format!(
                "service `{}` for tenant `{}` has a retirement claim in progress",
                definition.name, definition.tenant_id
            )));
        }
        let current = state
            .definitions
            .get(&key)
            .or(catalog_definition.as_ref())
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "service `{}` for tenant `{}` was not found",
                    definition.name, definition.tenant_id
                ))
            })?;
        if current != &definition {
            return Err(Error::PreconditionFailed(format!(
                "service `{}` changed before provision insertion",
                definition.name
            )));
        }
        Ok(())
    }
}

fn validate_standalone_decision(
    decision: &TenantIsolationDecision,
    source: &SandboxResourceSource,
    actual_backend: nimbus_sandbox::SandboxBackendKind,
) -> Result<(), Error> {
    validate_exact_decision_workload(
        decision,
        WorkloadKind::Sandbox,
        &source.profile,
        Some(&source.id),
        &source.spec,
    )?;
    decision.ensure_sandbox_spec_matches(
        &source.spec,
        actual_backend,
        "standalone sandbox desired-source reservation",
    )?;
    decision.network().ensure_sandbox_egress_matches(
        &source.spec,
        "standalone sandbox desired-source reservation",
    )?;
    decision.volumes().ensure_sandbox_mounts_match(
        &source.spec,
        "standalone sandbox desired-source reservation",
    )
}

fn validate_exact_decision_workload(
    decision: &TenantIsolationDecision,
    expected_kind: WorkloadKind,
    expected_name: &str,
    expected_sandbox_id: Option<&str>,
    spec: &SandboxSpec,
) -> Result<(), Error> {
    let workload = decision.workload();
    let identity = decision.workload_identity();
    if decision.tenant_id() != &spec.tenant_id
        || workload.kind() != expected_kind
        || workload.name() != expected_name
        || workload.sandbox_backend() != Some(spec.backend)
        || workload.sandbox_id() != expected_sandbox_id
        || identity.deployment_generation().is_none()
    {
        return Err(Error::InvalidInput(format!(
            "tenant isolation decision {} is crossed with desired workload `{expected_name}` or lacks its compute-owned lifecycle generation",
            decision.id().as_str()
        )));
    }
    Ok(())
}
