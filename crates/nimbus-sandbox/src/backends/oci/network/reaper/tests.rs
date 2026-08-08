use super::super::OciNetworkConfig;
use super::*;
use crate::backends::oci::network::{
    SingleNodeSegmentAllocator, default_network_attachment_id, direct_test_ipam_authority,
};
use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkStateStore, NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim,
    NetworkSegmentAllocator,
};
use tempfile::tempdir;

use crate::instance::SandboxId;

fn reservation_claim(attempt: &str) -> NetworkReservationClaim {
    let provider =
        NetworkProviderId::for_registration_key("nimbus-sandbox.network-launch-coordinator.test");
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(provider, format!("attempt:{attempt}"))
            .expect("claim fixture should validate"),
    )
}

#[test]
fn exact_pre_effect_compensation_removes_ipam_before_segment_authority() {
    let dir = tempdir().expect("state root");
    let tenant = TenantId::new("tenant-exact-compensation").expect("tenant fixture");
    let sandbox = SandboxId::new("sandbox-exact-compensation");
    let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("layout should initialize");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let claim = reservation_claim("exact-compensation");
    let attachment_id = super::super::default_network_attachment_id(&sandbox);
    super::super::placement::place_sandbox_on_block(
        &allocator,
        &ipam_authority,
        &tenant,
        &layout,
        &sandbox,
        super::super::placement::OciPlacementAuthority::new(&attachment_id, &claim),
        super::super::placement::OciPlacementProvider::new(
            super::super::provider_locator::OciAttachmentProviderKind::Container,
            |segment, reservation_claim| OciNetworkConfig {
                attachment_id: attachment_id.clone(),
                network_name: segment.network_name().to_owned(),
                network_interface: segment.network_interface().to_owned(),
                network_subnet: segment.cidr().to_string(),
                segment_id: segment.segment_id().as_str().to_owned(),
                reservation_claim: reservation_claim.clone(),
                network_id: segment.network_id().as_str().to_owned(),
                ..OciNetworkConfig::default()
            },
        ),
    )
    .expect("placement should reserve attachment and IPAM");
    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_ok(),
        "placement must create durable IPAM before port reservation"
    );

    release_reserved_network_launch_after_ports(
        ReservedNetworkLaunchAuthority::new(
            &allocator,
            &ipam_authority,
            ReservedNetworkLaunchIdentity::new(&layout, &tenant, &sandbox, &attachment_id, &claim),
            super::super::provider_locator::OciAttachmentProviderKind::Container,
        ),
        Ok(()),
    )
    .expect("exact compensation should complete");

    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_err(),
        "IPAM must be gone before segment authority is reusable"
    );
    assert!(
        allocator
            .inspect_segments(&tenant)
            .expect("segment authority should inspect")
            .is_none(),
        "last exact hold should finalize the tenant segment"
    );
}

