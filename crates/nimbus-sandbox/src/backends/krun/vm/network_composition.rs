//! Injected OCI network-process composition for krun backends.

use std::sync::Arc;

#[cfg(test)]
use crate::backends::oci::egress::EgressProxyRegistry;
use crate::backends::oci::network::{OciNetworkProcess, OciNetworkProcessError};
#[cfg(test)]
use crate::backends::oci::port_lifecycle::NetavarkPortLifetimeRegistry;

use super::{KrunSandboxBackend, KrunSandboxBackendConfig};

impl KrunSandboxBackend {
    #[cfg(test)]
    pub(crate) fn segment_allocator_handle_for_test(
        &self,
    ) -> Arc<crate::backends::oci::network::OciSegmentAllocator> {
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

    /// Construct a krun facade under the one process network authority.
    pub fn with_network_process(
        mut config: KrunSandboxBackendConfig,
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
}
