use std::num::NonZeroU16;

use nimbus_core::TenantId;

use super::super::*;
use crate::{
    ListenerId, NetworkLeaseEpoch, NetworkPlanId, NetworkProviderHandle, NetworkProviderId,
    NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId, PortBindClaim,
    PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint,
    PortExposure, PortLeaseAccounting, PortLeaseFence, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};

const FIRST_PORT: u16 = 45_751;

struct ActiveMember {
    request: PortLeaseRequest,
    binding: PortLeaseBinding,
    lifetime: PortLeaseLifetimeGuard,
}

struct PlannedFixture {
    _root: tempfile::TempDir,
    authority: LocalPortLeaseAuthority,
    plan_members: Vec<PortLeaseRequest>,
    active: Vec<ActiveMember>,
}

impl PlannedFixture {
    fn new(active_roles: &[&str]) -> Self {
        let roles = active_roles
            .iter()
            .map(|role| (*role, PortLeaseEffectScope::ProcessBound))
            .collect::<Vec<_>>();
        Self::new_with_scopes(&roles)
    }

    fn new_with_scopes(active_roles: &[(&str, PortLeaseEffectScope)]) -> Self {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let tenant = TenantId::new("tenant-terminal-settlement")
            .expect("fixture tenant identity should validate");
        let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant, "workload-generation-seven");
        let plan_members = vec![
            request(&tenant, &plan_id, "alpha", FIRST_PORT),
            request(&tenant, &plan_id, "beta", FIRST_PORT + 1),
            request(&tenant, &plan_id, "pep", FIRST_PORT + 2),
        ];
        let reservation = reservation_claim();
        authority
            .reserve_batch_for_coordinator(plan_members.clone(), &reservation)
            .expect("complete workload plan should reserve");

        let mut active = Vec::new();
        for (role, effect_scope) in active_roles {
            let request = plan_members
                .iter()
                .find(|request| request.owner_id() == &owner_id(&tenant, role))
                .expect("requested active fixture member should exist")
                .clone();
            let claim = bind_claim(&format!("initial-{role}"));
            let binding = binding(&request, claim.provider_attempt().clone());
            let lifetime = authority
                .claim_bind_plan_member_with_lifetime(
                    &plan_members,
                    &request,
                    &reservation,
                    claim.clone(),
                    *effect_scope,
                )
                .expect("planned member should claim its process lifetime");
            authority
                .adopt_claimed_and_activate_plan_member_with_lifetime(
                    &plan_members,
                    &request,
                    &reservation,
                    &claim,
                    binding.clone(),
                    &lifetime,
                )
                .expect("planned member should activate");
            active.push(ActiveMember {
                request,
                binding,
                lifetime,
            });
        }
        Self {
            _root: root,
            authority,
            plan_members,
            active,
        }
    }
}

#[test]
fn process_bound_terminal_subset_is_atomic_and_preserves_plan_siblings() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let bindings = active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    let unrelated = &plan_members[2];
    let unrelated_before = sibling_bytes(&authority, unrelated);

    let withdrawing = authority
        .withdraw_process_bound_plan_members_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("selected process-bound members should withdraw atomically");
    assert!(
        withdrawing
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Withdrawing)
    );
    let released = authority
        .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("selected process-bound members should release atomically");
    assert!(
        released
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
    assert_eq!(sibling_bytes(&authority, unrelated), unrelated_before);
    assert_ne!(authority_bytes(&authority), Vec::<u8>::new());
}

#[test]
fn live_terminal_replay_is_idempotent_and_preserves_exact_evidence() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let bindings = active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();

    authority
        .withdraw_process_bound_plan_members_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("first withdrawal should succeed");
    let withdrawing_bytes = authority_bytes(&authority);
    authority
        .withdraw_process_bound_plan_members_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("exact withdrawal replay should succeed");
    assert_eq!(authority_bytes(&authority), withdrawing_bytes);

    authority
        .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("first terminal release should succeed");
    let released_bytes = authority_bytes(&authority);
    let replay = authority
        .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("exact terminal replay should succeed");
    assert!(
        replay
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
    assert_eq!(authority_bytes(&authority), released_bytes);
}

