//! Static attachment capability evidence owned by sandbox backend compositions.
//!
//! Registrations describe only source-proven support. They do not probe local
//! binaries or devices, perform provider effects, or grant execution authority.

use std::sync::Arc;

use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentMode,
    NetworkAttachmentProviderRegistration, NetworkCapabilityRequirements,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkIsolationMode, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkManagementMode, NetworkProviderId,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};
use thiserror::Error;

use crate::SandboxBackendKind;

/// Stable identity key for the container host-managed attachment composition.
pub const CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY: &str =
    "nimbus-sandbox.container.host-managed-attachment";
/// Stable identity key for the krun host-managed attachment composition.
pub const KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY: &str =
    "nimbus-sandbox.krun.host-managed-attachment";
/// Stable identity key for the sandbox-owned egress PEP listener composition.
pub(crate) const SANDBOX_EGRESS_PEP_PROVIDER_KEY: &str = "nimbus-sandbox.egress-pep";

/// Effect-free network requirements sourced from one OCI sandbox backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxNetworkPlanRequirements {
    required_attachment_provider_id: NetworkProviderId,
    pep_provider_id: NetworkProviderId,
    capability_requirements: NetworkCapabilityRequirements,
    requires_pep_readiness: bool,
}

impl SandboxNetworkPlanRequirements {
    /// Exact source-owned attachment registration required by this backend.
    pub fn required_attachment_provider_id(&self) -> &NetworkProviderId {
        &self.required_attachment_provider_id
    }

    /// Exact source-owned provider identity for the sandbox egress PEP listener.
    pub fn pep_provider_id(&self) -> &NetworkProviderId {
        &self.pep_provider_id
    }

    /// Provider-neutral capability requirements for plan compilation.
    pub fn capability_requirements(&self) -> &NetworkCapabilityRequirements {
        &self.capability_requirements
    }

    /// Whether current execute-mode OCI compositions require PEP readiness.
    pub const fn requires_pep_readiness(&self) -> bool {
        self.requires_pep_readiness
    }
}

/// Project one sandbox backend's source-owned network-plan requirements.
///
/// This function reads no runtime configuration and performs no provider,
/// filesystem, socket, environment, clock, or random effect. It reports
/// requirements and stable provider identities; it does not select a provider.
pub fn sandbox_network_plan_requirements(
    backend: SandboxBackendKind,
) -> SandboxNetworkPlanRequirements {
    let kind = match backend {
        SandboxBackendKind::Container => SandboxAttachmentRegistrationKind::Container,
        SandboxBackendKind::Krun => SandboxAttachmentRegistrationKind::Krun,
    };
    SandboxNetworkPlanRequirements {
        required_attachment_provider_id: host_managed_attachment_provider_id(kind),
        pep_provider_id: NetworkProviderId::for_registration_key(SANDBOX_EGRESS_PEP_PROVIDER_KEY),
        capability_requirements: host_managed_attachment_requirements(kind),
        requires_pep_readiness: true,
    }
}

/// Why a configured sandbox backend cannot advertise its attachment evidence.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SandboxAttachmentRegistrationError {
    /// The backend is configured to render plans without owning Execute effects.
    #[error(
        "host-managed attachment registration {provider_key} is unavailable: PlanOnly does not own Execute effects"
    )]
    PlanOnly { provider_key: &'static str },
    /// Host-managed attachment effects are unsupported on the current target.
    #[error(
        "host-managed attachment registration {provider_key} is unavailable on target {target_os}: Execute attachments require Linux"
    )]
    UnsupportedTarget {
        provider_key: &'static str,
        target_os: &'static str,
    },
    /// Container machine forwarding belongs to a different provider composition.
    #[error(
        "host-managed attachment registration {provider_key} is unavailable: container machine forwarding is a different attachment composition"
    )]
    MachinePortForwarderConfigured { provider_key: &'static str },
    /// Cached startup reconciliation failure prevents new attachment work.
    #[error(
        "host-managed attachment registration {provider_key} is unavailable: startup reconciliation did not complete: {reason}"
    )]
    StartupReconciliationFailed {
        provider_key: &'static str,
        reason: Arc<str>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SandboxAttachmentRegistrationKind {
    Container,
    Krun,
}

impl SandboxAttachmentRegistrationKind {
    pub(crate) const fn provider_key(self) -> &'static str {
        match self {
            Self::Container => CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            Self::Krun => KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        }
    }

    fn attachment_modes(self) -> &'static [NetworkAttachmentMode] {
        match self {
            Self::Container => &[NetworkAttachmentMode::IsolatedNamespace],
            Self::Krun => &[
                NetworkAttachmentMode::IsolatedNamespace,
                NetworkAttachmentMode::VirtualMachineGuest,
            ],
        }
    }
}

fn host_managed_attachment_capabilities(
    kind: SandboxAttachmentRegistrationKind,
) -> NetworkAttachmentCapabilitySet {
    NetworkAttachmentCapabilitySet::new(
        NetworkManagementMode::NimbusHostManaged,
        kind.attachment_modes().iter().copied(),
        [
            NetworkIsolationMode::WorkloadNamespace,
            NetworkIsolationMode::TenantSegment,
        ],
    )
}

