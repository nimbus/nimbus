use super::*;
use nimbus_network::{NetworkProviderHandle, NetworkProviderId};
use proptest::prelude::*;
use std::convert::Infallible;
use std::fs;
use tempfile::tempdir;

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("tenant id should parse")
}

fn attachment(id: &str) -> NetworkAttachmentId {
    NetworkAttachmentId::for_workload_attachment(id, super::super::DEFAULT_ATTACHMENT_NAME)
}

fn quarantine_and_release(
    allocator: &SingleNodeSegmentAllocator,
    tenant: &TenantId,
    attachment: &NetworkAttachmentId,
) -> NetworkSegmentReleaseOutcome<OciSegmentRealization> {
    assert_eq!(
        allocator
            .quarantine(tenant, attachment, None)
            .expect("quarantine should succeed"),
        NetworkSegmentQuarantineOutcome::CleanupPending
    );
    allocator
        .release(tenant, attachment, None)
        .expect("release should succeed after quarantine")
}

fn finalize_cleanup(
    allocator: &SingleNodeSegmentAllocator,
    cleanup: &NetworkSegmentCleanup<OciSegmentRealization>,
) {
    assert_eq!(
        allocator
            .finalize_release(cleanup)
            .expect("cleanup finalization should succeed"),
        NetworkSegmentFinalizeOutcome::Released
    );
}

#[test]
fn empty_unclaimed_allocation_quarantine_is_rejected_without_mutation() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let tenant = tenant("tenant-unclaimed-allocation");
    allocator
        .segment_for(&tenant)
        .expect("segment observation should create an unclaimed allocation");
    let before = fs::read(allocator.state_path()).expect("authority should read before quarantine");

    let error = allocator
        .quarantine(&tenant, &attachment("unowned-workload"), None)
        .expect_err("quarantine must require an exact attachment owner");

    assert!(
        error.to_string().contains("no attachment ownership"),
        "the rejection should name the missing attachment authority: {error}"
    );
    assert_eq!(
        fs::read(allocator.state_path()).expect("authority should read after rejection"),
        before,
        "rejected quarantine must leave the unclaimed allocation byte-unchanged"
    );
}

#[test]
fn quarantine_transitions_only_the_exact_held_attachment() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let tenant = tenant("tenant-exact-quarantine");
    let held = attachment("held-workload");
    let wrong = attachment("wrong-workload");
    allocator
        .acquire(&tenant, &held)
        .expect("exact attachment should acquire");

    assert_eq!(
        allocator
            .quarantine(&tenant, &wrong, None)
            .expect("unknown attachment quarantine should remain idempotent"),
        NetworkSegmentQuarantineOutcome::AlreadyReleased
    );
    assert!(
        allocator.has_hold(tenant.as_str(), "held-workload")
            && !allocator.has_pending_hold(tenant.as_str(), "held-workload"),
        "wrong attachment must not quarantine the exact held attachment"
    );
    assert_eq!(
        allocator
            .quarantine(&tenant, &held, None)
            .expect("exact held attachment should quarantine"),
        NetworkSegmentQuarantineOutcome::CleanupPending
    );
    assert!(
        allocator.has_pending_hold(tenant.as_str(), "held-workload"),
        "the exact attachment must become cleanup-pending"
    );
}

#[test]
fn wrong_attachment_quarantine_preserves_a_different_reservation_claim() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let tenant = tenant("tenant-claimed-quarantine");
    let claimed = attachment("claimed-workload");
    let wrong = attachment("wrong-workload");
    let provider =
        NetworkProviderId::for_registration_key("nimbus-sandbox.quarantine-test-coordinator");
    let claim = NetworkReservationClaim::new(
        NetworkProviderHandle::new(provider, "attempt:claimed-workload")
            .expect("claim fixture should validate"),
    );
    allocator
        .reserve_attachment_for_coordinator(&tenant, &claimed, &claim)
        .expect("claimed attachment should reserve");
    let selected = allocator
        .segments_for(&tenant)
        .expect("tenant segment should inspect")
        .into_iter()
        .next()
        .expect("reservation should allocate the primary segment");
    allocator
        .bind_reserved_attachment_to_segment(&tenant, &claimed, selected.segment_id(), &claim)
        .expect("test placement should bind the reserved attachment");

    assert_eq!(
        allocator
            .quarantine(&tenant, &wrong, None)
            .expect("unknown attachment quarantine should remain idempotent"),
        NetworkSegmentQuarantineOutcome::AlreadyReleased
    );
    allocator
        .adopt_reserved_attachment(&tenant, &claimed, &claim)
        .expect("wrong attachment must not mutate a different coordinator's reservation");
}