#[test]
fn dead_owner_active_withdrawing_and_cleanup_pending_subsets_release_exactly() {
    for initial_phase in [
        PortLeasePhase::Active,
        PortLeasePhase::Withdrawing,
        PortLeasePhase::CleanupPending,
    ] {
        let PlannedFixture {
            _root,
            authority,
            plan_members,
            active,
        } = PlannedFixture::new(&["alpha", "beta"]);
        let bindings = active
            .iter()
            .map(|active| (active.request.clone(), active.binding.clone()))
            .collect::<Vec<_>>();
        let requests = bindings
            .iter()
            .map(|(request, _)| request.clone())
            .collect::<Vec<_>>();
        let lifetimes = active
            .into_iter()
            .map(|active| active.lifetime)
            .collect::<Vec<_>>();
        if initial_phase == PortLeasePhase::Withdrawing {
            authority
                .withdraw_process_bound_plan_members_with_lifetimes(
                    &plan_members,
                    &bindings,
                    &lifetime_refs(&lifetimes),
                )
                .expect("live owner should checkpoint withdrawal");
        }
        drop(lifetimes);
        let recoveries = authority
            .recover_dead_plan_members(&plan_members, &requests)
            .expect("dead process-bound subset should recover");
        if initial_phase == PortLeasePhase::CleanupPending {
            authority
                .mark_cleanup_pending_plan_members_after_owner_death(
                    &plan_members,
                    &requests,
                    &recoveries,
                )
                .expect("dead subset should checkpoint cleanup pending");
        }
        let released = authority
            .release_process_bound_plan_members_after_owner_death(
                &plan_members,
                &requests,
                &recoveries,
            )
            .expect("dead subset should terminally release");
        assert!(
            released
                .iter()
                .all(|record| record.phase() == PortLeasePhase::Released)
        );
        let released_bytes = authority_bytes(&authority);
        authority
            .release_process_bound_plan_members_after_owner_death(
                &plan_members,
                &requests,
                &recoveries,
            )
            .expect("exact dead-owner terminal replay should succeed");
        assert_eq!(authority_bytes(&authority), released_bytes);
        assert_eq!(
            authority
                .inspect(plan_members[2].lease_id())
                .expect("PEP sibling should inspect")
                .expect("PEP sibling should remain durable")
                .phase(),
            PortLeasePhase::Reserved
        );
    }
}

#[test]
fn duplicate_incomplete_and_crossed_live_inputs_are_byte_unchanged() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let bindings = active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    let before = authority_bytes(&authority);
    let duplicate = vec![bindings[0].clone(), bindings[0].clone()];
    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &plan_members,
                &duplicate,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &plan_members[..2],
                &bindings,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    let crossed_binding = binding(&bindings[1].0, provider_handle("crossed-binding"));
    let crossed = vec![
        (bindings[0].0.clone(), crossed_binding),
        bindings[1].clone(),
    ];
    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &plan_members,
                &crossed,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    let mut stale = bindings.clone();
    stale[0].0.generation = NetworkResourceGeneration::new(8);
    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &plan_members,
                &stale,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);
}

#[test]
fn wrong_lifetime_scope_and_mixed_live_state_are_byte_unchanged() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let bindings = active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    let before = authority_bytes(&authority);
    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &plan_members,
                &bindings[..1],
                &lifetime_refs(&lifetimes[1..]),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    authority
        .withdraw_process_bound_plan_members_with_lifetimes(
            &plan_members,
            &bindings[..1],
            &lifetime_refs(&lifetimes[..1]),
        )
        .expect("one owned member may withdraw as a subset");
    let mixed_before = authority_bytes(&authority);
    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &plan_members,
                &bindings,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), mixed_before);
    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &bindings,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), mixed_before);

    let PlannedFixture {
        _root: _provider_root,
        authority: provider_authority,
        plan_members: provider_plan,
        active: provider_active,
    } = PlannedFixture::new_with_scopes(&[
        ("alpha", PortLeaseEffectScope::ProcessBound),
        ("beta", PortLeaseEffectScope::ProviderManaged),
    ]);
    let provider_bindings = provider_active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let provider_lifetimes = provider_active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    let provider_before = authority_bytes(&provider_authority);
    assert!(
        provider_authority
            .withdraw_process_bound_plan_members_with_lifetimes(
                &provider_plan,
                &provider_bindings,
                &lifetime_refs(&provider_lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&provider_authority), provider_before);
}

