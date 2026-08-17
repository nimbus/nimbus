use std::future::pending;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nimbus_core::TenantEventRecord;
use nimbus_engine::Engine;
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkResourceGeneration, PortBindingProvenance, PortLeasePhase,
};
use nimbus_storage::{FaultInjector, FaultPoint};
use nimbus_testing::EngineFixture;
use tokio::time::timeout;

use super::*;
use crate::adapters::wire::{WireProtocolAdapter, WireProtocolTaskFuture, WireProtocolTasks};
use crate::listener_lease::ServerListenerLeaseAuthority;
use crate::{ExternalServerListenerContext, ServeOptions, serve};

enum TaskBehavior {
    Pending,
    ReturnOk,
    ReturnError(&'static str),
    Panic(&'static str),
    BuildError(&'static str),
    BuildPanic(&'static str),
    ListenerFactoryPanic(&'static str),
}

struct TestAdapter {
    name: &'static str,
    behavior: TaskBehavior,
}

enum GuardAction {
    Allow,
    Reject(&'static str),
    ArmProjectionFailure(Arc<ProjectionFault>),
}

struct ObservedAdapter {
    name: &'static str,
    bound_addr: Arc<Mutex<Option<SocketAddr>>>,
    guard: GuardAction,
    behavior: TaskBehavior,
}

impl WireProtocolAdapter for ObservedAdapter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn protocol(&self) -> &'static str {
        "test"
    }

    fn bind_addr(&self) -> SocketAddr {
        "127.0.0.1:0".parse().expect("test address should parse")
    }

    fn guard(&self, addr: SocketAddr) -> io::Result<()> {
        *self
            .bound_addr
            .lock()
            .expect("observed address lock should remain healthy") = Some(addr);
        match &self.guard {
            GuardAction::Allow => Ok(()),
            GuardAction::Reject(message) => {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, *message))
            }
            GuardAction::ArmProjectionFailure(fault) => {
                fault.armed.store(true, Ordering::Release);
                Ok(())
            }
        }
    }

    fn build_tasks(self: Box<Self>, _engine: Arc<Engine>) -> io::Result<WireProtocolTasks> {
        let behavior = self.behavior;
        Ok(WireProtocolTasks::new("listener", move |listener| {
            task_future(listener, behavior)
        }))
    }
}

struct OccupiedAdapter {
    address: SocketAddr,
}

impl WireProtocolAdapter for OccupiedAdapter {
    fn name(&self) -> &'static str {
        "occupied"
    }

    fn protocol(&self) -> &'static str {
        "test"
    }

    fn bind_addr(&self) -> SocketAddr {
        self.address
    }

    fn guard(&self, _addr: SocketAddr) -> io::Result<()> {
        unreachable!("the occupied address must fail before its guard")
    }

    fn build_tasks(self: Box<Self>, _engine: Arc<Engine>) -> io::Result<WireProtocolTasks> {
        unreachable!("the occupied address must fail before task construction")
    }
}

#[derive(Default)]
struct ProjectionFault {
    armed: AtomicBool,
    failed: AtomicBool,
    failure_observed: tokio::sync::Notify,
}

impl FaultInjector for ProjectionFault {
    fn check(&self, _point: FaultPoint) -> nimbus_core::Result<()> {
        // This fault targets one listener-projection transaction. An
        // unscoped storage check cannot prove that transaction identity and
        // must not consume the arm.
        Ok(())
    }

    fn check_for_tenant(
        &self,
        point: FaultPoint,
        tenant_id: &nimbus_core::TenantId,
        records: &[TenantEventRecord],
    ) -> nimbus_core::Result<()> {
        if !crate::system_tenant::is_system_tenant_id(tenant_id)
            || !records.iter().any(is_port_listener_projection_record)
        {
            return Ok(());
        }
        if self.armed.swap(false, Ordering::AcqRel) {
            self.failed.store(true, Ordering::Release);
            self.failure_observed.notify_waiters();
            return Err(nimbus_core::Error::Internal(format!(
                "injected listener projection failure at {}",
                point.as_str()
            )));
        }
        Ok(())
    }
}

fn is_port_listener_projection_record(record: &TenantEventRecord) -> bool {
    let writes_listener = record
        .writes
        .iter()
        .any(|write| write.table.as_str() == "listeners");
    let writes_port = record
        .writes
        .iter()
        .any(|write| write.table.as_str() == "ports");
    writes_listener && writes_port
}

