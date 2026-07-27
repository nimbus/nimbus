use std::collections::VecDeque;
use std::io::{self, Cursor, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use nimbus_core::TenantId;
use nimbus_network::{LocalPortLeaseAuthority, PortBindFailureKind, PortLeasePhase};
use tempfile::TempDir;

use super::{
    MachinePortConnectionSet, MachinePortIoOperation, MachinePortPreparationReleaseAuthority,
    accept_machine_port_proxy_with, accept_machine_port_proxy_with_listener_observer,
    configure_machine_port_io_polling_with, copy_machine_port_stream, machine_port_proxy_routes,
    prepare_machine_port_proxies, prepare_machine_port_proxies_with_release_authority,
    proxy_machine_port_connection_with, proxy_machine_port_connection_with_spawner,
    run_machine_port_connection_task, start_machine_port_proxies,
};
use crate::backends::oci::port_lease::new_launch_reservation_claim;
use crate::backends::oci::port_manager::{PortManager, ReservedLaunchPorts, SandboxLaunchPortPlan};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

fn reserve_machine_proxy_test_launch(
    manager: &PortManager,
    tenant: &TenantId,
    sandbox_id: &SandboxId,
    bindings: &[SandboxPortBinding],
) -> ReservedLaunchPorts {
    let reservation_claim =
        new_launch_reservation_claim().expect("machine proxy test claim should mint");
    manager
        .reserve_launch_ports_for_sandbox(
            SandboxLaunchPortPlan::new(tenant, sandbox_id, bindings, &[]),
            &reservation_claim,
        )
        .expect("machine proxy test launch should reserve through the production API")
}

fn assert_listener_remains_unreached(
    listener: &TcpListener,
    observation_window: Duration,
    invariant: &str,
) {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                drop(stream);
                panic!(
                    "{invariant}: target unexpectedly accepted a connection from {peer} \
                     after {:?}",
                    started.elapsed()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= observation_window {
                    return;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                panic!(
                    "{invariant}: target accept failed with {error} after {:?}",
                    started.elapsed()
                );
            }
        }
    }
}

fn wait_for_forwarded_connection(
    listener: &TcpListener,
    timeout: Duration,
    invariant: &str,
) -> TcpStream {
    let started = Instant::now();
    let endpoint = listener
        .local_addr()
        .expect("test listener address should remain observable");
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    panic!(
                        "{invariant}: listener {endpoint} did not accept a forwarded \
                         connection within {timeout:?}; elapsed {elapsed:?}; last state: \
                         WouldBlock"
                    );
                }
                thread::sleep(Duration::from_millis(10).min(timeout.saturating_sub(elapsed)));
            }
            Err(error) => {
                panic!(
                    "{invariant}: listener {endpoint} failed before accepting a forwarded \
                     connection after {:?}: {error}",
                    started.elapsed()
                );
            }
        }
    }
}

#[test]
fn machine_port_proxy_route_separates_guest_listener_from_external_publication() {
    let first = SandboxPortBinding::tcp("http", 41_500, 8080)
        .with_host_address(std::net::IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
    let second = first
        .clone()
        .with_host_address(std::net::IpAddr::V4(Ipv4Addr::new(198, 51, 100, 8)));

    let first_route =
        machine_port_proxy_routes(&[Ipv4Addr::LOCALHOST], std::slice::from_ref(&first))
            .expect("first route should normalize")
            .pop()
            .expect("one first route");
    let second_route =
        machine_port_proxy_routes(&[Ipv4Addr::LOCALHOST], std::slice::from_ref(&second))
            .expect("second route should normalize")
            .pop()
            .expect("one second route");

    assert_ne!(
        first_route, second_route,
        "route reuse must authenticate the exact external publication address even when the \
         guest listener and container target are unchanged"
    );
    assert_eq!(
        first_route.guest_listener_addr,
        SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), first.host_port)
    );
    assert_eq!(
        second_route.guest_listener_addr,
        SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), second.host_port)
    );
    assert_eq!(
        first_route.guest_listener_addr, second_route.guest_listener_addr,
        "gvproxy's inferred VM target must reach the same IPv4 wildcard guest listener"
    );
    assert_eq!(first_route.target_addr, second_route.target_addr);
    assert_eq!(
        first_route.external_publication_addr,
        SocketAddr::new(first.host_address, first.host_port)
    );
    assert_eq!(
        second_route.external_publication_addr,
        SocketAddr::new(second.host_address, second.host_port)
    );

    let ipv6 = SandboxPortBinding::tcp("https", 41_501, 8443)
        .with_host_address(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
    let ipv6_route = machine_port_proxy_routes(&[Ipv4Addr::LOCALHOST], std::slice::from_ref(&ipv6))
        .expect("IPv6 route should normalize")
        .pop()
        .expect("one IPv6 route");
    assert_eq!(
        ipv6_route.guest_listener_addr,
        SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), ipv6.host_port),
        "an IPv6 external publication still forwards into gvproxy's IPv4 guest network"
    );
    assert_eq!(
        ipv6_route.external_publication_addr,
        SocketAddr::new(ipv6.host_address, ipv6.host_port),
        "the route must authenticate the exact external IPv6 publication separately"
    );
}

