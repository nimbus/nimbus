use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::num::NonZeroU16;
use std::time::Duration;

use nimbus_network::{
    ListenerId, LocalNetworkManager, LocalPortLeaseAuthority, NetworkCapabilityRegistry,
    NetworkLeaseEpoch, NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim,
    NetworkReservationLifetimeAttempt, NetworkResourceGeneration, NetworkResourceId, PortBindClaim,
    PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint,
    PortExposure, PortLeaseBinding, PortLeaseEffectScope, PortLeaseError, PortLeaseId,
    PortLeasePhase, PortLeaseRecoveryAttempt, PortLeaseRequest, PortProtocol, PortRequestMode,
};
use nimbus_process_harness::{
    ContentionOutcome, PortWindow, ProcessRoleSpec, SubprocessCrashCutHarness,
    TwoProcessContentionHarness, run_contention_child, run_crash_cut_child,
    run_crash_recovery_child,
};

const CHILD_TEST: &str = "network_port_lease_child";
const RECOVERY_CHILD_TEST: &str = "network_port_lease_recovery_child";
const MODE_ENV: &str = "NIMBUS_NETWORK_PORT_LEASE_TEST_MODE";
const LISTENER_PORT_ENV: &str = "NIMBUS_NETWORK_PORT_LEASE_LISTENER_PORT";
/// Reservation fixtures address this port as a number only. Nothing binds it,
/// so it stays a plain constant rather than a claimed window port.
const PORT: u16 = 41_473;
const ACTIVE_LISTENER_BOUNDARY: &str = "network.port-lease.active-listener-owned";
const RECOVERED_LISTENER_OBSERVATION: &str = "kernel-free:lease-released:replacement-reserved";
const ABANDONED_RESERVATION_BOUNDARY: &str =
    "network.port-lease.reserved-before-manifest-publication";
const RECOVERED_RESERVATION_OBSERVATION: &str =
    "lifetime-dead:no-effect-authenticated:lease-released:replacement-reserved";

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
fn fresh_process_reconciles_a_dead_process_owned_listener_before_port_reuse() {
    let root = tempfile::tempdir().expect("state root should exist");
    // The crash child binds this port; the recovery child re-binds the same
    // number once the parent has killed that owner. This parent holds the
    // window across the kill, so the replacement bind proves the dead owner's
    // socket is gone rather than racing an unrelated process for it.
    let ports = PortWindow::claim();
    SubprocessCrashCutHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            ACTIVE_LISTENER_BOUNDARY,
            RECOVERED_LISTENER_OBSERVATION,
            recovery_child("active-listener-owner", "crash-active-listener")
                .env(LISTENER_PORT_ENV, ports.port(0).to_string()),
            recovery_child("fresh-listener-reconciler", "recover-active-listener"),
        )
        .unwrap_or_else(|error| panic!("dead-listener recovery failed: {error}"));
}

#[test]
fn fresh_process_releases_only_an_abandoned_never_bound_reservation() {
    let root = tempfile::tempdir().expect("state root should exist");
    SubprocessCrashCutHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            ABANDONED_RESERVATION_BOUNDARY,
            RECOVERED_RESERVATION_OBSERVATION,
            recovery_child(
                "reservation-publication-owner",
                "crash-reserved-before-publication",
            ),
            recovery_child(
                "fresh-reservation-reconciler",
                "recover-abandoned-reservation",
            ),
        )
        .unwrap_or_else(|error| panic!("abandoned-reservation recovery failed: {error}"));
}