impl ProjectionFault {
    async fn wait_for_failure(&self) {
        loop {
            let observed = self.failure_observed.notified();
            tokio::pin!(observed);
            observed.as_mut().enable();
            if self.failed.load(Ordering::Acquire) {
                return;
            }
            observed.await;
        }
    }
}

impl TestAdapter {
    fn new(name: &'static str, behavior: TaskBehavior) -> Self {
        Self { name, behavior }
    }
}

impl WireProtocolAdapter for TestAdapter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn protocol(&self) -> &'static str {
        "test"
    }

    fn bind_addr(&self) -> SocketAddr {
        "127.0.0.1:0".parse().expect("test address should parse")
    }

    fn guard(&self, _addr: SocketAddr) -> io::Result<()> {
        Ok(())
    }

    fn build_tasks(self: Box<Self>, _engine: Arc<Engine>) -> io::Result<WireProtocolTasks> {
        let behavior = self.behavior;
        match behavior {
            TaskBehavior::BuildError(message) => Err(io::Error::other(message)),
            TaskBehavior::BuildPanic(message) => panic!("{message}"),
            TaskBehavior::ListenerFactoryPanic(message) => {
                Ok(WireProtocolTasks::new("listener", move |listener| {
                    drop(listener);
                    panic!("{message}");
                }))
            }
            behavior => Ok(WireProtocolTasks::new("listener", move |listener| {
                task_future(listener, behavior)
            })),
        }
    }
}

fn task_future(
    listener: tokio::net::TcpListener,
    behavior: TaskBehavior,
) -> WireProtocolTaskFuture {
    match behavior {
        TaskBehavior::Pending => Box::pin(async move {
            let _listener = listener;
            pending::<io::Result<()>>().await
        }),
        TaskBehavior::ReturnOk => Box::pin(async move {
            drop(listener);
            Ok(())
        }),
        TaskBehavior::ReturnError(message) => Box::pin(async move {
            drop(listener);
            Err(io::Error::other(message))
        }),
        TaskBehavior::Panic(message) => Box::pin(async move {
            drop(listener);
            panic!("{message}");
        }),
        TaskBehavior::BuildError(_)
        | TaskBehavior::BuildPanic(_)
        | TaskBehavior::ListenerFactoryPanic(_) => {
            unreachable!("build-only behavior cannot become a task future")
        }
    }
}

async fn active_listener(
    state_root: &Path,
    ordinal: usize,
    adapter: &'static str,
) -> (
    tokio::net::TcpListener,
    ActiveServerListenerLease,
    SocketAddr,
) {
    let authority = ServerListenerLeaseAuthority::reconstruct_direct(state_root)
        .expect("test listener authority should reconstruct");
    let requested = "127.0.0.1:0".parse().expect("test address should parse");
    let prepared = authority
        .prepare_sibling(ordinal, adapter, requested)
        .expect("test listener should prepare");
    let listener = tokio::net::TcpListener::bind(requested)
        .await
        .expect("test listener should bind");
    let address = listener
        .local_addr()
        .expect("test listener address should resolve");
    let leased = prepared
        .adopt(listener)
        .expect("test listener should activate");
    let (listener, lease, _) = leased.into_parts();
    (listener, lease, address)
}

fn phase_for_address(state_root: &Path, address: SocketAddr) -> PortLeasePhase {
    LocalPortLeaseAuthority::open(state_root)
        .expect("port authority should reopen")
        .list()
        .expect("port records should list")
        .into_iter()
        .find(|record| {
            record
                .binding()
                .is_some_and(|binding| binding.actual_port().get() == address.port())
        })
        .expect("listener lease should remain inspectable")
        .phase()
}

fn observed_address(address: &Arc<Mutex<Option<SocketAddr>>>) -> SocketAddr {
    address
        .lock()
        .expect("observed address lock should remain healthy")
        .expect("adapter guard should record its bound address")
}

async fn assert_address_rebinds(address: SocketAddr) {
    tokio::net::TcpListener::bind(address)
        .await
        .unwrap_or_else(|error| panic!("listener {address} must be closed: {error}"));
}