struct BackpressureWriter {
    steps: VecDeque<std::result::Result<usize, io::ErrorKind>>,
    written: Vec<u8>,
}

impl Write for BackpressureWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self.steps.pop_front() {
            Some(Ok(limit)) => {
                let written = limit.min(buffer.len());
                self.written.extend_from_slice(&buffer[..written]);
                Ok(written)
            }
            Some(Err(kind)) => Err(io::Error::from(kind)),
            None => {
                self.written.extend_from_slice(buffer);
                Ok(buffer.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn machine_port_copy_retries_backpressure_without_losing_written_prefix() {
    let payload = (0..20_000)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let mut reader = Cursor::new(payload.as_slice());
    let mut writer = BackpressureWriter {
        steps: VecDeque::from([
            Ok(3),
            Err(io::ErrorKind::TimedOut),
            Err(io::ErrorKind::WouldBlock),
            Ok(5),
        ]),
        written: Vec::new(),
    };
    let shutdown = AtomicBool::new(false);

    copy_machine_port_stream(&mut reader, &mut writer, &shutdown);

    assert_eq!(
        writer.written, payload,
        "retryable backpressure must preserve every byte after a partial write"
    );
}

#[test]
fn machine_port_io_polling_rejects_each_configuration_failure() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test listener should bind");
    let client = TcpStream::connect(listener.local_addr().expect("test address should resolve"))
        .expect("test client should connect");
    let (target, _) = listener.accept().expect("test target should accept");

    for failed_operation in [
        MachinePortIoOperation::ClientRead,
        MachinePortIoOperation::ClientWrite,
        MachinePortIoOperation::TargetRead,
        MachinePortIoOperation::TargetWrite,
    ] {
        let error = configure_machine_port_io_polling_with(&client, &target, |_, operation, _| {
            if operation == failed_operation {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected timeout setup failure",
                ))
            } else {
                Ok(())
            }
        })
        .expect_err("every timeout setup failure must stop copy-pump admission");
        assert_eq!(error.operation, failed_operation);
        assert_eq!(error.source.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            error.to_string().contains("injected timeout setup failure"),
            "the provider failure must retain its source: {error}"
        );
    }
}

#[test]
fn machine_port_connection_surfaces_io_polling_failure_before_copy_pumps() {
    let inbound_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("inbound listener should bind");
    let _client = TcpStream::connect(
        inbound_listener
            .local_addr()
            .expect("inbound address should resolve"),
    )
    .expect("inbound client should connect");
    let (inbound, _) = inbound_listener
        .accept()
        .expect("inbound listener should accept");
    let target_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("target listener should bind");
    let shutdown = Arc::new(AtomicBool::new(false));

    let error = proxy_machine_port_connection_with(
        inbound,
        target_listener
            .local_addr()
            .expect("target address should resolve"),
        shutdown,
        |_, operation, _| {
            if operation == MachinePortIoOperation::ClientRead {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected timeout setup failure",
                ))
            } else {
                Ok(())
            }
        },
    )
    .expect_err("polling setup failure must return before copy pumps can block");

    assert!(
        error.contains("client read") && error.contains("injected timeout setup failure"),
        "connection setup must retain operation and provider cause: {error}"
    );
}

#[test]
fn second_copy_worker_spawn_failure_joins_first_worker_before_return() {
    let inbound_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("inbound listener should bind");
    let _client = TcpStream::connect(
        inbound_listener
            .local_addr()
            .expect("inbound address should resolve"),
    )
    .expect("inbound client should connect");
    let (inbound, _) = inbound_listener
        .accept()
        .expect("inbound listener should accept");
    let target_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("target listener should bind");
    let shutdown = Arc::new(AtomicBool::new(false));
    let provider_shutdown = Arc::clone(&shutdown);
    let (first_exited_tx, first_exited_rx) = mpsc::sync_channel(1);
    let mut spawn_calls = 0_u32;

    let error = proxy_machine_port_connection_with_spawner(
        inbound,
        target_listener
            .local_addr()
            .expect("target address should resolve"),
        shutdown,
        |_, _, _| Ok(()),
        |direction, task| {
            spawn_calls += 1;
            if spawn_calls == 1 {
                assert_eq!(direction, "client-to-target");
                let first_exited_tx = first_exited_tx.clone();
                thread::Builder::new()
                    .name("test-machine-port-client-to-target".to_owned())
                    .spawn(move || {
                        task();
                        first_exited_tx
                            .send(())
                            .expect("first-worker exit should remain observable");
                    })
            } else {
                assert_eq!(direction, "target-to-client");
                Err(io::Error::other(
                    "injected second copy-worker spawn failure",
                ))
            }
        },
    )
    .expect_err("the injected second-worker spawn failure must be returned");

    assert!(
        error.contains("target-to-client")
            && error.contains("injected second copy-worker spawn failure"),
        "the failure must retain its direction and OS cause: {error}"
    );
    first_exited_rx
        .try_recv()
        .expect("return must acknowledge the first copy worker instead of detaching it");
    assert!(
        !provider_shutdown.load(Ordering::SeqCst),
        "one connection's spawn failure must not stop the provider-wide listener"
    );
}

