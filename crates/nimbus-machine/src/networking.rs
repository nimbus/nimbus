//! Pure machine-connectivity capability facts and deterministic satisfaction.
//!
//! These values describe the narrow host-to-machine connectivity contract.
//! They do not register a network provider, select a fallback, perform an
//! effect, or authorize segment allocation.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkAttachmentMode, NetworkCapabilityMismatch,
    NetworkControlPlaneLocality, NetworkExposure, NetworkIsolationMode, NetworkManagementMode,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};
use serde::{Deserialize, Serialize};

use crate::provider::MachineProvider;

/// Machine-connectivity facts offered by one source-owned adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineConnectivityCapabilities {
    attachment: NetworkAttachmentCapabilitySet,
    exposures: BTreeSet<NetworkExposure>,
    sovereignty: NetworkSovereigntyCapabilities,
}

impl MachineConnectivityCapabilities {
    /// Construct a complete, effect-free machine-connectivity report.
    pub fn new(
        attachment: NetworkAttachmentCapabilitySet,
        exposures: impl IntoIterator<Item = NetworkExposure>,
        sovereignty: NetworkSovereigntyCapabilities,
    ) -> Self {
        Self {
            attachment,
            exposures: exposures.into_iter().collect(),
            sovereignty,
        }
    }

    /// Attachment ownership, shape, and isolation evidence.
    pub fn attachment(&self) -> &NetworkAttachmentCapabilitySet {
        &self.attachment
    }

    /// Host exposure classes proven by the adapter.
    pub fn exposures(&self) -> &BTreeSet<NetworkExposure> {
        &self.exposures
    }

    /// Sovereignty evidence proven by the adapter.
    pub fn sovereignty(&self) -> &NetworkSovereigntyCapabilities {
        &self.sovereignty
    }

    /// Prove that this exact report satisfies the caller's admitted contract.
    ///
    /// The provider identity is diagnostic only. Failure never selects or
    /// invokes another provider.
    pub fn ensure_satisfied(
        &self,
        provider: MachineProvider,
        requirements: &MachineConnectivityRequirements,
    ) -> Result<(), MachineConnectivityError> {
        let mut mismatches = Vec::new();

        if self.attachment.management_mode() != requirements.attachment.management_mode() {
            mismatches.push(NetworkCapabilityMismatch::ManagementMode {
                required: requirements.attachment.management_mode(),
                offered: self.attachment.management_mode(),
            });
        }
        for required in requirements
            .attachment
            .attachment_modes()
            .difference(self.attachment.attachment_modes())
        {
            mismatches.push(NetworkCapabilityMismatch::AttachmentMode {
                required: *required,
            });
        }
        for required in requirements
            .attachment
            .isolation_modes()
            .difference(self.attachment.isolation_modes())
        {
            mismatches.push(NetworkCapabilityMismatch::IsolationMode {
                required: *required,
            });
        }
        for required in requirements.exposures.difference(&self.exposures) {
            mismatches.push(NetworkCapabilityMismatch::Exposure {
                required: *required,
            });
        }
        if self.sovereignty.control_plane_locality()
            > requirements.sovereignty.maximum_control_plane_locality()
        {
            mismatches.push(NetworkCapabilityMismatch::ControlPlaneLocality {
                maximum_allowed: requirements.sovereignty.maximum_control_plane_locality(),
                offered: self.sovereignty.control_plane_locality(),
            });
        }
        for dependency in self
            .sovereignty
            .required_external_dependencies()
            .difference(requirements.sovereignty.allowed_external_dependencies())
        {
            mismatches.push(NetworkCapabilityMismatch::ExternalDependency {
                disallowed: *dependency,
            });
        }
        if requirements.sovereignty.offline_restart_required()
            && !self.sovereignty.offline_restart_supported()
        {
            mismatches.push(NetworkCapabilityMismatch::OfflineRestart {
                required: true,
                offered: false,
            });
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(MachineConnectivityError::Unsatisfied {
                provider,
                mismatches,
            })
        }
    }
}

/// Admitted requirements for one machine-connectivity composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineConnectivityRequirements {
    attachment: NetworkAttachmentCapabilitySet,
    exposures: BTreeSet<NetworkExposure>,
    sovereignty: NetworkSovereigntyRequirements,
}

impl MachineConnectivityRequirements {
    /// Construct fully explicit machine-connectivity requirements.
    pub fn new(
        attachment: NetworkAttachmentCapabilitySet,
        exposures: impl IntoIterator<Item = NetworkExposure>,
        sovereignty: NetworkSovereigntyRequirements,
    ) -> Self {
        Self {
            attachment,
            exposures: exposures.into_iter().collect(),
            sovereignty,
        }
    }

    /// Required attachment ownership, shape, and isolation.
    pub fn attachment(&self) -> &NetworkAttachmentCapabilitySet {
        &self.attachment
    }

    /// Required host exposure classes.
    pub fn exposures(&self) -> &BTreeSet<NetworkExposure> {
        &self.exposures
    }

    /// Admitted sovereignty constraints.
    pub fn sovereignty(&self) -> &NetworkSovereigntyRequirements {
        &self.sovereignty
    }
}

/// Stable failure to obtain or satisfy machine-connectivity evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MachineConnectivityError {
    /// The selected provider has no source-owned report on this host.
    ProviderUnavailable { provider: MachineProvider },
    /// Available evidence does not satisfy the admitted requirements.
    Unsatisfied {
        provider: MachineProvider,
        mismatches: Vec<NetworkCapabilityMismatch>,
    },
}

impl MachineConnectivityError {
    /// Provider whose availability or evidence failed.
    pub const fn provider(&self) -> MachineProvider {
        match self {
            Self::ProviderUnavailable { provider } | Self::Unsatisfied { provider, .. } => {
                *provider
            }
        }
    }

    /// Ordered capability mismatches, or an empty slice when unavailable.
    pub fn mismatches(&self) -> &[NetworkCapabilityMismatch] {
        match self {
            Self::ProviderUnavailable { .. } => &[],
            Self::Unsatisfied { mismatches, .. } => mismatches,
        }
    }
}

impl Display for MachineConnectivityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderUnavailable { provider } => write!(
                formatter,
                "the {} machine provider has no available connectivity capability evidence on this host",
                provider.as_str().to_ascii_uppercase()
            ),
            Self::Unsatisfied {
                provider,
                mismatches,
            } => {
                write!(
                    formatter,
                    "machine provider `{}` does not satisfy connectivity requirements: ",
                    provider.as_str()
                )?;
                for (index, mismatch) in mismatches.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{mismatch}")?;
                }
                Ok(())
            }
        }
    }
}

impl StdError for MachineConnectivityError {}

impl MachineProvider {
    /// Return source-proven connectivity evidence available on this host.
    ///
    /// The implemented krunkit and vfkit compositions are macOS-only. WSL2's
    /// provider-managed topology is declared, but WIN2/WIN5 still own its
    /// adapter and reachability evidence, so it remains unavailable.
    pub fn connectivity_capabilities(
        self,
    ) -> Result<MachineConnectivityCapabilities, MachineConnectivityError> {
        if cfg!(target_os = "macos") && matches!(self, Self::Krunkit | Self::Vfkit) {
            return Ok(host_managed_applehv_capabilities());
        }
        Err(MachineConnectivityError::ProviderUnavailable { provider: self })
    }
}

fn host_managed_applehv_capabilities() -> MachineConnectivityCapabilities {
    MachineConnectivityCapabilities::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::VirtualMachineGuest],
            [NetworkIsolationMode::WorkloadNamespace],
        ),
        [NetworkExposure::Loopback],
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}
