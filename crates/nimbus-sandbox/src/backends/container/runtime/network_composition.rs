//! Injected OCI network-process composition for container backends.

use std::sync::Arc;

use nimbus_network::LocalNetworkAttachmentAuthority;

use crate::backends::oci::egress::{
    EgressProxyRegistry, egress_decision_log_root, egress_trust_anchor_root,
};
use crate::backends::oci::network::{
    ConfiguredSegmentAllocator, MachinePortProxyLifetimeRegistry, OciIpamAuthority,
    OciNetworkProcess, OciNetworkProcessError, OciSegmentAllocator,
    reconcile_startup_network_state,
};
use crate::backends::oci::port_lifecycle::{NetavarkPortLifetimeRegistry, OciPortLeaseCoordinator};

use super::manifest::reconcile_startup_manifest_publications;
use super::{ContainerSandboxBackend, ContainerSandboxBackendConfig};

impl ContainerSandboxBackend {
    #[cfg(test)]
    pub(crate) fn segment_allocator_handle_for_test(&self) -> Arc<OciSegmentAllocator> {
        Arc::clone(&self.segment_allocator)
    }

    #[cfg(test)]
    pub(crate) fn egress_registry_handle_for_test(&self) -> EgressProxyRegistry {
        self.egress_proxies.clone()
    }

    #[cfg(test)]
    pub(crate) fn netavark_port_lifetimes_handle_for_test(&self) -> NetavarkPortLifetimeRegistry {
        self.netavark_port_lifetimes.clone()
    }

    #[cfg(test)]
    pub(crate) fn machine_port_proxies_handle_for_test(&self) -> MachinePortProxyLifetimeRegistry {
        self.machine_port_proxies.clone()
    }

    /// Construct an explicit direct adapter without claiming process-global state.
    pub fn new(config: ContainerSandboxBackendConfig) -> Self {
        let segment_allocator: Arc<OciSegmentAllocator> =
            Arc::new(ConfiguredSegmentAllocator::reconstruct_direct(
                &config.network_state_root,
                &config.node_network_supernet,
                config.node_tenant_subnet_prefix,
            ));
        Self::with_segment_allocator(config, segment_allocator)
    }

    pub(crate) fn with_segment_allocator(
        config: ContainerSandboxBackendConfig,
        segment_allocator: Arc<OciSegmentAllocator>,
    ) -> Self {
        Self::with_segment_allocator_and_process(config, segment_allocator, None)
    }

    /// Construct a container facade under the one process network authority.
    pub fn with_network_process(
        mut config: ContainerSandboxBackendConfig,
        process: Arc<OciNetworkProcess>,
    ) -> Result<Self, OciNetworkProcessError> {
        config.network_state_root = process.authenticate_backend_config(
            &config.network_state_root,
            &config.node_network_supernet,
            config.node_tenant_subnet_prefix,
        )?;
        let segment_allocator = process.segment_allocator();
        Ok(Self::with_segment_allocator_and_process(
            config,
            segment_allocator,
            Some(process),
        ))
    }

    fn with_segment_allocator_and_process(
        config: ContainerSandboxBackendConfig,
        segment_allocator: Arc<OciSegmentAllocator>,
        network_process: Option<Arc<OciNetworkProcess>>,
    ) -> Self {
        let ipam_authority = network_process.as_ref().map_or_else(
            || OciIpamAuthority::reconstruct_direct(&config.network_state_root),
            |process| process.ipam_authority(),
        );
        let port_lease_coordinator = network_process.as_ref().map_or_else(
            || {
                OciPortLeaseCoordinator::reconstruct_direct(
                    &config.network_state_root,
                    config.published_port_range.clone(),
                )
                .with_max_ports_per_tenant(config.max_published_ports_per_tenant)
            },
            |process| {
                process.port_lease_coordinator(
                    config.published_port_range.clone(),
                    config.max_published_ports_per_tenant,
                )
            },
        );
        let egress_proxies = match network_process.as_ref() {
            Some(process) => process.egress_registry(
                egress_decision_log_root(&config.workload_state_root),
                egress_trust_anchor_root(&config.workload_state_root),
            ),
            None => EgressProxyRegistry::with_roots_and_port_authority(
                egress_decision_log_root(&config.workload_state_root),
                egress_trust_anchor_root(&config.workload_state_root),
                &config.network_state_root,
                port_lease_coordinator.cloned_authority(),
            ),
        };
        Self::with_network_authorities(
            config,
            segment_allocator,
            ipam_authority,
            port_lease_coordinator,
            egress_proxies,
            network_process,
        )
    }