#[test]
fn machine_port_connection_setup_failure_is_isolated_from_listener_shutdown() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let result = run_machine_port_connection_task(Arc::clone(&shutdown), |_| {
        Err("injected connection-local polling setup failure".to_owned())
    });

    assert!(
        result.is_ok(),
        "a connection-local setup failure must complete only that connection worker"
    );
    assert!(
        !shutdown.load(Ordering::SeqCst),
        "a connection-local setup failure must not own provider-wide listener shutdown"
    );
}

#[test]
fn machine_listener_accepts_after_connection_local_setup_failure() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test listener should bind");
    listener
        .set_nonblocking(true)
        .expect("test listener should become nonblocking");
    let address = listener
        .local_addr()
        .expect("test listener address should resolve");
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let listener_owned = Arc::new(AtomicBool::new(true));
    let attempts = Arc::new(AtomicUsize::new(0));
    let worker_attempts = Arc::clone(&attempts);
    let (observed_tx, observed_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        accept_machine_port_proxy_with(
            listener,
            address,
            worker_shutdown,
            listener_owned,
            move |stream, _, _| {
                drop(stream);
                let attempt = worker_attempts.fetch_add(1, Ordering::SeqCst) + 1;
                observed_tx
                    .send(attempt)
                    .map_err(|error| format!("connection observation failed: {error}"))?;
                if attempt == 1 {
                    Err("injected first-connection setup failure".to_owned())
                } else {
                    Ok(())
                }
            },
        )
    });

    drop(TcpStream::connect(address).expect("first connection should enter the listener"));
    assert_eq!(
        observed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first failed connection should be observed"),
        1
    );
    assert!(
        !shutdown.load(Ordering::SeqCst),
        "the first connection failure must not stop the provider"
    );
    drop(TcpStream::connect(address).expect("second connection should enter the listener"));
    assert_eq!(
        observed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("listener should observe a second connection"),
        2,
        "the same listener must remain available after connection-local failure"
    );

    shutdown.store(true, Ordering::SeqCst);
    // The worker polls shutdown and may close the listener before this
    // best-effort wake races the next poll.
    let _ = TcpStream::connect(address);
    worker
        .join()
        .expect("accept worker should not panic")
        .expect("explicit provider shutdown should join cleanly");
}

#[test]
fn machine_connection_set_reaps_completed_workers_and_reuses_capacity() {
    let mut connections = MachinePortConnectionSet::new(1, Arc::new(AtomicBool::new(false)));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    assert!(
        connections
            .try_spawn(move || {
                started_tx.send(()).expect("start signal should send");
                release_rx.recv().expect("release signal should arrive");
                Ok(())
            })
            .expect("first connection worker should spawn")
    );
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first worker should reach its bounded barrier");

    let overflow_ran = Arc::new(AtomicBool::new(false));
    let overflow_probe = Arc::clone(&overflow_ran);
    assert!(
        !connections
            .try_spawn(move || {
                overflow_probe.store(true, Ordering::SeqCst);
                Ok(())
            })
            .expect("capacity rejection should not fail the listener"),
        "a full connection set must reject unbounded worker growth"
    );
    assert!(
        !overflow_ran.load(Ordering::SeqCst),
        "the rejected task and its accepted socket must be dropped without running"
    );

    release_tx.send(()).expect("first worker should release");
    let deadline = Instant::now() + Duration::from_secs(1);
    while connections.active_len() != 0 {
        connections
            .reap_completed()
            .expect("a completed connection worker should join cleanly");
        assert!(
            Instant::now() < deadline,
            "completed connection worker was not reaped within one second"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        connections
            .try_spawn(|| Ok(()))
            .expect("reaped capacity should admit a new worker"),
        "capacity must be reusable after a completed connection is joined"
    );
    connections
        .join_all()
        .expect("replacement connection worker should join cleanly");
}

#[test]
fn machine_proxy_worker_panic_reports_once_then_acknowledges_joined_absence() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let join = thread::spawn(|| -> std::result::Result<(), String> {
        panic!("injected machine accept-worker panic")
    });
    let mut proxy = super::MachinePortProxy {
        bind_addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 9),
        shutdown,
        listener_owned: Arc::new(AtomicBool::new(false)),
        stop_state: super::MachinePortProxyStopState::Running(join),
    };

    let first = proxy
        .shutdown()
        .expect_err("the first shutdown must report the worker panic");
    assert!(
        first
            .to_string()
            .contains("accept worker panicked during shutdown"),
        "the joined panic must remain diagnostic: {first}"
    );
    proxy
        .shutdown()
        .expect("retry must acknowledge the already-joined provider absence");
}

