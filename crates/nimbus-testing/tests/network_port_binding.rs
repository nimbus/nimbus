use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, PortBindAttempt, PortBindFailure,
    PortBindFailureKind, PortBindRealm, PortBindTarget, PortBindingMismatch, PortBindingProvenance,
    PortBindingSpec, PortBoundEndpoint, PortExposure, PortLeaseBinding, PortLeaseError,
    PortLeaseId, PortLeasePhase, PortLeaseRequest, PortProtocol, PortRequestMode,
};
use wait_timeout::ChildExt;

const CHILD_TEST: &str = "external_binder_child";
const CHILD_MODE_ENV: &str = "NIMBUS_NETWORK_PORT_BINDING_CHILD";
const CHILD_BOUND_PREFIX: &str = "NIMBUS_EXTERNAL_BOUND ";
const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn external_addr_in_use_is_durable_and_cannot_publish() {
    let root = tempfile::tempdir().expect("state root should exist");
    let external = ExternalBinder::start();
    let port = NonZeroU16::new(external.bound_addr().port())
        .expect("kernel-assigned external port should be non-zero");
    let request = request(
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        port,
    );
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    authority
        .reserve(request.clone())
        .expect("exact port should reserve before provider bind");

    assert_eq!(
        external.bound_addr(),
        SocketAddr::from((Ipv4Addr::LOCALHOST, port.get()))
    );

    let bind_error = TcpListener::bind(external.bound_addr())
        .expect_err("external process must win the real provider-equivalent bind");
    assert_eq!(bind_error.kind(), std::io::ErrorKind::AddrInUse);

    let attempt = bind_attempt(external.bound_addr());
    let failure = PortBindFailure::new(
        PortBindFailureKind::AddrInUse,
        attempt.clone(),
        provider_handle("direct-bind-attempt"),
    );
    let failed = authority
        .record_bind_failure_without_effect(&request, failure.clone())
        .expect("real bind failure should commit");
    assert_eq!(failed.phase(), PortLeasePhase::Failed);
    assert_eq!(failed.failure(), Some(&failure));
    assert_eq!(failed.binding(), None);
    assert!(matches!(
        authority.activate(&request),
        Err(PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Failed,
            ..
        })
    ));

    drop(authority);
    let restarted = LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
    let durable = restarted
        .inspect(request.lease_id())
        .expect("failed lease should inspect")
        .expect("failed lease should remain durable");
    assert_eq!(durable.failure(), Some(&failure));
    assert_eq!(durable.phase(), PortLeasePhase::Failed);
    assert_eq!(
        durable
            .failure()
            .expect("failure evidence should exist")
            .attempt(),
        &attempt
    );

    external.finish();
}

