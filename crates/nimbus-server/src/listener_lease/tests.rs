use std::path::Path;

use nimbus_network::{PortBindingProvenance, PortLeaseEffectScope, PortLeasePhase};

use super::*;

fn external_context(incarnation: &str, generation: u64) -> ExternalServerListenerContext {
    ExternalServerListenerContext::new(
        format!("test-external:{incarnation}"),
        NetworkResourceGeneration::new(generation),
    )
    .expect("fixture external-listener context should validate")
}

fn direct_authority(state_root: &Path) -> ServerListenerLeaseAuthority {
    ServerListenerLeaseAuthority::reconstruct_direct(state_root)
        .expect("direct listener authority should reconstruct once")
}

#[tokio::test]
async fn provider_assigned_bind_is_claimed_before_effect_and_released_after_close() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let authority = direct_authority(state_root.path());
    let requested_addr = "127.0.0.1:0".parse().expect("fixture address should parse");

    let prepared = authority
        .prepare_main(requested_addr)
        .expect("provider-assigned listener should prepare");
    let durable = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should open")
        .list()
        .expect("port records should list");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].phase(), PortLeasePhase::Reserved);
    assert!(
        durable[0].bind_claim().is_some(),
        "the durable bind claim must precede the kernel effect"
    );
    assert_eq!(durable[0].reserved_port(), None);

    let raw = tokio::net::TcpListener::bind(requested_addr)
        .await
        .expect("kernel should assign a listener");
    let actual_addr = raw.local_addr().expect("bound address should resolve");
    let leased = prepared
        .adopt(raw)
        .expect("concrete listener should adopt and activate");
    let active = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(active[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        active[0]
            .binding()
            .expect("Active lease should retain binding evidence")
            .actual_port()
            .get(),
        actual_addr.port()
    );
    assert_eq!(
        active[0]
            .binding()
            .expect("Active lease should retain binding evidence")
            .provenance(),
        PortBindingProvenance::ProviderAssigned
    );

    leased
        .close_and_settle()
        .expect("confirmed close should release authority");
    let released = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(released[0].phase(), PortLeasePhase::Released);
}

#[tokio::test]
async fn exact_bind_collision_records_terminal_no_effect_failure() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let external = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("external owner should bind");
    let occupied_addr = external
        .local_addr()
        .expect("external address should resolve");
    let prepared = direct_authority(state_root.path())
        .prepare_main(occupied_addr)
        .expect("exact durable request should prepare before the kernel collision");

    let error = tokio::net::TcpListener::bind(occupied_addr)
        .await
        .expect_err("the external owner must win the real kernel bind");
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    let returned = prepared
        .record_bind_failure(error)
        .expect("durable failure receipt should commit")
        .into_error();
    assert_eq!(returned.kind(), io::ErrorKind::AddrInUse);

    let durable = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].phase(), PortLeasePhase::Failed);
    assert_eq!(
        durable[0]
            .failure()
            .expect("failed lease should retain exact evidence")
            .kind(),
        PortBindFailureKind::AddrInUse
    );
    assert_eq!(
        durable[0]
            .failure()
            .expect("failed lease should retain exact evidence")
            .attempt()
            .port(),
        occupied_addr.port()
    );
}

#[tokio::test]
async fn external_listener_adoption_records_external_provenance() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("external owner should bind");
    let actual_addr = raw.local_addr().expect("bound address should resolve");
    let leased = direct_authority(state_root.path())
        .adopt_external_main(raw)
        .expect("external listener should adopt");

    let active = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].phase(), PortLeasePhase::Active);
    let binding = active[0]
        .binding()
        .expect("Active external lease should retain binding evidence");
    assert_eq!(binding.actual_port().get(), actual_addr.port());
    assert_eq!(binding.provenance(), PortBindingProvenance::ExternallyOwned);

    leased
        .close_and_settle()
        .expect("local close should withdraw external adoption");
    let withdrawn = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(withdrawn[0].phase(), PortLeasePhase::Withdrawing);
    assert_eq!(
        withdrawn[0]
            .binding()
            .expect("withdrawn external fence should retain binding evidence")
            .provenance(),
        PortBindingProvenance::ExternallyOwned
    );
}

