//! Narrow privileged host-effect port for the shared attachment lifecycle.

use std::net::Ipv4Addr;

use super::{OciAttachmentContext, OciIpamAuthority, recovery};
use crate::backends::oci::network::{
    create_persistent_network_namespace, remove_persistent_network_namespace,
    setup_container_network, teardown_container_network,
};
use crate::error::Result;

/// Allocation, IPAM, port authority, and compensation policy remain concrete
/// lifecycle dependencies. Only privileged namespace/Netavark effects are
/// substitutable so the same algorithm can be exercised deterministically on
/// non-Linux test hosts.
pub(super) trait AttachmentHostEffects {
    fn inspect_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> recovery::AttachmentProviderObservation {
        recovery::inspect_provider(ipam, context)
    }

    fn create_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()>;

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<Vec<Ipv4Addr>>;

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<()>;

    fn remove_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()>;
}

pub(super) struct RealAttachmentHostEffects;

impl AttachmentHostEffects for RealAttachmentHostEffects {
    fn create_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        create_persistent_network_namespace(&context.layout.netns_path)
    }

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<Vec<Ipv4Addr>> {
        setup_container_network(ipam, &context.operation())
    }

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<()> {
        teardown_container_network(ipam, &context.operation())
    }

    fn remove_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        remove_persistent_network_namespace(&context.layout.netns_path)
    }
}
