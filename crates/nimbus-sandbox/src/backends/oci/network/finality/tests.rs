use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentReservationState, NetworkSegmentAllocator, NetworkSegmentReleaseOutcome,
    PortBindTarget, PortExposure, PortLeasePhase, PortRequestMode,
};
use tempfile::TempDir;

use super::*;
use crate::backends::oci::network::{
    OciSegmentRealization, RecordingSegmentAllocator, allocate_container_ips,
    default_network_attachment_id,
};
use crate::backends::oci::port_lease::{
    OciPortLeaseIntent, inspect_exact, port_lease_request, release_reserved_batch_without_effect,
    reserve,
};

fn fixture(
    label: &str,
) -> (
    TempDir,
    RecordingSegmentAllocator,
    TenantId,
    SandboxId,
    OciNetworkLayout,
    OciNetworkConfig,
    OciSegmentRealization,
) {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = TenantId::new(format!("terminal-{label}")).expect("tenant should validate");
    let sandbox_id = SandboxId::new(format!("terminal-{label}"));
    let allocator = RecordingSegmentAllocator::new(tenant_id.clone(), "127.84.0.0/24", 84);
    let segment = allocator
        .segment_for(&tenant_id)
        .expect("recording segment should inspect");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
    layout
        .ensure_directories()
        .expect("network layout should exist");
    let config = OciNetworkConfig {
        segment_id: segment.segment_id().as_str().to_owned(),
        network_subnet: segment.cidr().to_string(),
        network_name: segment.network_name().to_owned(),
        network_interface: segment.network_interface().to_owned(),
        network_id: segment.network_id().as_str().to_owned(),
        ..OciNetworkConfig::default()
    };
    (
        temp_dir, allocator, tenant_id, sandbox_id, layout, config, segment,
    )
}

fn finality<'a>(
    allocator: &'a RecordingSegmentAllocator,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    layout: &'a OciNetworkLayout,
    config: Option<&'a OciNetworkConfig>,
    port_leases: &'a [nimbus_network::PortLeaseRequest],
) -> TerminalNetworkAuthoritySet<'a> {
    TerminalNetworkAuthoritySet::new(
        allocator,
        tenant_id,
        sandbox_id,
        layout,
        config,
        port_leases,
        None,
    )
}

#[test]
fn terminal_finality_rejects_reserved_port_authority_until_exact_release() {
    let (temp_dir, allocator, tenant_id, sandbox_id, layout, _, _) = fixture("port");
    let request = port_lease_request(
        &tenant_id,
        &sandbox_id,
        "http",
        OciPortLeaseIntent::tenant_published(
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
        ),
        PortRequestMode::Exact(NonZeroU16::new(18_484).expect("port should be non-zero")),
    );
    let (request, _, claim) = reserve(temp_dir.path(), request).expect("port lease should reserve");
    let requests = vec![request.clone()];

    let error = finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        None,
        &requests,
    )
    .require_released()
    .expect_err("reserved port authority must reject terminal finality");
    assert!(
        error.to_string().contains("port lease")
            && error.to_string().contains("Reserved")
            && error
                .to_string()
                .contains(claim.coordinator_attempt().provider_id().as_str()),
        "diagnostic must identify the exact retained authority: {error}"
    );

    release_reserved_batch_without_effect(temp_dir.path(), &requests, &claim)
        .expect("exact never-bound release should succeed");
    assert_eq!(
        inspect_exact(temp_dir.path(), &request)
            .expect("released lease should inspect")
            .phase(),
        PortLeasePhase::Released
    );
    finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        None,
        &requests,
    )
    .require_released()
    .expect("exactly released port authority should admit terminal finality");
}