#[tokio::test]
async fn dead_process_bound_listener_drop_reconciles_before_next_prepare() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let first_authority = direct_authority(state_root.path());
    let requested_addr = "127.0.0.1:0".parse().expect("fixture address should parse");
    let first_prepared = first_authority
        .prepare_main(requested_addr)
        .expect("first listener should prepare");
    let first_raw = tokio::net::TcpListener::bind(requested_addr)
        .await
        .expect("first listener should bind");
    let actual_addr = first_raw
        .local_addr()
        .expect("bound address should resolve");
    let first = first_prepared
        .adopt(first_raw)
        .expect("first listener should activate");
    drop(first);

    let retained = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        retained[0]
            .active_lifetime()
            .expect("dropped listener must retain lifetime evidence")
            .effect_scope(),
        PortLeaseEffectScope::ProcessBound
    );

    let second_prepared = direct_authority(state_root.path())
        .prepare_main(actual_addr)
        .expect("fresh preparation should reconcile the dead process-bound owner");
    let second_raw = tokio::net::TcpListener::bind(actual_addr)
        .await
        .expect("replacement listener should bind the released port");
    let second = second_prepared
        .adopt(second_raw)
        .expect("replacement listener should activate");
    let records = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(
        records
            .iter()
            .filter(|record| record.phase() == PortLeasePhase::Released)
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.phase() == PortLeasePhase::Active)
            .count(),
        1
    );
    second
        .close_and_settle()
        .expect("replacement should close cleanly");
}

#[tokio::test]
async fn external_listener_drop_remains_provider_managed_and_fenced() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("external owner should bind");
    let actual_addr = raw.local_addr().expect("bound address should resolve");
    let leased = direct_authority(state_root.path())
        .adopt_external_main(raw)
        .expect("external listener should adopt");
    drop(leased);

    let error = match direct_authority(state_root.path()).prepare_main(actual_addr) {
        Ok(_) => panic!("process death cannot release an external adoption"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    let retained = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        retained[0]
            .active_lifetime()
            .expect("external adoption must retain lifetime evidence")
            .effect_scope(),
        PortLeaseEffectScope::ProviderManaged
    );
}

#[tokio::test]
async fn fresh_authority_reclaims_the_same_surviving_external_listener() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let context = external_context("inherited-main", 1);
    let external_owner =
        std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
    external_owner
        .set_nonblocking(true)
        .expect("external listener should become nonblocking");
    let inherited = external_owner
        .try_clone()
        .expect("fresh process fixture should inherit the same listener");
    let addr = external_owner
        .local_addr()
        .expect("external address should resolve");
    let first = direct_authority(state_root.path())
        .with_external_main_context(context.clone())
        .adopt_external_main(
            tokio::net::TcpListener::from_std(external_owner)
                .expect("first process should adopt its descriptor"),
        )
        .expect("first external owner should activate");
    let first_lifetime = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list")[0]
        .active_lifetime()
        .expect("first external owner should carry a lifetime");
    drop(first);

    let second = direct_authority(state_root.path())
        .with_external_main_context(context)
        .adopt_external_main(
            tokio::net::TcpListener::from_std(inherited)
                .expect("fresh process should adopt the inherited descriptor"),
        )
        .expect("fresh authority should reclaim the exact surviving listener");
    assert_eq!(second.local_addr().expect("listener should inspect"), addr);
    let records = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(records.len(), 1, "recovery must not fork listener identity");
    assert_eq!(records[0].phase(), PortLeasePhase::Active);
    assert!(
        records[0]
            .active_lifetime()
            .expect("replacement owner should carry a lifetime")
            .generation()
            > first_lifetime.generation(),
        "fresh ownership must fence the dead server generation"
    );

    second
        .close_and_settle()
        .expect("fresh external owner should withdraw cleanly");
}

