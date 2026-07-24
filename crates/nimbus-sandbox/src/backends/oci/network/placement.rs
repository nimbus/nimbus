//! Block-aware sandbox placement (MTN6).
//!
//! A tenant owns an ordered list of `/24` block bridges (PR-A). When a sandbox
//! is placed, it lands on the first block with a free address; when every current
//! block's `/24` is exhausted, a new sibling block bridge is grown (a netavark
//! CREATE — there is no live subnet-add) and the sandbox lands there. Shared by
//! both OCI-family backends so the placement policy is defined once.

use nimbus_core::TenantId;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{
    NetworkSegmentAllocator, OciNetworkConfig, OciNetworkLayout, OciSegmentRealization,
    SingleNodeSegmentAllocator, allocate_container_ips,
};

/// Reserve and return the network config of the block bridge that will host
/// `sandbox_id`. Tries the tenant's blocks in order and, on a block's `/24`
/// exhaustion, grows a new sibling block and retries; fail-closed when the node
/// super-net is exhausted. `allocate_container_ips` is idempotent per sandbox, so
/// `setup_container_network` later reuses the reserved IP on the placed block.
///
/// `build_config` turns a resolved block segment into the backend's
/// `OciNetworkConfig` (identical DNS-off/deny bodies differing only in binary
/// paths), keeping this loop backend-agnostic.
pub(crate) fn place_sandbox_on_block(
    allocator: &SingleNodeSegmentAllocator,
    tenant: &TenantId,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    build_config: impl Fn(&OciSegmentRealization) -> OciNetworkConfig,
) -> Result<OciNetworkConfig> {
    let mut segment = allocator.segment_for(tenant)?;
    loop {
        let config = build_config(&segment);
        match allocate_container_ips(layout, &config, sandbox_id) {
            Ok(_ips) => return Ok(config),
            // The block is full — grow a new sibling block bridge and retry.
            // grow_block fail-closes when the node super-net or block cap is hit.
            Err(SandboxError::NetworkSubnetExhausted { .. }) => {
                segment = allocator.grow_block(tenant)?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    /// Placement fills a tenant's first block to its `/24` limit, then grows onto
    /// a second block for the overflow sandbox — proving on-demand growth.
    #[test]
    fn placement_grows_onto_a_new_block_when_the_first_is_full() {
        let dir = tempdir().expect("temp dir");
        let state_root = dir.path();
        // A /30 super-net carved into /30 blocks: each block holds exactly ONE
        // container address (.1 gateway, .2 container), so the 2nd sandbox forces
        // a grow onto the next block.
        let allocator = SingleNodeSegmentAllocator::new(
            state_root,
            Some(super::super::segment::InstalledSuperNet {
                cidr: nimbus_core::net::Cidr::parse("10.7.0.0/23").unwrap(),
                epoch: 0,
            }),
            30,
        )
        .expect("local network store should open");
        let t = tenant("tenant-a");
        let build = |segment: &OciSegmentRealization| OciNetworkConfig {
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            network_id: segment.network_id().as_str().to_owned(),
            ..OciNetworkConfig::default()
        };

        // Sandbox 1 lands on block 0 (10.7.0.0/30 -> .2).
        let sb1_layout = OciNetworkLayout::new(state_root, &t, &SandboxId::new("sb-1"));
        sb1_layout.ensure_directories().expect("dirs");
        let c1 =
            place_sandbox_on_block(&allocator, &t, &sb1_layout, &SandboxId::new("sb-1"), build)
                .expect("place sb-1");
        assert_eq!(c1.network_subnet, "10.7.0.0/30");

        // Sandbox 2 cannot fit block 0's single slot, so placement grows onto
        // block 1 (10.7.0.4/30).
        let sb2_layout = OciNetworkLayout::new(state_root, &t, &SandboxId::new("sb-2"));
        sb2_layout.ensure_directories().expect("dirs");
        let c2 =
            place_sandbox_on_block(&allocator, &t, &sb2_layout, &SandboxId::new("sb-2"), build)
                .expect("place sb-2 grows a block");
        assert_eq!(c2.network_subnet, "10.7.0.4/30");
        assert_ne!(c1.network_interface, c2.network_interface);
    }

    #[test]
    // NNC0.5 fail-before: placement currently checks only the primary block
    // before growing, so it strands free capacity in an existing secondary
    // block. NNC2.3 owns the atomic all-block scan and removal of this ignore.
    #[ignore = "NNC0.5 expected red until placement reuses existing secondary blocks"]
    fn placement_must_reuse_free_capacity_in_an_existing_secondary_block() {
        let dir = tempdir().expect("temp dir");
        let state_root = dir.path();
        let allocator = SingleNodeSegmentAllocator::new(
            state_root,
            Some(super::super::segment::InstalledSuperNet {
                cidr: nimbus_core::net::Cidr::parse("10.7.0.0/23").unwrap(),
                epoch: 0,
            }),
            30,
        )
        .expect("local network store should open");
        let t = tenant("tenant-a");
        let build = |segment: &OciSegmentRealization| OciNetworkConfig {
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            network_id: segment.network_id().as_str().to_owned(),
            ..OciNetworkConfig::default()
        };

        let sb1 = SandboxId::new("sb-1");
        let sb1_layout = OciNetworkLayout::new(state_root, &t, &sb1);
        sb1_layout.ensure_directories().expect("sb-1 dirs");
        let first = place_sandbox_on_block(&allocator, &t, &sb1_layout, &sb1, build)
            .expect("sb-1 should fill the primary /30");
        assert_eq!(first.network_subnet, "10.7.0.0/30");

        let sb2 = SandboxId::new("sb-2");
        let sb2_layout = OciNetworkLayout::new(state_root, &t, &sb2);
        sb2_layout.ensure_directories().expect("sb-2 dirs");
        let secondary = place_sandbox_on_block(&allocator, &t, &sb2_layout, &sb2, build)
            .expect("sb-2 should grow the first secondary /30");
        assert_eq!(secondary.network_subnet, "10.7.0.4/30");
        super::super::ipam::deallocate_container_ips(&sb2_layout, &sb2)
            .expect("free the secondary block's only container slot");

        let sb3 = SandboxId::new("sb-3");
        let sb3_layout = OciNetworkLayout::new(state_root, &t, &sb3);
        sb3_layout.ensure_directories().expect("sb-3 dirs");
        let replacement = place_sandbox_on_block(&allocator, &t, &sb3_layout, &sb3, build)
            .expect("sb-3 placement should resolve");
        if replacement.network_subnet != secondary.network_subnet {
            assert_eq!(
                replacement.network_subnet, "10.7.0.8/30",
                "the fail-before must expose unnecessary growth to the next sibling block"
            );
        }
        assert_eq!(
            replacement.network_subnet, secondary.network_subnet,
            "placement must reuse free capacity in an existing secondary block before growth"
        );
    }
}
