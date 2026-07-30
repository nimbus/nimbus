//! Concrete durable authority for portable network attachment lifecycle.
//!
//! This module binds the generic resource state machine to one tenant-qualified
//! attachment collection in the node-local network store. Provider inspection
//! and effects remain in upper adapters; this authority owns only durable
//! identity, fencing, phase, selected provider identity, and opaque handle
//! adoption.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use nimbus_core::TenantId;
use serde::{Deserialize, Serialize};

use crate::{
    DurableNetworkResourceState, LocalNetworkStateStore, NetworkAttachmentId,
    NetworkAttachmentSegmentAssociation, NetworkPlan, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceId, NetworkResourceVersion, NetworkStateError, NetworkStateMutation,
    NetworkStatePartition, NetworkStateStoreError, NetworkStateTransactionError,
    NetworkStateTransition,
};

/// Tenant-qualified durable attachment lifecycle record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableNetworkAttachmentState {
    tenant_id: TenantId,
    selected_provider_id: NetworkProviderId,
    association: NetworkAttachmentSegmentAssociation,
    resource: DurableNetworkResourceState,
}

impl DurableNetworkAttachmentState {
    /// Tenant whose connectivity authority contains this attachment.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Provider registration selected before effects.
    pub fn selected_provider_id(&self) -> &NetworkProviderId {
        &self.selected_provider_id
    }

    /// Exact allocator reservation, selected segment, and fencing epoch.
    pub fn association(&self) -> &NetworkAttachmentSegmentAssociation {
        &self.association
    }

    /// Portable resource version, phase, and opaque provider handle.
    pub fn resource(&self) -> &DurableNetworkResourceState {
        &self.resource
    }

    fn attachment_id(&self) -> Result<&NetworkAttachmentId, NetworkAttachmentStateError> {
        match self.resource.version().resource_id() {
            NetworkResourceId::Attachment(attachment_id) => Ok(attachment_id),
            resource_id => Err(NetworkAttachmentStateError::CorruptAuthority {
                reason: format!(
                    "attachment authority contains non-attachment resource {resource_id:?}"
                ),
            }),
        }
    }