#[test]
fn confirmed_stop_release_rejects_crossed_duplicate_and_wrong_lifetime_inputs() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let bindings = active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    authority
        .withdraw_process_bound_plan_members_with_lifetimes(
            &plan_members,
            &bindings,
            &lifetime_refs(&lifetimes),
        )
        .expect("complete listener subset should withdraw");
    let before = authority_bytes(&authority);

    let crossed = vec![
        (
            bindings[0].0.clone(),
            binding(&bindings[1].0, provider_handle("crossed-release")),
        ),
        bindings[1].clone(),
    ];
    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &crossed,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    let duplicate = vec![bindings[0].clone(), bindings[0].clone()];
    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &duplicate,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &bindings[..1],
                &lifetime_refs(&lifetimes[1..]),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    let mut stale = bindings.clone();
    stale[0].0.generation = NetworkResourceGeneration::new(8);
    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &stale,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    authority
        .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
            &plan_members,
            &bindings[..1],
            &lifetime_refs(&lifetimes[..1]),
        )
        .expect("one exact withdrawn member may release as an owned subset");
    let mixed_replay_before = authority_bytes(&authority);
    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &bindings,
                &lifetime_refs(&lifetimes),
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), mixed_replay_before);
}

#[test]
fn dead_owner_mixed_state_crossing_and_live_owner_are_byte_unchanged() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let bindings = active
        .iter()
        .map(|active| (active.request.clone(), active.binding.clone()))
        .collect::<Vec<_>>();
    let requests = bindings
        .iter()
        .map(|(request, _)| request.clone())
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    let live_before = authority_bytes(&authority);
    assert!(
        authority
            .recover_dead_plan_members(&plan_members, &requests)
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), live_before);

    authority
        .withdraw_process_bound_plan_members_with_lifetimes(
            &plan_members,
            &bindings[..1],
            &lifetime_refs(&lifetimes[..1]),
        )
        .expect("one selected listener should withdraw");
    drop(lifetimes);
    let recoveries = authority
        .recover_dead_plan_members(&plan_members, &requests)
        .expect("both dead members should recover despite their mixed phases");
    let mixed_before = authority_bytes(&authority);
    assert!(
        authority
            .release_process_bound_plan_members_after_owner_death(
                &plan_members,
                &requests,
                &recoveries,
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), mixed_before);
    assert!(
        authority
            .release_process_bound_plan_members_after_owner_death(
                &plan_members,
                &requests[..1],
                &recoveries[1..],
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), mixed_before);
}

