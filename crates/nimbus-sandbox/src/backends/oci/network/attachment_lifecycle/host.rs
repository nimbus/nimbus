//! Narrow privileged host-effect port for the shared attachment lifecycle.

use std::net::Ipv4Addr;

use super::{OciAttachmentContext, OciIpamAuthority, recovery};
use crate::backends::oci::network::{
    create_persistent_network_namespace,
    netavark::{
        PreparedNetavarkSetup, PreparedNetavarkTeardown, execute_prepared_container_network_setup,
        execute_prepared_container_network_teardown, prepare_container_network_setup,
        prepare_container_network_teardown,
    },
    remove_persistent_network_namespace,
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

    fn prepare_provider_setup(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkSetup>;

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkSetup,
    ) -> Result<Vec<Ipv4Addr>>;

    fn prepare_provider_teardown(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkTeardown>;

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkTeardown,
    ) -> Result<()>;

    fn remove_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()>;
}

pub(super) struct RealAttachmentHostEffects;

impl AttachmentHostEffects for RealAttachmentHostEffects {
    fn create_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        create_persistent_network_namespace(&context.layout.netns_path)
    }

    fn prepare_provider_setup(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkSetup> {
        prepare_container_network_setup(ipam, &context.operation())
    }

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkSetup,
    ) -> Result<Vec<Ipv4Addr>> {
        execute_prepared_container_network_setup(ipam, &context.operation(), prepared)
    }

    fn prepare_provider_teardown(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkTeardown> {
        prepare_container_network_teardown(ipam, &context.operation())
    }

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkTeardown,
    ) -> Result<()> {
        execute_prepared_container_network_teardown(ipam, &context.operation(), prepared)
    }

    fn remove_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        remove_persistent_network_namespace(&context.layout.netns_path)
    }
}