    /// Reconstruct retained authorities once in the separate runner process.
    pub(super) fn reconstruct_for_runner(config: ContainerSandboxBackendConfig) -> Self {
        let segment_allocator: Arc<OciSegmentAllocator> =
            Arc::new(ConfiguredSegmentAllocator::reconstruct_for_runner(
                &config.network_state_root,
                &config.node_network_supernet,
                config.node_tenant_subnet_prefix,
            ));
        let ipam_authority = OciIpamAuthority::reconstruct_for_runner(&config.network_state_root);
        let port_lease_coordinator = OciPortLeaseCoordinator::reconstruct_for_runner(
            &config.network_state_root,
            config.published_port_range.clone(),
        )
        .with_max_ports_per_tenant(config.max_published_ports_per_tenant);
        let egress_proxies = EgressProxyRegistry::with_roots_and_port_authority(
            egress_decision_log_root(&config.workload_state_root),
            egress_trust_anchor_root(&config.workload_state_root),
            &config.network_state_root,
            port_lease_coordinator.cloned_authority(),
        );
        Self::with_network_authorities(
            config,
            segment_allocator,
            ipam_authority,
            port_lease_coordinator,
            egress_proxies,
            None,
        )
    }

    fn with_network_authorities(
        config: ContainerSandboxBackendConfig,
        segment_allocator: Arc<OciSegmentAllocator>,
        ipam_authority: OciIpamAuthority,
        port_lease_coordinator: OciPortLeaseCoordinator,
        egress_proxies: EgressProxyRegistry,
        network_process: Option<Arc<OciNetworkProcess>>,
    ) -> Self {
        let attachment_authority = network_process.as_ref().map_or_else(
            || LocalNetworkAttachmentAuthority::open(&config.network_state_root),
            |process| Ok(process.attachment_authority()),
        );
        let startup_reconciliation_error = attachment_authority
            .as_ref()
            .err()
            .map(|error| Arc::<str>::from(error.to_string()))
            .or_else(|| {
                reconcile_startup_manifest_publications(&config.workload_state_root)
                    .and_then(|()| {
                        reconcile_startup_network_state(
                            &config.workload_state_root,
                            &ipam_authority,
                            segment_allocator.as_ref(),
                        )
                    })
                    .err()
                    .map(|error| Arc::<str>::from(error.to_string()))
            });
        let netavark_port_lifetimes = network_process
            .as_ref()
            .map_or_else(NetavarkPortLifetimeRegistry::default, |process| {
                process.netavark_port_lifetimes()
            });
        let machine_port_proxies = network_process
            .as_ref()
            .map_or_else(MachinePortProxyLifetimeRegistry::default, |process| {
                process.machine_port_proxy_lifetimes()
            });
        Self {
            config,
            segment_allocator,
            attachment_authority: attachment_authority.ok(),
            ipam_authority,
            port_lease_coordinator,
            egress_proxies,
            netavark_port_lifetimes,
            machine_port_proxies,
            _network_process: network_process,
            startup_reconciliation_error,
            #[cfg(test)]
            restart_launch_test_probe: None,
            #[cfg(test)]
            runner_handoff_failure: None,
            #[cfg(test)]
            runner_lifecycle_lock_test_probe: None,
            #[cfg(test)]
            post_egress_reload_ack_observer: None,
        }
    }
}