#[tokio::test]
async fn kth_guard_failure_unwinds_every_prior_listener() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let first_addr = Arc::new(Mutex::new(None));
    let rejected_addr = Arc::new(Mutex::new(None));
    let options = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server authority should reconstruct")
        .with_test_wire_adapter(Box::new(ObservedAdapter {
            name: "first-guard-member",
            bound_addr: Arc::clone(&first_addr),
            guard: GuardAction::Allow,
            behavior: TaskBehavior::Pending,
        }))
        .with_test_wire_adapter(Box::new(ObservedAdapter {
            name: "rejected-guard-member",
            bound_addr: Arc::clone(&rejected_addr),
            guard: GuardAction::Reject("injected guard refusal"),
            behavior: TaskBehavior::Pending,
        }));
    let main = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("main listener should bind");

    let error = serve(main, options)
        .await
        .expect_err("the kth guard must fail server startup");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("injected guard refusal"));
    let first_addr = observed_address(&first_addr);
    let rejected_addr = observed_address(&rejected_addr);
    assert_address_rebinds(first_addr).await;
    assert_address_rebinds(rejected_addr).await;
    assert_eq!(
        phase_for_address(fixture.data_dir(), first_addr),
        PortLeasePhase::Released
    );
}

#[tokio::test]
async fn listener_projection_failure_keeps_every_listener_active_and_retries() {
    let projection_fault = Arc::new(ProjectionFault::default());
    let engine_fault: Arc<dyn FaultInjector> = projection_fault.clone();
    let fixture = EngineFixture::new(move |path| {
        Engine::new_with_simulation(path, Arc::new(nimbus_core::SystemWallClock), engine_fault)
    });
    let first_addr = Arc::new(Mutex::new(None));
    let rejected_addr = Arc::new(Mutex::new(None));
    let options = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server authority should reconstruct")
        .with_test_wire_adapter(Box::new(ObservedAdapter {
            name: "first-projection-member",
            bound_addr: Arc::clone(&first_addr),
            guard: GuardAction::Allow,
            behavior: TaskBehavior::Pending,
        }))
        .with_test_wire_adapter(Box::new(ObservedAdapter {
            name: "rejected-projection-member",
            bound_addr: Arc::clone(&rejected_addr),
            guard: GuardAction::ArmProjectionFailure(Arc::clone(&projection_fault)),
            behavior: TaskBehavior::Pending,
        }));
    let main = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("main listener should bind");

    let server = tokio::spawn(serve(main, options));
    timeout(Duration::from_secs(5), projection_fault.wait_for_failure())
        .await
        .expect("the injected projection fault must be observed");
    assert!(
        !server.is_finished(),
        "a projection failure must not terminate the listener group"
    );
    let system_tenant = crate::system_tenant::system_tenant_id().expect("system id should parse");
    timeout(Duration::from_secs(5), async {
        loop {
            let listeners = fixture
                .engine()
                .list_documents_async(
                    system_tenant.clone(),
                    nimbus_core::TableName::new("listeners").expect("table should parse"),
                )
                .await
                .unwrap_or_default();
            if listeners.len() == 3 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("projection retry must restore the main and both sibling rows");
    assert!(
        !server.is_finished(),
        "projection recovery must not terminate the listener group"
    );
    for address in [
        observed_address(&first_addr),
        observed_address(&rejected_addr),
    ] {
        tokio::net::TcpStream::connect(address)
            .await
            .unwrap_or_else(|error| panic!("listener {address} must remain active: {error}"));
        assert_eq!(
            phase_for_address(fixture.data_dir(), address),
            PortLeasePhase::Active
        );
    }
    server.abort();
    let error = server
        .await
        .expect_err("aborted server task should report cancellation");
    assert!(error.is_cancelled());
}

#[tokio::test]
async fn child_exit_is_propagated_through_serve_leased() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let sibling_addr = Arc::new(Mutex::new(None));
    let options = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server authority should reconstruct")
        .with_test_wire_adapter(Box::new(ObservedAdapter {
            name: "exiting-adapter",
            bound_addr: Arc::clone(&sibling_addr),
            guard: GuardAction::Allow,
            behavior: TaskBehavior::ReturnOk,
        }));
    let main = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("main listener should bind");

    let error = timeout(Duration::from_secs(5), serve(main, options))
        .await
        .expect("task death must stop the main server")
        .expect_err("a successful child exit is an unexpected server failure");

    assert!(error.to_string().contains("exiting-adapter:listener"));
    assert!(error.to_string().contains("without an error"));
    let address = observed_address(&sibling_addr);
    assert_address_rebinds(address).await;
    assert_eq!(
        phase_for_address(fixture.data_dir(), address),
        PortLeasePhase::Released
    );
}

#[tokio::test]
async fn setup_failure_withdraws_but_does_not_release_an_inherited_main_listener() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let external =
        std::net::TcpListener::bind("127.0.0.1:0").expect("external main owner should bind");
    external
        .set_nonblocking(true)
        .expect("external listener should become nonblocking");
    let main_addr = external
        .local_addr()
        .expect("external main address should resolve");
    let inherited = external
        .try_clone()
        .expect("test process should inherit the descriptor");
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("external sibling owner should bind");
    let occupied_addr = occupied
        .local_addr()
        .expect("occupied sibling address should resolve");
    let context = ExternalServerListenerContext::new(
        "nnc7.1a-inherited-main",
        NetworkResourceGeneration::new(1),
    )
    .expect("external provider context should validate");
    let options = ServeOptions::reconstruct_direct(fixture.engine())
        .expect("test server authority should reconstruct")
        .with_external_main_listener_context(context)
        .with_test_wire_adapter(Box::new(OccupiedAdapter {
            address: occupied_addr,
        }));
    let inherited = tokio::net::TcpListener::from_std(inherited)
        .expect("Tokio should adopt the inherited descriptor");

    let error = serve(inherited, options)
        .await
        .expect_err("the occupied sibling must fail startup");

    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    let record = LocalPortLeaseAuthority::open(fixture.data_dir())
        .expect("port authority should reopen")
        .list()
        .expect("port records should list")
        .into_iter()
        .find(|record| {
            record
                .binding()
                .is_some_and(|binding| binding.actual_port().get() == main_addr.port())
        })
        .expect("external main lease should remain inspectable");
    assert_eq!(record.phase(), PortLeasePhase::Withdrawing);
    assert_eq!(
        record
            .binding()
            .expect("external binding should remain")
            .provenance(),
        PortBindingProvenance::ExternallyOwned
    );
    let bind_error = std::net::TcpListener::bind(main_addr)
        .expect_err("the external owner must retain its host port");
    assert_eq!(bind_error.kind(), io::ErrorKind::AddrInUse);
    drop(external);
}

#[tokio::test]
async fn task_build_error_closes_and_settles_the_untransferred_listener() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let (listener, lease, address) = active_listener(fixture.data_dir(), 0, "build-error").await;
    let mut group = WireListenerGroup::new();

    let error = group
        .prepare(
            Box::new(TestAdapter::new(
                "build-error",
                TaskBehavior::BuildError("injected build error"),
            )),
            listener,
            lease,
            fixture.engine(),
        )
        .expect_err("task construction failure must reject the listener");

    assert!(error.to_string().contains("build-error task construction"));
    assert!(error.to_string().contains("injected build error"));
    assert_eq!(
        phase_for_address(fixture.data_dir(), address),
        PortLeasePhase::Released
    );
    tokio::net::TcpListener::bind(address)
        .await
        .expect("rejected task construction must close the listener");
    group
        .shutdown(Ok(()))
        .await
        .expect("a rejected member must not enter the group");
}