#[test]
fn failed_connection_worker_stops_idle_listener_without_future_traffic() {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("failed-worker listener should bind");
    listener
        .set_nonblocking(true)
        .expect("failed-worker listener should become nonblocking");
    let bind_addr = listener
        .local_addr()
        .expect("failed-worker listener should expose its address");
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let listener_owned = Arc::new(AtomicBool::new(true));
    let worker_listener_owned = Arc::clone(&listener_owned);
    let (entered_tx, entered_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        accept_machine_port_proxy_with(
            listener,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9)),
            worker_shutdown,
            worker_listener_owned,
            move |_, _, _| -> std::result::Result<(), String> {
                entered_tx
                    .send(())
                    .expect("failed connection should signal");
                panic!("injected connection-worker panic");
            },
        )
    });

    drop(TcpStream::connect(bind_addr).expect("one connection should enter the listener"));
    entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the failing worker should start");
    let deadline = Instant::now() + Duration::from_secs(1);
    while !join.is_finished() {
        assert!(
            Instant::now() < deadline,
            "the idle accept loop did not reap its failed worker without future traffic"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let mut proxy = super::MachinePortProxy {
        bind_addr,
        shutdown,
        listener_owned,
        stop_state: super::MachinePortProxyStopState::Running(join),
    };
    assert!(
        !proxy.provider_is_running(),
        "an exited accept worker must not count as a live provider"
    );
    let first = proxy
        .shutdown()
        .expect_err("the first stop must report the connection-worker panic");
    assert!(
        first
            .to_string()
            .contains("machine port connection worker panicked"),
        "the exact provider failure must remain diagnostic: {first}"
    );
    proxy
        .shutdown()
        .expect("retry should acknowledge the already-joined provider absence");
    drop(
        TcpListener::bind(bind_addr)
            .expect("joined failed provider must release its exact listener"),
    );

    let healthy_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("healthy listener should bind");
    healthy_listener
        .set_nonblocking(true)
        .expect("healthy listener should become nonblocking");
    let healthy_addr = healthy_listener
        .local_addr()
        .expect("healthy listener should expose its address");
    let healthy_shutdown = Arc::new(AtomicBool::new(false));
    let healthy_worker_shutdown = Arc::clone(&healthy_shutdown);
    let healthy_listener_owned = Arc::new(AtomicBool::new(true));
    let worker_healthy_listener_owned = Arc::clone(&healthy_listener_owned);
    let (healthy_tx, healthy_rx) = mpsc::channel();
    let healthy_join = thread::spawn(move || {
        accept_machine_port_proxy_with(
            healthy_listener,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9)),
            healthy_worker_shutdown,
            worker_healthy_listener_owned,
            move |_, _, _| {
                healthy_tx
                    .send(())
                    .expect("healthy connection should signal");
                Ok(())
            },
        )
    });
    drop(TcpStream::connect(healthy_addr).expect("healthy listener should accept"));
    healthy_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("healthy worker should run");
    drop(TcpStream::connect(healthy_addr).expect("healthy listener should accept again"));
    healthy_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("reaped healthy capacity should admit another connection");
    assert!(
        !healthy_join.is_finished(),
        "successful connection completion must leave the listener running"
    );
    let mut healthy_proxy = super::MachinePortProxy {
        bind_addr: healthy_addr,
        shutdown: healthy_shutdown,
        listener_owned: healthy_listener_owned,
        stop_state: super::MachinePortProxyStopState::Running(healthy_join),
    };
    assert!(healthy_proxy.provider_is_running());
    healthy_proxy
        .shutdown()
        .expect("healthy provider should stop explicitly");
}

