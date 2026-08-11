use std::fs;
use std::num::NonZeroU16;
use std::path::Path;

use nimbus_core::TenantId;

use super::super::*;
use crate::{
    ListenerId, LocalNetworkStateStore, NetworkLeaseEpoch, NetworkPlanId, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, NetworkResourceId, PortBindRealm, PortBindTarget,
    PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure, PortLeaseAccounting,
    PortLeaseBinding, PortLeaseFence, PortProtocol, PortPublicationIntent, PortRequestMode,
};

const PORT: u16 = 44_081;

#[path = "tests/recovery_fencing.rs"]
mod recovery_fencing;

#[test]
fn reserve_and_claim_plan_batch_preserves_order_and_is_provider_managed_only() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
    ];

    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("the complete direct-provider plan should reserve and claim");

    assert_eq!(reservation.records().len(), claims.len());
    assert_eq!(reservation.lifetimes().len(), claims.len());
    for (((expected_request, expected_claim), record), lifetime) in claims
        .iter()
        .zip(reservation.records())
        .zip(reservation.lifetimes())
    {
        assert_eq!(record.request(), expected_request);
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(record.bind_claim(), Some(expected_claim));
        let PortRequestMode::Exact(expected_port) = expected_request.binding().port() else {
            panic!("fixture request must select an exact port");
        };
        assert_eq!(record.reserved_port(), Some(*expected_port));
        assert_eq!(record.active_lifetime(), Some(lifetime.lifetime()));
        assert_eq!(lifetime.request(), expected_request);
        assert_eq!(
            lifetime.lifetime().effect_scope(),
            PortLeaseEffectScope::ProviderManaged
        );
    }
}

#[test]
fn planned_request_round_trips_its_durable_batch_identity() {
    let request = planned_request_for("alpha", PORT);
    let encoded = serde_json::to_vec(&request).expect("planned request should serialize");
    let decoded: PortLeaseRequest =
        serde_json::from_slice(&encoded).expect("planned request should deserialize");

    assert_eq!(decoded, request);
    assert_eq!(decoded.plan_id(), Some(&plan_id()));
}

#[test]
fn list_plan_is_read_only_complete_and_stably_ordered() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("complete plan should reserve");
    let bytes_before = authority_bytes(root.path());

    let listed = authority
        .list_plan(&plan_id())
        .expect("plan recovery projection should load");

    assert_eq!(
        listed
            .iter()
            .map(|record| record.request().lease_id().clone())
            .collect::<Vec<_>>(),
        vec![lease_id("alpha"), lease_id("beta")]
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
    drop(reservation);
}

