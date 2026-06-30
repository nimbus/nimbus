use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxError, SandboxHandle};
use nimbus_workloads::InMemoryDesiredWorkloadStore;

use crate::{SandboxResource, ServiceDefinition, SessionResource};

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

#[derive(Default)]
pub(super) struct ServiceManagerState {
    pub(super) handles: BTreeMap<TenantServiceKey, SandboxHandle>,
    pub(super) definitions: BTreeMap<TenantServiceKey, ServiceDefinition>,
    pub(super) sandbox_resources: BTreeMap<String, SandboxResource>,
    pub(super) sessions: BTreeMap<String, SessionResource>,
    pub(super) session_channels: BTreeMap<SessionChannelKey, SessionChannelState>,
    pub(super) desired_workloads: InMemoryDesiredWorkloadStore,
    pub(super) activations_in_progress: BTreeSet<TenantServiceKey>,
    pub(super) next_definition_version: u64,
    pub(super) next_sandbox_resource_version: u64,
    pub(super) next_session_version: u64,
}

pub(super) enum ActivationClaim {
    Claimed,
    AlreadyActive,
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