#[test]
fn durable_cleanup_authority_rejects_incomplete_persisted_fencing() {
    let cases = [
        (Some("10.31.0.0/16"), None, false),
        (None, Some(NetworkLeaseEpoch::new(31)), false),
        (None, None, true),
    ];
    for (index, (cidr, epoch, include_tenant)) in cases.into_iter().enumerate() {
        let dir = tempdir().expect("temp dir");
        let store = LocalNetworkStateStore::open(dir.path()).expect("local authority should open");
        store
            .transaction(
                &NetworkStatePartition::SegmentAllocations,
                |state: &mut SegmentState| {
                    state.supernet_cidr = cidr.map(str::to_owned);
                    state.supernet_epoch = epoch;
                    if include_tenant {
                        state.tenants.insert(
                            "tenant-corrupt".to_owned(),
                            TenantEntry {
                                blocks: vec![SegmentBlock {
                                    local_slot: 0,
                                    segment_id: NetworkSegmentId::generate(),
                                }],
                                attachments: BTreeMap::new(),
                                allocation_cleanup_pending: true,
                                pending_reservation_cleanup_claim: None,
                            },
                        );
                    }
                    Ok::<_, Infallible>(())
                },
            )
            .expect("fixture should persist a checksum-valid malformed payload");
        let authority_path = LocalNetworkStateStore::authority_path_for(dir.path());
        let before = fs::read(&authority_path).expect("fixture authority should exist");

        let error = match DurableSegmentCleanupAuthority::open(dir.path(), DEFAULT_TENANT_PREFIX) {
            Ok(_) => {
                panic!("cleanup must fail closed when durable CIDR/epoch fencing is incomplete")
            }
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("incomplete durable cleanup fencing"),
            "case {index} must name the invalid cleanup fence: {error}"
        );
        assert_eq!(
            fs::read(&authority_path).expect("rejected open must preserve authority"),
            before,
            "case {index} must fail without mutating the malformed authority"
        );
    }
}

fn grow(allocator: &SingleNodeSegmentAllocator, tenant: &TenantId) -> OciSegmentRealization {
    let observed = allocator
        .segments_for(tenant)
        .expect("observed segment set should resolve");
    match allocator
        .grow_block_if_current(tenant, &observed)
        .expect("growth should resolve")
    {
        NetworkSegmentGrowth::Grown(segment) => segment,
        NetworkSegmentGrowth::ObservationStale => {
            panic!("single-threaded fixture observation must remain current")
        }
    }
}

#[test]
fn two_tenants_get_distinct_subnets_bridges_and_ids() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let a = allocator
        .segment_for(&tenant("tenant-a"))
        .expect("assign a");
    let b = allocator
        .segment_for(&tenant("tenant-b"))
        .expect("assign b");

    assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
    assert_eq!(b.cidr().to_string(), "10.0.1.0/24");
    assert!(
        !a.cidr().overlaps(&b.cidr()),
        "tenant subnets must not overlap"
    );
    assert_ne!(a.network_interface(), b.network_interface());
    assert_ne!(a.network_id().as_str(), b.network_id().as_str());
}

#[test]
fn assign_is_idempotent_per_tenant() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let first = allocator.segment_for(&tenant("tenant-a")).expect("assign");
    let again = allocator
        .segment_for(&tenant("tenant-a"))
        .expect("re-assign");
    assert_eq!(first.cidr(), again.cidr());
    assert_eq!(first.segment_id(), again.segment_id());
    assert_eq!(first.network_id().as_str(), again.network_id().as_str());
}

