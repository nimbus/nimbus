use std::num::NonZeroU16;

use nimbus_core::TenantId;

use super::*;
use crate::{
    ListenerId, NetworkLeaseEpoch, NetworkPlanId, NetworkProviderHandle, NetworkProviderId,
    NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId, PortBindClaim,
    PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint,
    PortExposure, PortLeaseAccounting, PortLeaseFence, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};

const FIRST_PORT: u16 = 45_701;

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
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let tenant = TenantId::new("tenant-restart-retain")
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
        for role in active_roles {
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
                    PortLeaseEffectScope::ProcessBound,
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

    fn authority_bytes(&self) -> Vec<u8> {
        std::fs::read(self.authority.authority_path())
            .expect("durable authority bytes should be readable")
    }
}

#[test]
fn planned_confirmed_stop_retains_owned_subset_and_preserves_unrelated_member() {
    let fixture = PlannedFixture::new(&["alpha"]);
    let active = &fixture.active[0];
    let unrelated = &fixture.plan_members[2];
    let unrelated_before = serde_json::to_vec(
        &fixture
            .authority
            .inspect(unrelated.lease_id())
            .expect("unrelated member should inspect")
            .expect("unrelated member should remain durable"),
    )
    .expect("unrelated member should serialize");
    let first_lifetime = active.lifetime.lifetime();

    let retained = fixture
        .authority
        .prepare_rebind_plan_members_after_confirmed_stop_with_lifetimes(
            &fixture.plan_members,
            &[(active.request.clone(), active.binding.clone())],
            std::slice::from_ref(&active.lifetime),
        )
        .expect("exact process-owned subset should retain atomically");
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].phase(), PortLeasePhase::Reserved);
    assert_eq!(
        retained[0].confirmed_stopped_binding(),
        Some(&active.binding)
    );
    assert!(retained[0].binding().is_none());
    assert!(retained[0].active_lifetime().is_none());
    assert_eq!(
        serde_json::to_vec(
            &fixture
                .authority
                .inspect(unrelated.lease_id())
                .expect("unrelated member should inspect")
                .expect("unrelated member should remain durable"),
        )
        .expect("unrelated member should serialize"),
        unrelated_before,
        "the unrelated PEP member must remain byte-for-byte unchanged"
    );

    let request = active.request.clone();
    let confirmed = active.binding.clone();
    drop(fixture.active);
    let next_claim = bind_claim("replacement-alpha");
    let next = fixture
        .authority
        .claim_rebind_plan_member_with_lifetime(
            &fixture.plan_members,
            &request,
            &confirmed,
            next_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("retained member should claim its next process lifetime");
    assert!(next.lifetime().generation() > first_lifetime.generation());
    let replacement = binding(&request, next_claim.provider_attempt().clone());
    assert_eq!(replacement.actual_port(), confirmed.actual_port());
    let rebound = fixture
        .authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &fixture.plan_members,
            &request,
            &confirmed,
            &next_claim,
            replacement,
            &next,
        )
        .expect("same numeric slot should activate under the higher lifetime");
    assert_eq!(rebound.phase(), PortLeasePhase::Active);
}

#[test]
fn crossed_plan_witness_rejects_without_mutation() {
    let fixture = PlannedFixture::new(&["alpha"]);
    let active = &fixture.active[0];
    let tenant =
        TenantId::new("tenant-restart-retain").expect("fixture tenant identity should validate");
    let crossed_plan = NetworkPlanId::for_tenant_workload_plan(&tenant, "crossed-generation");
    let mut witness = fixture.plan_members.clone();
    witness[2] = witness[2].clone().with_plan_id(crossed_plan);
    let before = fixture.authority_bytes();

    assert!(
        fixture
            .authority
            .prepare_rebind_plan_members_after_confirmed_stop_with_lifetimes(
                &witness,
                &[(active.request.clone(), active.binding.clone())],
                std::slice::from_ref(&active.lifetime),
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.authority.authority_path())
            .expect("durable authority bytes should remain readable"),
        before
    );
}

#[test]
fn crossed_member_or_lifetime_rejects_without_mutation() {
    let fixture = PlannedFixture::new(&["alpha", "beta"]);
    let alpha = &fixture.active[0];
    let beta = &fixture.active[1];
    let before = fixture.authority_bytes();

    assert!(matches!(
        fixture
            .authority
            .prepare_rebind_plan_members_after_confirmed_stop_with_lifetimes(
                &fixture.plan_members,
                &[(alpha.request.clone(), alpha.binding.clone())],
                std::slice::from_ref(&beta.lifetime),
            ),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(fixture.authority_bytes(), before);
}

#[test]
fn duplicate_selected_member_is_rejected_without_mutation() {
    let fixture = PlannedFixture::new(&["alpha"]);
    let active = &fixture.active[0];
    let before = fixture.authority_bytes();
    let duplicate = [
        (active.request.clone(), active.binding.clone()),
        (active.request.clone(), active.binding.clone()),
    ];

    assert!(matches!(
        fixture
            .authority
            .prepare_rebind_plan_members_after_confirmed_stop_with_lifetimes(
                &fixture.plan_members,
                &duplicate,
                std::slice::from_ref(&active.lifetime),
            ),
        Err(PortLeaseError::IdentityConflict { .. })
    ));
    assert_eq!(fixture.authority_bytes(), before);
}

#[test]
fn invalid_sibling_binding_leaves_complete_selected_subset_active() {
    let fixture = PlannedFixture::new(&["alpha", "beta"]);
    let alpha_request = fixture.active[0].request.clone();
    let alpha_binding = fixture.active[0].binding.clone();
    let beta_request = fixture.active[1].request.clone();
    let beta_binding = fixture.active[1].binding.clone();
    let crossed_beta = binding(&alpha_request, beta_binding.provider_handle().clone());
    let selected_requests = [alpha_request.clone(), beta_request.clone()];
    let before = fixture.authority_bytes();
    let lifetimes = fixture
        .active
        .into_iter()
        .map(|active| active.lifetime)
        .collect::<Vec<_>>();

    assert!(
        fixture
            .authority
            .prepare_rebind_plan_members_after_confirmed_stop_with_lifetimes(
                &fixture.plan_members,
                &[(alpha_request, alpha_binding), (beta_request, crossed_beta),],
                &lifetimes,
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.authority.authority_path())
            .expect("durable authority bytes should remain readable"),
        before
    );
    for request in &selected_requests {
        assert_eq!(
            fixture
                .authority
                .inspect(request.lease_id())
                .expect("selected member should inspect")
                .expect("selected member should remain durable")
                .phase(),
            PortLeasePhase::Active
        );
    }
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
        NetworkProviderId::for_registration_key("nimbus-server.restart-retain-test"),
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