#[test]
fn closed_listener_is_not_live_while_connection_workers_drain() {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("draining listener should bind");
    listener
        .set_nonblocking(true)
        .expect("draining listener should become nonblocking");
    let bind_addr = listener
        .local_addr()
        .expect("draining listener should expose its address");
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let listener_owned = Arc::new(AtomicBool::new(true));
    let worker_listener_owned = Arc::clone(&listener_owned);
    let connection_index = Arc::new(AtomicUsize::new(0));
    let worker_connection_index = Arc::clone(&connection_index);
    let (drain_entered_tx, drain_entered_rx) = mpsc::channel();
    let (release_drain_tx, release_drain_rx) = mpsc::channel();
    let (panic_entered_tx, panic_entered_rx) = mpsc::channel();
    let (liveness_cleared_tx, liveness_cleared_rx) = mpsc::channel();
    let (release_listener_drop_tx, release_listener_drop_rx) = mpsc::channel();
    let release_drain_rx = Arc::new(std::sync::Mutex::new(release_drain_rx));
    let join = thread::spawn(move || {
        accept_machine_port_proxy_with_listener_observer(
            listener,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9)),
            worker_shutdown,
            worker_listener_owned,
            move |_, _, _| {
                if worker_connection_index.fetch_add(1, Ordering::SeqCst) == 0 {
                    drain_entered_tx
                        .send(())
                        .expect("draining worker should signal");
                    release_drain_rx
                        .lock()
                        .expect("drain release lock should not be poisoned")
                        .recv_timeout(Duration::from_secs(2))
                        .expect("parent or deadline must release the draining worker");
                    Ok(())
                } else {
                    panic_entered_tx
                        .send(())
                        .expect("panicking worker should signal");
                    panic!("injected connection-worker panic during listener drain");
                }
            },
            move || {
                liveness_cleared_tx
                    .send(())
                    .expect("listener liveness transition should signal");
                release_listener_drop_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("parent or deadline must release physical listener drop");
            },
        )
    });
    let mut proxy = super::MachinePortProxy {
        bind_addr,
        shutdown,
        listener_owned,
        stop_state: super::MachinePortProxyStopState::Running(join),
    };

    drop(TcpStream::connect(bind_addr).expect("draining connection should enter"));
    drain_entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("draining worker should enter");
    drop(TcpStream::connect(bind_addr).expect("panicking connection should enter"));
    panic_entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("panicking worker should enter");
    liveness_cleared_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("worker failure should clear listener liveness before drain");

    let reported_running = proxy.provider_is_running();
    let premature_rebind = TcpListener::bind(bind_addr);
    assert_eq!(
        premature_rebind
            .expect_err("liveness must clear before the kernel listener is dropped")
            .kind(),
        io::ErrorKind::AddrInUse,
        "the liveness transition must precede physical socket release"
    );
    let accept_worker_still_draining = matches!(
        &proxy.stop_state,
        super::MachinePortProxyStopState::Running(join) if !join.is_finished()
    );

    release_listener_drop_tx
        .send(())
        .expect("physical listener drop should release");
    let rebound = (0..100)
        .find_map(|_| match TcpListener::bind(bind_addr) {
            Ok(listener) => Some(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                thread::sleep(Duration::from_millis(10));
                None
            }
            Err(error) => panic!("listener rebind failed unexpectedly: {error}"),
        })
        .expect("closed listener must become kernel-rebindable while connections drain");
    release_drain_tx
        .send(())
        .expect("draining worker should release");
    let first = proxy
        .shutdown()
        .expect_err("the first stop should report the worker panic");
    assert!(
        first
            .to_string()
            .contains("machine port connection worker panicked"),
        "the provider failure must remain diagnostic: {first}"
    );
    proxy
        .shutdown()
        .expect("retry should acknowledge the drained provider absence");
    drop(rebound);

    assert!(
        accept_worker_still_draining,
        "the proof must observe listener absence before connection drain completes"
    );
    assert!(
        !reported_running,
        "a closed listener must not be reported live while connection workers drain"
    );
}

#[test]
fn panicking_accept_scope_drains_tracked_connection_workers_before_unwind_returns() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = mpsc::channel();
    let (begin_unwind_tx, begin_unwind_rx) = mpsc::channel();
    let (trigger_unwind_tx, trigger_unwind_rx) = mpsc::channel();
    let (drained_tx, drained_rx) = mpsc::channel();
    let (unwind_done_tx, unwind_done_rx) = mpsc::channel();
    let driver_shutdown = Arc::clone(&shutdown);
    let driver = thread::spawn(move || {
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            let mut connections = MachinePortConnectionSet::new(1, Arc::clone(&driver_shutdown));
            let worker_shutdown = Arc::clone(&driver_shutdown);
            assert!(
                connections
                    .try_spawn(move || {
                        started_tx.send(()).expect("start signal should send");
                        let deadline = Instant::now() + Duration::from_secs(2);
                        while !worker_shutdown.load(Ordering::SeqCst) && Instant::now() < deadline {
                            thread::sleep(Duration::from_millis(5));
                        }
                        drained_tx.send(()).expect("drain signal should send");
                        Ok(())
                    })
                    .expect("tracked connection worker should spawn")
            );
            trigger_unwind_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("parent should trigger the bounded unwind");
            begin_unwind_tx
                .send(())
                .expect("unwind start should signal");
            panic!("injected accept-scope panic");
        }));
        unwind_done_tx
            .send(unwind.is_err())
            .expect("unwind completion should signal");
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("connection worker should enter its bounded wait");
    trigger_unwind_tx
        .send(())
        .expect("the supervised driver should begin unwinding");
    begin_unwind_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the driver should report its unwind boundary");
    let drained_before_manual_cleanup = drained_rx.recv_timeout(Duration::from_secs(1));
    if drained_before_manual_cleanup.is_err() {
        shutdown.store(true, Ordering::SeqCst);
        drained_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("bounded fail-before cleanup must release the supervised worker");
    }
    assert!(
        drained_before_manual_cleanup.is_ok(),
        "connection-set Drop must signal and join every tracked worker before unwind returns"
    );
    assert!(
        unwind_done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the bounded unwind driver must complete"),
        "the injected accept-scope panic must occur"
    );
    driver
        .join()
        .expect("the supervised unwind driver must not escape its catcher");
}