#[test]
fn node_supernets_mint_distinct_stable_segment_ids_at_the_same_local_slot() {
    let node_a_root = tempdir().expect("node A temp dir");
    let node_b_root = tempdir().expect("node B temp dir");
    let node_a = SingleNodeSegmentAllocator::for_node_supernet(
        node_a_root.path(),
        "10.10.0.0/16",
        DEFAULT_TENANT_PREFIX,
    )
    .expect("node A allocator");
    let node_b = SingleNodeSegmentAllocator::for_node_supernet(
        node_b_root.path(),
        "10.20.0.0/16",
        DEFAULT_TENANT_PREFIX,
    )
    .expect("node B allocator");

    let segment_a = node_a
        .segment_for(&tenant("tenant-shared"))
        .expect("node A segment");
    let segment_b = node_b
        .segment_for(&tenant("tenant-shared"))
        .expect("node B segment");

    assert_eq!(segment_a.network_interface(), "nb-0");
    assert_eq!(segment_b.network_interface(), "nb-0");
    assert_eq!(segment_a.cidr().to_string(), "10.10.0.0/24");
    assert_eq!(segment_b.cidr().to_string(), "10.20.0.0/24");
    assert_ne!(
        segment_a.segment_id(),
        segment_b.segment_id(),
        "global segment identity must not alias merely because two nodes use local slot zero"
    );

    let restarted_a = SingleNodeSegmentAllocator::for_node_supernet(
        node_a_root.path(),
        "10.10.0.0/16",
        DEFAULT_TENANT_PREFIX,
    )
    .expect("restarted node A allocator")
    .segment_for(&tenant("tenant-shared"))
    .expect("restarted node A segment");
    let restarted_b = SingleNodeSegmentAllocator::for_node_supernet(
        node_b_root.path(),
        "10.20.0.0/16",
        DEFAULT_TENANT_PREFIX,
    )
    .expect("restarted node B allocator")
    .segment_for(&tenant("tenant-shared"))
    .expect("restarted node B segment");

    assert_eq!(restarted_a.segment_id(), segment_a.segment_id());
    assert_eq!(restarted_b.segment_id(), segment_b.segment_id());
    assert_eq!(restarted_a.tenant_id(), &tenant("tenant-shared"));
    assert_eq!(restarted_b.tenant_id(), &tenant("tenant-shared"));
    assert_eq!(restarted_a.lease_epoch(), NetworkLeaseEpoch::new(0));
    assert_eq!(restarted_b.lease_epoch(), NetworkLeaseEpoch::new(0));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn disjoint_node_supernets_never_alias_segment_identity_and_restart_stably(
        node_a_octet in 1u8..=120,
        node_b_octet in 121u8..=240,
        tenant_count in 1usize..=6,
        epoch_a_raw in any::<u16>(),
        epoch_b_raw in any::<u16>(),
    ) {
        let node_a_root = tempdir().expect("node A temp dir");
        let node_b_root = tempdir().expect("node B temp dir");
        let epoch_a = NetworkLeaseEpoch::new(u64::from(epoch_a_raw));
        let epoch_b = NetworkLeaseEpoch::new(u64::from(epoch_b_raw));
        let supernet_a = Cidr::parse(&format!("10.{node_a_octet}.0.0/16"))
            .expect("generated node A super-net should parse");
        let supernet_b = Cidr::parse(&format!("10.{node_b_octet}.0.0/16"))
            .expect("generated node B super-net should parse");
        let node_a = SingleNodeSegmentAllocator::new(
            node_a_root.path(),
            Some(InstalledSuperNet {
                cidr: supernet_a,
                epoch: epoch_a,
            }),
            DEFAULT_TENANT_PREFIX,
        )
        .expect("node A allocator");
        let node_b = SingleNodeSegmentAllocator::new(
            node_b_root.path(),
            Some(InstalledSuperNet {
                cidr: supernet_b,
                epoch: epoch_b,
            }),
            DEFAULT_TENANT_PREFIX,
        )
        .expect("node B allocator");
        let mut every_id = BTreeSet::new();
        let mut expected = Vec::with_capacity(tenant_count);

        for index in 0..tenant_count {
            let tenant = tenant(&format!("tenant-{index}"));
            let segment_a = node_a
                .segment_for(&tenant)
                .expect("node A segment should allocate");
            let segment_b = node_b
                .segment_for(&tenant)
                .expect("node B segment should allocate");

            prop_assert_eq!(
                segment_a.network_interface(),
                segment_b.network_interface(),
                "same local slot deliberately produces the same provider-local interface name"
            );
            prop_assert!(!segment_a.cidr().overlaps(&segment_b.cidr()));
            prop_assert_ne!(segment_a.segment_id(), segment_b.segment_id());
            prop_assert_eq!(segment_a.lease_epoch(), epoch_a);
            prop_assert_eq!(segment_b.lease_epoch(), epoch_b);
            prop_assert!(every_id.insert(segment_a.segment_id().clone()));
            prop_assert!(every_id.insert(segment_b.segment_id().clone()));
            expected.push((
                tenant,
                segment_a.segment_id().clone(),
                segment_b.segment_id().clone(),
            ));
        }

        let restarted_a = SingleNodeSegmentAllocator::new(
            node_a_root.path(),
            Some(InstalledSuperNet {
                cidr: supernet_a,
                epoch: epoch_a,
            }),
            DEFAULT_TENANT_PREFIX,
        )
        .expect("restarted node A allocator");
        let restarted_b = SingleNodeSegmentAllocator::new(
            node_b_root.path(),
            Some(InstalledSuperNet {
                cidr: supernet_b,
                epoch: epoch_b,
            }),
            DEFAULT_TENANT_PREFIX,
        )
        .expect("restarted node B allocator");

        for (tenant, expected_a, expected_b) in expected {
            let actual_a = restarted_a
                .segment_for(&tenant)
                .expect("node A identity should survive restart");
            let actual_b = restarted_b
                .segment_for(&tenant)
                .expect("node B identity should survive restart");
            prop_assert_eq!(actual_a.segment_id(), &expected_a);
            prop_assert_eq!(actual_b.segment_id(), &expected_b);
            prop_assert_eq!(actual_a.lease_epoch(), epoch_a);
            prop_assert_eq!(actual_b.lease_epoch(), epoch_b);
        }
    }
}

#[test]
#[ignore = "NNC0.9 explicit allocation-scale characterization"]
fn durable_segment_assignment_scale_baseline() {
    const SAMPLE_COUNT: usize = 21;

    for existing_tenants in [0usize, 64, 256, 1_024] {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::for_node_supernet(
            dir.path(),
            "10.0.0.0/8",
            DEFAULT_TENANT_PREFIX,
        )
        .expect("baseline allocator should accept the node super-net");

        let seed_started = std::time::Instant::now();
        for index in 0..existing_tenants {
            allocator
                .segment_for(&tenant(&format!("baseline-seed-{index:04}")))
                .expect("baseline seed assignment should fit the super-net");
        }
        let seed_elapsed_ns = seed_started.elapsed().as_nanos();

        let mut samples_ns = Vec::with_capacity(SAMPLE_COUNT);
        for sample in 0..SAMPLE_COUNT {
            let expected_index = existing_tenants + sample;
            let expected_cidr = Cidr::parse("10.0.0.0/8")
                .expect("baseline super-net should parse")
                .nth_subnet(
                    DEFAULT_TENANT_PREFIX,
                    u64::try_from(expected_index).expect("baseline index fits u64"),
                )
                .expect("baseline subnet should fit");
            let started = std::time::Instant::now();
            let segment = allocator
                .segment_for(&tenant(&format!("baseline-sample-{sample:02}")))
                .expect("baseline sample assignment should fit the super-net");
            samples_ns.push(started.elapsed().as_nanos());
            assert_eq!(
                segment.cidr(),
                expected_cidr,
                "durable allocation must remain lowest-free and collision-free at scale"
            );
        }
        samples_ns.sort_unstable();
        let p95_index = (SAMPLE_COUNT * 95).div_ceil(100) - 1;

        println!(
            "NNC0.9 segment-allocation-baseline existing_tenants={existing_tenants} seed_total_ns={seed_elapsed_ns} samples={SAMPLE_COUNT} median_ns={} p95_ns={}",
            samples_ns[SAMPLE_COUNT / 2],
            samples_ns[p95_index]
        );
    }
}