#[test]
fn externally_owned_prebound_listener_adopts_exact_identity_and_address() {
    let root = tempfile::tempdir().expect("state root should exist");
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("pre-bound listener should bind");
    let addr = listener
        .local_addr()
        .expect("pre-bound listener should expose local address");
    let port = NonZeroU16::new(addr.port()).expect("kernel-assigned listener port is non-zero");
    let lease_request = request(
        "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        port,
    );
    let endpoint = bound_endpoint(addr);
    let handle = provider_handle("systemd:nimbus.socket:fd0");
    let binding = PortLeaseBinding::new(
        endpoint.clone(),
        PortBindingProvenance::ExternallyOwned,
        handle.clone(),
    );
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    authority
        .reserve(lease_request.clone())
        .expect("pre-bound address should reserve under its stable lease");
    let adopted = authority
        .adopt(&lease_request, binding.clone())
        .expect("matching inherited listener should adopt");
    assert_eq!(adopted.request().lease_id(), lease_request.lease_id());
    assert_eq!(adopted.binding(), Some(&binding));
    assert_eq!(
        adopted.binding().expect("binding should exist").endpoint(),
        &endpoint
    );
    assert_eq!(
        adopted
            .binding()
            .expect("binding should exist")
            .provider_handle(),
        &handle
    );
    assert_eq!(
        adopted
            .binding()
            .expect("binding should exist")
            .provenance(),
        PortBindingProvenance::ExternallyOwned
    );

    let active = authority
        .activate(&lease_request)
        .expect("durably adopted inherited listener should activate");
    assert_eq!(active.phase(), PortLeasePhase::Active);

    drop(authority);
    let restarted = LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
    let durable = restarted
        .inspect(lease_request.lease_id())
        .expect("active lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(durable.binding(), Some(&binding));

    restarted
        .withdraw(&lease_request)
        .expect("inherited listener should withdraw from publication");
    let replacement = request(
        "01ARZ3NDEKTSV4RRFFQ69G5FAY",
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        port,
    );
    assert!(matches!(
        restarted.reserve(replacement),
        Err(PortLeaseError::PortConflict {
            existing_phase: PortLeasePhase::Withdrawing,
            ..
        })
    ));

    let client = TcpStream::connect(addr).expect("external listener must remain externally owned");
    let (accepted, peer) = listener
        .accept()
        .expect("external listener should still accept after authority release");
    assert_eq!(
        accepted
            .local_addr()
            .expect("accepted stream should expose local address"),
        addr
    );
    assert_eq!(
        peer,
        client
            .local_addr()
            .expect("client should expose its local address")
    );
}

#[test]
fn adopted_address_and_provenance_must_satisfy_the_durable_request() {
    let root = tempfile::tempdir().expect("state root should exist");
    let port = NonZeroU16::new(41_473).expect("fixture port should be non-zero");
    let request = request(
        "01ARZ3NDEKTSV4RRFFQ69G5FAX",
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        port,
    );
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    authority
        .reserve(request.clone())
        .expect("exact request should reserve");

    let wrong_address = PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            port,
        )
        .expect("wildcard endpoint should validate"),
        PortBindingProvenance::ExternallyOwned,
        provider_handle("wrong-address"),
    );
    assert!(matches!(
        authority.adopt(&request, wrong_address),
        Err(PortLeaseError::BindingMismatch {
            mismatch: PortBindingMismatch::Target,
            ..
        })
    ));

    let wrong_provenance = PortLeaseBinding::new(
        bound_endpoint(SocketAddr::from((Ipv4Addr::LOCALHOST, port.get()))),
        PortBindingProvenance::ProviderAssigned,
        provider_handle("wrong-provenance"),
    );
    assert!(matches!(
        authority.adopt(&request, wrong_provenance),
        Err(PortLeaseError::BindingMismatch {
            mismatch: PortBindingMismatch::Provenance,
            ..
        })
    ));

    let durable = authority
        .inspect(request.lease_id())
        .expect("lease should inspect")
        .expect("lease should exist");
    assert_eq!(durable.phase(), PortLeasePhase::Reserved);
    assert_eq!(durable.binding(), None);
    assert_eq!(durable.failure(), None);
}

#[test]
#[ignore = "spawned only by the real external-binder parent"]
fn external_binder_child() {
    assert_eq!(
        std::env::var(CHILD_MODE_ENV).as_deref(),
        Ok("external-binder")
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("external child should bind a kernel-selected port");
    println!(
        "{CHILD_BOUND_PREFIX}{}",
        listener
            .local_addr()
            .expect("external listener should expose its address")
    );
    std::io::stdout()
        .flush()
        .expect("external child acknowledgement should flush");

    let mut release = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut release)
        .expect("external child release command should read");
    assert_eq!(release.trim(), "release");
}

fn request(payload: &str, target: PortBindTarget, port: NonZeroU16) -> PortLeaseRequest {
    let lease_id: PortLeaseId = format!("netportlease_{payload}")
        .parse()
        .expect("fixture lease ID should parse");
    let owner_id: ListenerId = format!("netlistener_{payload}")
        .parse()
        .expect("fixture owner ID should parse");
    PortLeaseRequest::new(
        lease_id,
        owner_id.into(),
        None,
        NetworkResourceGeneration::new(7),
        NetworkLeaseEpoch::new(11),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            target,
            PortExposure::Loopback,
            PortRequestMode::Exact(port),
        ),
    )
}

fn bound_endpoint(addr: SocketAddr) -> PortBoundEndpoint {
    let target = match addr {
        SocketAddr::V4(addr) => PortBindTarget::ipv4_specific(*addr.ip()),
        SocketAddr::V6(_) => panic!("fixture expects an IPv4 loopback listener"),
    };
    PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        target,
        NonZeroU16::new(addr.port()).expect("bound listener port is non-zero"),
    )
    .expect("real bound endpoint should validate")
}

