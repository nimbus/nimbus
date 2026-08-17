use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_network::{
    ListenerId, LocalNetworkManager, LocalNetworkManagerBootstrap, LocalNetworkStateStore,
    NetworkCapabilityRegistry, NetworkLeaseEpoch, NetworkResourceGeneration, PortBindRealm,
    PortBindTarget, PortBindingSpec, PortExposure, PortLeaseAccounting, PortLeaseFence,
    PortLeaseId, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_server::{PreboundServerListeners, ServeOptions};

static MANAGER_TEST_SERIALIZER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn freeze_empty(bootstrap: LocalNetworkManagerBootstrap) -> Arc<LocalNetworkManager> {
    bootstrap
        .freeze(NetworkCapabilityRegistry::new([]).expect("empty test registry should validate"))
}

fn protocol_only_listener_options(
    engine: Arc<Engine>,
    manager: Arc<LocalNetworkManager>,
) -> ServeOptions {
    assert_eq!(
        manager.capability_registry().selections().count(),
        0,
        "listener-only fixtures must not advertise workload providers"
    );
    ServeOptions::protocol_only_with_authority(engine, manager.authority())
}

#[cfg(unix)]
#[test]
fn manager_derived_listener_authority_survives_alias_retarget_until_final_drop() {
    use std::os::unix::fs::symlink;

    let _manager_test_guard = MANAGER_TEST_SERIALIZER.blocking_lock();
    let root = tempfile::tempdir().expect("fixture root should exist");
    let node_root = root.path().join("node");
    let foreign_root = root.path().join("foreign");
    std::fs::create_dir_all(&node_root).expect("node root should exist");
    std::fs::create_dir_all(&foreign_root).expect("foreign root should exist");
    let alias = root.path().join("node-alias");
    symlink(&node_root, &alias).expect("node alias should be created");

    let bootstrap =
        LocalNetworkManager::bootstrap(&alias).expect("fixture should claim node authority");
    let manager = freeze_empty(bootstrap);
    let engine = Arc::new(
        Engine::new(root.path().join("engine")).expect("fixture engine should initialize"),
    );
    let options = protocol_only_listener_options(engine, manager);

    std::fs::remove_file(&alias).expect("original alias should be removed");
    symlink(&foreign_root, &alias).expect("alias should be retargeted");

    let prepared = options
        .prepare_main_listener((Ipv4Addr::LOCALHOST, 0).into())
        .expect("retained authority should prepare against the original node root");
    drop(options);

    assert!(
        !LocalNetworkStateStore::authority_path_for(&foreign_root).exists(),
        "retargeting the alias must not open or mutate the foreign authority"
    );
    let duplicate = LocalNetworkManager::bootstrap(&foreign_root)
        .expect_err("the prepared listener must retain the process composition claim");
    assert!(
        duplicate.to_string().contains("already initialized"),
        "the retained process-claim error should remain actionable: {duplicate}"
    );

    drop(prepared);
    let reopened = LocalNetworkManager::bootstrap(&foreign_root)
        .expect("the final server-listener authority drop should permit a new manager");
    drop(reopened);
}

#[cfg(unix)]
#[tokio::test]
async fn manager_derived_main_sibling_and_external_paths_retain_one_authority() {
    use std::os::unix::fs::symlink;

    let _manager_test_guard = MANAGER_TEST_SERIALIZER.lock().await;
    let root = tempfile::tempdir().expect("fixture root should exist");
    let node_root = root.path().join("node");
    let foreign_root = root.path().join("foreign");
    std::fs::create_dir_all(&node_root).expect("node root should exist");
    std::fs::create_dir_all(&foreign_root).expect("foreign root should exist");
    let alias = root.path().join("node-alias");
    symlink(&node_root, &alias).expect("node alias should be created");
    let bootstrap =
        LocalNetworkManager::bootstrap(&alias).expect("fixture should claim node authority");
    let authority = bootstrap.authority();
    let manager = freeze_empty(bootstrap);
    let engine = Arc::new(
        Engine::new(root.path().join("engine")).expect("fixture engine should initialize"),
    );
    let options = protocol_only_listener_options(engine, manager);
    let mut prebound = PreboundServerListeners::new(authority);

    std::fs::remove_file(&alias).expect("original alias should be removed");
    symlink(&foreign_root, &alias).expect("alias should be retargeted");

    let sibling_request = (Ipv4Addr::LOCALHOST, 0).into();
    let sibling = prebound
        .prepare("dev-mongodb-provider-assigned", sibling_request)
        .expect("manager-derived sibling should prepare after alias retarget");
    let sibling_raw =
        std::net::TcpListener::bind(sibling_request).expect("sibling listener should bind");
    let sibling = sibling
        .adopt_std(sibling_raw)
        .expect("sibling listener should activate");
    prebound
        .insert("mongodb", sibling)
        .expect("sibling should retain its manager-derived authority");

    let main_request = (Ipv4Addr::LOCALHOST, 0).into();
    let main = options
        .prepare_main_listener(main_request)
        .expect("manager-derived main should prepare after alias retarget");
    let main_raw = tokio::net::TcpListener::bind(main_request)
        .await
        .expect("main listener should bind");
    let main = main.adopt(main_raw).expect("main listener should activate");

    let external_raw = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("external listener should bind");
    let external = options
        .adopt_external_main_listener(external_raw)
        .expect("manager-derived external adoption should activate");
    drop(options);

    assert!(
        !LocalNetworkStateStore::authority_path_for(&foreign_root).exists(),
        "no listener path may follow the retargeted alias"
    );
    LocalNetworkManager::bootstrap(&foreign_root)
        .expect_err("live main/sibling/external handles must retain the process claim");

    main.close_and_settle().expect("main should settle");
    external
        .close_and_settle()
        .expect("external should withdraw");
    prebound
        .close_and_settle()
        .expect("sibling bundle should settle");
    drop(
        LocalNetworkManager::bootstrap(&foreign_root)
            .expect("final listener-authority drop should permit manager reopen"),
    );
}

#[test]
fn divergent_prebound_authority_is_rejected_before_main_preparation() {
    let _manager_test_guard = MANAGER_TEST_SERIALIZER.blocking_lock();
    let root = tempfile::tempdir().expect("fixture root should exist");
    let source_root = root.path().join("source-node");
    let foreign_root = root.path().join("foreign-node");
    let bootstrap =
        LocalNetworkManager::bootstrap(&source_root).expect("source manager should initialize");
    let mut prebound = PreboundServerListeners::new(bootstrap.authority());
    let requested = (Ipv4Addr::LOCALHOST, 0).into();
    let prepared = prebound
        .prepare("dev-mongodb-provider-assigned", requested)
        .expect("source authority should reserve before bind");
    let raw =
        std::net::TcpListener::bind(requested).expect("source-owned listener should bind once");
    let listener = prepared
        .adopt_std(raw)
        .expect("source listener should activate");
    let source_addr = listener
        .local_addr()
        .expect("source listener address should resolve");
    prebound
        .insert("mongodb", listener)
        .expect("source listener should enter its bundle");

    let engine = Arc::new(Engine::new(&foreign_root).expect("fixture engine should initialize"));
    let engine_state_root = engine.data_dir().to_path_buf();
    let options = ServeOptions::reconstruct_direct(Arc::clone(&engine))
        .expect("explicit direct fixture authority should initialize");
    let error = match options.with_prebound_listener_authority(&prebound) {
        Ok(_) => panic!("a divergent prebound authority must not replace server authority"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains("network authority"),
        "the failure should identify the authority mismatch: {error}"
    );

    prebound
        .close_and_settle()
        .expect("the source bundle must remain settleable by its own authority");
    std::net::TcpListener::bind(source_addr)
        .expect("rejected handoff must close and release the source listener");
    let foreign_authority_path = LocalNetworkStateStore::authority_path_for(&engine_state_root);
    assert!(
        foreign_authority_path
            .parent()
            .is_some_and(std::path::Path::exists),
        "the explicit direct reconstruction should open and validate its authority once up front"
    );
    assert!(
        !foreign_authority_path.exists(),
        "rejecting a divergent prebound handoff must not create durable listener records"
    );
}

#[test]
fn sandbox_shaped_reservation_conflicts_with_server_before_kernel_bind() {
    let _manager_test_guard = MANAGER_TEST_SERIALIZER.blocking_lock();
    let root = tempfile::tempdir().expect("fixture root should exist");
    let node_root = root.path().join("node");
    let bootstrap =
        LocalNetworkManager::bootstrap(&node_root).expect("node manager should initialize");
    let authority = bootstrap.authority();
    let manager = freeze_empty(bootstrap);
    let selector =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("selector should bind");
    let requested_addr = selector
        .local_addr()
        .expect("selected address should resolve");
    drop(selector);

    let tenant = TenantId::new("nnc4-6d-conflict").expect("fixture tenant should validate");
    let listener = ListenerId::for_tenant_workload_listener(&tenant, "sandbox", "published-http");
    let request = PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        listener.into(),
        Some(tenant),
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(
                NonZeroU16::new(requested_addr.port()).expect("selected port should be non-zero"),
            ),
        ),
    );
    authority
        .port_leases()
        .reserve(request.clone())
        .expect("sandbox-shaped winner should reserve");
    let durable_before =
        std::fs::read(authority.authority_path()).expect("winner should be durable");
    let engine = Arc::new(
        Engine::new(root.path().join("engine")).expect("fixture engine should initialize"),
    );
    let server = protocol_only_listener_options(engine, manager);

    let error = match server.prepare_main_listener(requested_addr) {
        Ok(_) => panic!("server must lose to the sandbox-shaped durable reservation"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    assert_eq!(
        std::fs::read(authority.authority_path()).expect("authority should remain readable"),
        durable_before,
        "the losing server preparation must not mutate durable authority"
    );
    let probe =
        std::net::TcpListener::bind(requested_addr).expect("the losing server must not bind");
    assert_eq!(
        probe.local_addr().expect("probe address should resolve"),
        requested_addr
    );
    drop(probe);

    authority
        .port_leases()
        .withdraw(&request)
        .expect("fixture winner should withdraw");
    authority
        .port_leases()
        .release(&request)
        .expect("fixture winner should release");
}