fn host_managed_lifecycle_capabilities() -> NetworkLifecycleCapabilitySet {
    NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ])
}

pub(crate) fn host_managed_attachment_provider_id(
    kind: SandboxAttachmentRegistrationKind,
) -> NetworkProviderId {
    NetworkProviderId::for_registration_key(kind.provider_key())
}

/// Provider-neutral desired requirements corresponding to one admitted
/// host-managed attachment registration.
pub(crate) fn host_managed_attachment_requirements(
    kind: SandboxAttachmentRegistrationKind,
) -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        host_managed_attachment_capabilities(kind),
        NetworkEndpointCapabilitySet::new([NetworkAddressFamily::Ipv4], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        host_managed_lifecycle_capabilities(),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

pub(crate) fn host_managed_attachment_registration(
    kind: SandboxAttachmentRegistrationKind,
    execute_mode: bool,
    machine_port_forwarder_configured: bool,
    startup_reconciliation_error: Option<&Arc<str>>,
) -> Result<NetworkAttachmentProviderRegistration, SandboxAttachmentRegistrationError> {
    host_managed_attachment_registration_for_target(
        kind,
        execute_mode,
        machine_port_forwarder_configured,
        startup_reconciliation_error,
        cfg!(target_os = "linux"),
        std::env::consts::OS,
    )
}

fn host_managed_attachment_registration_for_target(
    kind: SandboxAttachmentRegistrationKind,
    execute_mode: bool,
    machine_port_forwarder_configured: bool,
    startup_reconciliation_error: Option<&Arc<str>>,
    target_is_linux: bool,
    target_os: &'static str,
) -> Result<NetworkAttachmentProviderRegistration, SandboxAttachmentRegistrationError> {
    let provider_key = kind.provider_key();
    if !execute_mode {
        return Err(SandboxAttachmentRegistrationError::PlanOnly { provider_key });
    }
    if !target_is_linux {
        return Err(SandboxAttachmentRegistrationError::UnsupportedTarget {
            provider_key,
            target_os,
        });
    }
    if machine_port_forwarder_configured {
        return Err(
            SandboxAttachmentRegistrationError::MachinePortForwarderConfigured { provider_key },
        );
    }
    if let Some(reason) = startup_reconciliation_error {
        return Err(
            SandboxAttachmentRegistrationError::StartupReconciliationFailed {
                provider_key,
                reason: Arc::clone(reason),
            },
        );
    }

    Ok(NetworkAttachmentProviderRegistration::new(
        host_managed_attachment_provider_id(kind),
        host_managed_attachment_capabilities(kind),
        [NetworkAddressFamily::Ipv4],
        host_managed_lifecycle_capabilities(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn sandbox_network_plan_requirement_projection_is_exact_and_effect_free() {
        let container =
            crate::sandbox_network_plan_requirements(crate::SandboxBackendKind::Container);
        let repeated =
            crate::sandbox_network_plan_requirements(crate::SandboxBackendKind::Container);
        let krun = crate::sandbox_network_plan_requirements(crate::SandboxBackendKind::Krun);

        assert_eq!(
            container, repeated,
            "the projection must be pure and stable"
        );
        assert_ne!(
            container.required_attachment_provider_id(),
            krun.required_attachment_provider_id(),
            "backend-specific attachment registrations must retain distinct identities"
        );
        assert_eq!(container.pep_provider_id(), krun.pep_provider_id());
        assert_eq!(
            container.pep_provider_id(),
            &crate::backends::oci::port_lease::OciPortProvider::EgressPep.provider_id(),
            "the public source projection and lease adapter must share one PEP provider identity"
        );
        assert!(container.requires_pep_readiness());
        assert!(krun.requires_pep_readiness());

        assert_eq!(
            container
                .capability_requirements()
                .attachment()
                .attachment_modes(),
            &BTreeSet::from([NetworkAttachmentMode::IsolatedNamespace])
        );
        assert_eq!(
            krun.capability_requirements()
                .attachment()
                .attachment_modes(),
            &BTreeSet::from([
                NetworkAttachmentMode::IsolatedNamespace,
                NetworkAttachmentMode::VirtualMachineGuest,
            ])
        );
        for projection in [&container, &krun] {
            assert_eq!(
                projection
                    .capability_requirements()
                    .attachment()
                    .management_mode(),
                NetworkManagementMode::NimbusHostManaged
            );
            assert_eq!(
                projection
                    .capability_requirements()
                    .sovereignty()
                    .maximum_control_plane_locality(),
                NetworkControlPlaneLocality::LocalOnly
            );
            assert!(
                projection
                    .capability_requirements()
                    .sovereignty()
                    .allowed_external_dependencies()
                    .is_empty()
            );
            assert!(
                projection
                    .capability_requirements()
                    .sovereignty()
                    .offline_restart_required()
            );
        }
    }

    #[test]
    fn linux_registration_facts_are_conservative_and_backend_specific() {
        let container = host_managed_attachment_registration_for_target(
            SandboxAttachmentRegistrationKind::Container,
            true,
            false,
            None,
            true,
            "linux",
        )
        .expect("container Execute composition should report Linux facts");
        let krun = host_managed_attachment_registration_for_target(
            SandboxAttachmentRegistrationKind::Krun,
            true,
            false,
            None,
            true,
            "linux",
        )
        .expect("krun Execute composition should report Linux facts");

        assert_eq!(
            container.provider_id(),
            &NetworkProviderId::for_registration_key(
                CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY
            )
        );
        assert_eq!(
            krun.provider_id(),
            &NetworkProviderId::for_registration_key(KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY)
        );
        assert_eq!(
            container.attachment().attachment_modes(),
            &BTreeSet::from([NetworkAttachmentMode::IsolatedNamespace])
        );
        assert_eq!(
            krun.attachment().attachment_modes(),
            &BTreeSet::from([
                NetworkAttachmentMode::IsolatedNamespace,
                NetworkAttachmentMode::VirtualMachineGuest,
            ])
        );
        let isolation_modes = BTreeSet::from([
            NetworkIsolationMode::WorkloadNamespace,
            NetworkIsolationMode::TenantSegment,
        ]);
        let lifecycle = BTreeSet::from([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]);
        for registration in [&container, &krun] {
            assert_eq!(
                registration.attachment().management_mode(),
                NetworkManagementMode::NimbusHostManaged
            );
            assert_eq!(
                registration.attachment().isolation_modes(),
                &isolation_modes
            );
            assert_eq!(
                registration.address_families(),
                &BTreeSet::from([NetworkAddressFamily::Ipv4])
            );
            assert_eq!(registration.lifecycle().features(), &lifecycle);
            assert_eq!(
                registration.sovereignty().control_plane_locality(),
                NetworkControlPlaneLocality::LocalOnly
            );
            assert!(
                registration
                    .sovereignty()
                    .required_external_dependencies()
                    .is_empty()
            );
            assert!(registration.sovereignty().offline_restart_supported());
            assert!(
                !registration
                    .attachment()
                    .attachment_modes()
                    .contains(&NetworkAttachmentMode::HostNetwork)
            );
            assert!(
                !registration
                    .attachment()
                    .attachment_modes()
                    .contains(&NetworkAttachmentMode::ProviderVirtualNetwork)
            );
            assert!(
                !registration
                    .attachment()
                    .isolation_modes()
                    .contains(&NetworkIsolationMode::ProviderBoundary)
            );
            assert!(
                !registration
                    .address_families()
                    .contains(&NetworkAddressFamily::Ipv6)
            );
        }
    }

    #[test]
    fn configuration_guards_refuse_non_owning_compositions() {
        let plan_only = host_managed_attachment_registration_for_target(
            SandboxAttachmentRegistrationKind::Container,
            false,
            false,
            None,
            true,
            "linux",
        )
        .expect_err("PlanOnly must not advertise Execute capabilities");
        assert_eq!(
            plan_only,
            SandboxAttachmentRegistrationError::PlanOnly {
                provider_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            }
        );

        let unsupported = host_managed_attachment_registration_for_target(
            SandboxAttachmentRegistrationKind::Krun,
            true,
            false,
            None,
            false,
            "macos",
        )
        .expect_err("non-Linux targets must not advertise Linux effects");
        assert_eq!(
            unsupported,
            SandboxAttachmentRegistrationError::UnsupportedTarget {
                provider_key: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
                target_os: "macos",
            }
        );

        let machine_forwarder = host_managed_attachment_registration_for_target(
            SandboxAttachmentRegistrationKind::Container,
            true,
            true,
            None,
            true,
            "linux",
        )
        .expect_err("machine forwarding is a different composition");
        assert_eq!(
            machine_forwarder,
            SandboxAttachmentRegistrationError::MachinePortForwarderConfigured {
                provider_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            }
        );
    }

    #[test]
    fn cached_startup_reconciliation_failure_refuses_registration() {
        let reason = Arc::<str>::from("injected startup reconciliation failure");

        for (kind, provider_key) in [
            (
                SandboxAttachmentRegistrationKind::Container,
                CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            ),
            (
                SandboxAttachmentRegistrationKind::Krun,
                KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            ),
        ] {
            let error = host_managed_attachment_registration_for_target(
                kind,
                true,
                false,
                Some(&reason),
                true,
                "linux",
            )
            .expect_err("cached startup reconciliation failure must refuse registration");

            assert_eq!(
                error,
                SandboxAttachmentRegistrationError::StartupReconciliationFailed {
                    provider_key,
                    reason: Arc::clone(&reason),
                }
            );
            assert_eq!(
                error.to_string(),
                format!(
                    "host-managed attachment registration {provider_key} is unavailable: startup reconciliation did not complete: {reason}"
                )
            );
        }
    }
}