#[test]
fn refcount_frees_the_index_only_after_the_last_sandbox_releases() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let a = tenant("tenant-a");

    // Two sandboxes of tenant-a hold the same segment.
    let s1 = allocator
        .acquire(&a, &attachment("sb-1"))
        .expect("acquire sb-1");
    let s2 = allocator
        .acquire(&a, &attachment("sb-2"))
        .expect("acquire sb-2");
    assert_eq!(s1.cidr().to_string(), "10.0.0.0/24");
    assert_eq!(s1.cidr(), s2.cidr(), "same tenant shares one segment");

    // Releasing one leaves the tenant live — the bridge stays, index held.
    assert!(matches!(
        quarantine_and_release(&allocator, &a, &attachment("sb-1")),
        NetworkSegmentReleaseOutcome::StillLive
    ));
    // A fresh tenant does NOT get tenant-a's still-held index.
    let b = allocator
        .acquire(&tenant("tenant-b"), &attachment("sb-b"))
        .expect("acquire b");
    assert_eq!(b.cidr().to_string(), "10.0.1.0/24");

    // Releasing the LAST sandbox quarantines the allocation; it remains
    // unavailable until provider cleanup is identity-fenced and finalized.
    let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) =
        quarantine_and_release(&allocator, &a, &attachment("sb-2"))
    else {
        panic!("the last release should enter cleanup pending");
    };
    let fenced = allocator
        .acquire(&tenant("tenant-fenced"), &attachment("sb-fenced"))
        .expect("a distinct local slot should remain available");
    assert_eq!(fenced.cidr().to_string(), "10.0.2.0/24");
    finalize_cleanup(&allocator, &cleanup);
    // The freed lowest index (10.0.0.0/24) is handed to the next new tenant.
    let c = allocator
        .acquire(&tenant("tenant-c"), &attachment("sb-c"))
        .expect("acquire c");
    assert_eq!(c.cidr().to_string(), "10.0.0.0/24");
    assert_ne!(
        c.segment_id(),
        s1.segment_id(),
        "reusing a cleaned local slot must mint a new global allocation identity"
    );

    // Releasing an unknown sandbox is idempotent.
    assert!(matches!(
        allocator
            .release(&tenant("nobody"), &attachment("ghost"), None)
            .expect("release ghost"),
        NetworkSegmentReleaseOutcome::AlreadyReleased
    ));
}

#[test]
fn cleanup_pending_survives_restart_and_reuses_only_after_fenced_finalize() {
    let dir = tempdir().expect("temp dir");
    let tenant_a = tenant("tenant-a");
    let attachment_a = attachment("sb-a");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let original = allocator
        .acquire(&tenant_a, &attachment_a)
        .expect("original allocation should succeed");
    assert_eq!(original.cidr().to_string(), "10.0.0.0/24");
    assert_eq!(
        allocator
            .quarantine(&tenant_a, &attachment_a, None)
            .expect("quarantine should persist"),
        NetworkSegmentQuarantineOutcome::CleanupPending
    );

    let restarted = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let reacquire_error = restarted
        .acquire(&tenant_a, &attachment_a)
        .expect_err("a quarantined attachment must not reactivate after restart");
    assert!(
        reacquire_error.to_string().contains("cleanup-pending"),
        "the refusal must identify the durable quarantine: {reacquire_error}"
    );
    let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) = restarted
        .release(&tenant_a, &attachment_a, None)
        .expect("confirmed detach should release the pending hold")
    else {
        panic!("last hold should leave allocation cleanup pending");
    };

    let restarted_pending = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let other = restarted_pending
        .acquire(&tenant("tenant-other"), &attachment("sb-other"))
        .expect("another slot should remain allocatable");
    assert_eq!(
        other.cidr().to_string(),
        "10.0.1.0/24",
        "the cleanup-pending location must remain unavailable after restart"
    );
    finalize_cleanup(&restarted_pending, &cleanup);
    assert_eq!(
        restarted_pending
            .finalize_release(&cleanup)
            .expect("repeated finalization should be idempotent"),
        NetworkSegmentFinalizeOutcome::AlreadyReleased
    );

    let restarted_released = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let recovered = restarted_released
        .acquire(&tenant("tenant-recovered"), &attachment("sb-recovered"))
        .expect("finalized location should be reusable");
    assert_eq!(recovered.cidr(), original.cidr());
    assert_ne!(recovered.segment_id(), original.segment_id());
}

