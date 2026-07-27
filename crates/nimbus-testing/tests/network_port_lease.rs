use std::num::NonZeroU16;
use std::time::Duration;

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, NetworkResourceId, PortBindClaim, PortBindRealm,
    PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure,
    PortLeaseBinding, PortLeaseError, PortLeaseId, PortLeasePhase, PortLeaseRequest, PortProtocol,
    PortRequestMode,
};
use nimbus_testing::{
    ContentionOutcome, ProcessRoleSpec, TwoProcessContentionHarness, run_contention_child,
};

const CHILD_TEST: &str = "network_port_lease_child";
const MODE_ENV: &str = "NIMBUS_NETWORK_PORT_LEASE_TEST_MODE";
const PORT: u16 = 41_473;

#[test]
fn two_real_processes_cannot_reserve_the_same_host_port() {
    let root = tempfile::tempdir().expect("state root should exist");
    let result = TwoProcessContentionHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            [child("alpha", "exact"), child("beta", "exact")],
        )
        .unwrap_or_else(|error| panic!("port-lease contention failed: {error}"));

    assert_ne!(result.winner(), result.contender());

    let authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should reopen");
    let records = authority.list().expect("durable leases should be readable");
    assert_eq!(
        records.len(),
        1,
        "one shared lock/store domain must retain exactly one winner"
    );
    assert_eq!(
        records[0].request().owner_id(),
        &owner_id(result.winner()),
        "the durable owner must agree with the process-harness winner"
    );
    assert_eq!(records[0].reserved_port().map(NonZeroU16::get), Some(PORT));
    assert_eq!(
        records[0].phase(),
        PortLeasePhase::Active,
        "the sole durable winner must complete activation"
    );
}

#[test]
fn two_real_processes_choose_distinct_slots_from_one_range() {
    let root = tempfile::tempdir().expect("state root should exist");
    TwoProcessContentionHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            [child("alpha", "range"), child("beta", "range")],
        )
        .unwrap_or_else(|error| panic!("range contention failed: {error}"));

    let authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should reopen");
    let mut ports = authority
        .list()
        .expect("durable leases should be readable")
        .into_iter()
        .map(|record| {
            assert_eq!(record.phase(), PortLeasePhase::Reserved);
            record
                .reserved_port()
                .expect("range reservation must select a slot")
                .get()
        })
        .collect::<Vec<_>>();
    ports.sort_unstable();
    assert_eq!(ports, vec![PORT, PORT + 1]);
}

#[test]
fn two_real_processes_cannot_adopt_one_provider_assigned_port() {
    let root = tempfile::tempdir().expect("state root should exist");
    let result = TwoProcessContentionHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            [child("alpha", "provider"), child("beta", "provider")],
        )
        .unwrap_or_else(|error| panic!("provider-adoption contention failed: {error}"));

    let authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should reopen");
    let records = authority.list().expect("durable leases should be readable");
    assert_eq!(records.len(), 2, "both stable identities must survive");
    let winner = records
        .iter()
        .find(|record| record.request().owner_id() == &owner_id(result.winner()))
        .expect("winner record should exist");
    let contender = records
        .iter()
        .find(|record| record.request().owner_id() == &owner_id(result.contender()))
        .expect("contender record should exist");
    assert_eq!(winner.phase(), PortLeasePhase::Binding);
    assert_eq!(winner.reserved_port().map(NonZeroU16::get), Some(PORT));
    assert_eq!(contender.phase(), PortLeasePhase::Reserved);
    assert_eq!(
        contender.reserved_port(),
        None,
        "failed adoption must publish no numeric fence"
    );
}

#[test]
fn two_real_processes_same_request_get_exactly_one_bind_attempt_claim() {
    let root = tempfile::tempdir().expect("state root should exist");
    let result = TwoProcessContentionHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            [
                child("alpha", "same-request-claim"),
                child("beta", "same-request-claim"),
            ],
        )
        .unwrap_or_else(|error| panic!("same-request bind-claim contention failed: {error}"));

    let authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should reopen");
    let records = authority.list().expect("durable leases should be readable");
    assert_eq!(records.len(), 1, "identical request replay has one record");
    assert_eq!(records[0].phase(), PortLeasePhase::Reserved);
    let claim = records[0]
        .bind_claim()
        .expect("the winning provider attempt must remain durably claimed");
    assert_eq!(
        claim.provider_attempt().expose_to_provider(),
        format!("same-request-claim-{}", result.winner()),
        "durable attempt identity must agree with the real-process winner"
    );
    assert_eq!(
        records[0].request().owner_id(),
        &owner_id("alpha"),
        "both processes must have replayed the exact same stable request"
    );
}