#[test]
fn exact_pre_effect_compensation_removes_ipam_while_sibling_hold_remains() {
    let dir = tempdir().expect("state root");
    let tenant = TenantId::new("tenant-sibling-compensation").expect("tenant fixture");
    let cancelled = SandboxId::new("sandbox-cancelled");
    let sibling = SandboxId::new("sandbox-sibling");
    let cancelled_layout = OciNetworkLayout::under_root(dir.path(), &tenant, &cancelled);
    let sibling_layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sibling);
    let ipam_authority = direct_test_ipam_authority(&cancelled_layout);
    cancelled_layout
        .ensure_directories()
        .expect("cancelled layout should initialize");
    sibling_layout
        .ensure_directories()
        .expect("sibling layout should initialize");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let cancelled_claim = reservation_claim("cancelled");
    let sibling_claim = reservation_claim("sibling");
    for (layout, sandbox, claim) in [
        (&cancelled_layout, &cancelled, &cancelled_claim),
        (&sibling_layout, &sibling, &sibling_claim),
    ] {
        let attachment_id = super::super::default_network_attachment_id(sandbox);
        super::super::placement::place_sandbox_on_block(
            &allocator,
            &ipam_authority,
            &tenant,
            layout,
            sandbox,
            super::super::placement::OciPlacementAuthority::new(&attachment_id, claim),
            super::super::placement::OciPlacementProvider::new(
                super::super::provider_locator::OciAttachmentProviderKind::Container,
                |segment, reservation_claim| OciNetworkConfig {
                    attachment_id: attachment_id.clone(),
                    network_name: segment.network_name().to_owned(),
                    network_interface: segment.network_interface().to_owned(),
                    network_subnet: segment.cidr().to_string(),
                    segment_id: segment.segment_id().as_str().to_owned(),
                    reservation_claim: reservation_claim.clone(),
                    network_id: segment.network_id().as_str().to_owned(),
                    ..OciNetworkConfig::default()
                },
            ),
        )
        .expect("placement should reserve attachment and IPAM");
    }
    let cancelled_attachment_id = super::super::default_network_attachment_id(&cancelled);

    release_reserved_network_launch_after_ports(
        ReservedNetworkLaunchAuthority::new(
            &allocator,
            &ipam_authority,
            ReservedNetworkLaunchIdentity::new(
                &cancelled_layout,
                &tenant,
                &cancelled,
                &cancelled_attachment_id,
                &cancelled_claim,
            ),
            super::super::provider_locator::OciAttachmentProviderKind::Container,
        ),
        Ok(()),
    )
    .expect("exact compensation should remove only the cancelled launch");

    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &cancelled_layout, &cancelled,)
            .is_err(),
        "the cancelled launch must not leak IPAM merely because a sibling retains the segment"
    );
    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &sibling_layout, &sibling).is_ok(),
        "the live sibling's IPAM allocation must remain intact"
    );
    assert!(
        !allocator.has_hold(tenant.as_str(), cancelled.as_str()),
        "the exact cancelled attachment must be released"
    );
    assert!(
        allocator.has_hold(tenant.as_str(), sibling.as_str()),
        "the sibling attachment must continue to hold the tenant segment"
    );
    assert!(
        allocator
            .inspect_segments(&tenant)
            .expect("segment authority should inspect")
            .is_some(),
        "a sibling attachment must keep the tenant segment live"
    );
}

#[test]
fn foreign_pre_effect_claim_preserves_ipam_and_segment_authority_byte_for_byte() {
    let dir = tempdir().expect("state root");
    let tenant = TenantId::new("tenant-foreign-compensation").expect("tenant fixture");
    let sandbox = SandboxId::new("sandbox-foreign-compensation");
    let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("layout should initialize");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let winner = reservation_claim("winner");
    let attachment_id = super::super::default_network_attachment_id(&sandbox);
    super::super::placement::place_sandbox_on_block(
        &allocator,
        &ipam_authority,
        &tenant,
        &layout,
        &sandbox,
        super::super::placement::OciPlacementAuthority::new(&attachment_id, &winner),
        super::super::placement::OciPlacementProvider::new(
            super::super::provider_locator::OciAttachmentProviderKind::Container,
            |segment, reservation_claim| OciNetworkConfig {
                attachment_id: attachment_id.clone(),
                network_name: segment.network_name().to_owned(),
                network_interface: segment.network_interface().to_owned(),
                network_subnet: segment.cidr().to_string(),
                segment_id: segment.segment_id().as_str().to_owned(),
                reservation_claim: reservation_claim.clone(),
                network_id: segment.network_id().as_str().to_owned(),
                ..OciNetworkConfig::default()
            },
        ),
    )
    .expect("winner should reserve attachment and IPAM");
    let authority_path = LocalNetworkStateStore::authority_path_for(dir.path());
    let before = std::fs::read(&authority_path).expect("authority should be durable");

    let error = release_reserved_network_launch_after_ports(
        ReservedNetworkLaunchAuthority::new(
            &allocator,
            &ipam_authority,
            ReservedNetworkLaunchIdentity::new(
                &layout,
                &tenant,
                &sandbox,
                &attachment_id,
                &reservation_claim("foreign"),
            ),
            super::super::provider_locator::OciAttachmentProviderKind::Container,
        ),
        Ok(()),
    )
    .expect_err("a foreign coordinator must not compensate the winner");
    assert!(
        error
            .to_string()
            .contains("different launch reservation coordinator"),
        "claim rejection should remain explicit: {error}"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("rejected authority should remain readable"),
        before,
        "claim authentication must precede every IPAM or segment mutation"
    );
    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_ok(),
        "the winning coordinator's IPAM evidence must remain intact"
    );
    assert!(
        allocator.has_hold(tenant.as_str(), sandbox.as_str()),
        "the winning coordinator's reserved attachment must remain intact"
    );
}