#[test]
fn inspect_plan_members_returns_one_authenticated_read_only_snapshot() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    authority
        .reserve_batch(members.to_vec())
        .expect("complete plan should reserve");
    let bytes_before = authority_bytes(root.path());

    let requested = [members[1].clone(), members[0].clone()];
    let records = authority
        .inspect_plan_members(&members, &requested)
        .expect("exact plan subset should inspect from one snapshot");

    assert_eq!(
        records
            .iter()
            .map(|record| record.request().lease_id())
            .collect::<Vec<_>>(),
        vec![members[1].lease_id(), members[0].lease_id()]
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);

    let crossed = [planned_request_for("beta", PORT + 2)];
    assert!(matches!(
        authority
            .inspect_plan_members(&members, &crossed)
            .expect_err("crossed immutable member must fail closed"),
        PortLeaseError::BindingMismatch { .. }
            | PortLeaseError::IdentityConflict { .. }
            | PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn scalar_reserve_cannot_establish_or_poison_a_planned_member_set() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = planned_request_for("alpha", PORT);
    authority
        .reserve(request_for("gamma", PORT + 2))
        .expect("standalone control record should establish durable bytes");
    let bytes_before = authority_bytes(root.path());

    let error = authority
        .reserve(request.clone())
        .expect_err("a scalar call cannot declare complete planned membership");

    assert!(matches!(
        error,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
    assert!(
        authority
            .list_plan(request.plan_id().expect("fixture is planned"))
            .expect("plan should list")
            .is_empty()
    );
}

#[test]
fn scalar_operations_accept_only_an_established_single_member_plan() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = planned_request_for("alpha", PORT);
    authority
        .reserve_batch(vec![request.clone()])
        .expect("one-member plan must be established atomically");

    let replay = authority
        .reserve(request.clone())
        .expect("scalar replay is safe after exact singleton membership is durable");
    assert_eq!(replay.request(), &request);
    let claim = bind_claim("alpha");
    let claimed = authority
        .claim_bind(&request, None, claim.clone())
        .expect("singleton planned claim should retain scalar behavior");
    assert_eq!(claimed.bind_claim(), Some(&claim));
}

#[test]
fn scalar_claim_cannot_partially_mutate_a_multi_member_plan() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let requests = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    authority
        .reserve_batch(requests.to_vec())
        .expect("complete plan should reserve");
    let records_before = authority.list().expect("plan should list");
    let bytes_before = authority_bytes(root.path());

    let error = authority
        .claim_bind(&requests[0], None, bind_claim("alpha"))
        .expect_err("scalar claim must reject multi-member plan authority");

    assert!(matches!(
        error,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority.list().expect("plan should list"), records_before);
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn scalar_withdraw_release_and_recovery_cannot_split_a_multi_member_plan() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("complete plan should reserve");
    let (_, lifetimes) = reservation.into_parts();
    let bindings = [
        (
            claims[0].0.clone(),
            claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            claims[1].0.clone(),
            claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &lifetimes)
        .expect("complete plan should activate");
    let active_before = authority.list().expect("active plan should list");
    let active_bytes = authority_bytes(root.path());

    let withdraw = authority
        .withdraw(&claims[0].0)
        .expect_err("scalar withdrawal must reject multi-member plan authority");
    assert!(matches!(
        withdraw,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(
        authority.list().expect("active plan should list"),
        active_before
    );
    assert_eq!(authority_bytes(root.path()), active_bytes);

    drop(lifetimes);
    let recovery = authority
        .recover_dead_lifetime(&claims[0].0)
        .expect_err("scalar recovery must reject multi-member plan authority");
    assert!(matches!(
        recovery,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(
        authority.list().expect("dead plan should list"),
        active_before
    );
    assert_eq!(authority_bytes(root.path()), active_bytes);

    let requests = claims
        .iter()
        .rev()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();
    let recovered = authority
        .recover_dead_lifetimes(&requests)
        .expect("complete planned recovery should acquire every dead lifetime");
    assert_eq!(
        recovered
            .iter()
            .map(|guard| guard.request().lease_id().clone())
            .collect::<Vec<_>>(),
        requests
            .iter()
            .map(|request| request.lease_id().clone())
            .collect::<Vec<_>>()
    );
    drop(recovered);

    let release_root = tempfile::tempdir().expect("release state root should exist");
    let release_authority =
        LocalPortLeaseAuthority::open(release_root.path()).expect("release authority should open");
    release_authority
        .reserve_batch(requests.clone())
        .expect("complete release fixture plan should reserve");
    release_authority
        .transaction(|state| {
            for request in &requests {
                crate::port_lease::exact_record_mut(state, request)?.phase =
                    PortLeasePhase::Withdrawing;
            }
            Ok(())
        })
        .expect("release fixture should enter a provider-free withdrawing state");
    let release_before = release_authority
        .list()
        .expect("withdrawing plan should list");
    let release_bytes = authority_bytes(release_root.path());

    let release = release_authority
        .release(&requests[0])
        .expect_err("scalar release must reject multi-member plan authority");
    assert!(matches!(
        release,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(
        release_authority
            .list()
            .expect("withdrawing plan should remain complete"),
        release_before
    );
    assert_eq!(authority_bytes(release_root.path()), release_bytes);
}

#[test]
fn durable_plan_membership_rejects_subset_and_extension_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("initial complete plan should reserve");
    let bytes_before = authority_bytes(root.path());

    let subset = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims[..1])
        .expect_err("a caller subset must not authenticate durable plan authority");
    assert!(matches!(
        subset,
        PortLeaseError::PlanMembershipConflict {
            plan_id: ref rejected,
        }
            if rejected == &plan_id()
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);

    drop(reservation);
    let mut extended = claims.to_vec();
    extended.push((planned_request_for("gamma", PORT + 2), bind_claim("gamma")));
    let extension = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&extended)
        .expect_err("durable plan membership must not grow on replay");
    assert!(matches!(
        extension,
        PortLeaseError::PlanMembershipConflict {
            plan_id: ref rejected,
        }
            if rejected == &plan_id()
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
    assert_eq!(
        authority
            .inspect(extended[2].0.lease_id())
            .expect("extension candidate should inspect"),
        None
    );
}

#[test]
fn activation_rejects_a_plan_subset_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("complete plan should reserve");
    let (_, lifetimes) = reservation.into_parts();
    let bindings = [
        (
            claims[0].0.clone(),
            claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            claims[1].0.clone(),
            claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    let bytes_before = authority_bytes(root.path());

    let error = authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings[..1], None, &lifetimes[..1])
        .expect_err("partial plan activation must fail before durable mutation");

    assert!(matches!(
        error,
        PortLeaseError::PlanMembershipConflict {
            plan_id: ref rejected,
        }
            if rejected == &plan_id()
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn planned_member_lifecycle_authenticates_complete_witness_without_mutating_sibling() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    let launch_claim = reservation_claim("planned-member-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete planned launch should reserve");

    let claim = bind_claim("alpha");
    let lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("exact member should claim under the complete witness");
    let sibling_after_claim = authority
        .inspect(members[1].lease_id())
        .expect("sibling should inspect")
        .expect("sibling should remain durable");
    assert_eq!(sibling_after_claim.phase(), PortLeasePhase::Reserved);
    assert!(sibling_after_claim.bind_claim().is_none());
    assert_eq!(sibling_after_claim.reservation_claim(), Some(&launch_claim));

    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            &claim,
            binding("alpha", PORT),
            &lifetime,
        )
        .expect("exact member should activate independently");
    let active = authority
        .inspect(members[0].lease_id())
        .expect("active member should inspect")
        .expect("active member should remain durable");
    assert_eq!(active.phase(), PortLeasePhase::Active);
    assert_eq!(active.adoption_claim(), Some(&claim));
    let sibling = authority
        .inspect(members[1].lease_id())
        .expect("sibling should inspect")
        .expect("sibling should remain durable");
    assert_eq!(sibling.phase(), PortLeasePhase::Reserved);
    assert!(sibling.bind_claim().is_none());
    assert!(sibling.binding().is_none());
    assert_eq!(sibling.reservation_claim(), Some(&launch_claim));
}

#[test]
fn planned_member_claim_rejects_crossed_witness_or_claim_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    let launch_claim = reservation_claim("planned-member-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete planned launch should reserve");
    let before = authority_bytes(root.path());

    let omitted = members[..1].to_vec();
    let extra = [
        members[0].clone(),
        members[1].clone(),
        planned_request_for("gamma", PORT + 2),
    ];
    let duplicate = [members[0].clone(), members[0].clone(), members[1].clone()];
    for witness in [&omitted[..], &extra[..], &duplicate[..]] {
        let error = authority
            .claim_bind_plan_member_with_lifetime(
                witness,
                &members[0],
                &launch_claim,
                bind_claim("alpha"),
                PortLeaseEffectScope::ProcessBound,
            )
            .expect_err("crossed plan witness must fail before mutation");
        assert!(
            matches!(
                &error,
                PortLeaseError::PlanMembershipConflict { .. }
                    | PortLeaseError::IdentityConflict { .. }
            ),
            "crossed witness returned unexpected error: {error:?}"
        );
        assert_eq!(authority_bytes(root.path()), before);
    }

    let wrong_claim = reservation_claim("crossed-launch");
    authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &wrong_claim,
            bind_claim("alpha"),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect_err("crossed launch claim must fail before mutation");
    assert_eq!(authority_bytes(root.path()), before);
    for member in &members {
        let record = authority
            .inspect(member.lease_id())
            .expect("member should inspect")
            .expect("member should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(record.bind_claim().is_none());
        assert!(record.active_lifetime().is_none());
    }
}

#[test]
fn never_bound_plan_subset_release_authenticates_full_witness_and_preserves_siblings() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    let launch_claim = reservation_claim("planned-never-bound-release");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete planned launch should reserve");
    let sibling_before = authority
        .inspect(members[1].lease_id())
        .expect("sibling should inspect")
        .expect("sibling should remain durable");
    let bytes_before = authority_bytes(root.path());

    let incomplete = authority
        .release_reserved_batch_without_effect(std::slice::from_ref(&members[0]), &launch_claim)
        .expect_err("a selected planned member cannot impersonate the complete plan");
    assert!(matches!(
        incomplete,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);

    let released = authority
        .release_reserved_plan_members_without_effect(
            &members,
            std::slice::from_ref(&members[0]),
            &launch_claim,
        )
        .expect("the complete witness should authorize its selected no-effect member");
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].phase(), PortLeasePhase::Released);
    assert_eq!(
        authority
            .inspect(members[1].lease_id())
            .expect("sibling should inspect"),
        Some(sibling_before.clone()),
        "unselected plan authority must remain byte-for-byte unchanged"
    );
    assert_eq!(
        authority
            .release_reserved_plan_members_without_effect(
                &members,
                std::slice::from_ref(&members[0]),
                &launch_claim,
            )
            .expect("exact selected release should replay"),
        released
    );

    let crossed_witness = [members[0].clone()];
    let before_crossed = authority_bytes(root.path());
    let crossed = authority
        .release_reserved_plan_members_without_effect(
            &crossed_witness,
            std::slice::from_ref(&members[1]),
            &launch_claim,
        )
        .expect_err("an incomplete witness must fail before sibling release");
    assert!(matches!(
        crossed,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(root.path()), before_crossed);
    assert_eq!(
        authority
            .inspect(members[1].lease_id())
            .expect("crossed rejection should inspect"),
        Some(sibling_before)
    );
}

#[test]
fn planned_subset_lifecycle_is_atomic_and_preserves_active_sibling() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
        planned_request_for("gamma", PORT + 2),
    ];
    let launch_claim = reservation_claim("planned-subset-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete planned launch should reserve");

    let sibling_claim = bind_claim("gamma");
    let sibling_lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[2],
            &launch_claim,
            sibling_claim.clone(),
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("independent sibling provider should claim its member");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[2],
            &launch_claim,
            &sibling_claim,
            binding("gamma", PORT + 2),
            &sibling_lifetime,
        )
        .expect("independent sibling provider should activate");
    let sibling_before = authority
        .inspect(members[2].lease_id())
        .expect("sibling should inspect")
        .expect("sibling should remain durable");

    let subset_claims = [
        (members[0].clone(), bind_claim("alpha")),
        (members[1].clone(), bind_claim("beta")),
    ];
    let subset_lifetimes = authority
        .claim_bind_plan_members_with_lifetimes(
            &members,
            &subset_claims,
            &launch_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("published subset should claim atomically under complete witness");
    assert_eq!(
        authority
            .inspect(members[2].lease_id())
            .expect("sibling should inspect")
            .expect("sibling should remain durable"),
        sibling_before
    );

    let subset_bindings = [
        (
            members[0].clone(),
            subset_claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            members[1].clone(),
            subset_claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    authority
        .adopt_claimed_and_activate_plan_members_with_lifetimes(
            &members,
            &subset_bindings,
            &launch_claim,
            &subset_lifetimes,
        )
        .expect("published subset should activate atomically under complete witness");
    for member in &members[..2] {
        assert_eq!(
            authority
                .inspect(member.lease_id())
                .expect("published member should inspect")
                .expect("published member should remain durable")
                .phase(),
            PortLeasePhase::Active
        );
    }
    assert_eq!(
        authority
            .inspect(members[2].lease_id())
            .expect("sibling should inspect")
            .expect("sibling should remain durable"),
        sibling_before
    );
}

#[test]
fn dead_plan_member_recovery_preserves_reserved_siblings_and_replays_exactly() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
        planned_request_for("gamma", PORT + 2),
    ];
    let launch_claim = reservation_claim("mixed-recovery-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete mixed-state plan should reserve");
    let claim = bind_claim("alpha");
    let lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            claim.clone(),
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("active member should claim under the complete plan");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            &claim,
            binding("alpha", PORT),
            &lifetime,
        )
        .expect("active member should adopt independently");
    drop(lifetime);
    let records_before = authority.list().expect("mixed-state plan should list");
    let bytes_before = authority_bytes(root.path());

    let recovered = authority
        .recover_dead_plan_members(&members, std::slice::from_ref(&members[0]))
        .expect("dead active member should recover beside reserved siblings");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].request(), &members[0]);
    let live_error = authority
        .recover_dead_plan_members(&members, std::slice::from_ref(&members[0]))
        .expect_err("held recovery authority must fence a second owner");
    assert!(matches!(
        live_error,
        PortLeaseError::LifetimeOwnerLive { ref lease_id }
            if lease_id == members[0].lease_id()
    ));
    assert_eq!(
        authority.list().expect("plan should remain unchanged"),
        records_before
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);

    drop(recovered);
    let replay = authority
        .recover_dead_plan_members(&members, std::slice::from_ref(&members[0]))
        .expect("released recovery authority should replay exactly");
    assert_eq!(replay[0].request(), &members[0]);
    assert_eq!(
        authority.list().expect("plan should remain unchanged"),
        records_before
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn dead_plan_subset_recovery_is_atomic_when_any_requested_owner_is_live() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
        planned_request_for("gamma", PORT + 2),
    ];
    let launch_claim = reservation_claim("subset-recovery-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete subset plan should reserve");
    let claims = [
        (members[0].clone(), bind_claim("alpha")),
        (members[1].clone(), bind_claim("beta")),
    ];
    let mut lifetimes = authority
        .claim_bind_plan_members_with_lifetimes(
            &members,
            &claims,
            &launch_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("active subset should claim atomically");
    let bindings = [
        (
            members[0].clone(),
            claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            members[1].clone(),
            claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    authority
        .adopt_claimed_and_activate_plan_members_with_lifetimes(
            &members,
            &bindings,
            &launch_claim,
            &lifetimes,
        )
        .expect("active subset should adopt atomically");
    let live_beta = lifetimes.pop().expect("beta lifetime should exist");
    drop(lifetimes.pop().expect("alpha lifetime should exist"));
    let active_subset = [members[1].clone(), members[0].clone()];
    let bytes_before = authority_bytes(root.path());

    let error = authority
        .recover_dead_plan_members(&members, &active_subset)
        .expect_err("one live owner must reject the whole recovery subset");
    assert!(matches!(
        error,
        PortLeaseError::LifetimeOwnerLive { ref lease_id }
            if lease_id == members[1].lease_id()
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
    let alpha = authority
        .recover_dead_plan_members(&members, std::slice::from_ref(&members[0]))
        .expect("failed batch must release an earlier acquired stable lock");
    drop(alpha);

    drop(live_beta);
    let recovered = authority
        .recover_dead_plan_members(&members, &active_subset)
        .expect("every dead active member should recover in caller order");
    assert_eq!(
        recovered
            .iter()
            .map(PortLeaseRecoveryGuard::request)
            .collect::<Vec<_>>(),
        active_subset.iter().collect::<Vec<_>>()
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn whole_plan_withdrawal_is_atomic_when_one_member_is_invalid() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("complete plan should reserve");
    let (_, lifetimes) = reservation.into_parts();
    let bindings = [
        (
            claims[0].0.clone(),
            claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            claims[1].0.clone(),
            claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &lifetimes)
        .expect("complete plan should activate");
    let mut invalid = claims[1].0.clone();
    invalid.generation = NetworkResourceGeneration::new(8);
    let attempted = [claims[0].0.clone(), invalid];
    let bytes_before = authority_bytes(root.path());

    let error = authority
        .withdraw_provider_managed_batch_with_lifetimes(&attempted, &lifetimes)
        .expect_err("one stale member must reject the complete withdrawal");

    assert!(
        matches!(
            error,
            PortLeaseError::StaleFence(_)
                | PortLeaseError::PlanMembershipConflict { .. }
                | PortLeaseError::LifetimeMismatch { .. }
        ),
        "unexpected withdrawal error: {error:?}"
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
    for (request, _) in &claims {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("active member should inspect")
                .expect("active member should remain")
                .phase(),
            PortLeasePhase::Active
        );
    }
}

#[test]
fn exact_live_plan_can_release_without_provider_io_and_clear_port_conflicts() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("complete plan should reserve");
    let (_, lifetimes) = reservation.into_parts();
    let released = authority
        .release_provider_managed_claim_batch_after_confirmed_absence_with_lifetimes(
            &claims, &lifetimes,
        )
        .expect("the live owner may prove no provider API byte was sent");

    assert!(
        released
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
    let replacement = PortLeaseRequest::new(
        "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAY"
            .parse()
            .expect("replacement lease ID should parse"),
        owner_id("gamma"),
        None,
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::Exact(nonzero_port(PORT)),
        ),
    );
    authority
        .reserve(replacement)
        .expect("terminal no-effect release must clear the host-port conflict");
}

#[test]
fn active_and_ambiguous_plan_withdrawal_preserve_exact_cleanup_evidence() {
    let active_root = tempfile::tempdir().expect("active state root should exist");
    let active_authority =
        LocalPortLeaseAuthority::open(active_root.path()).expect("active authority should open");
    let active_claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let active_reservation = active_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&active_claims)
        .expect("active plan should reserve");
    let (_, active_lifetimes) = active_reservation.into_parts();
    let bindings = [
        (
            active_claims[0].0.clone(),
            active_claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            active_claims[1].0.clone(),
            active_claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    active_authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &active_lifetimes)
        .expect("active plan should activate");
    let active_requests = active_claims
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();

    let withdrawing = active_authority
        .withdraw_provider_managed_batch_with_lifetimes(&active_requests, &active_lifetimes)
        .expect("complete active plan should withdraw atomically");

    for (record, (_, _, expected_binding)) in withdrawing.iter().zip(&bindings) {
        assert_eq!(record.phase(), PortLeasePhase::Withdrawing);
        assert_eq!(record.binding(), Some(expected_binding));
        assert!(record.adoption_claim().is_some());
        assert!(record.bind_claim().is_none());
        assert!(record.active_lifetime().is_some());
    }
    drop(active_lifetimes);
    let recoveries = active_authority
        .recover_dead_lifetimes(&active_requests)
        .expect("the stopped provider batch should recover as one exact set");
    let before_partial = authority_bytes(active_root.path());
    assert!(matches!(
        active_authority.retain_provider_managed_batch_after_confirmed_absence(
            &active_requests,
            &recoveries[..1],
        ),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        authority_bytes(active_root.path()),
        before_partial,
        "partial absence authority must leave the complete batch byte-stable"
    );
    let retained = active_authority
        .retain_provider_managed_batch_after_confirmed_absence(&active_requests, &recoveries)
        .expect("confirmed provider absence should retain one non-bindable batch");
    for (record, (_, _, expected_binding)) in retained.iter().zip(&bindings) {
        assert_eq!(record.phase(), PortLeasePhase::CleanupPending);
        assert_eq!(record.binding(), Some(expected_binding));
        assert!(record.adoption_claim().is_some());
        assert!(record.bind_claim().is_none());
        assert!(record.active_lifetime().is_none());
    }
    let retained_bytes = authority_bytes(active_root.path());
    assert_eq!(
        active_authority
            .retain_provider_managed_batch_after_confirmed_absence(&active_requests, &recoveries,)
            .expect("the exact retained absence checkpoint should replay"),
        retained
    );
    assert_eq!(
        authority_bytes(active_root.path()),
        retained_bytes,
        "retained absence replay must be byte-stable"
    );
    assert!(matches!(
        active_authority.retain_provider_managed_batch_after_confirmed_absence(
            &active_requests[..1],
            &recoveries[..1],
        ),
        Err(PortLeaseError::PlanMembershipConflict { .. })
    ));
    assert_eq!(
        authority_bytes(active_root.path()),
        retained_bytes,
        "an equal-length retained subset must not mutate any sibling"
    );
    let before_partial_release = authority_bytes(active_root.path());
    assert!(matches!(
        active_authority.release_retained_provider_managed_batch_after_confirmed_absence(
            &active_requests[..1],
        ),
        Err(PortLeaseError::PlanMembershipConflict { .. })
    ));
    assert_eq!(
        authority_bytes(active_root.path()),
        before_partial_release,
        "a retained plan subset must not release any sibling"
    );
    drop(recoveries);
    drop(active_authority);
    let active_authority = LocalPortLeaseAuthority::open(active_root.path())
        .expect("retained authority should reopen");
    let released = active_authority
        .release_retained_provider_managed_batch_after_confirmed_absence(&active_requests)
        .expect("the complete retained absence batch should release atomically");
    assert!(released.iter().all(|record| {
        record.phase() == PortLeasePhase::Released
            && record.active_lifetime().is_none()
            && record.binding().is_some()
            && record.adoption_claim().is_some()
    }));
    assert_eq!(
        active_authority
            .release_retained_provider_managed_batch_after_confirmed_absence(&active_requests)
            .expect("the exact terminal batch should replay"),
        released
    );

    let pending_root = tempfile::tempdir().expect("pending state root should exist");
    let pending_authority =
        LocalPortLeaseAuthority::open(pending_root.path()).expect("pending authority should open");
    let pending_claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let pending_reservation = pending_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&pending_claims)
        .expect("ambiguous plan should reserve");
    let (_, pending_lifetimes) = pending_reservation.into_parts();
    let pending_requests = pending_claims
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();

    let pending = pending_authority
        .withdraw_provider_managed_batch_with_lifetimes(&pending_requests, &pending_lifetimes)
        .expect("complete unadopted plan should quarantine atomically");

    for (record, (_, expected_claim)) in pending.iter().zip(&pending_claims) {
        assert_eq!(record.phase(), PortLeasePhase::CleanupPending);
        assert_eq!(record.bind_claim(), Some(expected_claim));
        assert!(record.binding().is_none());
        assert!(record.adoption_claim().is_none());
        assert!(record.active_lifetime().is_some());
    }
}

#[test]
fn dead_pre_checkpoint_provider_batches_retain_and_unadopted_release_replays() {
    let active_root = tempfile::tempdir().expect("active state root should exist");
    let active_authority =
        LocalPortLeaseAuthority::open(active_root.path()).expect("active authority should open");
    let active_claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let active_reservation = active_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&active_claims)
        .expect("active plan should reserve");
    let (_, active_lifetimes) = active_reservation.into_parts();
    let bindings = [
        (
            active_claims[0].0.clone(),
            active_claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            active_claims[1].0.clone(),
            active_claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    active_authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &active_lifetimes)
        .expect("active plan should activate");
    let active_requests = active_claims
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();
    drop(active_lifetimes);
    let active_recoveries = active_authority
        .recover_dead_lifetimes(&active_requests)
        .expect("dead active batch should recover before a withdrawal checkpoint exists");
    let active_retained = active_authority
        .retain_provider_managed_batch_after_confirmed_absence(&active_requests, &active_recoveries)
        .expect("exact provider absence should retain the dead active batch");
    for (record, (_, _, expected_binding)) in active_retained.iter().zip(&bindings) {
        assert_eq!(record.phase(), PortLeasePhase::CleanupPending);
        assert_eq!(record.binding(), Some(expected_binding));
        assert!(record.adoption_claim().is_some());
        assert!(record.bind_claim().is_none());
        assert!(record.active_lifetime().is_none());
    }

    let pending_root = tempfile::tempdir().expect("pending state root should exist");
    let pending_authority =
        LocalPortLeaseAuthority::open(pending_root.path()).expect("pending authority should open");
    let pending_claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let pending_reservation = pending_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&pending_claims)
        .expect("unadopted plan should reserve");
    let (_, pending_lifetimes) = pending_reservation.into_parts();
    let pending_requests = pending_claims
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();
    drop(pending_lifetimes);
    let pending_recoveries = pending_authority
        .recover_dead_lifetimes(&pending_requests)
        .expect("dead reserved batch should recover before a withdrawal checkpoint exists");
    let retained = pending_authority
        .retain_provider_managed_batch_after_confirmed_absence(
            &pending_requests,
            &pending_recoveries,
        )
        .expect("exact provider absence should retain the dead unadopted batch");
    assert!(
        retained
            .iter()
            .zip(&pending_claims)
            .all(|(record, (_, claim))| {
                record.phase() == PortLeasePhase::CleanupPending
                    && record.bind_claim() == Some(claim)
                    && record.binding().is_none()
                    && record.adoption_claim().is_none()
                    && record.active_lifetime().is_none()
            })
    );
    let retained_bytes = authority_bytes(pending_root.path());
    assert_eq!(
        pending_authority
            .retain_provider_managed_batch_after_confirmed_absence(
                &pending_requests,
                &pending_recoveries,
            )
            .expect("the exact unadopted retained checkpoint should replay"),
        retained
    );
    assert_eq!(
        authority_bytes(pending_root.path()),
        retained_bytes,
        "unadopted retained replay must be byte-stable"
    );
    let before_partial_release = authority_bytes(pending_root.path());
    assert!(matches!(
        pending_authority.release_retained_provider_managed_batch_after_confirmed_absence(
            &pending_requests[..1],
        ),
        Err(PortLeaseError::PlanMembershipConflict { .. })
    ));
    assert_eq!(
        authority_bytes(pending_root.path()),
        before_partial_release,
        "an unadopted retained subset must leave every sibling byte-stable"
    );
    drop(pending_recoveries);
    drop(pending_authority);
    let pending_authority = LocalPortLeaseAuthority::open(pending_root.path())
        .expect("unadopted retained authority should reopen");
    let released = pending_authority
        .release_retained_provider_managed_batch_after_confirmed_absence(&pending_requests)
        .expect("the complete unadopted retained batch should release atomically");
    assert!(released.iter().all(|record| {
        record.phase() == PortLeasePhase::Released
            && record.bind_claim().is_none()
            && record.binding().is_none()
            && record.adoption_claim().is_none()
            && record.active_lifetime().is_none()
    }));
    assert_eq!(
        pending_authority
            .release_retained_provider_managed_batch_after_confirmed_absence(&pending_requests)
            .expect("the exact unadopted terminal batch should replay"),
        released
    );
}

#[test]
fn live_and_dead_terminal_paths_reject_plan_subsets_byte_unchanged() {
    let live_root = tempfile::tempdir().expect("live state root should exist");
    let live_authority =
        LocalPortLeaseAuthority::open(live_root.path()).expect("live authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = live_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("live plan should reserve");
    let (_, lifetimes) = reservation.into_parts();
    let bindings = [
        (
            claims[0].0.clone(),
            claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            claims[1].0.clone(),
            claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    live_authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &lifetimes)
        .expect("live plan should activate");
    let expected = bindings
        .iter()
        .map(|(request, _, binding)| (request.clone(), binding.clone()))
        .collect::<Vec<_>>();
    let live_bytes = authority_bytes(live_root.path());

    let live_error = live_authority
        .release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
            &expected[..1],
            &lifetimes[..1],
        )
        .expect_err("live terminal release must reject a plan subset");

    assert!(matches!(
        live_error,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(live_root.path()), live_bytes);
    drop(lifetimes);

    let requests = claims
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();
    let recoveries = live_authority
        .recover_dead_lifetimes(&requests)
        .expect("complete dead plan should yield recovery authority");
    let dead_bytes = authority_bytes(live_root.path());

    let quarantine_error = live_authority
        .mark_cleanup_pending_batch_after_owner_death(&requests[..1], &recoveries[..1])
        .expect_err("dead-owner quarantine must reject a plan subset");
    assert!(matches!(
        quarantine_error,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(live_root.path()), dead_bytes);

    live_authority
        .mark_cleanup_pending_batch_after_owner_death(&requests, &recoveries)
        .expect("complete dead plan should quarantine");
    let pending_bytes = authority_bytes(live_root.path());
    let release_error = live_authority
        .release_provider_managed_batch_after_confirmed_stop(&requests[..1], &recoveries[..1])
        .expect_err("dead-owner terminal release must reject a plan subset");
    assert!(matches!(
        release_error,
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert_eq!(authority_bytes(live_root.path()), pending_bytes);
}

#[test]
fn internal_port_conflict_leaves_new_batch_absent_and_existing_bytes_unchanged() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let existing = request_for("gamma", PORT + 2);
    let existing_record = authority
        .reserve(existing.clone())
        .expect("existing request should reserve");
    let bytes_before = authority_bytes(root.path());
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT), bind_claim("beta")),
    ];

    let error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect_err("an intra-batch conflict must reject every request");

    assert!(matches!(error, PortLeaseError::PortConflict { .. }));
    assert_new_requests_absent(&authority, &claims);
    assert_eq!(
        authority
            .inspect(existing.lease_id())
            .expect("existing request should inspect"),
        Some(existing_record)
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);

    authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims[..1])
        .expect("a transactional conflict must release every acquired lifetime lock");
}

#[test]
fn preexisting_identity_conflict_discards_earlier_staged_reservations() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let existing = request_for("alpha", PORT);
    let existing_record = authority
        .reserve(existing.clone())
        .expect("existing request should reserve");
    let bytes_before = authority_bytes(root.path());
    let conflicting = planned_request_for("alpha", PORT + 2);
    let new_request = planned_request_for("beta", PORT + 1);
    let claims = [
        (new_request.clone(), bind_claim("beta")),
        (conflicting, bind_claim("alpha")),
    ];

    let error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect_err("an existing identity conflict must reject every request");

    assert!(matches!(error, PortLeaseError::IdentityConflict { .. }));
    assert_eq!(
        authority
            .inspect(new_request.lease_id())
            .expect("new request should inspect"),
        None
    );
    assert_eq!(
        authority
            .inspect(existing.lease_id())
            .expect("existing request should inspect"),
        Some(existing_record)
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn duplicate_input_identity_is_typed_and_mutates_nothing() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let existing = request_for("gamma", PORT + 2);
    let existing_record = authority
        .reserve(existing.clone())
        .expect("existing request should reserve");
    let bytes_before = authority_bytes(root.path());
    let duplicate = planned_request_for("alpha", PORT);
    let claims = [
        (duplicate.clone(), bind_claim("alpha")),
        (duplicate.clone(), bind_claim("beta")),
    ];

    let error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect_err("duplicate stable identity must be rejected");

    assert!(matches!(
        error,
        PortLeaseError::IdentityConflict { ref lease_id }
            if lease_id == duplicate.lease_id()
    ));
    assert_eq!(
        authority
            .inspect(duplicate.lease_id())
            .expect("duplicate request should inspect"),
        None
    );
    assert_eq!(
        authority
            .inspect(existing.lease_id())
            .expect("existing request should inspect"),
        Some(existing_record)
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn contended_member_rejects_the_batch_and_releases_partial_locks() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let requests = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
        planned_request_for("gamma", PORT + 2),
    ];
    authority
        .reserve_batch(requests.to_vec())
        .expect("complete contended fixture plan should reserve");
    let contended_request = requests[2].clone();
    let LifetimeLockAttempt::Acquired(contended) = authority
        .try_acquire_lifetime_lock(contended_request.lease_id())
        .expect("gamma lifetime lock should open")
    else {
        panic!("gamma lifetime lock should initially be free");
    };
    let records_before = authority.list().expect("fixture plan should list");
    let bytes_before = authority_bytes(root.path());
    let attempted = [
        (requests[0].clone(), bind_claim("alpha")),
        (requests[1].clone(), bind_claim("beta")),
        (requests[2].clone(), bind_claim("gamma")),
    ];

    let error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&attempted)
        .expect_err("one contended member must reject the complete batch");

    assert!(
        matches!(
            error,
            PortLeaseError::LifetimeOwnerLive { ref lease_id }
                if lease_id == contended_request.lease_id()
        ),
        "unexpected contention error: {error:?}"
    );
    assert_eq!(
        authority.list().expect("fixture plan should list"),
        records_before
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);

    drop(contended);
    let retry = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&attempted)
        .expect("a complete retry proves earlier partial locks were released");
    assert_eq!(retry.records().len(), requests.len());
}

#[test]
fn lifetime_generation_exhaustion_discards_every_staged_change() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let exhausted = planned_request_for("beta", PORT + 1);
    let new_request = planned_request_for("alpha", PORT);
    authority
        .reserve_batch(vec![new_request.clone(), exhausted.clone()])
        .expect("complete exhausted fixture plan should reserve");
    let new_before = authority
        .inspect(new_request.lease_id())
        .expect("new fixture should inspect")
        .expect("new fixture should remain");
    authority
        .transaction(|state| {
            let record = exact_record_mut(state, &exhausted)?;
            record.last_lifetime_generation = u64::MAX;
            Ok(())
        })
        .expect("fixture should install a valid exhausted generation");
    let exhausted_before = authority
        .inspect(exhausted.lease_id())
        .expect("exhausted fixture should inspect")
        .expect("exhausted fixture should remain");
    let bytes_before = authority_bytes(root.path());
    let claims = [
        (new_request.clone(), bind_claim("alpha")),
        (exhausted.clone(), bind_claim("beta")),
    ];

    let error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect_err("generation exhaustion must reject every request");

    assert!(matches!(
        error,
        PortLeaseError::LifetimeGenerationExhausted { ref lease_id }
            if lease_id == exhausted.lease_id()
    ));
    assert_eq!(
        authority
            .inspect(new_request.lease_id())
            .expect("new request should inspect"),
        Some(new_before)
    );
    assert_eq!(
        authority
            .inspect(exhausted.lease_id())
            .expect("exhausted fixture should inspect"),
        Some(exhausted_before)
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn existing_phase_and_bind_claim_conflicts_leave_new_siblings_absent() {
    let phase_root = tempfile::tempdir().expect("phase state root should exist");
    let phase_authority =
        LocalPortLeaseAuthority::open(phase_root.path()).expect("phase authority should open");
    let withdrawing = planned_request_for("gamma", PORT + 2);
    let new_request = planned_request_for("alpha", PORT);
    phase_authority
        .reserve_batch(vec![new_request.clone(), withdrawing.clone()])
        .expect("complete phase fixture plan should reserve");
    let new_before = phase_authority
        .inspect(new_request.lease_id())
        .expect("phase sibling should inspect")
        .expect("phase sibling should remain");
    let withdrawing_before = phase_authority
        .transaction(|state| {
            let record = exact_record_mut(state, &withdrawing)?;
            record.phase = PortLeasePhase::Withdrawing;
            Ok(record.clone())
        })
        .expect("fixture should install a preexisting withdrawing member");
    let phase_bytes_before = authority_bytes(phase_root.path());
    let phase_claims = [
        (new_request.clone(), bind_claim("alpha")),
        (withdrawing.clone(), bind_claim("gamma")),
    ];

    let phase_error = phase_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&phase_claims)
        .expect_err("a non-reserved member must reject the batch");

    assert!(matches!(
        phase_error,
        PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Withdrawing,
            operation: PortLeaseOperation::BeginLifetime,
            ..
        }
    ));
    assert_eq!(
        phase_authority
            .inspect(new_request.lease_id())
            .expect("new phase sibling should inspect"),
        Some(new_before)
    );
    assert_eq!(
        phase_authority
            .inspect(withdrawing.lease_id())
            .expect("withdrawing fixture should inspect"),
        Some(withdrawing_before)
    );
    assert_eq!(authority_bytes(phase_root.path()), phase_bytes_before);

    let claim_root = tempfile::tempdir().expect("claim state root should exist");
    let claim_authority =
        LocalPortLeaseAuthority::open(claim_root.path()).expect("claim authority should open");
    let claimed = planned_request_for("beta", PORT + 1);
    let new_request = planned_request_for("alpha", PORT);
    claim_authority
        .reserve_batch(vec![new_request.clone(), claimed.clone()])
        .expect("complete claim fixture plan should reserve");
    let new_claim = bind_claim("alpha");
    let existing_claim = bind_claim("beta");
    claim_authority
        .claim_bind_batch(
            &[
                (new_request.clone(), new_claim.clone()),
                (claimed.clone(), existing_claim),
            ],
            None,
        )
        .expect("fixture should install a complete lifetime-free claim batch");
    let new_before = claim_authority
        .inspect(new_request.lease_id())
        .expect("claim sibling should inspect")
        .expect("claim sibling should remain");
    let claimed_before = claim_authority
        .inspect(claimed.lease_id())
        .expect("claimed fixture should inspect")
        .expect("claimed fixture should remain");
    let claim_bytes_before = authority_bytes(claim_root.path());
    let claim_claims = [
        (new_request.clone(), new_claim),
        (claimed.clone(), bind_claim("gamma")),
    ];

    let claim_error = claim_authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claim_claims)
        .expect_err("a foreign durable bind claim must reject the batch");

    assert!(matches!(
        claim_error,
        PortLeaseError::BindClaimConflict { ref lease_id }
            if lease_id == claimed.lease_id()
    ));
    assert_eq!(
        claim_authority
            .inspect(new_request.lease_id())
            .expect("new claim sibling should inspect"),
        Some(new_before)
    );
    assert_eq!(
        claim_authority
            .inspect(claimed.lease_id())
            .expect("claimed fixture should inspect"),
        Some(claimed_before)
    );
    assert_eq!(authority_bytes(claim_root.path()), claim_bytes_before);
}

#[test]
fn exact_replay_requires_live_owner_or_explicit_dead_owner_recovery() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claims = [
        (planned_request_for("alpha", PORT), bind_claim("alpha")),
        (planned_request_for("beta", PORT + 1), bind_claim("beta")),
    ];
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("initial batch should reserve and claim");
    let records_before = reservation.records().to_vec();
    let bytes_before = authority_bytes(root.path());

    let live_error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect_err("a live exact owner must reject replay");
    assert!(matches!(
        live_error,
        PortLeaseError::LifetimeOwnerLive { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);

    drop(reservation);
    let dead_error = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect_err("a dead owner must remain explicit recovery work");
    assert!(matches!(
        dead_error,
        PortLeaseError::LifetimeConflict { .. }
    ));
    assert_eq!(
        claims
            .iter()
            .map(|(request, _)| {
                authority
                    .inspect(request.lease_id())
                    .expect("dead-owner request should inspect")
                    .expect("dead-owner request should remain")
            })
            .collect::<Vec<_>>(),
        records_before
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);
    let requests = claims
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();
    let recoveries = authority
        .recover_dead_lifetimes(&requests)
        .expect("failed dead-owner replay must release every acquired lock");
    assert_eq!(recoveries.len(), requests.len());
}

fn assert_new_requests_absent(
    authority: &LocalPortLeaseAuthority,
    claims: &[(PortLeaseRequest, PortBindClaim)],
) {
    for (request, _) in claims {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("new request should inspect"),
            None,
            "failed batch must not durably reserve {}",
            request.lease_id()
        );
    }
}

fn authority_bytes(root: &Path) -> Vec<u8> {
    fs::read(LocalNetworkStateStore::authority_path_for(root))
        .expect("fixture authority state should exist")
}

fn request_for(role: &str, port: u16) -> PortLeaseRequest {
    PortLeaseRequest::new(
        lease_id(role),
        owner_id(role),
        None,
        PortLeaseFence::new(
            NetworkResourceGeneration::new(7),
            NetworkLeaseEpoch::new(11),
        ),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::Exact(nonzero_port(port)),
        ),
    )
}

fn planned_request_for(role: &str, port: u16) -> PortLeaseRequest {
    let tenant_id = TenantId::new("tenant-a").expect("fixture tenant should validate");
    PortLeaseRequest::new(
        lease_id(role),
        owner_id(role),
        Some(tenant_id),
        PortLeaseFence::new(
            NetworkResourceGeneration::new(7),
            NetworkLeaseEpoch::new(11),
        ),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::Exact(nonzero_port(port)),
        ),
    )
    .with_plan_id(plan_id())
}

fn lease_id(role: &str) -> PortLeaseId {
    match role {
        "alpha" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "beta" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "gamma" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAX",
        other => panic!("unexpected fixture role {other:?}"),
    }
    .parse()
    .expect("fixture lease ID should parse")
}

fn owner_id(role: &str) -> NetworkResourceId {
    let listener: ListenerId = match role {
        "alpha" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "beta" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "gamma" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAX",
        other => panic!("unexpected fixture role {other:?}"),
    }
    .parse()
    .expect("fixture listener ID should parse");
    listener.into()
}

fn bind_claim(role: &str) -> PortBindClaim {
    PortBindClaim::new(provider_handle(format!("attempt-{role}")))
}

fn binding(role: &str, port: u16) -> PortLeaseBinding {
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            nonzero_port(port),
        )
        .expect("fixture endpoint should validate"),
        PortBindingProvenance::NimbusOwned,
        provider_handle(format!("binding-{role}")),
    )
}

fn plan_id() -> NetworkPlanId {
    let tenant_id = TenantId::new("tenant-a").expect("fixture tenant should validate");
    NetworkPlanId::for_tenant_workload_plan(&tenant_id, "sandbox-incarnation-a")
}

fn provider_handle(resource: String) -> NetworkProviderHandle {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider ID should parse");
    NetworkProviderHandle::new(provider_id, resource)
        .expect("fixture provider handle should validate")
}

fn reservation_claim(resource: &str) -> NetworkReservationClaim {
    NetworkReservationClaim::new(provider_handle(resource.to_owned()))
}

fn nonzero_port(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).expect("fixture port should be non-zero")
}