#[test]
fn release_without_durable_quarantine_fails_without_mutation() {
    let dir = tempdir().expect("temp dir");
    let tenant_a = tenant("tenant-a");
    let attachment_a = attachment("sb-a");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    allocator
        .acquire(&tenant_a, &attachment_a)
        .expect("allocation should succeed");
    let before =
        fs::read(allocator.state_path()).expect("authority should read before invalid release");

    let error = allocator
        .release(&tenant_a, &attachment_a, None)
        .expect_err("release must not bypass durable quarantine");

    assert!(
        error.to_string().contains("durably quarantined"),
        "the refusal must identify the missing lifecycle phase: {error}"
    );
    assert_eq!(
        fs::read(allocator.state_path()).expect("authority should read after invalid release"),
        before,
        "an out-of-order release must not change any authority byte"
    );
    assert!(allocator.has_hold(tenant_a.as_str(), "sb-a"));
    assert!(!allocator.has_pending_hold(tenant_a.as_str(), "sb-a"));
}

#[test]
fn wrong_or_stale_cleanup_fence_cannot_release_an_allocation() {
    let dir = tempdir().expect("temp dir");
    let tenant_a = tenant("tenant-a");
    let attachment_a = attachment("sb-a");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    allocator
        .acquire(&tenant_a, &attachment_a)
        .expect("original allocation should succeed");
    let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) =
        quarantine_and_release(&allocator, &tenant_a, &attachment_a)
    else {
        panic!("last release should enter cleanup pending");
    };

    let wrong = NetworkSegmentCleanup::new(
        tenant_a.clone(),
        vec![NetworkSegmentId::generate()],
        cleanup.lease_epoch(),
        cleanup.segments().to_vec(),
    );
    let before_wrong =
        fs::read(allocator.state_path()).expect("authority should read before wrong proof");
    let wrong_error = allocator
        .finalize_release(&wrong)
        .expect_err("wrong segment identity must not finalize");
    assert!(wrong_error.to_string().contains("stale"));
    assert_eq!(
        fs::read(allocator.state_path()).expect("authority should read after wrong proof"),
        before_wrong,
        "rejected cleanup proof must not mutate durable authority"
    );

    finalize_cleanup(&allocator, &cleanup);
    let replacement = allocator
        .acquire(&tenant_a, &attachment("sb-replacement"))
        .expect("replacement allocation should succeed");
    let before_stale =
        fs::read(allocator.state_path()).expect("authority should read before stale proof");
    let stale_error = allocator
        .finalize_release(&cleanup)
        .expect_err("old cleanup must not release a replacement allocation");
    assert!(
        stale_error.to_string().contains("not ready") || stale_error.to_string().contains("stale")
    );
    assert_eq!(
        fs::read(allocator.state_path()).expect("authority should read after stale proof"),
        before_stale,
        "stale callback must not mutate the replacement allocation"
    );
    assert_ne!(
        replacement.segment_id(),
        cleanup
            .segment_ids()
            .first()
            .expect("cleanup has one segment")
    );
}

#[test]
fn concurrent_finalization_releases_one_identity_exactly_once() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let dir = tempdir().expect("temp dir");
    let root = Arc::new(dir.path().to_path_buf());
    let tenant_a = tenant("tenant-a");
    let attachment_a = attachment("sb-a");
    let allocator = SingleNodeSegmentAllocator::single_node_default(root.as_ref());
    allocator
        .acquire(&tenant_a, &attachment_a)
        .expect("original allocation should succeed");
    let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) =
        quarantine_and_release(&allocator, &tenant_a, &attachment_a)
    else {
        panic!("last release should enter cleanup pending");
    };

    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            let cleanup = cleanup.clone();
            thread::spawn(move || {
                let allocator = SingleNodeSegmentAllocator::single_node_default(root.as_ref());
                barrier.wait();
                allocator
                    .finalize_release(&cleanup)
                    .expect("identity-fenced finalization should be idempotent")
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("finalizer should not panic"))
        .collect();

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == NetworkSegmentFinalizeOutcome::Released)
            .count(),
        1,
        "exactly one concurrent finalizer may release the allocation"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == NetworkSegmentFinalizeOutcome::AlreadyReleased)
            .count(),
        1,
        "the losing finalizer must observe idempotent completion"
    );

    let allocator = SingleNodeSegmentAllocator::single_node_default(root.as_ref());
    let first = allocator
        .acquire(&tenant("tenant-first"), &attachment("sb-first"))
        .expect("freed slot should be reusable once");
    let second = allocator
        .acquire(&tenant("tenant-second"), &attachment("sb-second"))
        .expect("next tenant should receive a different slot");
    assert_eq!(first.cidr().to_string(), "10.0.0.0/24");
    assert_eq!(second.cidr().to_string(), "10.0.1.0/24");
    assert_ne!(first.segment_id(), second.segment_id());
}

#[test]
fn exhaustion_fails_closed() {
    let dir = tempdir().expect("temp dir");
    // A /30 super-net carved into /30 tenant subnets holds exactly one tenant.
    let supernet = InstalledSuperNet {
        cidr: Cidr::parse("10.9.0.0/30").unwrap(),
        epoch: NetworkLeaseEpoch::new(0),
    };
    let allocator = SingleNodeSegmentAllocator::new(dir.path(), Some(supernet), 30)
        .expect("local network store should open");
    allocator
        .segment_for(&tenant("t0"))
        .expect("first tenant fits");
    let error = allocator
        .segment_for(&tenant("t1"))
        .expect_err("second tenant must not fit a single-child super-net");
    assert!(
        format!("{error}").contains("pool exhausted"),
        "exhaustion must fail closed, got: {error}"
    );
}