#[test]
#[ignore = "spawned only by the real-process port-lease contention parent"]
fn network_port_lease_child() {
    let mode = std::env::var(MODE_ENV).expect("child test mode should be set");

    run_contention_child(|context| {
        let manager = process_manager(context.state_root())?;
        let authority = manager.port_leases();
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

#[test]
#[ignore = "spawned only by the real-process port-lease recovery parent"]
fn network_port_lease_recovery_child() {
    let mode = std::env::var(MODE_ENV).expect("child test mode should be set");
    match mode.as_str() {
        "crash-active-listener" => run_crash_cut_child(|context| {
            let manager = process_manager(context.state_root())?;
            let authority = manager.port_leases();
            let request = recovery_request("alpha", PortRequestMode::ProviderAssigned);
            authority
                .reserve(request.clone())
                .map_err(|error| format!("failed to reserve crash-owned lease: {error}"))?;
            let claim = bind_claim("alpha");
            let lifetime = authority
                .claim_bind_with_lifetime(
                    &request,
                    None,
                    claim.clone(),
                    PortLeaseEffectScope::ProcessBound,
                )
                .map_err(|error| {
                    format!("failed to claim crash-owned bind and lifetime: {error}")
                })?;
            // The request stays provider-assigned: the lease still adopts
            // whatever port the socket reports. Only the number's source
            // changes, from an ephemeral pick nobody owns after this process
            // dies to a port the parent's window keeps claimed.
            let fixture_port: u16 = std::env::var(LISTENER_PORT_ENV)
                .map_err(|error| format!("missing crash-owned listener port: {error}"))?
                .parse()
                .map_err(|error| format!("invalid crash-owned listener port: {error}"))?;
            let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, fixture_port))
                .map_err(|error| format!("failed to bind crash-owned listener: {error}"))?;
            let actual_port = NonZeroU16::new(
                listener
                    .local_addr()
                    .map_err(|error| format!("failed to inspect crash-owned listener: {error}"))?
                    .port(),
            )
            .ok_or_else(|| "crash-owned listener reported port zero".to_owned())?;
            authority
                .adopt_claimed_and_activate_with_lifetime(
                    &request,
                    None,
                    &claim,
                    recovery_binding("alpha", actual_port),
                    &lifetime,
                )
                .map_err(|error| {
                    format!("failed to adopt and activate crash-owned listener: {error}")
                })?;
            context.reach_boundary(ACTIVE_LISTENER_BOUNDARY)?;
            drop(listener);
            Err("crash child resumed after the parent-owned kill boundary".to_owned())
        })
        .unwrap_or_else(|error| panic!("active-listener crash child failed: {error}")),
        "recover-active-listener" => run_crash_recovery_child(|context| {
            let manager = process_manager(context.state_root())?;
            let authority = manager.port_leases();
            let active = authority
                .inspect(&lease_id("alpha"))
                .map_err(|error| format!("failed to inspect crash-owned lease: {error}"))?
                .ok_or_else(|| "crash-owned lease disappeared".to_owned())?;
            if active.phase() != PortLeasePhase::Active {
                return Err(format!(
                    "crash-owned lease should remain Active before reconciliation, got {:?}",
                    active.phase()
                ));
            }
            let actual_port = active
                .reserved_port()
                .ok_or_else(|| "active crash-owned lease omitted its actual port".to_owned())?;
            let request = recovery_request("alpha", PortRequestMode::ProviderAssigned);
            let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
                .recover_dead_lifetime(&request)
                .map_err(|error| format!("failed to acquire dead-owner recovery: {error}"))?
            else {
                return Err("killed listener owner was still reported live or settled".to_owned());
            };
            authority
                .mark_cleanup_pending_after_owner_death(&request, &recovery)
                .map_err(|error| format!("failed to quarantine dead listener: {error}"))?;
            authority
                .release_process_bound_after_owner_death(&request, &recovery)
                .map_err(|error| format!("failed to release dead process binding: {error}"))?;

            let replacement = recovery_request("beta", PortRequestMode::Exact(actual_port));
            match authority.reserve(replacement) {
                Ok(_) => {
                    let replacement_listener = TcpListener::bind(SocketAddrV4::new(
                        Ipv4Addr::LOCALHOST,
                        actual_port.get(),
                    ))
                    .map_err(|error| {
                        format!(
                            "released process-bound listener should permit the replacement bind \
                             on port {actual_port}: {error}"
                        )
                    })?;
                    drop(replacement_listener);
                    Ok(RECOVERED_LISTENER_OBSERVATION.to_owned())
                }
                Err(PortLeaseError::PortConflict { .. }) => {
                    Ok("kernel-free:lease-active:replacement-conflict".to_owned())
                }
                Err(error) => Err(format!(
                    "unexpected replacement reservation outcome after owner death: {error}"
                )),
            }
        })
        .unwrap_or_else(|error| panic!("active-listener recovery child failed: {error}")),
        "crash-reserved-before-publication" => run_crash_cut_child(|context| {
            let manager = process_manager(context.state_root())?;
            let authority = manager.port_leases();
            let claim = launch_reservation_claim();
            let _lifetime = match authority
                .try_acquire_reservation_lifetime(&claim)
                .map_err(|error| format!("failed to acquire publication lifetime: {error}"))?
            {
                NetworkReservationLifetimeAttempt::Acquired(lifetime) => lifetime,
                NetworkReservationLifetimeAttempt::LiveOwner => {
                    return Err("first publication coordinator unexpectedly contended".to_owned());
                }
            };
            authority
                .reserve_batch_for_coordinator(
                    vec![recovery_request(
                        "alpha",
                        PortRequestMode::Exact(port(PORT)),
                    )],
                    &claim,
                )
                .map_err(|error| format!("failed to reserve pre-publication lease: {error}"))?;
            context.reach_boundary(ABANDONED_RESERVATION_BOUNDARY)?;
            Err("reservation child resumed after the parent-owned kill boundary".to_owned())
        })
        .unwrap_or_else(|error| panic!("reservation crash child failed: {error}")),
        "recover-abandoned-reservation" => run_crash_recovery_child(|context| {
            let manager = process_manager(context.state_root())?;
            let authority = manager.port_leases();
            let request = recovery_request("alpha", PortRequestMode::Exact(port(PORT)));
            let retained = authority
                .inspect(request.lease_id())
                .map_err(|error| format!("failed to inspect abandoned reservation: {error}"))?
                .ok_or_else(|| "abandoned reservation disappeared".to_owned())?;
            if retained.phase() != PortLeasePhase::Reserved
                || retained.bind_claim().is_some()
                || retained.binding().is_some()
                || retained.failure().is_some()
            {
                return Err(format!(
                    "fresh recovery requires exact never-bound evidence, found {retained:?}"
                ));
            }
            let claim = launch_reservation_claim();
            let lifetime = match authority
                .try_acquire_reservation_lifetime(&claim)
                .map_err(|error| format!("failed to inspect dead publication lifetime: {error}"))?
            {
                NetworkReservationLifetimeAttempt::Acquired(lifetime) => lifetime,
                NetworkReservationLifetimeAttempt::LiveOwner => {
                    return Err("killed publication coordinator was still reported live".to_owned());
                }
            };
            let released = authority
                .release_reserved_batch_without_effect_with_lifetime(
                    std::slice::from_ref(&request),
                    &lifetime,
                )
                .map_err(|error| {
                    format!("failed to release exact never-bound reservation: {error}")
                })?;
            if released[0].phase() != PortLeasePhase::Released {
                return Err(format!(
                    "abandoned reservation should be Released, got {:?}",
                    released[0].phase()
                ));
            }
            let replayed = authority
                .release_reserved_batch_without_effect_with_lifetime(&[request], &lifetime)
                .map_err(|error| format!("exact release replay failed: {error}"))?;
            if replayed != released {
                return Err(
                    "exact abandoned-reservation replay changed durable evidence".to_owned(),
                );
            }
            authority
                .reserve(recovery_request("beta", PortRequestMode::Exact(port(PORT))))
                .map_err(|error| {
                    format!("released reservation should permit replacement: {error}")
                })?;
            Ok(RECOVERED_RESERVATION_OBSERVATION.to_owned())
        })
        .unwrap_or_else(|error| panic!("abandoned-reservation recovery child failed: {error}")),
        other => panic!("unknown port-lease recovery child mode {other:?}"),
    }
}

fn process_manager(
    state_root: &std::path::Path,
) -> Result<std::sync::Arc<LocalNetworkManager>, String> {
    let capabilities = NetworkCapabilityRegistry::new([])
        .map_err(|error| format!("failed to build fail-closed capability registry: {error}"))?;
    LocalNetworkManager::open(state_root, capabilities)
        .map_err(|error| format!("failed to open process network manager: {error}"))
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

fn recovery_child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(RECOVERY_CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(MODE_ENV, mode)
}

fn launch_reservation_claim() -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("nimbus-testing.launch-reservation"),
            "attempt:nnc3.8-a6",
        )
        .expect("fixture launch reservation claim should validate"),
    )
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

fn recovery_request(role: &str, port_request: PortRequestMode) -> PortLeaseRequest {
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
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
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

fn recovery_binding(role: &str, actual_port: NonZeroU16) -> PortLeaseBinding {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            actual_port,
        )
        .expect("fixture recovery endpoint should validate"),
        PortBindingProvenance::ProviderAssigned,
        NetworkProviderHandle::new(provider_id, format!("process-{role}"))
            .expect("fixture recovery provider handle should validate"),
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
