//! Block-aware sandbox placement (MTN6).
//!
//! A tenant owns an ordered list of `/24` block bridges (PR-A). When a sandbox
//! is placed, it lands on the first block with a free address; when every current
//! block's `/24` is exhausted, a new sibling block bridge is grown (a netavark
//! CREATE — there is no live subnet-add) and the sandbox lands there. Shared by
//! both OCI-family backends so the placement policy is defined once.

use nimbus_core::TenantId;
#[cfg(test)]
use nimbus_network::{NetworkLeaseEpoch, NetworkProviderHandle, NetworkProviderId};
use nimbus_network::{NetworkReservationClaim, NetworkSegmentGrowth};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{
    OciNetworkConfig, OciNetworkLayout, OciSegmentAllocator, OciSegmentRealization,
    default_network_attachment_id, ipam::allocate_container_ips_on_first_available,
};

/// Reserve and return the network config of the block bridge that will host
/// `sandbox_id`. One IPAM transaction scans the tenant's complete ordered block
/// set and reserves on the first block with capacity. Only an all-block
/// exhaustion result permits compare-and-swap-fenced growth; concurrent growth
/// makes this caller rescan instead of appending a redundant sibling.
/// `allocate_container_ips` remains idempotent, so `setup_container_network`
/// later reuses the reserved IP on the placed block.
///
/// `build_config` turns a resolved block segment into the backend's
/// `OciNetworkConfig` (identical DNS-off/deny bodies differing only in binary
/// paths), keeping this loop backend-agnostic.
pub(crate) fn place_sandbox_on_block(
    allocator: &OciSegmentAllocator,
    tenant: &TenantId,
    layout: &OciNetworkLayout,
    sandbox_id: &SandboxId,
    reservation_claim: &NetworkReservationClaim,
    build_config: impl Fn(&OciSegmentRealization, &NetworkReservationClaim) -> OciNetworkConfig,
) -> Result<OciNetworkConfig> {
    let attachment_id = default_network_attachment_id(sandbox_id);
    let placement = (|| {
        allocator.reserve_attachment_for_coordinator(tenant, &attachment_id, reservation_claim)?;
        loop {
            let segments = allocator.segments_for(tenant)?;
            let observed_block_count = segments.len();
            let configs = segments
                .iter()
                .map(|segment| build_config(segment, reservation_claim))
                .collect::<Vec<_>>();
            if configs
                .iter()
                .any(|config| &config.reservation_claim != reservation_claim)
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "OCI placement config reservation claim diverged from the authoritative \
                         launch coordinator for sandbox {sandbox_id} before IPAM"
                    ),
                });
            }
            match allocate_container_ips_on_first_available(
                layout,
                &configs,
                sandbox_id,
                reservation_claim,
            ) {
                Ok(allocation) => {
                    let selected = configs.into_iter().nth(allocation.block_index).ok_or_else(
                        || SandboxError::OperationFailed {
                            message: format!(
                                "OCI IPAM selected missing block {} from {observed_block_count} observed blocks",
                                allocation.block_index
                            ),
                        },
                    )?;
                    let bound = allocator.bind_reserved_attachment_to_segment(
                        tenant,
                        &attachment_id,
                        &allocation.segment_id,
                        reservation_claim,
                    )?;
                    if bound.segment_id() != &allocation.segment_id
                        || selected.segment_id != allocation.segment_id.as_str()
                    {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "OCI placement selected segment {} but allocator/config evidence diverged",
                                allocation.segment_id
                            ),
                        });
                    }
                    return Ok(selected);
                }
                // Every observed block is full. Growth is allowed only if that
                // ordered observation is still current under the segment lock.
                Err(SandboxError::NetworkSubnetExhausted { .. }) => {
                    match allocator.grow_block_if_current(tenant, &segments)? {
                        NetworkSegmentGrowth::Grown(_) | NetworkSegmentGrowth::ObservationStale => {
                        }
                    }
                }
                Err(error) => return Err(error),
            }
        }
    })();
    placement.map_err(|error| {
        super::reaper::compensate_reserved_network_launch_without_effect(
            allocator,
            layout,
            tenant,
            sandbox_id,
            reservation_claim,
            error,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::oci::network::{
        OciSegmentAllocator, RecordingSegmentAllocator, SegmentAllocatorOperation,
        SingleNodeSegmentAllocator,
    };
    use nimbus_network::NetworkSegmentAllocator;
    use proptest::prelude::*;
    use std::collections::BTreeSet;
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    fn config_for_segment(
        segment: &OciSegmentRealization,
        reservation_claim: &NetworkReservationClaim,
    ) -> OciNetworkConfig {
        OciNetworkConfig {
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            segment_id: segment.segment_id().as_str().to_owned(),
            reservation_claim: reservation_claim.clone(),
            network_id: segment.network_id().as_str().to_owned(),
            ..OciNetworkConfig::default()
        }
    }

    fn reservation_claim(label: &str) -> NetworkReservationClaim {
        let provider = NetworkProviderId::for_registration_key(
            "nimbus-sandbox.network-launch-coordinator.test",
        );
        NetworkReservationClaim::new(
            NetworkProviderHandle::new(provider, format!("attempt:{label}"))
                .expect("fixture claim should validate"),
        )
    }

    fn place_sandbox_on_block(
        allocator: &OciSegmentAllocator,
        tenant: &TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        build_config: impl Fn(&OciSegmentRealization, &NetworkReservationClaim) -> OciNetworkConfig,
    ) -> Result<OciNetworkConfig> {
        super::place_sandbox_on_block(
            allocator,
            tenant,
            layout,
            sandbox_id,
            &reservation_claim(sandbox_id.as_str()),
            build_config,
        )
    }

    #[test]
    fn initial_attachment_reservation_ack_loss_enters_exact_compensation() {
        let dir = tempdir().expect("temp dir should create");
        let tenant = tenant("tenant-reservation-ack-loss");
        let sandbox_id = SandboxId::new("reservation-ack-loss");
        let claim = reservation_claim("reservation-ack-loss");
        let expected_claim = claim.clone();
        let recorder = Arc::new(
            RecordingSegmentAllocator::new(tenant.clone(), "10.83.0.0/24", 83)
                .with_reserve_attachment_observer(move |observed| {
                    assert_eq!(observed, &expected_claim);
                    Err(SandboxError::OperationFailed {
                        message: "injected attachment reservation acknowledgement loss".to_owned(),
                    })
                }),
        );
        let allocator: Arc<OciSegmentAllocator> = recorder.clone();
        let layout = OciNetworkLayout::new(dir.path(), &tenant, &sandbox_id);
        layout.ensure_directories().expect("layout should exist");
        let build_count = Arc::new(AtomicUsize::new(0));
        let build_count_for_call = Arc::clone(&build_count);

        let error = super::place_sandbox_on_block(
            allocator.as_ref(),
            &tenant,
            &layout,
            &sandbox_id,
            &claim,
            move |segment, reservation_claim| {
                build_count_for_call.fetch_add(1, Ordering::SeqCst);
                config_for_segment(segment, reservation_claim)
            },
        )
        .expect_err("ambiguous initial reservation must enter exact compensation");

        assert!(
            error
                .to_string()
                .contains("injected attachment reservation acknowledgement loss"),
            "the initial reservation failure must remain primary: {error}"
        );
        assert_eq!(
            build_count.load(Ordering::SeqCst),
            0,
            "placement/config construction must not start after reservation failure"
        );
        let attachment = default_network_attachment_id(&sandbox_id);
        let operations = recorder.operations();
        assert_eq!(
            operations.len(),
            4,
            "exact compensation trace: {operations:?}"
        );
        assert_eq!(
            operations[0],
            SegmentAllocatorOperation::ReserveAttachment(tenant.clone(), attachment.clone(),)
        );
        assert_eq!(
            operations[1],
            SegmentAllocatorOperation::ReleaseReservedAttachment(
                tenant.clone(),
                attachment.clone(),
            )
        );
        assert_eq!(
            operations[2],
            SegmentAllocatorOperation::FinalizeReservedAttachment(tenant.clone(), attachment,)
        );
        assert!(
            matches!(
                &operations[3],
                SegmentAllocatorOperation::FinalizeRelease(final_tenant, _)
                    if final_tenant == &tenant
            ),
            "exact cleanup must finalize the coordinator-owned allocation: {operations:?}"
        );
    }

    #[test]
    fn placement_rejects_a_config_with_foreign_reservation_authority_before_ipam() {
        let dir = tempdir().expect("temp dir should create");
        let tenant = tenant("tenant-foreign-config-claim");
        let sandbox_id = SandboxId::new("foreign-config-claim");
        let claim = reservation_claim("authoritative");
        let foreign_claim = reservation_claim("foreign");
        let allocator = SingleNodeSegmentAllocator::new(
            dir.path(),
            Some(super::super::segment::InstalledSuperNet {
                cidr: nimbus_core::net::Cidr::parse("10.91.0.0/24")
                    .expect("super-net should parse"),
                epoch: NetworkLeaseEpoch::new(1),
            }),
            30,
        )
        .expect("local network store should open");
        let layout = OciNetworkLayout::new(dir.path(), &tenant, &sandbox_id);
        layout.ensure_directories().expect("layout should exist");

        let error = super::place_sandbox_on_block(
            &allocator,
            &tenant,
            &layout,
            &sandbox_id,
            &claim,
            move |segment, _| config_for_segment(segment, &foreign_claim),
        )
        .expect_err("foreign config authority must fail before IPAM mutation");

        assert!(
            error.to_string().contains("reservation claim")
                && error.to_string().contains("diverged"),
            "rejection must name the duplicated authority: {error}"
        );
        assert!(
            super::super::ipam::load_container_ips(&layout, &sandbox_id).is_err(),
            "foreign config authority must not leave an IPAM reservation"
        );
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
                epoch: NetworkLeaseEpoch::new(0),
            }),
            30,
        )
        .expect("local network store should open");
        let t = tenant("tenant-a");
        // Sandbox 1 lands on block 0 (10.7.0.0/30 -> .2).
        let sb1_layout = OciNetworkLayout::new(state_root, &t, &SandboxId::new("sb-1"));
        sb1_layout.ensure_directories().expect("dirs");
        let c1 = place_sandbox_on_block(
            &allocator,
            &t,
            &sb1_layout,
            &SandboxId::new("sb-1"),
            config_for_segment,
        )
        .expect("place sb-1");
        assert_eq!(c1.network_subnet, "10.7.0.0/30");

        // Sandbox 2 cannot fit block 0's single slot, so placement grows onto
        // block 1 (10.7.0.4/30).
        let sb2_layout = OciNetworkLayout::new(state_root, &t, &SandboxId::new("sb-2"));
        sb2_layout.ensure_directories().expect("dirs");
        let c2 = place_sandbox_on_block(
            &allocator,
            &t,
            &sb2_layout,
            &SandboxId::new("sb-2"),
            config_for_segment,
        )
        .expect("place sb-2 grows a block");
        assert_eq!(c2.network_subnet, "10.7.0.4/30");
        assert_ne!(c1.network_interface, c2.network_interface);

        let sb2 = SandboxId::new("sb-2");
        let selected = super::super::ipam::allocate_container_ips_on_first_available(
            &sb2_layout,
            &[c1.clone(), c2.clone()],
            &sb2,
            &reservation_claim(sb2.as_str()),
        )
        .expect("IPAM replay should preserve the exact selected block");
        assert_eq!(
            selected.segment_id.as_str(),
            c2.segment_id,
            "IPAM must persist stable segment identity independently of the assigned IP"
        );
        let adopted = allocator
            .adopt_reserved_attachment(
                &t,
                &default_network_attachment_id(&sb2),
                &reservation_claim(sb2.as_str()),
            )
            .expect("exact placement should be adoptable");
        assert_eq!(
            adopted.segment_id().as_str(),
            c2.segment_id,
            "adoption must return the selected secondary segment, never the primary"
        );
    }

    #[test]
    fn placement_must_reuse_free_capacity_in_an_existing_secondary_block() {
        let dir = tempdir().expect("temp dir");
        let state_root = dir.path();
        let allocator = SingleNodeSegmentAllocator::new(
            state_root,
            Some(super::super::segment::InstalledSuperNet {
                cidr: nimbus_core::net::Cidr::parse("10.7.0.0/23").unwrap(),
                epoch: NetworkLeaseEpoch::new(0),
            }),
            30,
        )
        .expect("local network store should open");
        let t = tenant("tenant-a");
        let sb1 = SandboxId::new("sb-1");
        let sb1_layout = OciNetworkLayout::new(state_root, &t, &sb1);
        sb1_layout.ensure_directories().expect("sb-1 dirs");
        let first = place_sandbox_on_block(&allocator, &t, &sb1_layout, &sb1, config_for_segment)
            .expect("sb-1 should fill the primary /30");
        assert_eq!(first.network_subnet, "10.7.0.0/30");

        let sb2 = SandboxId::new("sb-2");
        let sb2_layout = OciNetworkLayout::new(state_root, &t, &sb2);
        sb2_layout.ensure_directories().expect("sb-2 dirs");
        let secondary =
            place_sandbox_on_block(&allocator, &t, &sb2_layout, &sb2, config_for_segment)
                .expect("sb-2 should grow the first secondary /30");
        assert_eq!(secondary.network_subnet, "10.7.0.4/30");
        super::super::ipam::deallocate_container_ips_after_confirmed_detach(
            &sb2_layout,
            &sb2,
            &secondary.reservation_claim,
        )
        .expect("free the secondary block's only container slot");

        let sb3 = SandboxId::new("sb-3");
        let sb3_layout = OciNetworkLayout::new(state_root, &t, &sb3);
        sb3_layout.ensure_directories().expect("sb-3 dirs");
        let replacement =
            place_sandbox_on_block(&allocator, &t, &sb3_layout, &sb3, config_for_segment)
                .expect("sb-3 placement should resolve");
        assert_eq!(
            replacement.network_subnet, secondary.network_subnet,
            "placement must reuse free capacity in an existing secondary block before growth"
        );
        assert_eq!(
            allocator
                .segments_for(&t)
                .expect("segment set should read")
                .len(),
            2,
            "reusing secondary capacity must not grow a third block"
        );
    }

    #[test]
    fn placement_retry_recovers_the_existing_secondary_reservation() {
        let dir = tempdir().expect("temp dir");
        let state_root = dir.path();
        let allocator = SingleNodeSegmentAllocator::new(
            state_root,
            Some(super::super::segment::InstalledSuperNet {
                cidr: nimbus_core::net::Cidr::parse("10.7.0.0/23").unwrap(),
                epoch: NetworkLeaseEpoch::new(0),
            }),
            30,
        )
        .expect("local network store should open");
        let tenant = tenant("tenant-a");

        let primary = SandboxId::new("primary");
        let primary_layout = OciNetworkLayout::new(state_root, &tenant, &primary);
        primary_layout.ensure_directories().expect("primary dirs");
        place_sandbox_on_block(
            &allocator,
            &tenant,
            &primary_layout,
            &primary,
            config_for_segment,
        )
        .expect("primary placement should resolve");

        let secondary = SandboxId::new("secondary");
        let secondary_layout = OciNetworkLayout::new(state_root, &tenant, &secondary);
        secondary_layout
            .ensure_directories()
            .expect("secondary dirs");
        let first = place_sandbox_on_block(
            &allocator,
            &tenant,
            &secondary_layout,
            &secondary,
            config_for_segment,
        )
        .expect("secondary placement should resolve");
        let retry = place_sandbox_on_block(
            &allocator,
            &tenant,
            &secondary_layout,
            &secondary,
            config_for_segment,
        )
        .expect("idempotent placement retry should resolve");

        assert_eq!(retry.network_subnet, first.network_subnet);
        assert_eq!(
            allocator
                .segments_for(&tenant)
                .expect("segment set should read")
                .len(),
            2,
            "recovering an existing reservation must not grow"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]

        #[test]
        fn placement_reuses_any_free_existing_block_before_growth(
            block_count in 2usize..=6,
            free_seed in any::<usize>(),
        ) {
            let dir = tempdir().expect("temp dir");
            let state_root = dir.path();
            let allocator = SingleNodeSegmentAllocator::new(
                state_root,
                Some(super::super::segment::InstalledSuperNet {
                    cidr: nimbus_core::net::Cidr::parse("10.8.0.0/23").unwrap(),
                    epoch: NetworkLeaseEpoch::new(0),
                }),
                30,
            )
            .expect("local network store should open");
            let tenant = tenant("property-tenant");
            let mut placements = Vec::with_capacity(block_count);

            for index in 0..block_count {
                let sandbox = SandboxId::new(format!("seed-{index}"));
                let layout = OciNetworkLayout::new(state_root, &tenant, &sandbox);
                layout.ensure_directories().expect("seed dirs");
                let config = place_sandbox_on_block(
                    &allocator,
                    &tenant,
                    &layout,
                    &sandbox,
                    config_for_segment,
                )
                .expect("seed placement should resolve");
                placements.push((sandbox, layout, config));
            }

            let free_index = free_seed % block_count;
            let expected_subnet = placements[free_index].2.network_subnet.clone();
            super::super::ipam::deallocate_container_ips_after_confirmed_detach(
                &placements[free_index].1,
                &placements[free_index].0,
                &placements[free_index].2.reservation_claim,
            )
            .expect("selected block should deallocate");

            let replacement = SandboxId::new("replacement");
            let replacement_layout = OciNetworkLayout::new(state_root, &tenant, &replacement);
            replacement_layout
                .ensure_directories()
                .expect("replacement dirs");
            let selected = place_sandbox_on_block(
                &allocator,
                &tenant,
                &replacement_layout,
                &replacement,
                config_for_segment,
            )
            .expect("replacement placement should resolve");

            prop_assert_eq!(selected.network_subnet, expected_subnet);
            prop_assert_eq!(
                allocator
                    .segments_for(&tenant)
                    .expect("segment set should read")
                    .len(),
                block_count,
                "a free observed block must prevent growth"
            );
        }
    }

    #[test]
    fn concurrent_exhaustion_grows_only_the_required_block_set() {
        const PLACERS: usize = 6;

        let dir = tempdir().expect("temp dir");
        let state_root = dir.path().to_path_buf();
        let allocator = Arc::new(
            SingleNodeSegmentAllocator::new(
                &state_root,
                Some(super::super::segment::InstalledSuperNet {
                    cidr: nimbus_core::net::Cidr::parse("10.9.0.0/23").unwrap(),
                    epoch: NetworkLeaseEpoch::new(0),
                }),
                30,
            )
            .expect("local network store should open"),
        );
        let tenant = tenant("concurrent-tenant");
        let barrier = Arc::new(Barrier::new(PLACERS));
        let threads = (0..PLACERS)
            .map(|index| {
                let allocator = Arc::clone(&allocator);
                let barrier = Arc::clone(&barrier);
                let state_root = state_root.clone();
                let tenant = tenant.clone();
                std::thread::spawn(move || {
                    let sandbox = SandboxId::new(format!("concurrent-{index}"));
                    let layout = OciNetworkLayout::new(&state_root, &tenant, &sandbox);
                    layout.ensure_directories().expect("placement dirs");
                    barrier.wait();
                    place_sandbox_on_block(
                        allocator.as_ref(),
                        &tenant,
                        &layout,
                        &sandbox,
                        config_for_segment,
                    )
                    .expect("concurrent placement should resolve")
                    .network_subnet
                })
            })
            .collect::<Vec<_>>();

        let subnets = threads
            .into_iter()
            .map(|thread| thread.join().expect("placement thread should join"))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            subnets.len(),
            PLACERS,
            "each /30 has one workload address, so concurrent placers need one distinct block each"
        );
        assert_eq!(
            allocator
                .segments_for(&tenant)
                .expect("segment set should read")
                .len(),
            PLACERS,
            "compare-and-swap growth must not append redundant blocks"
        );
    }
}