#[test]
#[ignore = "spawned only by the real-process port-lease contention parent"]
fn network_port_lease_child() {
    let mode = std::env::var(MODE_ENV).expect("child test mode should be set");

    run_contention_child(|context| {
        let authority = LocalPortLeaseAuthority::open(context.state_root())
            .map_err(|error| format!("failed to open port authority: {error}"))?;
        match mode.as_str() {
            "exact" => {
                let request = request(context.role(), exact_port());
                match authority.reserve(request.clone()) {
                    Ok(_) => {
                        let claim = bind_claim(context.role());
                        authority
                            .claim_bind(&request, None, claim.clone())
                            .map_err(|error| {
                                format!("failed to claim provider attempt: {error}")
                            })?;
                        authority
                            .adopt_claimed(
                                &request,
                                None,
                                &claim,
                                binding(context.role(), PortBindingProvenance::NimbusOwned),
                            )
                            .map_err(|error| {
                                format!("failed to adopt provider evidence: {error}")
                            })?;
                        authority
                            .activate_claimed(&request, &claim)
                            .map_err(|error| format!("failed to activate port lease: {error}"))?;
                        Ok(ContentionOutcome::Won)
                    }
                    Err(PortLeaseError::PortConflict { .. }) => Ok(ContentionOutcome::Lost),
                    Err(error) => Err(format!("unexpected reservation failure: {error}")),
                }
            }
            "range" => {
                let range = PortRequestMode::range(port(PORT), port(PORT + 1))
                    .map_err(|error| format!("invalid fixture range: {error}"))?;
                let record = authority
                    .reserve(request(context.role(), range))
                    .map_err(|error| format!("range reservation failed: {error}"))?;
                match record.reserved_port().map(NonZeroU16::get) {
                    Some(PORT) => Ok(ContentionOutcome::Won),
                    Some(value) if value == PORT + 1 => Ok(ContentionOutcome::Lost),
                    other => Err(format!("unexpected selected range port: {other:?}")),
                }
            }
            "provider" => {
                let request = request(context.role(), PortRequestMode::ProviderAssigned);
                authority
                    .reserve(request.clone())
                    .map_err(|error| format!("provider identity reservation failed: {error}"))?;
                let claim = bind_claim(context.role());
                authority
                    .claim_bind(&request, None, claim.clone())
                    .map_err(|error| format!("provider attempt claim failed: {error}"))?;
                match authority.adopt_claimed(
                    &request,
                    None,
                    &claim,
                    binding(context.role(), PortBindingProvenance::ProviderAssigned),
                ) {
                    Ok(_) => Ok(ContentionOutcome::Won),
                    Err(PortLeaseError::PortConflict { .. }) => Ok(ContentionOutcome::Lost),
                    Err(error) => Err(format!("unexpected provider adoption failure: {error}")),
                }
            }
            "same-request-claim" => {
                let request = request("alpha", exact_port());
                authority
                    .reserve(request.clone())
                    .map_err(|error| format!("shared request reservation failed: {error}"))?;
                match authority.claim_bind(&request, None, bind_claim(context.role())) {
                    Ok(_) => Ok(ContentionOutcome::Won),
                    Err(PortLeaseError::BindClaimConflict { .. }) => Ok(ContentionOutcome::Lost),
                    Err(error) => Err(format!("unexpected bind-claim failure: {error}")),
                }
            }
            other => Err(format!("unexpected child mode {other:?}")),
        }
    })
    .unwrap_or_else(|error| panic!("port-lease child failed: {error}"));
}

fn child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(MODE_ENV, mode)
}

fn request(role: &str, port_request: PortRequestMode) -> PortLeaseRequest {
    PortLeaseRequest::new(
        lease_id(role),
        owner_id(role),
        None,
        nimbus_network::PortLeaseFence::new(
            NetworkResourceGeneration::new(7),
            NetworkLeaseEpoch::new(11),
        ),
        nimbus_network::PortLeaseAccounting::HostInternal,
        nimbus_network::PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            port_request,
        ),
    )
}

fn exact_port() -> PortRequestMode {
    PortRequestMode::Exact(port(PORT))
}

fn port(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).expect("fixture port is non-zero")
}

fn lease_id(role: &str) -> PortLeaseId {
    match role {
        "alpha" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "beta" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        other => panic!("unexpected fixture role {other:?}"),
    }
    .parse()
    .expect("fixture lease id should parse")
}

fn owner_id(role: &str) -> NetworkResourceId {
    let value: ListenerId = match role {
        "alpha" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "beta" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        other => panic!("unexpected fixture role {other:?}"),
    }
    .parse()
    .expect("fixture listener id should parse");
    value.into()
}

fn binding(role: &str, provenance: PortBindingProvenance) -> PortLeaseBinding {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            port(PORT),
        )
        .expect("fixture endpoint should validate"),
        provenance,
        NetworkProviderHandle::new(provider_id, format!("process-{role}"))
            .expect("fixture provider handle should validate"),
    )
}

fn bind_claim(role: &str) -> PortBindClaim {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    PortBindClaim::new(
        NetworkProviderHandle::new(provider_id, format!("same-request-claim-{role}"))
            .expect("fixture bind claim should validate"),
    )
}
