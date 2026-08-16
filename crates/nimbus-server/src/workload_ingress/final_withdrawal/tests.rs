use std::net::Ipv4Addr;
use std::num::NonZeroU16;

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkPlanId, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, NetworkResourceId, PortBindClaim, PortBindRealm,
    PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure,
    PortLeaseAccounting, PortLeaseFence, PortLeaseId, PortLeasePhase, PortLeaseRequest,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};

use super::{
    AuthenticatedAbsentIngressInspection, AuthenticatedDurableIngressPlan, FinalIngressPhase,
    PublishedIngressAuthority, inspect_authenticated_absent_ingress,
    settle_authenticated_absent_ingress,
};

#[test]
fn direct_fixture_never_authenticates_a_portable_publication() {
    let authority = PublishedIngressAuthority::direct_fixture();
    assert!(authority.reference.is_none());
    assert!(authority.provider_source_digest.is_none());
    assert!(authority.workload_source_digest.is_none());
    assert_eq!(FinalIngressPhase::Published, FinalIngressPhase::Published);
}

#[test]
fn final_withdrawal_releases_exact_restart_retained_publication_without_rebind() {
    let root = tempfile::tempdir().expect("retained final-withdrawal root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("port authority should open");
    let tenant_id = nimbus_core::TenantId::new("tenant-final-retained")
        .expect("fixture tenant should validate");
    let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant_id, "workload-a");
    let listener_id = ListenerId::for_tenant_workload_listener(&tenant_id, "workload-a", "default");
    let request = PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener_id),
        NetworkResourceId::from(listener_id),
        Some(tenant_id),
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(NonZeroU16::new(15_992).expect("fixture port is non-zero")),
        ),
    )
    .with_plan_id(plan_id);
    authority
        .reserve_batch(vec![request.clone()])
        .expect("complete published plan should reserve");
    let provider_handle = NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key("nimbus-server.final-retained-test"),
        "final-retained-binding",
    )
    .expect("fixture provider handle should validate");
    let claim = PortBindClaim::new(provider_handle.clone());
    let binding = nimbus_network::PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            NonZeroU16::new(15_992).expect("fixture port is non-zero"),
        )
        .expect("fixture endpoint should validate"),
        PortBindingProvenance::NimbusOwned,
        provider_handle,
    );
    authority
        .claim_bind(&request, None, claim.clone())
        .expect("published bind should claim");
    authority
        .adopt_claimed(&request, None, &claim, binding.clone())
        .expect("published bind should adopt");
    authority
        .activate_claimed(&request, &claim)
        .expect("published bind should activate");
    let retained = authority
        .prepare_rebind_after_confirmed_stop(&request, &binding)
        .expect("owner exit should retain exact confirmed-stop evidence");
    assert_eq!(retained.phase(), PortLeasePhase::Reserved);
    assert!(retained.active_lifetime().is_none());
    assert_eq!(retained.confirmed_stopped_binding(), Some(&binding));
    let before_inspection = authority
        .list()
        .expect("retained authority should inspect before final withdrawal");
    let Ok(AuthenticatedAbsentIngressInspection::RetryRequired(inspected)) =
        inspect_authenticated_absent_ingress(
            &authority,
            AuthenticatedDurableIngressPlan {
                plan_members: vec![request.clone()],
                ingress_records: vec![retained.clone()],
            },
        )
    else {
        panic!("inspection should authorize one fenced effect retry");
    };
    assert_eq!(inspected, vec![retained.clone()]);
    assert_eq!(
        authority
            .list()
            .expect("retained authority should inspect after final withdrawal inspection"),
        before_inspection,
        "inspection must not consume confirmed-stop evidence"
    );

    let Ok(settled) = settle_authenticated_absent_ingress(
        &authority,
        AuthenticatedDurableIngressPlan {
            plan_members: vec![request.clone()],
            ingress_records: vec![retained],
        },
    ) else {
        panic!("final withdrawal should consume confirmed-stop evidence without a rebind");
    };

    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].phase(), PortLeasePhase::Released);
    assert!(settled[0].confirmed_stopped_binding().is_none());
    assert_eq!(
        authority
            .release_plan_members_after_confirmed_stop(
                std::slice::from_ref(&request),
                std::slice::from_ref(&request),
            )
            .expect("terminal final-withdrawal replay should be idempotent"),
        settled
    );
}