#[tokio::test]
async fn rebound_same_address_external_listener_cannot_reclaim_prior_provider_incarnation() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let original_context = external_context("original-main", 1);
    let original =
        std::net::TcpListener::bind("127.0.0.1:0").expect("original external owner should bind");
    original
        .set_nonblocking(true)
        .expect("original listener should become nonblocking");
    let addr = original
        .local_addr()
        .expect("original address should resolve");
    let first = direct_authority(state_root.path())
        .with_external_main_context(original_context)
        .adopt_external_main(
            tokio::net::TcpListener::from_std(original)
                .expect("first process should adopt the original descriptor"),
        )
        .expect("original external owner should activate");
    drop(first);

    let rebound = std::net::TcpListener::bind(addr)
        .expect("a newly created provider socket should rebind the released address");
    rebound
        .set_nonblocking(true)
        .expect("rebound listener should become nonblocking");
    let before = LocalPortLeaseAuthority::open(state_root.path())
        .expect("portable authority should reopen")
        .list()
        .expect("port records should list");
    let error = match direct_authority(state_root.path())
        .with_external_main_context(external_context("replacement-main", 1))
        .adopt_external_main(
            tokio::net::TcpListener::from_std(rebound)
                .expect("replacement listener should enter Tokio"),
        ) {
        Ok(_) => panic!("a new provider incarnation must not inherit old listener authority"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert_eq!(
        LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .list()
            .expect("port records should list"),
        before,
        "provider-incarnation substitution must not mutate durable authority"
    );
}

#[tokio::test]
async fn external_listener_recovery_rejects_stale_provider_generation() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let original = std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
    original
        .set_nonblocking(true)
        .expect("external listener should become nonblocking");
    let inherited = original
        .try_clone()
        .expect("the fixture should inherit the exact same descriptor");
    let first = direct_authority(state_root.path())
        .with_external_main_context(external_context("stable-main", 2))
        .adopt_external_main(
            tokio::net::TcpListener::from_std(original)
                .expect("first process should adopt the descriptor"),
        )
        .expect("current provider generation should activate");
    drop(first);

    let before = LocalPortLeaseAuthority::open(state_root.path())
        .expect("portable authority should reopen")
        .list()
        .expect("port records should list");
    let error = match direct_authority(state_root.path())
        .with_external_main_context(external_context("stable-main", 1))
        .adopt_external_main(
            tokio::net::TcpListener::from_std(inherited)
                .expect("stale contender should adopt its cloned descriptor"),
        ) {
        Ok(_) => panic!("a stale provider generation must not reclaim the descriptor"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert_eq!(
        LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .list()
            .expect("port records should list"),
        before,
        "stale-generation rejection must not mutate durable authority"
    );
}

#[tokio::test]
async fn external_main_pre_adoption_crash_reclaims_supplied_descriptor() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let context = external_context("pre-adoption-main", 1);
    let external = std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
    external
        .set_nonblocking(true)
        .expect("external listener should become nonblocking");
    let addr = external
        .local_addr()
        .expect("external address should resolve");
    let first_authority =
        direct_authority(state_root.path()).with_external_main_context(context.clone());
    let request = first_authority
        .external_main_request(addr)
        .expect("external request should build");
    let prepared = first_authority
        .prepare_request(
            first_authority.network_authority.clone(),
            request.clone(),
            addr,
            PortBindingProvenance::ExternallyOwned,
        )
        .expect("first process should durably claim before adoption");
    let first_claim = prepared.claim.clone();
    let first_lifetime = prepared.lifetime.lifetime();
    drop(prepared);

    let reclaimed = direct_authority(state_root.path())
        .with_external_main_context(context)
        .adopt_external_main(
            tokio::net::TcpListener::from_std(external)
                .expect("replacement should adopt the inherited descriptor"),
        )
        .expect("dead pre-adoption owner should be reclaimed");
    assert_eq!(
        reclaimed.local_addr().expect("listener should inspect"),
        addr
    );
    let record = LocalPortLeaseAuthority::open(state_root.path())
        .expect("portable authority should reopen")
        .inspect(request.lease_id())
        .expect("external request should inspect")
        .expect("external request should remain");
    assert_eq!(record.phase(), PortLeasePhase::Active);
    assert_eq!(record.adoption_claim(), Some(&first_claim));
    assert!(record.bind_claim().is_none());
    assert_eq!(
        record
            .binding()
            .expect("reclaimed request should carry binding")
            .provider_handle(),
        first_claim.provider_attempt()
    );
    assert!(
        record
            .active_lifetime()
            .expect("replacement should own a lifetime")
            .generation()
            > first_lifetime.generation()
    );

    reclaimed
        .close_and_settle()
        .expect("external replacement should withdraw cleanly");
}

#[tokio::test]
async fn external_main_pre_adoption_live_owner_rejects_reclaim() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let context = external_context("live-pre-adoption-main", 1);
    let external = std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
    external
        .set_nonblocking(true)
        .expect("external listener should become nonblocking");
    let inherited = external
        .try_clone()
        .expect("contender should receive the same descriptor");
    let addr = external
        .local_addr()
        .expect("external address should resolve");
    let first_authority =
        direct_authority(state_root.path()).with_external_main_context(context.clone());
    let request = first_authority
        .external_main_request(addr)
        .expect("external request should build");
    let prepared = first_authority
        .prepare_request(
            first_authority.network_authority.clone(),
            request.clone(),
            addr,
            PortBindingProvenance::ExternallyOwned,
        )
        .expect("first process should retain its live pre-adoption claim");
    let before = LocalPortLeaseAuthority::open(state_root.path())
        .expect("portable authority should reopen")
        .inspect(request.lease_id())
        .expect("external request should inspect")
        .expect("external request should remain");

    let error = match direct_authority(state_root.path())
        .with_external_main_context(context)
        .adopt_external_main(
            tokio::net::TcpListener::from_std(inherited)
                .expect("contender should adopt its cloned descriptor"),
        ) {
        Ok(_) => panic!("a live pre-adoption owner must reject reclaim"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert_eq!(
        LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .inspect(request.lease_id())
            .expect("rejected request should inspect"),
        Some(before),
        "live-owner rejection must not mutate durable authority"
    );
    drop(prepared);
    drop(external);
}

#[tokio::test]
async fn adoption_failure_closes_socket_and_releases_never_bound_owned_claim() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let requested_owner = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("requested-port selector should bind");
    let requested_addr = requested_owner
        .local_addr()
        .expect("requested address should resolve");
    let prepared = direct_authority(state_root.path())
        .prepare_main(requested_addr)
        .expect("exact request should prepare independently of the kernel owner");
    let wrong_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mismatched listener should bind");
    let wrong_addr = wrong_listener
        .local_addr()
        .expect("mismatched listener address should resolve");

    let error = match prepared.adopt(wrong_listener) {
        Ok(_) => panic!("an exact request must reject a listener on another port"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::Other);
    let durable = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].phase(), PortLeasePhase::Released);
    tokio::net::TcpListener::bind(wrong_addr)
        .await
        .expect("failed adoption must close the concrete listener");
}