fn bind_attempt(addr: SocketAddr) -> PortBindAttempt {
    let target = match addr {
        SocketAddr::V4(addr) => PortBindTarget::ipv4_specific(*addr.ip()),
        SocketAddr::V6(_) => panic!("fixture expects an IPv4 loopback listener"),
    };
    PortBindAttempt::new(PortProtocol::Tcp, PortBindRealm::Host, target, addr.port())
        .expect("real bind attempt should validate")
}

fn provider_handle(value: &str) -> NetworkProviderHandle {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider ID should parse");
    NetworkProviderHandle::new(provider_id, value).expect("fixture provider handle should validate")
}

struct ExternalBinder {
    child: Child,
    stdin: Option<ChildStdin>,
    bound_addr: SocketAddr,
    stdout_reader: Option<JoinHandle<Vec<String>>>,
    stderr_reader: Option<JoinHandle<String>>,
    finished: bool,
}

impl ExternalBinder {
    fn start() -> Self {
        let mut child =
            Command::new(std::env::current_exe().expect("test executable should resolve"))
                .arg("--exact")
                .arg(CHILD_TEST)
                .arg("--ignored")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env(CHILD_MODE_ENV, "external-binder")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("external binder child should spawn");
        let stdin = child
            .stdin
            .take()
            .expect("external child stdin should be piped");
        let stdout = child
            .stdout
            .take()
            .expect("external child stdout should be piped");
        let stderr = child
            .stderr
            .take()
            .expect("external child stderr should be piped");
        let (bound_tx, bound_rx) = mpsc::sync_channel(1);
        let stdout_reader = thread::spawn(move || {
            let mut lines = Vec::new();
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("external child stdout should read");
                if let Some(marker_index) = line.find(CHILD_BOUND_PREFIX) {
                    let addr = &line[marker_index + CHILD_BOUND_PREFIX.len()..];
                    let parsed = addr
                        .parse::<SocketAddr>()
                        .expect("external child bound address should parse");
                    let _ = bound_tx.send(parsed);
                }
                lines.push(line);
            }
            lines
        });
        let stderr_reader = thread::spawn(move || {
            let mut output = String::new();
            BufReader::new(stderr)
                .read_to_string(&mut output)
                .expect("external child stderr should read");
            output
        });

        let bound_addr = match bound_rx.recv_timeout(PROCESS_TIMEOUT) {
            Ok(addr) => addr,
            Err(error) => {
                let _ = child.kill();
                let status = child.wait().ok();
                let stdout = stdout_reader.join().unwrap_or_default().join("\n");
                let stderr = stderr_reader.join().unwrap_or_default();
                panic!(
                    "external child did not acknowledge bind: {error}; status={status:?}; \
                     stdout={stdout:?}; stderr={stderr:?}"
                );
            }
        };

        Self {
            child,
            stdin: Some(stdin),
            bound_addr,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            finished: false,
        }
    }

    fn bound_addr(&self) -> SocketAddr {
        self.bound_addr
    }

    fn finish(mut self) {
        let result = self.release_and_wait();
        self.finished = result.is_ok();
        if let Err(error) = result {
            panic!("external child cleanup failed: {error}");
        }
    }

    fn release_and_wait(&mut self) -> Result<(), String> {
        if let Some(mut stdin) = self.stdin.take() {
            stdin
                .write_all(b"release\n")
                .map_err(|error| format!("release command failed: {error}"))?;
            stdin
                .flush()
                .map_err(|error| format!("release flush failed: {error}"))?;
        }

        let status = match self.child.wait_timeout(PROCESS_TIMEOUT) {
            Ok(Some(status)) => status,
            Ok(None) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(format!(
                    "external child did not exit within {PROCESS_TIMEOUT:?}"
                ));
            }
            Err(error) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(format!("external child status failed: {error}"));
            }
        };

        let stdout = self
            .stdout_reader
            .take()
            .ok_or_else(|| "stdout reader was already consumed".to_owned())?
            .join()
            .map_err(|_| "stdout reader panicked".to_owned())?
            .join("\n");
        let stderr = self
            .stderr_reader
            .take()
            .ok_or_else(|| "stderr reader was already consumed".to_owned())?
            .join()
            .map_err(|_| "stderr reader panicked".to_owned())?;
        if !status.success() {
            return Err(format!(
                "external child exited with {status}; stdout={stdout:?}; stderr={stderr:?}"
            ));
        }
        if !stderr.is_empty() {
            return Err(format!("external child stderr was not empty: {stderr:?}"));
        }
        Ok(())
    }
}

impl Drop for ExternalBinder {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}