#[test]
fn failed_bridge_cleanup_must_fence_segment_from_reuse() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let original_tenant = TenantId::new("tenant-original").expect("tenant should parse");
    let original_sandbox = SandboxId::new("sandbox-original");
    let original_attachment_id = default_network_attachment_id(&original_sandbox);
    let original = allocator
        .acquire(&original_tenant, &original_attachment_id)
        .expect("original segment should allocate");

    let mut surviving_bridges = Vec::new();
    let cleanup_errors = release_network_segment_hold_with(
        &allocator,
        &original_tenant,
        &original_attachment_id,
        None,
        |segment| {
            surviving_bridges.push(segment.network_interface().to_owned());
            Err(SandboxError::OperationFailed {
                message: "forced bridge provider cleanup failure".to_owned(),
            })
        },
    );
    assert_eq!(cleanup_errors.len(), 1);
    assert!(
        cleanup_errors[0]
            .to_string()
            .contains("forced bridge provider cleanup failure")
    );
    assert_eq!(
        surviving_bridges,
        [original.network_interface().to_owned()],
        "the failed provider cleanup leaves the original bridge effect present"
    );

    let replacement = allocator
        .acquire(
            &TenantId::new("tenant-replacement").expect("tenant should parse"),
            &default_network_attachment_id(&SandboxId::new("sandbox-replacement")),
        )
        .expect("replacement segment should allocate");
    assert_ne!(
        replacement.cidr(),
        original.cidr(),
        "a segment with a surviving provider effect must remain fenced from reuse"
    );

    let mut successful_reaps = 0usize;
    let retry_errors = release_network_segment_hold_with(
        &allocator,
        &original_tenant,
        &original_attachment_id,
        None,
        |segment| {
            successful_reaps += 1;
            assert_eq!(segment.segment_id(), original.segment_id());
            Ok(())
        },
    );
    assert!(retry_errors.is_empty(), "cleanup retry should succeed");
    assert_eq!(successful_reaps, 1, "the bridge should be deleted once");

    let recovered = allocator
        .acquire(
            &TenantId::new("tenant-recovered").expect("tenant should parse"),
            &default_network_attachment_id(&SandboxId::new("sandbox-recovered")),
        )
        .expect("confirmed cleanup should make the slot reusable");
    assert_eq!(recovered.cidr(), original.cidr());
    assert_ne!(
        recovered.segment_id(),
        original.segment_id(),
        "reused location must receive a new stable identity"
    );

    let repeated_errors = release_network_segment_hold_with(
        &allocator,
        &original_tenant,
        &original_attachment_id,
        None,
        |_| {
            successful_reaps += 1;
            Ok(())
        },
    );
    assert!(
        repeated_errors.is_empty(),
        "repeated finalization should be idempotent"
    );
    assert_eq!(
        successful_reaps, 1,
        "already-finalized cleanup must not repeat provider deletion"
    );
    let next = allocator
        .acquire(
            &TenantId::new("tenant-next").expect("tenant should parse"),
            &default_network_attachment_id(&SandboxId::new("sandbox-next")),
        )
        .expect("next segment should allocate");
    assert_eq!(
        next.cidr().to_string(),
        "10.0.2.0/24",
        "the recovered slot is owned exactly once, not handed out twice"
    );
}
