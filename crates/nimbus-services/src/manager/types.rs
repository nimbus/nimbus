use std::collections::BTreeMap;

use nimbus_core::TenantId;

use crate::{
    SandboxResourceObservation, SandboxResourceSource, ServiceDefinition,
    ServiceDefinitionObservation, SessionResource, WorkloadSourceRetirementClaim,
};

use super::session_channels::{SessionChannelKey, SessionChannelState};
use super::tenant_retirement::TenantSourceRetirementBarrier;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TenantServiceKey {
    pub(super) tenant_id: TenantId,
    pub(super) service_name: String,
}

impl TenantServiceKey {
    pub(super) fn new(tenant_id: &TenantId, service_name: &str) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            service_name: service_name.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TenantSandboxResourceKey {
    pub(super) tenant_id: TenantId,
    pub(super) resource_id: String,
}

impl TenantSandboxResourceKey {
    pub(super) fn new(tenant_id: &TenantId, resource_id: &str) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            resource_id: resource_id.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum WorkloadSourceRetirementKey {
    Service(TenantServiceKey),
    Sandbox(TenantSandboxResourceKey),
}

#[derive(Default)]
pub(super) struct ServiceManagerState {
    pub(super) service_definition_observations:
        BTreeMap<TenantServiceKey, ServiceDefinitionObservation>,
    pub(super) definitions: BTreeMap<TenantServiceKey, ServiceDefinition>,
    pub(super) sandbox_resource_sources: BTreeMap<TenantSandboxResourceKey, SandboxResourceSource>,
    pub(super) sandbox_resource_observations:
        BTreeMap<TenantSandboxResourceKey, SandboxResourceObservation>,
    pub(super) sessions: BTreeMap<String, SessionResource>,
    pub(super) session_channels: BTreeMap<SessionChannelKey, SessionChannelState>,
    /// Process-local services policy claims. Durable lifecycle authority stays
    /// in the workload saga store; these claims only fence source mutation,
    /// provision insertion, session admission, and terminal projection.
    pub(super) source_retirement_claims:
        BTreeMap<WorkloadSourceRetirementKey, WorkloadSourceRetirementClaim>,
    /// Process-local tenant source fence paired with the immutable source
    /// snapshot captured when the Engine incarnation entered retirement.
    pub(super) tenant_source_retirements: BTreeMap<TenantId, TenantSourceRetirementBarrier>,
    pub(super) next_definition_version: u64,
    pub(super) next_session_version: u64,
}