#[tokio::test]
async fn durable_reservation_conflict_is_reported_as_addr_in_use() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let selector = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("port selector should bind");
    let requested_addr = selector
        .local_addr()
        .expect("selected address should resolve");
    drop(selector);
    let _winner = direct_authority(state_root.path())
        .prepare_main(requested_addr)
        .expect("first authority should prepare");

    let error = match direct_authority(state_root.path()).prepare_main(requested_addr) {
        Ok(_) => panic!("a second durable owner must lose the exact port"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    let probe = tokio::net::TcpListener::bind(requested_addr)
        .await
        .expect("the durable loser must not run a kernel bind effect");
    assert_eq!(
        probe.local_addr().expect("probe address should resolve"),
        requested_addr
    );
}

#[tokio::test]
async fn bind_failure_receipt_error_is_distinguishable_from_recorded_collision() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let external = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("external owner should bind");
    let occupied_addr = external
        .local_addr()
        .expect("external address should resolve");
    let prepared = direct_authority(state_root.path())
        .prepare_main(occupied_addr)
        .expect("exact durable request should prepare");
    let error = tokio::net::TcpListener::bind(occupied_addr)
        .await
        .expect_err("the external owner must win the real kernel bind");
    std::fs::write(
        state_root
            .path()
            .join("networks")
            .join("control-plane")
            .join("state.json"),
        b"corrupt after prepare",
    )
    .expect("authority fixture should corrupt");

    let receipt_error = prepared
        .record_bind_failure(error)
        .expect_err("a corrupt authority must not masquerade as a recorded bind failure");
    assert_eq!(receipt_error.kind(), io::ErrorKind::AddrInUse);
    assert!(
        receipt_error
            .to_string()
            .contains("failed to record durable no-effect bind failure")
    );
}

#[test]
fn dropping_prebound_bundle_closes_socket_and_settles_active_lease() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let mut listeners = PreboundServerListeners::reconstruct_direct(state_root.path())
        .expect("pre-bound listener authority should reconstruct once");
    let requested_addr = "127.0.0.1:0"
        .parse()
        .expect("provider-assigned address should parse");
    let prepared = listeners
        .prepare("dev-mongodb-provider-assigned", requested_addr)
        .expect("pre-bound listener should reserve");
    let raw = std::net::TcpListener::bind(requested_addr)
        .expect("provider should bind its requested socket");
    let listener = prepared
        .adopt_std(raw)
        .expect("pre-bound listener should activate");
    let actual_addr = listener
        .local_addr()
        .expect("pre-bound address should resolve");
    listeners
        .insert("mongodb", listener)
        .expect("listener should enter the handoff bundle");

    drop(listeners);

    std::net::TcpListener::bind(actual_addr)
        .expect("dropping pre-serve ownership must close the retained socket");
    let records = LocalPortLeaseAuthority::open(state_root.path())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Released);
}