#[test]
fn assign_fails_closed_until_a_supernet_is_installed() {
    let dir = tempdir().expect("temp dir");
    let mut allocator = SingleNodeSegmentAllocator::new(dir.path(), None, DEFAULT_TENANT_PREFIX)
        .expect("local network store should open");
    let error = allocator
        .segment_for(&tenant("tenant-a"))
        .expect_err("no super-net installed must fail closed");
    assert!(
        format!("{error}").contains("unassigned"),
        "must fail closed as unassigned, got: {error}"
    );
    allocator.install_supernet(InstalledSuperNet {
        cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
        epoch: NetworkLeaseEpoch::new(0),
    });
    let seg = allocator
        .segment_for(&tenant("tenant-a"))
        .expect("assign after install");
    assert_eq!(seg.cidr().to_string(), "10.0.0.0/24");
}

#[test]
fn a_stale_epoch_carve_fails_closed_on_load() {
    let dir = tempdir().expect("temp dir");
    let epoch0 = SingleNodeSegmentAllocator::new(
        dir.path(),
        Some(InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
            epoch: NetworkLeaseEpoch::new(0),
        }),
        DEFAULT_TENANT_PREFIX,
    )
    .expect("epoch 0 store should open");
    epoch0
        .segment_for(&tenant("tenant-a"))
        .expect("carve at epoch 0");
    // A later allocator with a bumped epoch must refuse the stale state.
    let epoch1 = SingleNodeSegmentAllocator::new(
        dir.path(),
        Some(InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
            epoch: NetworkLeaseEpoch::new(1),
        }),
        DEFAULT_TENANT_PREFIX,
    )
    .expect("epoch 1 store should open");
    let error = epoch1
        .segment_for(&tenant("tenant-b"))
        .expect_err("stale-epoch state must fail closed");
    assert!(
        format!("{error}").contains("epoch"),
        "must fail closed on epoch mismatch, got: {error}"
    );
}

#[test]
fn stale_epoch_rejects_every_create_and_growth_entrypoint_without_mutation() {
    let dir = tempdir().expect("temp dir");
    let existing_tenant = tenant("tenant-existing");
    let existing_attachment = attachment("sandbox-existing");
    let old_epoch = SingleNodeSegmentAllocator::new(
        dir.path(),
        Some(InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
            epoch: NetworkLeaseEpoch::new(7),
        }),
        DEFAULT_TENANT_PREFIX,
    )
    .expect("old-epoch allocator should open");
    old_epoch
        .acquire(&existing_tenant, &existing_attachment)
        .expect("old-epoch allocation should succeed");
    let old_observation = old_epoch
        .segments_for(&existing_tenant)
        .expect("old-epoch observation should resolve");

    let stale = SingleNodeSegmentAllocator::new(
        dir.path(),
        Some(InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
            epoch: NetworkLeaseEpoch::new(8),
        }),
        DEFAULT_TENANT_PREFIX,
    )
    .expect("new-epoch allocator should open");
    let before = fs::read(stale.state_path()).expect("authority should read before attempts");

    let errors = [
        stale
            .segment_for(&tenant("tenant-segment-for"))
            .expect_err("segment_for must reject stale state"),
        stale
            .segments_for(&existing_tenant)
            .expect_err("segments_for must reject stale state"),
        stale
            .acquire(&tenant("tenant-acquire"), &attachment("sandbox-acquire"))
            .expect_err("acquire must reject stale state"),
        stale
            .grow_block_if_current(&existing_tenant, &old_observation)
            .expect_err("growth must reject stale state"),
    ];
    for error in errors {
        assert!(
            error.to_string().contains("epoch"),
            "every stale create/grow entrypoint must fail at the epoch fence: {error}"
        );
    }

    let after = fs::read(stale.state_path()).expect("authority should read after attempts");
    assert_eq!(
        after, before,
        "rejected stale-epoch create/grow attempts must not mutate durable authority"
    );
}

#[test]
fn concurrent_acquire_release_across_threads_stays_consistent_under_the_lock() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempdir().expect("temp dir");
    let root = Arc::new(dir.path().to_path_buf());
    // 8 threads, each a distinct tenant, contending on the shared on-disk
    // shared network authority: acquire a sole sandbox then
    // release it, which must drain the tenant.
    let handles: Vec<_> = (0..8u32)
        .map(|i| {
            let root = Arc::clone(&root);
            thread::spawn(move || {
                let allocator = SingleNodeSegmentAllocator::single_node_default(&root);
                let tenant = tenant(&format!("t-{i}"));
                let attachment = attachment(&format!("sb-{i}"));
                let segment = allocator.acquire(&tenant, &attachment).expect("acquire");
                assert!(segment.cidr().to_string().starts_with("10.0."));
                let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) =
                    quarantine_and_release(&allocator, &tenant, &attachment)
                else {
                    return false;
                };
                finalize_cleanup(&allocator, &cleanup);
                true
            })
        })
        .collect();
    for handle in handles {
        assert!(handle.join().expect("thread should not panic"));
    }
    // Every tenant drained, so the freed lowest index is reused: the next new
    // tenant gets 10.0.0.0/24 (no leaked reservations under contention).
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let segment = allocator
        .acquire(&tenant("after"), &attachment("sb"))
        .expect("acquire after drain");
    assert_eq!(segment.cidr().to_string(), "10.0.0.0/24");
}