#[test]
fn machine_port_proxy_rejects_bind_without_port_lease() {
    let state = TempDir::new().expect("state root");
    let manager =
        PortManager::new(state.path(), 41_500..=41_500).with_machine_port_proxy_bindings();
    let binding = SandboxPortBinding::tcp("http", 0, 8080);

    let tenant = TenantId::new("machine-no-lease").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-no-lease");
    let result = prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        &[binding],
        &[],
        &manager,
    );

    assert!(
        result.is_err(),
        "MachinePortProxy must not bind or serve without an active host-port lease"
    );
}

#[test]
fn restart_route_failure_retains_reserved_machine_listener_authority() {
    let state = TempDir::new().expect("state root");
    let port_probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
    let port = port_probe
        .local_addr()
        .expect("probe address should resolve")
        .port();
    drop(port_probe);
    let manager = PortManager::new(state.path(), port..=port).with_machine_port_proxy_bindings();
    let tenant = TenantId::new("machine-restart-route").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-restart-route");
    let binding = SandboxPortBinding::tcp("http", port, 8080);
    let reservations = reserve_machine_proxy_test_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
    );

    match prepare_machine_port_proxies_with_release_authority(
        &tenant,
        &sandbox_id,
        &[],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
        MachinePortPreparationReleaseAuthority::Retain,
    ) {
        Ok(_) => panic!("restart preparation without a route must fail closed"),
        Err(error) => error,
    };

    let record = LocalPortLeaseAuthority::open(state.path())
        .expect("authority should reopen")
        .inspect(reservations.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("persisted listener authority should remain durable");
    assert_eq!(
        record.phase(),
        PortLeasePhase::Reserved,
        "restart-retained authority must not be released by a preparation-only route failure"
    );
    assert!(
        record.bind_claim().is_none() && record.binding().is_none(),
        "route failure precedes any bind attempt or provider effect"
    );

    match prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
    ) {
        Ok(_) => panic!("fresh launch preparation without a route must fail closed"),
        Err(error) => assert!(
            error
                .to_string()
                .contains("without a container IPv4 address"),
            "the provider-plan failure remains primary: {error}"
        ),
    }
    let released = LocalPortLeaseAuthority::open(state.path())
        .expect("authority should reopen")
        .inspect(reservations.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("released authority should remain auditable");
    assert_eq!(
        released.phase(),
        PortLeasePhase::Released,
        "the explicit fresh-launch capability may retire proven never-bound authority"
    );
}

#[test]
fn machine_port_proxy_collision_records_durable_no_effect_failure() {
    let external =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("external guest owner should bind");
    let port = external
        .local_addr()
        .expect("external address should resolve")
        .port();
    let state = TempDir::new().expect("state root");
    let manager = PortManager::new(state.path(), port..=port).with_machine_port_proxy_bindings();
    let tenant = TenantId::new("machine-collision").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-collision");
    let binding = SandboxPortBinding::tcp("http", port, 8080);
    let reservations = reserve_machine_proxy_test_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
    );

    let error = match prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
    ) {
        Ok(_) => panic!("the real provider bind must lose to the external owner"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("failed to bind machine port proxy"),
        "the provider error should retain effect context: {error}"
    );

    let authority = LocalPortLeaseAuthority::open(state.path()).expect("authority should restart");
    let record = authority
        .inspect(reservations.published_leases[0].lease_id())
        .expect("inspection should succeed")
        .expect("failed lease evidence must persist");
    assert_eq!(record.phase(), PortLeasePhase::Failed);
    let failure = record.failure().expect("failed phase must carry evidence");
    assert_eq!(failure.kind(), PortBindFailureKind::AddrInUse);
    assert_eq!(failure.attempt().port(), port);
    drop(external);
}