    fn validate(&self) -> Result<(), NetworkAttachmentStateError> {
        let attachment_id = self.attachment_id()?;
        if self.association.lease_epoch() != self.resource.version().lease_epoch() {
            return Err(NetworkAttachmentStateError::CorruptAuthority {
                reason: format!(
                    "attachment {} association epoch {} does not match resource epoch {}",
                    attachment_id,
                    self.association.lease_epoch().as_u64(),
                    self.resource.version().lease_epoch().as_u64()
                ),
            });
        }
        if let Some(handle) = self.resource.provider_handle()
            && handle.provider_id() != &self.selected_provider_id
        {
            return Err(NetworkAttachmentStateError::CorruptAuthority {
                reason: format!(
                    "attachment {} selected provider {} but its durable handle belongs to {}",
                    attachment_id,
                    self.selected_provider_id,
                    handle.provider_id()
                ),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkAttachmentState {
    records: BTreeMap<String, DurableNetworkAttachmentState>,
}

impl NetworkAttachmentState {
    fn validate(&self) -> Result<(), NetworkAttachmentStateError> {
        for (key, record) in &self.records {
            record.validate()?;
            let expected = attachment_key(record.tenant_id(), record.attachment_id()?);
            if key != &expected {
                return Err(NetworkAttachmentStateError::CorruptAuthority {
                    reason: format!(
                        "attachment map key {key:?} does not match tenant {} and resource {}",
                        record.tenant_id(),
                        record.attachment_id()?
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Node-local attachment lifecycle authority backed by the one network store.
#[derive(Clone, Debug)]
pub struct LocalNetworkAttachmentAuthority {
    store: LocalNetworkStateStore,
}

impl LocalNetworkAttachmentAuthority {
    pub(crate) fn from_store(
        store: LocalNetworkStateStore,
    ) -> Result<Self, NetworkAttachmentStateError> {
        let authority = Self { store };
        authority.load_state()?;
        Ok(authority)
    }

    /// Open the shared node-local attachment authority.
    pub fn open(state_root: impl AsRef<Path>) -> Result<Self, NetworkAttachmentStateError> {
        Self::from_store(
            LocalNetworkStateStore::open(state_root).map_err(NetworkAttachmentStateError::Store)?,
        )
    }

    /// Canonical node state root shared by this authority.
    pub fn state_root(&self) -> &Path {
        self.store.state_root()
    }

    /// Canonical durable authority file used by diagnostics and proof tools.
    pub fn authority_path(&self) -> &Path {
        self.store.authority_path()
    }

    /// Reserve or authenticate one exact desired attachment generation.
    pub fn reserve(
        &self,
        tenant_id: &TenantId,
        selected_provider_id: NetworkProviderId,
        plan: &NetworkPlan,
        attachment_id: NetworkAttachmentId,
        association: NetworkAttachmentSegmentAssociation,
    ) -> Result<DurableNetworkAttachmentState, NetworkAttachmentStateError> {
        let lease_epoch = association.lease_epoch();
        let candidate = DurableNetworkAttachmentState {
            tenant_id: tenant_id.clone(),
            selected_provider_id,
            association,
            resource: DurableNetworkResourceState::reserve(
                plan,
                NetworkResourceId::Attachment(attachment_id.clone()),
                lease_epoch,
            ),
        };
        candidate.validate()?;
        let key = attachment_key(tenant_id, &attachment_id);
        self.transaction(|state| {
            if let Some(existing) = state.records.get(&key) {
                if existing.tenant_id != candidate.tenant_id {
                    return Err(NetworkAttachmentStateError::TenantConflict {
                        attachment_id: attachment_id.clone(),
                        expected: existing.tenant_id.clone(),
                        candidate: candidate.tenant_id.clone(),
                    });
                }
                if existing.selected_provider_id != candidate.selected_provider_id {
                    return Err(NetworkAttachmentStateError::SelectedProviderConflict {
                        attachment_id: attachment_id.clone(),
                        expected: existing.selected_provider_id.clone(),
                        candidate: candidate.selected_provider_id.clone(),
                    });
                }
                if existing.association != candidate.association {
                    return Err(NetworkAttachmentStateError::AssociationConflict {
                        attachment_id: attachment_id.clone(),
                    });
                }
                existing
                    .resource
                    .authenticate_version(candidate.resource.version())
                    .map_err(NetworkAttachmentStateError::State)?;
                return Ok(existing.clone());
            }
            state.records.insert(key, candidate.clone());
            Ok(candidate)
        })
    }

    /// Inspect one tenant-qualified attachment without changing authority.
    pub fn get(
        &self,
        tenant_id: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Option<DurableNetworkAttachmentState>, NetworkAttachmentStateError> {
        let state = self.load_state()?;
        Ok(state
            .records
            .get(&attachment_key(tenant_id, attachment_id))
            .cloned())
    }

    /// List all durable attachment records in deterministic key order.
    pub fn list(&self) -> Result<Vec<DurableNetworkAttachmentState>, NetworkAttachmentStateError> {
        Ok(self.load_state()?.records.into_values().collect())
    }

    /// Apply one exact generation-scoped phase transition.
    pub fn apply_transition(
        &self,
        tenant_id: &TenantId,
        transition: &NetworkStateTransition,
    ) -> Result<(NetworkStateMutation, DurableNetworkAttachmentState), NetworkAttachmentStateError>
    {
        let attachment_id = attachment_id_from_version(transition.version())?.clone();
        let key = attachment_key(tenant_id, &attachment_id);
        self.transaction(|state| {
            let record = state.records.get_mut(&key).ok_or_else(|| {
                NetworkAttachmentStateError::NotFound {
                    tenant_id: tenant_id.clone(),
                    attachment_id: attachment_id.clone(),
                }
            })?;
            if &record.tenant_id != tenant_id {
                return Err(NetworkAttachmentStateError::TenantConflict {
                    attachment_id: attachment_id.clone(),
                    expected: record.tenant_id.clone(),
                    candidate: tenant_id.clone(),
                });
            }
            let mutation = record
                .resource
                .apply_transition(transition)
                .map_err(NetworkAttachmentStateError::State)?;
            Ok((mutation, record.clone()))
        })
    }

    /// Adopt one opaque handle under the exact selected provider and version.
    pub fn record_provider_handle(
        &self,
        tenant_id: &TenantId,
        expected: &NetworkResourceVersion,
        provider_handle: NetworkProviderHandle,
    ) -> Result<(NetworkStateMutation, DurableNetworkAttachmentState), NetworkAttachmentStateError>
    {
        let attachment_id = attachment_id_from_version(expected)?.clone();
        let key = attachment_key(tenant_id, &attachment_id);
        self.transaction(|state| {
            let record = state.records.get_mut(&key).ok_or_else(|| {
                NetworkAttachmentStateError::NotFound {
                    tenant_id: tenant_id.clone(),
                    attachment_id: attachment_id.clone(),
                }
            })?;
            if &record.tenant_id != tenant_id {
                return Err(NetworkAttachmentStateError::TenantConflict {
                    attachment_id: attachment_id.clone(),
                    expected: record.tenant_id.clone(),
                    candidate: tenant_id.clone(),
                });
            }
            if provider_handle.provider_id() != &record.selected_provider_id {
                return Err(NetworkAttachmentStateError::HandleProviderConflict {
                    attachment_id: attachment_id.clone(),
                    selected: record.selected_provider_id.clone(),
                    candidate: provider_handle.provider_id().clone(),
                });
            }
            let mutation = record
                .resource
                .record_provider_handle(expected, provider_handle)
                .map_err(NetworkAttachmentStateError::State)?;
            Ok((mutation, record.clone()))
        })
    }

    fn load_state(&self) -> Result<NetworkAttachmentState, NetworkAttachmentStateError> {
        let state: NetworkAttachmentState = self
            .store
            .read(&NetworkStatePartition::AttachmentStates)
            .map_err(NetworkAttachmentStateError::Store)?
            .unwrap_or_default();
        state.validate()?;
        Ok(state)
    }

    fn transaction<Output>(
        &self,
        operation: impl FnOnce(
            &mut NetworkAttachmentState,
        ) -> Result<Output, NetworkAttachmentStateError>,
    ) -> Result<Output, NetworkAttachmentStateError> {
        self.store
            .transaction(
                &NetworkStatePartition::AttachmentStates,
                |state: &mut NetworkAttachmentState| {
                    state.validate()?;
                    let output = operation(state)?;
                    state.validate()?;
                    Ok(output)
                },
            )
            .map_err(NetworkAttachmentStateError::from_transaction)
    }
}

fn attachment_id_from_version(
    version: &NetworkResourceVersion,
) -> Result<&NetworkAttachmentId, NetworkAttachmentStateError> {
    match version.resource_id() {
        NetworkResourceId::Attachment(attachment_id) => Ok(attachment_id),
        resource_id => Err(NetworkAttachmentStateError::ResourceKindConflict {
            resource_id: resource_id.clone(),
        }),
    }
}

fn attachment_key(tenant_id: &TenantId, attachment_id: &NetworkAttachmentId) -> String {
    format!(
        "{}:{}:{}",
        tenant_id.as_str().len(),
        tenant_id.as_str(),
        attachment_id.as_str()
    )
}

/// Durable attachment authority or lifecycle rejection.
#[derive(Debug)]
pub enum NetworkAttachmentStateError {
    /// The shared store could not be safely read or committed.
    Store(NetworkStateStoreError),
    /// Checksum-valid state violates attachment invariants.
    CorruptAuthority { reason: String },
    /// No durable attachment has this tenant-qualified identity.
    NotFound {
        tenant_id: TenantId,
        attachment_id: NetworkAttachmentId,
    },
    /// A caller addressed a non-attachment resource.
    ResourceKindConflict { resource_id: NetworkResourceId },
    /// A tenant-qualified identity was replayed with another tenant.
    TenantConflict {
        attachment_id: NetworkAttachmentId,
        expected: TenantId,
        candidate: TenantId,
    },
    /// The desired generation was replayed with another selected provider.
    SelectedProviderConflict {
        attachment_id: NetworkAttachmentId,
        expected: NetworkProviderId,
        candidate: NetworkProviderId,
    },
    /// An existing attachment was replayed with another claim, segment, or epoch.
    AssociationConflict { attachment_id: NetworkAttachmentId },
    /// An opaque handle did not belong to the selected provider.
    HandleProviderConflict {
        attachment_id: NetworkAttachmentId,
        selected: NetworkProviderId,
        candidate: NetworkProviderId,
    },
    /// The portable resource state machine rejected identity, fencing, or phase.
    State(NetworkStateError),
}

impl NetworkAttachmentStateError {
    fn from_transaction(error: NetworkStateTransactionError<NetworkAttachmentStateError>) -> Self {
        match error {
            NetworkStateTransactionError::Store(error) => Self::Store(error),
            NetworkStateTransactionError::Operation(error) => error,
        }
    }
}

impl Display for NetworkAttachmentStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "attachment authority store failed: {error}"),
            Self::CorruptAuthority { reason } => {
                write!(formatter, "attachment authority is corrupt: {reason}")
            }
            Self::NotFound {
                tenant_id,
                attachment_id,
            } => write!(
                formatter,
                "attachment {attachment_id} has no durable authority for tenant {tenant_id}"
            ),
            Self::ResourceKindConflict { resource_id } => write!(
                formatter,
                "attachment authority cannot address non-attachment resource {resource_id:?}"
            ),
            Self::TenantConflict {
                attachment_id,
                expected,
                candidate,
            } => write!(
                formatter,
                "attachment {attachment_id} belongs to tenant {expected}, not {candidate}"
            ),
            Self::SelectedProviderConflict {
                attachment_id,
                expected,
                candidate,
            } => write!(
                formatter,
                "attachment {attachment_id} selected provider {expected}, not {candidate}"
            ),
            Self::AssociationConflict { attachment_id, .. } => write!(
                formatter,
                "attachment {attachment_id} has a different immutable segment association"
            ),
            Self::HandleProviderConflict {
                attachment_id,
                selected,
                candidate,
            } => write!(
                formatter,
                "attachment {attachment_id} selected provider {selected}, but handle belongs to \
                 {candidate}"
            ),
            Self::State(error) => write!(formatter, "attachment resource state rejected: {error}"),
        }
    }
}

impl StdError for NetworkAttachmentStateError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::State(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