#[test]
fn reconcile_orphans_quarantines_leaked_holds_without_reusing_allocations() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    // tenant-a holds two sandboxes; sb-1 will be a crash-leaked hold, sb-2 live.
    allocator
        .acquire(&tenant("tenant-a"), &attachment("sb-1"))
        .expect("acquire a/1");
    allocator
        .acquire(&tenant("tenant-a"), &attachment("sb-2"))
        .expect("acquire a/2");
    // tenant-b holds one sandbox, fully crash-leaked (nothing live).
    let b = allocator
        .acquire(&tenant("tenant-b"), &attachment("sb-b"))
        .expect("acquire b");
    assert_eq!(b.cidr().to_string(), "10.0.1.0/24");

    // Only tenant-a/sb-2 is actually live at startup.
    let mut live = BTreeSet::new();
    live.insert((tenant("tenant-a"), attachment("sb-2")));
    let quarantined = allocator.reconcile_orphans(&live).expect("reconcile");

    // tenant-b is fully orphaned, but netns absence alone is not provider
    // deletion proof: its segment is returned as quarantined and remains held.
    assert_eq!(
        quarantined.len(),
        1,
        "only the fully-orphaned tenant is quarantined"
    );
    assert_eq!(quarantined[0].cidr().to_string(), "10.0.1.0/24");
    assert!(
        allocator.has_hold("tenant-b", "sb-b") && allocator.has_pending_hold("tenant-b", "sb-b"),
        "the uncertain orphan must retain a pending durable hold"
    );
    // tenant-a keeps index 0 and tenant-b's quarantined index 1 remains
    // unavailable, so the next tenant receives index 2.
    let c = allocator
        .acquire(&tenant("tenant-c"), &attachment("sb-c"))
        .expect("acquire c");
    assert_eq!(c.cidr().to_string(), "10.0.2.0/24");
    // tenant-a's still-live sandbox keeps its original segment.
    let a = allocator
        .acquire(&tenant("tenant-a"), &attachment("sb-2"))
        .expect("re-acquire a/2");
    assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
    assert!(
        allocator.has_pending_hold("tenant-a", "sb-1"),
        "the partial orphan is quarantined without disrupting its live sibling"
    );
}

#[test]
fn grow_block_appends_a_distinct_block_and_never_collides_across_tenants() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let a = tenant("tenant-a");
    // tenant-a's primary block = index 0 (10.0.0.0/24).
    let a0 = allocator
        .acquire(&a, &attachment("sb-a"))
        .expect("acquire a");
    assert_eq!(a0.cidr().to_string(), "10.0.0.0/24");
    // Grow tenant-a: a second, distinct block/bridge at index 1.
    let a1 = grow(&allocator, &a);
    assert_eq!(a1.cidr().to_string(), "10.0.1.0/24");
    assert_ne!(a0.network_interface(), a1.network_interface());
    assert_ne!(a0.network_id().as_str(), a1.network_id().as_str());
    // The M1 guard: a DIFFERENT tenant must NEVER be handed tenant-a's grown
    // index 1 — the unioned lowest-free scan skips it to index 2.
    let b = allocator
        .acquire(&tenant("tenant-b"), &attachment("sb-b"))
        .expect("acquire b");
    assert_eq!(b.cidr().to_string(), "10.0.2.0/24");
}

#[test]
fn growth_fence_rejects_same_count_remove_and_recreate_aba() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let tenant = tenant("tenant-a");
    let old_attachment = attachment("old");
    allocator
        .acquire(&tenant, &old_attachment)
        .expect("old allocation should resolve");
    let stale_observation = allocator
        .segments_for(&tenant)
        .expect("old segment set should resolve");

    let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) =
        quarantine_and_release(&allocator, &tenant, &old_attachment)
    else {
        panic!("old allocation should enter cleanup pending");
    };
    finalize_cleanup(&allocator, &cleanup);
    let replacement = allocator
        .acquire(&tenant, &attachment("replacement"))
        .expect("replacement allocation should resolve");
    assert_ne!(
        stale_observation[0].segment_id(),
        replacement.segment_id(),
        "remove-and-recreate must mint a distinct stable identity even when the local slot is reused"
    );

    assert!(matches!(
        allocator
            .grow_block_if_current(&tenant, &stale_observation)
            .expect("stale growth should resolve"),
        NetworkSegmentGrowth::ObservationStale
    ));
    assert_eq!(
        allocator
            .segments_for(&tenant)
            .expect("replacement segment set should resolve")
            .len(),
        1,
        "an ABA-stale observation must not grow the replacement allocation"
    );
}