#[test]
fn retained_machine_proxy_bind_failure_returns_to_reserved_and_retries() {
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("port probe should bind");
    let port = port_probe
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    drop(port_probe);
    let state = TempDir::new().expect("state root");
    let manager = PortManager::new(state.path(), port..=port).with_machine_port_proxy_bindings();
    let tenant = TenantId::new("machine-retained-bind").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-retained-bind");
    let binding = SandboxPortBinding::tcp("http", port, 8080);
    let reservations = reserve_machine_proxy_test_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
    );

    let prepared = prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
    )
    .expect("initial provider should bind");
    let bind_claims = prepared
        .iter()
        .map(|proxy| proxy.bind_claim().clone())
        .collect::<Vec<_>>();
    let active_bindings = manager
        .activate_machine_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reservations.published_leases,
            &bind_claims,
        )
        .expect("initial provider should adopt and activate");
    drop(prepared);
    manager
        .prepare_machine_bindings_for_rebind(&reservations.published_leases, &active_bindings)
        .expect("confirmed provider absence should return the exact lease to Reserved");

    let blocker = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("external conflict should occupy the restart endpoint");
    match prepare_machine_port_proxies_with_release_authority(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
        MachinePortPreparationReleaseAuthority::Retain,
    ) {
        Ok(_) => panic!("the transient address conflict must fail this restart attempt"),
        Err(error) => assert!(
            error
                .to_string()
                .contains("failed to bind machine port proxy"),
            "the provider failure should remain explicit: {error}"
        ),
    }
    let retained = LocalPortLeaseAuthority::open(state.path())
        .expect("authority should reopen")
        .inspect(reservations.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("restart authority should remain durable");
    assert_eq!(
        retained.phase(),
        PortLeasePhase::Reserved,
        "a no-effect restart bind miss must remain retryable rather than terminal"
    );
    assert!(
        retained.bind_claim().is_none()
            && retained.binding().is_none()
            && retained.failure().is_none(),
        "the failed restart attempt must abandon only its exact bind claim"
    );

    drop(blocker);
    let prepared = prepare_machine_port_proxies_with_release_authority(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
        MachinePortPreparationReleaseAuthority::Retain,
    )
    .expect("the retained request should bind after the conflict clears");
    let retry_claims = prepared
        .iter()
        .map(|proxy| proxy.bind_claim().clone())
        .collect::<Vec<_>>();
    manager
        .activate_machine_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reservations.published_leases,
            &retry_claims,
        )
        .expect("the retry should adopt and activate");
    assert_eq!(
        LocalPortLeaseAuthority::open(state.path())
            .expect("authority should reopen")
            .inspect(reservations.published_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("retry authority should remain durable")
            .phase(),
        PortLeasePhase::Active
    );
    drop(prepared);
    manager
        .withdraw_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reservations.published_leases,
        )
        .expect("retry authority should withdraw after the held listener closes");
    manager
        .release_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reservations.published_leases,
        )
        .expect("confirmed provider absence should release retry authority");
}

#[test]
fn same_request_machine_replay_cannot_fail_or_release_held_batch() {
    let first_probe =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("first port probe should bind");
    let first_port = first_probe
        .local_addr()
        .expect("first port should resolve")
        .port();
    let second_probe =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("second port probe should bind");
    let second_port = second_probe
        .local_addr()
        .expect("second port should resolve")
        .port();
    assert_ne!(first_port, second_port);
    drop((first_probe, second_probe));

    let state = TempDir::new().expect("state root");
    let manager = PortManager::new(
        state.path(),
        first_port.min(second_port)..=first_port.max(second_port),
    )
    .with_machine_port_proxy_bindings();
    let tenant = TenantId::new("machine-same-request").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-same-request");
    let bindings = [
        SandboxPortBinding::tcp("http", first_port, 8080),
        SandboxPortBinding::tcp("admin", second_port, 8081),
    ];
    let reservations = reserve_machine_proxy_test_launch(&manager, &tenant, &sandbox_id, &bindings);
    let prepared = prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        &bindings,
        &reservations.published_leases,
        &manager,
    )
    .expect("first process should claim and hold both sockets");

    let replay_manager = PortManager::new(
        state.path(),
        first_port.min(second_port)..=first_port.max(second_port),
    )
    .with_machine_port_proxy_bindings();
    let error = match prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        &bindings,
        &reservations.published_leases,
        &replay_manager,
    ) {
        Ok(_) => panic!("a second process must not bind the claimed request batch"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("different in-flight provider bind attempt"),
        "the replay must lose at durable claim acquisition before binding: {error}"
    );

    let authority = LocalPortLeaseAuthority::open(state.path()).expect("authority should reopen");
    for request in &reservations.published_leases {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(record.bind_claim().is_some());
        assert_eq!(record.failure(), None);
        assert_eq!(record.binding(), None);
    }

    let bind_claims = prepared
        .iter()
        .map(|proxy| proxy.bind_claim().clone())
        .collect::<Vec<_>>();
    manager
        .activate_machine_bindings(
            &tenant,
            &sandbox_id,
            &bindings,
            &reservations.published_leases,
            &bind_claims,
        )
        .expect("the exact claimant should adopt and activate both listeners");
    let proxies = start_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &bindings,
        &reservations.published_leases,
        &manager,
        prepared,
    )
    .expect("the exact claimant should start both listeners");
    manager
        .withdraw_bindings(
            &tenant,
            &sandbox_id,
            &bindings,
            &reservations.published_leases,
        )
        .expect("exact active listeners should withdraw");
    for mut proxy in proxies {
        proxy.shutdown().expect("machine proxy should stop");
    }
    manager
        .release_bindings(
            &tenant,
            &sandbox_id,
            &bindings,
            &reservations.published_leases,
        )
        .expect("confirmed stopped listeners should release");
}

