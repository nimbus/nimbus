use std::num::NonZeroU16;
use std::time::Duration;

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, NetworkResourceId, PortLeaseBinding,
    PortLeaseError, PortLeaseId, PortLeasePhase, PortLeaseRequest,
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
        .run(root.path(), [child("alpha"), child("beta")])
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
    assert_eq!(records[0].request().requested_port().get(), PORT);
    assert_eq!(
        records[0].phase(),
        PortLeasePhase::Active,
        "the sole durable winner must complete activation"
    );
}

#[test]
#[ignore = "spawned only by the real-process port-lease contention parent"]
fn network_port_lease_child() {
    let mode = std::env::var(MODE_ENV).expect("child test mode should be set");
    assert_eq!(mode, "contend");

    run_contention_child(|context| {
        let authority = LocalPortLeaseAuthority::open(context.state_root())
            .map_err(|error| format!("failed to open port authority: {error}"))?;
        let request = request(context.role());
        match authority.reserve(request.clone()) {
            Ok(_) => {
                authority
                    .adopt(&request, binding(context.role()))
                    .map_err(|error| format!("failed to adopt provider evidence: {error}"))?;
                authority
                    .activate(&request)
                    .map_err(|error| format!("failed to activate port lease: {error}"))?;
                Ok(ContentionOutcome::Won)
            }
            Err(PortLeaseError::PortConflict { .. }) => Ok(ContentionOutcome::Lost),
            Err(error) => Err(format!("unexpected reservation failure: {error}")),
        }
    })
    .unwrap_or_else(|error| panic!("port-lease child failed: {error}"));
}

fn child(role: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(MODE_ENV, "contend")
}

fn request(role: &str) -> PortLeaseRequest {
    PortLeaseRequest::new_exact(
        lease_id(role),
        owner_id(role),
        None,
        NetworkResourceGeneration::new(7),
        NetworkLeaseEpoch::new(11),
        NonZeroU16::new(PORT).expect("fixture port is non-zero"),
    )
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

fn binding(role: &str) -> PortLeaseBinding {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    PortLeaseBinding::new(
        NonZeroU16::new(PORT).expect("fixture port is non-zero"),
        NetworkProviderHandle::new(provider_id, format!("process-{role}"))
            .expect("fixture provider handle should validate"),
    )
}