#[tokio::test]
async fn task_build_and_listener_factory_panics_close_and_settle_the_listener() {
    for (ordinal, behavior, expected) in [
        (
            0,
            TaskBehavior::BuildPanic("build panic"),
            "task construction panicked",
        ),
        (
            1,
            TaskBehavior::ListenerFactoryPanic("listener panic"),
            "listener task construction panicked",
        ),
    ] {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let (listener, lease, address) =
            active_listener(fixture.data_dir(), ordinal, "panic-build").await;
        let mut group = WireListenerGroup::new();

        let error = group
            .prepare(
                Box::new(TestAdapter::new("panic-build", behavior)),
                listener,
                lease,
                fixture.engine(),
            )
            .expect_err("a task-construction panic must fail closed");

        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(
            phase_for_address(fixture.data_dir(), address),
            PortLeasePhase::Released
        );
        tokio::net::TcpListener::bind(address)
            .await
            .expect("a panicking task factory must close the listener");
        group
            .shutdown(Ok(()))
            .await
            .expect("a rejected member must not enter the group");
    }
}

#[tokio::test]
async fn successful_child_exit_stops_the_server_and_settles_its_lease() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let (listener, lease, address) = active_listener(fixture.data_dir(), 0, "early-exit").await;
    let mut group = WireListenerGroup::new();
    group
        .prepare(
            Box::new(TestAdapter::new("early-exit", TaskBehavior::ReturnOk)),
            listener,
            lease,
            fixture.engine(),
        )
        .expect("test member should start");
    group.activate();

    let error = timeout(
        Duration::from_secs(1),
        group.supervise(pending::<io::Result<()>>()),
    )
    .await
    .expect("child completion must stop supervision")
    .expect_err("successful child completion is an unexpected server failure");
    assert!(error.to_string().contains("early-exit:listener"));
    assert!(error.to_string().contains("without an error"));
    let error = group
        .shutdown(Err(error))
        .await
        .expect_err("the task failure remains primary");
    assert!(error.to_string().contains("without an error"));
    assert_eq!(
        phase_for_address(fixture.data_dir(), address),
        PortLeasePhase::Released
    );
}