#[test]
fn later_machine_proxy_collision_releases_never_bound_siblings() {
    let first_probe =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("first port probe should bind");
    let first_port = first_probe
        .local_addr()
        .expect("first probe address should resolve")
        .port();
    let external =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("external guest owner should bind");
    let second_port = external
        .local_addr()
        .expect("external address should resolve")
        .port();
    assert_ne!(first_port, second_port, "test ports must be distinct");
    drop(first_probe);

    let state = TempDir::new().expect("state root");
    let manager = PortManager::new(
        state.path(),
        first_port.min(second_port)..=first_port.max(second_port),
    )
    .with_machine_port_proxy_bindings();
    let tenant = TenantId::new("machine-partial-collision").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-partial-collision");
    let bindings = [
        SandboxPortBinding::tcp("first", first_port, 8080),
        SandboxPortBinding::tcp("second", second_port, 8081),
    ];
    let reservations = reserve_machine_proxy_test_launch(&manager, &tenant, &sandbox_id, &bindings);

    let error = match prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        &bindings,
        &reservations.published_leases,
        &manager,
    ) {
        Ok(_) => panic!("the second real provider bind must lose to the external owner"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("failed to bind machine port proxy"),
        "the provider error should remain primary: {error}"
    );

    let authority = LocalPortLeaseAuthority::open(state.path()).expect("authority should restart");
    let first = authority
        .inspect(reservations.published_leases[0].lease_id())
        .expect("first inspection should succeed")
        .expect("first sibling evidence must persist");
    let second = authority
        .inspect(reservations.published_leases[1].lease_id())
        .expect("second inspection should succeed")
        .expect("failed member evidence must persist");
    assert_eq!(
        first.phase(),
        PortLeasePhase::Released,
        "dropping the first proxy must precede durable sibling release"
    );
    assert_eq!(
        second.phase(),
        PortLeasePhase::Failed,
        "the external collision remains terminal no-effect evidence"
    );
    TcpListener::bind((Ipv4Addr::LOCALHOST, first_port))
        .expect("the released sibling's real socket must no longer be held");
    drop(external);
}

#[test]
fn prepared_machine_proxy_is_inert_until_exact_lease_is_active() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("target listener should bind");
    target
        .set_nonblocking(true)
        .expect("target listener should become nonblocking");
    let target_port = target
        .local_addr()
        .expect("target address should resolve")
        .port();
    let port_probe =
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("proxy port guard should bind");
    let proxy_port = port_probe
        .local_addr()
        .expect("proxy address should resolve")
        .port();
    drop(port_probe);

    let state = TempDir::new().expect("state root");
    let manager =
        PortManager::new(state.path(), proxy_port..=proxy_port).with_machine_port_proxy_bindings();
    let tenant = TenantId::new("machine-prepare-order").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-prepare-order");
    let binding = SandboxPortBinding::tcp("http", proxy_port, target_port);
    let reservations = reserve_machine_proxy_test_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
    );
    let prepared = prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
    )
    .expect("preparation should bind and retain an inert socket");

    let mut client =
        TcpStream::connect((Ipv4Addr::LOCALHOST, proxy_port)).expect("backlog should connect");
    client
        .write_all(b"ping")
        .expect("request should enter the inert listener backlog");
    assert_listener_remains_unreached(
        &target,
        Duration::from_millis(500),
        "an inert prepared proxy must not forward before activation",
    );

    let start_error = match start_machine_port_proxies(
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
        prepared,
    ) {
        Ok(_) => panic!("a Reserved lease must not authorize the accept loop"),
        Err(error) => error,
    };
    assert!(
        start_error.to_string().contains("expected exact Active"),
        "start must fail at the durable activation guard: {start_error}"
    );
    drop(client);

    let prepared = prepare_machine_port_proxies(
        &tenant,
        &sandbox_id,
        &[Ipv4Addr::LOCALHOST],
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
    )
    .expect("the dropped inert socket should be preparable again");
    let bind_claims = prepared
        .iter()
        .map(|proxy| proxy.bind_claim().clone())
        .collect::<Vec<_>>();
    manager
        .activate_machine_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reservations.published_leases,
            &bind_claims,
        )
        .expect("the held provider socket should be adopted and activated");
    let proxies = start_machine_port_proxies(
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
        &reservations.published_leases,
        &manager,
        prepared,
    )
    .expect("exact Active evidence should authorize serving");
    let mut client =
        TcpStream::connect((Ipv4Addr::LOCALHOST, proxy_port)).expect("proxy should accept");
    client
        .write_all(b"ping")
        .expect("activated proxy should receive the request");

    let mut forwarded = wait_for_forwarded_connection(
        &target,
        Duration::from_secs(1),
        "an activated machine proxy must forward the queued connection",
    );
    forwarded
        .set_nonblocking(false)
        .expect("accepted target stream should become blocking");
    forwarded
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("target read should be bounded");
    let mut request = [0_u8; 4];
    forwarded
        .read_exact(&mut request)
        .expect("activated proxy should forward the queued request");
    assert_eq!(&request, b"ping");
    drop(client);
    for mut proxy in proxies {
        proxy
            .shutdown()
            .expect("explicit shutdown should drain every tracked connection");
    }
}