#[test]
fn two_growers_from_one_observation_append_exactly_one_block() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let tenant = tenant("tenant-a");
    let observation = allocator
        .segments_for(&tenant)
        .expect("initial segment set should resolve");

    assert!(matches!(
        allocator
            .grow_block_if_current(&tenant, &observation)
            .expect("first growth should resolve"),
        NetworkSegmentGrowth::Grown(_)
    ));
    assert!(matches!(
        allocator
            .grow_block_if_current(&tenant, &observation)
            .expect("second growth should resolve"),
        NetworkSegmentGrowth::ObservationStale
    ));
    assert_eq!(
        allocator
            .segments_for(&tenant)
            .expect("grown segment set should resolve")
            .len(),
        2,
        "only the first caller with a current observation may append"
    );
}

#[test]
fn draining_a_multi_block_tenant_returns_every_block_to_reap() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let a = tenant("tenant-a");
    allocator.acquire(&a, &attachment("sb-1")).expect("acquire");
    grow(&allocator, &a);
    grow(&allocator, &a);

    // The sole sandbox's release drains the tenant and returns ALL 3 block
    // bridges so the caller reaps every one.
    let NetworkSegmentReleaseOutcome::CleanupPending(cleanup) =
        quarantine_and_release(&allocator, &a, &attachment("sb-1"))
    else {
        panic!("expected the last release to enter cleanup pending");
    };
    let subnets: Vec<String> = cleanup
        .segments()
        .iter()
        .map(|s| s.cidr().to_string())
        .collect();
    assert_eq!(subnets, ["10.0.0.0/24", "10.0.1.0/24", "10.0.2.0/24"]);
    finalize_cleanup(&allocator, &cleanup);
    // All 3 indices are freed, so the next new tenant reuses index 0.
    let c = allocator
        .acquire(&tenant("tenant-c"), &attachment("sb-c"))
        .expect("acquire c");
    assert_eq!(c.cidr().to_string(), "10.0.0.0/24");
}

#[test]
fn grow_block_fails_closed_at_pool_exhaustion() {
    let dir = tempdir().expect("temp dir");
    // A /24 super-net carved into /24 blocks holds exactly ONE block.
    let supernet = InstalledSuperNet {
        cidr: Cidr::parse("10.9.0.0/24").unwrap(),
        epoch: NetworkLeaseEpoch::new(0),
    };
    let allocator = SingleNodeSegmentAllocator::new(dir.path(), Some(supernet), 24)
        .expect("local network store should open");
    let t = tenant("t0");
    allocator
        .acquire(&t, &attachment("sb"))
        .expect("primary block fits");
    let observed = allocator
        .segments_for(&t)
        .expect("observed segment set should resolve");
    let error = allocator
        .grow_block_if_current(&t, &observed)
        .expect_err("a second block must not fit a single-child super-net");
    assert!(
        format!("{error}").contains("pool exhausted"),
        "grow must fail closed on exhaustion, got: {error}"
    );
}

#[test]
fn torn_segment_state_error_must_name_the_authority_path() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    allocator
        .acquire(&tenant("tenant-original"), &attachment("sandbox-original"))
        .expect("original segment should allocate");
    let state_path = allocator.state_path();
    fs::write(state_path, b"{").expect("torn state should be installed");

    let error = match SingleNodeSegmentAllocator::for_node_supernet(
        dir.path(),
        DEFAULT_NODE_SUPERNET,
        DEFAULT_TENANT_PREFIX,
    ) {
        Ok(_) => panic!("torn segment authority must fail closed during startup"),
        Err(error) => error,
    };
    let rendered = error.to_string();
    assert!(
        rendered.contains("network authority state") && rendered.contains("corrupt"),
        "the failure must reach the checksummed authority boundary: {rendered}"
    );
    assert!(
        rendered.contains(&state_path.display().to_string()),
        "a corruption diagnostic must name the affected authority path: {rendered}"
    );
}

#[test]
fn semantically_valid_segment_state_corruption_must_not_reissue_a_live_segment() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let original = allocator
        .acquire(&tenant("tenant-original"), &attachment("sandbox-original"))
        .expect("original segment should allocate");
    let state_path = allocator.state_path();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(state_path).expect("authority should read"))
            .expect("authority envelope should parse");
    envelope["body"]["records"]["segment-allocations"]["tenants"] = serde_json::json!({});
    fs::write(
        state_path,
        serde_json::to_vec_pretty(&envelope).expect("tampered envelope should render"),
    )
    .expect("semantically corrupt state should be installed without updating its checksum");

    let error = match SingleNodeSegmentAllocator::for_node_supernet(
        dir.path(),
        DEFAULT_NODE_SUPERNET,
        DEFAULT_TENANT_PREFIX,
    ) {
        Ok(restarted) => {
            let replacement = restarted.acquire(
                &tenant("tenant-replacement"),
                &attachment("sandbox-replacement"),
            );
            if let Ok(segment) = &replacement {
                assert_eq!(
                    segment.cidr(),
                    original.cidr(),
                    "unchecked corruption would expose the audited live-segment reuse"
                );
            }
            replacement.expect_err(
                    "semantically valid corruption must fail closed instead of reissuing a live segment",
                )
        }
        Err(error) => error,
    };
    let rendered = error.to_string();
    assert!(
        ["checksum", "corrupt", "integrity", "version"]
            .iter()
            .any(|needle| rendered.to_ascii_lowercase().contains(needle)),
        "the store must reject corruption with a named integrity error: {rendered}"
    );
}