#[tokio::test]
async fn returned_error_and_panic_are_named_and_settle_the_listener() {
    for (ordinal, behavior, expected) in [
        (
            0,
            TaskBehavior::ReturnError("listener returned error"),
            "listener returned error",
        ),
        (1, TaskBehavior::Panic("listener panic"), "listener panic"),
    ] {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let (listener, lease, address) =
            active_listener(fixture.data_dir(), ordinal, "failed-child").await;
        let mut group = WireListenerGroup::new();
        group
            .prepare(
                Box::new(TestAdapter::new("failed-child", behavior)),
                listener,
                lease,
                fixture.engine(),
            )
            .expect("test member should start");
        group.activate();

        let error = timeout(
            Duration::from_secs(1),
            group.supervise(pending::<io::Result<()>>()),
        )
        .await
        .expect("child failure must stop supervision")
        .expect_err("child failure must propagate");
        assert!(error.to_string().contains("failed-child:listener"));
        assert!(error.to_string().contains(expected), "{error}");
        group
            .shutdown(Err(error))
            .await
            .expect_err("the task failure remains primary");
        assert_eq!(
            phase_for_address(fixture.data_dir(), address),
            PortLeasePhase::Released
        );
    }
}

#[tokio::test]
async fn shutdown_reports_every_task_failure_in_registration_order() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let mut group = WireListenerGroup::new();
    let mut addresses = Vec::new();
    for (ordinal, adapter, message) in [
        (0, "first-adapter", "first task failure"),
        (1, "second-adapter", "second task failure"),
    ] {
        let (listener, lease, address) =
            active_listener(fixture.data_dir(), ordinal, adapter).await;
        addresses.push(address);
        group
            .prepare(
                Box::new(TestAdapter::new(
                    adapter,
                    TaskBehavior::ReturnError(message),
                )),
                listener,
                lease,
                fixture.engine(),
            )
            .expect("test member should start");
    }
    group.activate();
    group
        .wait_for_all_tasks_finished(Duration::from_secs(1))
        .await;

    let error = group
        .shutdown(Err(io::Error::other("primary setup failure")))
        .await
        .expect_err("all child errors must be aggregated");
    let rendered = error.to_string();
    let first = rendered
        .find("first-adapter:listener")
        .expect("first result");
    let second = rendered
        .find("second-adapter:listener")
        .expect("second result");
    assert!(rendered.contains("primary setup failure"));
    assert!(rendered.contains("first task failure"));
    assert!(rendered.contains("second task failure"));
    assert!(
        first < second,
        "cleanup evidence must use registration order"
    );
    for address in addresses {
        assert_eq!(
            phase_for_address(fixture.data_dir(), address),
            PortLeasePhase::Released
        );
    }
}

#[tokio::test]
async fn shutdown_attempts_every_lease_settlement_after_state_corruption() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let mut group = WireListenerGroup::new();
    let mut authority_paths = Vec::<PathBuf>::new();
    let mut addresses = Vec::new();
    for (ordinal, adapter) in [(0, "first-lease"), (1, "second-lease")] {
        let root = tempfile::tempdir().expect("network root should create");
        let authority =
            LocalPortLeaseAuthority::open(root.path()).expect("port authority should initialize");
        authority_paths.push(authority.authority_path().to_path_buf());
        let (listener, lease, address) = active_listener(root.path(), ordinal, adapter).await;
        addresses.push((root, address));
        group
            .prepare(
                Box::new(TestAdapter::new(adapter, TaskBehavior::Pending)),
                listener,
                lease,
                fixture.engine(),
            )
            .expect("test member should start");
    }
    group.activate();
    for path in &authority_paths {
        std::fs::write(path, b"{").expect("authority corruption should write");
    }

    let error = group
        .shutdown(Err(io::Error::other("primary setup failure")))
        .await
        .expect_err("both ambiguous settlements must be reported");
    let rendered = error.to_string();
    let first = rendered.find("first-lease sibling").expect("first result");
    let second = rendered
        .find("second-lease sibling")
        .expect("second result");
    assert!(first < second, "lease results must use registration order");
    for (root, address) in addresses {
        tokio::net::TcpListener::bind(address)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "listener {} under {} must close despite settlement ambiguity: {error}",
                    address,
                    root.path().display()
                )
            });
    }
}