#[test]
fn terminal_finality_rejects_every_reserved_attachment_phase_until_absent() {
    let (_temp_dir, allocator, tenant_id, sandbox_id, layout, config, segment) =
        fixture("reserved-attachment");
    let attachment_id = default_network_attachment_id(&sandbox_id);
    allocator
        .reserve_attachment_for_coordinator(&tenant_id, &attachment_id, &config.reservation_claim)
        .expect("attachment should reserve");
    allocate_container_ips(&layout, &config, &sandbox_id).expect("IPAM should allocate");
    super::super::ipam::deallocate_container_ips_for_claim(
        &layout,
        &sandbox_id,
        &config.reservation_claim,
    )
    .expect("IPAM should become a terminal witness");

    let error = finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect_err("reserved attachment must reject terminal finality");
    assert!(
        error.to_string().contains("Reserved"),
        "reserved diagnostic should name its exact phase: {error}"
    );

    assert!(matches!(
        allocator
            .release_reserved_attachment_without_effect(
                &tenant_id,
                &attachment_id,
                &config.reservation_claim,
            )
            .expect("reserved release should enter cleanup pending"),
        NetworkSegmentReleaseOutcome::CleanupPending(_)
            | NetworkSegmentReleaseOutcome::AttachmentCleanupPending
    ));
    let error = finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect_err("reservation cleanup pending must reject terminal finality");
    assert!(
        error.to_string().contains("ReservationCleanupPending"),
        "cleanup diagnostic should name its exact phase: {error}"
    );

    let cleanup = allocator
        .finalize_reserved_attachment_without_effect(
            &tenant_id,
            &attachment_id,
            &config.reservation_claim,
        )
        .expect("exact reservation finalization should succeed");
    if let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) = cleanup {
        allocator
            .finalize_release(&cleanup)
            .expect("allocation finalization should succeed");
    }
    assert_eq!(
        allocator
            .inspect_attachment_reservation(&tenant_id, &attachment_id, &config.reservation_claim,)
            .expect("attachment should inspect"),
        NetworkAttachmentReservationState::Absent
    );
    finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect("absent attachment plus released IPAM should admit terminal finality");

    let _ = segment;
}

#[test]
fn terminal_finality_rejects_adopted_and_provider_cleanup_pending_attachments() {
    let (_temp_dir, allocator, tenant_id, sandbox_id, layout, config, segment) =
        fixture("adopted-attachment");
    let attachment_id = default_network_attachment_id(&sandbox_id);
    allocator
        .reserve_attachment_for_coordinator(&tenant_id, &attachment_id, &config.reservation_claim)
        .expect("attachment should reserve");
    allocator
        .bind_reserved_attachment_to_segment(
            &tenant_id,
            &attachment_id,
            segment.segment_id(),
            &config.reservation_claim,
        )
        .expect("attachment should bind to its exact segment");
    allocator
        .adopt_reserved_attachment(&tenant_id, &attachment_id, &config.reservation_claim)
        .expect("attachment should adopt");
    allocate_container_ips(&layout, &config, &sandbox_id).expect("IPAM should allocate");
    super::super::ipam::deallocate_container_ips_for_claim(
        &layout,
        &sandbox_id,
        &config.reservation_claim,
    )
    .expect("IPAM should become a terminal witness");

    let error = finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect_err("adopted attachment must reject terminal finality");
    assert!(
        error.to_string().contains("Adopted"),
        "adopted diagnostic should name its exact phase: {error}"
    );

    allocator
        .quarantine(&tenant_id, &attachment_id, Some(&config.reservation_claim))
        .expect("provider cleanup should quarantine the attachment");
    let error = finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect_err("provider cleanup pending must reject terminal finality");
    assert!(
        error.to_string().contains("ProviderCleanupPending"),
        "provider cleanup diagnostic should name its exact phase: {error}"
    );

    let cleanup = allocator
        .release(&tenant_id, &attachment_id, Some(&config.reservation_claim))
        .expect("provider-released attachment should leave allocation cleanup");
    if let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) = cleanup {
        allocator
            .finalize_release(&cleanup)
            .expect("allocation finalization should succeed");
    }
    finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect("absent attachment plus released IPAM should admit terminal finality");
}

#[test]
fn terminal_finality_rejects_live_ipam_even_after_attachment_release() {
    let (_temp_dir, allocator, tenant_id, sandbox_id, layout, config, _segment) =
        fixture("live-ipam");
    let attachment_id = default_network_attachment_id(&sandbox_id);
    allocator
        .reserve_attachment_for_coordinator(&tenant_id, &attachment_id, &config.reservation_claim)
        .expect("attachment should reserve");
    allocate_container_ips(&layout, &config, &sandbox_id).expect("IPAM should allocate");
    allocator
        .release_reserved_attachment_without_effect(
            &tenant_id,
            &attachment_id,
            &config.reservation_claim,
        )
        .expect("reservation should enter cleanup pending");
    let cleanup = allocator
        .finalize_reserved_attachment_without_effect(
            &tenant_id,
            &attachment_id,
            &config.reservation_claim,
        )
        .expect("attachment authority should become absent");
    if let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) = cleanup {
        allocator
            .finalize_release(&cleanup)
            .expect("allocation finalization should succeed");
    }

    let error = finality(
        &allocator,
        &tenant_id,
        &sandbox_id,
        &layout,
        Some(&config),
        &[],
    )
    .require_released()
    .expect_err("live IPAM must reject terminal finality");
    assert!(
        error.to_string().contains("IPAM")
            && error.to_string().contains("live")
            && error.to_string().contains(
                config
                    .reservation_claim
                    .coordinator_attempt()
                    .provider_id()
                    .as_str()
            ),
        "IPAM diagnostic should identify exact retained authority: {error}"
    );
}