#[test]
fn dead_owner_duplicate_incomplete_and_provider_managed_inputs_are_byte_unchanged() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active,
    } = PlannedFixture::new(&["alpha", "beta"]);
    let requests = active
        .iter()
        .map(|active| active.request.clone())
        .collect::<Vec<_>>();
    let lifetimes = active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    drop(lifetimes);
    let recoveries = authority
        .recover_dead_plan_members(&plan_members, &requests)
        .expect("dead process-bound members should recover");
    let before = authority_bytes(&authority);
    let duplicate = vec![requests[0].clone(), requests[0].clone()];
    assert!(
        authority
            .release_process_bound_plan_members_after_owner_death(
                &plan_members,
                &duplicate,
                &recoveries,
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);
    assert!(
        authority
            .release_process_bound_plan_members_after_owner_death(
                &plan_members[..2],
                &requests,
                &recoveries,
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);

    let PlannedFixture {
        _root: _provider_root,
        authority: provider_authority,
        plan_members: provider_plan,
        active: provider_active,
    } = PlannedFixture::new_with_scopes(&[("alpha", PortLeaseEffectScope::ProviderManaged)]);
    let provider_requests = provider_active
        .iter()
        .map(|active| active.request.clone())
        .collect::<Vec<_>>();
    let provider_lifetimes = provider_active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();
    drop(provider_lifetimes);
    let provider_recoveries = provider_authority
        .recover_dead_plan_members(&provider_plan, &provider_requests)
        .expect("dead provider-managed member should yield recovery authority");
    let provider_before = authority_bytes(&provider_authority);
    assert!(
        provider_authority
            .release_process_bound_plan_members_after_owner_death(
                &provider_plan,
                &provider_requests,
                &provider_recoveries,
            )
            .is_err()
    );
    assert_eq!(authority_bytes(&provider_authority), provider_before);
}

#[test]
fn empty_terminal_selections_reject_without_authority_changes() {
    let PlannedFixture {
        _root,
        authority,
        plan_members,
        active: _,
    } = PlannedFixture::new(&[]);
    let before = authority_bytes(&authority);
    assert!(
        authority
            .withdraw_process_bound_plan_members_with_lifetimes(&plan_members, &[], &[])
            .is_err()
    );
    assert!(
        authority
            .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
                &plan_members,
                &[],
                &[],
            )
            .is_err()
    );
    assert!(
        authority
            .release_process_bound_plan_members_after_owner_death(&plan_members, &[], &[])
            .is_err()
    );
    assert_eq!(authority_bytes(&authority), before);
}

fn authority_bytes(authority: &LocalPortLeaseAuthority) -> Vec<u8> {
    std::fs::read(authority.authority_path()).expect("durable authority bytes should be readable")
}

fn lifetime_refs(lifetimes: &[PortLeaseLifetimeGuard]) -> Vec<&PortLeaseLifetimeGuard> {
    lifetimes.iter().collect()
}

fn sibling_bytes(authority: &LocalPortLeaseAuthority, request: &PortLeaseRequest) -> Vec<u8> {
    serde_json::to_vec(
        &authority
            .inspect(request.lease_id())
            .expect("plan sibling should inspect")
            .expect("plan sibling should remain durable"),
    )
    .expect("plan sibling should serialize")
}

fn request(tenant: &TenantId, plan_id: &NetworkPlanId, role: &str, port: u16) -> PortLeaseRequest {
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&ListenerId::for_tenant_workload_listener(
            tenant,
            "workload-a",
            role,
        )),
        owner_id(tenant, role),
        Some(tenant.clone()),
        PortLeaseFence::new(
            NetworkResourceGeneration::new(7),
            NetworkLeaseEpoch::new(11),
        ),
        if role == "pep" {
            PortLeaseAccounting::HostInternal
        } else {
            PortLeaseAccounting::TenantPublished
        },
        if role == "pep" {
            PortPublicationIntent::Unpublished
        } else {
            PortPublicationIntent::host(std::net::Ipv4Addr::LOCALHOST.into())
        },
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(NonZeroU16::new(port).expect("fixture port must be non-zero")),
        ),
    )
    .with_plan_id(plan_id.clone())
}

fn owner_id(tenant: &TenantId, role: &str) -> NetworkResourceId {
    ListenerId::for_tenant_workload_listener(tenant, "workload-a", role).into()
}

fn reservation_claim() -> NetworkReservationClaim {
    NetworkReservationClaim::new(provider_handle("launch-reservation"))
}

fn bind_claim(attempt: &str) -> PortBindClaim {
    PortBindClaim::new(provider_handle(attempt))
}

fn provider_handle(attempt: &str) -> NetworkProviderHandle {
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key("nimbus-network.terminal-settlement-test"),
        attempt,
    )
    .expect("fixture provider handle should validate")
}

fn binding(request: &PortLeaseRequest, provider: NetworkProviderHandle) -> PortLeaseBinding {
    let PortRequestMode::Exact(port) = request.binding().port() else {
        panic!("fixture request should carry one exact port");
    };
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
            *port,
        )
        .expect("fixture endpoint should validate"),
        PortBindingProvenance::NimbusOwned,
        provider,
    )
}
