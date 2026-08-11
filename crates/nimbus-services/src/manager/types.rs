use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxError;

use crate::{
    SandboxResourceObservation, SandboxResourceSource, ServiceDefinition,
    ServiceDefinitionObservation, SessionResource, WorkloadSourceRetirementClaim,
};

use super::session_channels::{SessionChannelKey, SessionChannelState};

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
    /// Dynamic-definition mutations currently spanning an async retirement.
    ///
    /// This gate cannot authorize provision or provider start. It only keeps
    /// update/delete/session snapshots from crossing an in-flight definition
    /// deletion while that deletion retires an already-observed sandbox.
    pub(super) definition_mutations_in_progress: BTreeSet<TenantServiceKey>,
    pub(super) next_definition_version: u64,
    pub(super) next_session_version: u64,
}

pub(super) fn sandbox_backend_error(
    key: &TenantServiceKey,
    operation: &str,
    error: &SandboxError,
) -> Error {
    Error::Internal(format!(
        "failed to {operation} sandbox-backed service {} for tenant {}: {error}",
        key.service_name, key.tenant_id
    ))
}
