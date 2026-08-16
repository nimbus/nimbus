//! Bounded, substitutable application-readiness probes shared by sandbox backends.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use nimbus_network::{EndpointProtocol, PublishedEndpoint};
use serde::Serialize;

use crate::instance::SandboxStatus;

pub(crate) const DEFAULT_READINESS_PROBE_TIMEOUT: Duration = Duration::from_millis(1_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum ReadinessProbeTarget {
    Tcp(SocketAddr),
    Http(SocketAddr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum ReadinessProbeObservation {
    Ready,
    NotReady { reason: String },
    Unknown { reason: String },
}

pub(crate) trait ReadinessProbeProvider: Send + Sync {
    fn probe(&self, target: ReadinessProbeTarget, timeout: Duration) -> ReadinessProbeObservation;
}

/// Exact application-readiness provider evidence used by one status
/// projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ApplicationReadinessEvidence {
    target: Option<ReadinessProbeTarget>,
    observation: Option<ReadinessProbeObservation>,
    status: SandboxStatus,
}

impl ApplicationReadinessEvidence {
    pub(crate) fn status(&self) -> SandboxStatus {
        self.status
    }
}

#[derive(Debug, Default)]
pub(crate) struct SocketReadinessProbeProvider;

impl ReadinessProbeProvider for SocketReadinessProbeProvider {
    fn probe(&self, target: ReadinessProbeTarget, timeout: Duration) -> ReadinessProbeObservation {
        match target {
            ReadinessProbeTarget::Tcp(address) => {
                match TcpStream::connect_timeout(&address, timeout) {
                    Ok(_) => ReadinessProbeObservation::Ready,
                    Err(error) => not_ready(format!("TCP readiness probe failed: {error}")),
                }
            }
            ReadinessProbeTarget::Http(address) => probe_http(address, timeout),
        }
    }
}

#[cfg(test)]
pub(crate) fn application_readiness_status(
    current: SandboxStatus,
    endpoints: &[PublishedEndpoint],
    timeout: Duration,
    provider: &dyn ReadinessProbeProvider,
) -> SandboxStatus {
    inspect_application_readiness(current, endpoints, timeout, provider).status()
}

pub(crate) fn inspect_application_readiness(
    current: SandboxStatus,
    endpoints: &[PublishedEndpoint],
    timeout: Duration,
    provider: &dyn ReadinessProbeProvider,
) -> ApplicationReadinessEvidence {
    let target = readiness_probe_target(endpoints);
    let observation = target.map(|target| provider.probe(target, timeout));
    let status = match observation.as_ref() {
        None | Some(ReadinessProbeObservation::Ready) => SandboxStatus::Ready,
        Some(
            ReadinessProbeObservation::NotReady { .. } | ReadinessProbeObservation::Unknown { .. },
        ) if matches!(current, SandboxStatus::Ready | SandboxStatus::NotReady) => {
            SandboxStatus::NotReady
        }
        Some(
            ReadinessProbeObservation::NotReady { .. } | ReadinessProbeObservation::Unknown { .. },
        ) => SandboxStatus::Starting,
    };
    ApplicationReadinessEvidence {
        target,
        observation,
        status,
    }
}

pub(crate) fn readiness_probe_target(
    endpoints: &[PublishedEndpoint],
) -> Option<ReadinessProbeTarget> {
    endpoints
        .iter()
        .find_map(|endpoint| match endpoint.protocol {
            EndpointProtocol::Http => Some(ReadinessProbeTarget::Http(endpoint.address)),
            EndpointProtocol::Https | EndpointProtocol::Tcp => None,
        })
        .or_else(|| {
            endpoints
                .iter()
                .find_map(|endpoint| match endpoint.protocol {
                    EndpointProtocol::Https => Some(ReadinessProbeTarget::Tcp(endpoint.address)),
                    EndpointProtocol::Http | EndpointProtocol::Tcp => None,
                })
        })
        .or_else(|| {
            endpoints
                .iter()
                .find_map(|endpoint| match endpoint.protocol {
                    EndpointProtocol::Tcp => Some(ReadinessProbeTarget::Tcp(endpoint.address)),
                    EndpointProtocol::Http | EndpointProtocol::Https => None,
                })
        })
}

fn probe_http(address: SocketAddr, timeout: Duration) -> ReadinessProbeObservation {
    let Some(deadline) = Instant::now().checked_add(timeout) else {
        return unknown("HTTP readiness deadline overflowed");
    };
    let mut stream = match TcpStream::connect_timeout(&address, timeout) {
        Ok(stream) => stream,
        Err(error) => return not_ready(format!("HTTP readiness connect failed: {error}")),
    };
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return not_ready("HTTP readiness probe timed out after connect");
    };
    if remaining.is_zero() {
        return not_ready("HTTP readiness probe timed out after connect");
    }
    if let Err(error) = stream.set_read_timeout(Some(remaining)) {
        return unknown(format!("cannot set HTTP readiness read deadline: {error}"));
    }
    if let Err(error) = stream.set_write_timeout(Some(remaining)) {
        return unknown(format!("cannot set HTTP readiness write deadline: {error}"));
    }
    if let Err(error) = stream.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n") {
        return not_ready(format!("HTTP readiness request failed: {error}"));
    }
    let mut response = [0_u8; 256];
    let mut received = 0;
    while received < response.len() && !response[..received].contains(&b'\n') {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return not_ready("HTTP readiness response exceeded its deadline");
        };
        if remaining.is_zero() {
            return not_ready("HTTP readiness response exceeded its deadline");
        }
        if let Err(error) = stream.set_read_timeout(Some(remaining)) {
            return unknown(format!(
                "cannot refresh HTTP readiness read deadline: {error}"
            ));
        }
        match stream.read(&mut response[received..]) {
            Ok(0) => break,
            Ok(read) => received += read,
            Err(error) => {
                return not_ready(format!("HTTP readiness response failed: {error}"));
            }
        }
    }
    if !response[..received].contains(&b'\n') {
        return not_ready("HTTP readiness response ended before a complete status line");
    }
    if valid_http_status_line(&response[..received]) {
        ReadinessProbeObservation::Ready
    } else {
        not_ready("HTTP readiness response lacks a valid status line")
    }
}

fn valid_http_status_line(response: &[u8]) -> bool {
    let line = response
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut fields = line.split(|byte| *byte == b' ');
    let version = fields.next().unwrap_or_default();
    let status = fields.next().unwrap_or_default();
    matches!(version, b"HTTP/1.0" | b"HTTP/1.1")
        && status.len() == 3
        && status.iter().all(u8::is_ascii_digit)
        && (b'1'..=b'5').contains(&status[0])
}

fn not_ready(reason: impl Into<String>) -> ReadinessProbeObservation {
    ReadinessProbeObservation::NotReady {
        reason: reason.into(),
    }
}

fn unknown(reason: impl Into<String>) -> ReadinessProbeObservation {
    ReadinessProbeObservation::Unknown {
        reason: reason.into(),
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct FixedReadinessProbeProvider {
    observation: std::sync::Mutex<ReadinessProbeObservation>,
    calls: std::sync::Mutex<Vec<(ReadinessProbeTarget, Duration)>>,
}

#[cfg(test)]
impl FixedReadinessProbeProvider {
    pub(crate) fn new(observation: ReadinessProbeObservation) -> Self {
        Self {
            observation: std::sync::Mutex::new(observation),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn ready() -> Self {
        Self::new(ReadinessProbeObservation::Ready)
    }

    pub(crate) fn not_ready(reason: impl Into<String>) -> Self {
        Self::new(not_ready(reason))
    }

    pub(crate) fn unknown(reason: impl Into<String>) -> Self {
        Self::new(unknown(reason))
    }

    pub(crate) fn set_observation(&self, observation: ReadinessProbeObservation) {
        *self
            .observation
            .lock()
            .expect("fixed readiness observation lock should not be poisoned") = observation;
    }

    pub(crate) fn calls(&self) -> Vec<(ReadinessProbeTarget, Duration)> {
        self.calls
            .lock()
            .expect("fixed readiness calls lock should not be poisoned")
            .clone()
    }
}

#[cfg(test)]
impl ReadinessProbeProvider for FixedReadinessProbeProvider {
    fn probe(&self, target: ReadinessProbeTarget, timeout: Duration) -> ReadinessProbeObservation {
        self.calls
            .lock()
            .expect("fixed readiness calls lock should not be poisoned")
            .push((target, timeout));
        self.observation
            .lock()
            .expect("fixed readiness observation lock should not be poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;
    use std::time::Instant;

    use super::*;

    fn endpoint(name: &str, protocol: EndpointProtocol, address: SocketAddr) -> PublishedEndpoint {
        PublishedEndpoint::new(name, protocol, address)
    }

    fn serve_chunks(
        responses: Vec<Vec<u8>>,
        delay: Duration,
    ) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        listener
            .set_nonblocking(true)
            .expect("listener should become nonblocking");
        let address = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("accepted test stream should become blocking");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .expect("server read deadline");
                        stream
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .expect("server write deadline");
                        let mut request = [0_u8; 256];
                        let read = stream.read(&mut request).expect("request should arrive");
                        assert!(read > 0, "probe request must contain bytes");
                        for response in responses {
                            stream.write_all(&response).expect("response should write");
                            if !delay.is_zero() {
                                thread::sleep(delay);
                            }
                        }
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "test listener did not receive the bounded probe"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("test listener accept failed: {error}"),
                }
            }
        });
        (address, server)
    }

    fn serve_once(response: &[u8]) -> (SocketAddr, thread::JoinHandle<()>) {
        serve_chunks(vec![response.to_vec()], Duration::ZERO)
    }

    #[test]
    fn target_selection_prefers_http_then_https_then_tcp() {
        let tcp: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        let https: SocketAddr = "127.0.0.1:1002".parse().unwrap();
        let http: SocketAddr = "127.0.0.1:1003".parse().unwrap();
        assert_eq!(
            readiness_probe_target(&[
                endpoint("tcp", EndpointProtocol::Tcp, tcp),
                endpoint("https", EndpointProtocol::Https, https),
                endpoint("http", EndpointProtocol::Http, http),
            ]),
            Some(ReadinessProbeTarget::Http(http))
        );
        assert_eq!(
            readiness_probe_target(&[
                endpoint("tcp", EndpointProtocol::Tcp, tcp),
                endpoint("https", EndpointProtocol::Https, https),
            ]),
            Some(ReadinessProbeTarget::Tcp(https))
        );
        assert_eq!(
            readiness_probe_target(&[endpoint("tcp", EndpointProtocol::Tcp, tcp)]),
            Some(ReadinessProbeTarget::Tcp(tcp))
        );
    }

    #[test]
    fn deterministic_provider_captures_exact_target_timeout_and_recovery() {
        let address: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        let endpoints = [endpoint("http", EndpointProtocol::Http, address)];
        let provider = FixedReadinessProbeProvider::not_ready("warming");
        let timeout = Duration::from_millis(37);

        assert_eq!(
            application_readiness_status(SandboxStatus::Starting, &endpoints, timeout, &provider),
            SandboxStatus::Starting
        );
        provider.set_observation(ReadinessProbeObservation::Ready);
        assert_eq!(
            application_readiness_status(SandboxStatus::NotReady, &endpoints, timeout, &provider),
            SandboxStatus::Ready
        );
        assert_eq!(
            provider.calls(),
            vec![
                (ReadinessProbeTarget::Http(address), timeout),
                (ReadinessProbeTarget::Http(address), timeout),
            ]
        );
    }

    #[test]
    fn no_endpoint_is_ready_without_provider_io() {
        let provider = FixedReadinessProbeProvider::unknown("must not run");
        assert_eq!(
            application_readiness_status(
                SandboxStatus::Starting,
                &[],
                Duration::from_millis(1),
                &provider
            ),
            SandboxStatus::Ready
        );
        assert!(provider.calls().is_empty());
    }

    #[test]
    fn unknown_and_not_ready_fail_closed_for_starting_and_ready_workloads() {
        let address: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        let endpoints = [endpoint("tcp", EndpointProtocol::Tcp, address)];
        for provider in [
            FixedReadinessProbeProvider::not_ready("refused"),
            FixedReadinessProbeProvider::unknown("inspection unavailable"),
        ] {
            assert_eq!(
                application_readiness_status(
                    SandboxStatus::Starting,
                    &endpoints,
                    Duration::from_millis(1),
                    &provider
                ),
                SandboxStatus::Starting
            );
            assert_eq!(
                application_readiness_status(
                    SandboxStatus::Ready,
                    &endpoints,
                    Duration::from_millis(1),
                    &provider
                ),
                SandboxStatus::NotReady
            );
        }
    }

    #[test]
    fn deterministic_tcp_timeout_never_mints_readiness() {
        let address: SocketAddr = "127.0.0.1:1001".parse().unwrap();
        let endpoints = [endpoint("tcp", EndpointProtocol::Tcp, address)];
        let provider = FixedReadinessProbeProvider::not_ready("TCP readiness probe timed out");
        let timeout = Duration::from_millis(17);

        assert_eq!(
            application_readiness_status(SandboxStatus::Starting, &endpoints, timeout, &provider),
            SandboxStatus::Starting
        );
        assert_eq!(
            provider.calls(),
            vec![(ReadinessProbeTarget::Tcp(address), timeout)]
        );
    }

    #[test]
    fn real_http_probe_accepts_valid_status_and_rejects_invalid_or_malformed_status() {
        let provider = SocketReadinessProbeProvider;
        let (ready_address, ready_server) =
            serve_once(b"HTTP/1.0 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(
            provider.probe(
                ReadinessProbeTarget::Http(ready_address),
                Duration::from_secs(1)
            ),
            ReadinessProbeObservation::Ready
        );
        ready_server.join().expect("ready server should join");

        let (bad_address, bad_server) = serve_once(b"HTTP/not-a-status\r\n\r\n");
        assert!(matches!(
            provider.probe(
                ReadinessProbeTarget::Http(bad_address),
                Duration::from_secs(1)
            ),
            ReadinessProbeObservation::NotReady { .. }
        ));
        bad_server.join().expect("malformed server should join");

        let (truncated_address, truncated_server) = serve_once(b"HTTP/1.1 200 OK");
        assert!(matches!(
            provider.probe(
                ReadinessProbeTarget::Http(truncated_address),
                Duration::from_secs(1)
            ),
            ReadinessProbeObservation::NotReady { .. }
        ));
        truncated_server
            .join()
            .expect("truncated server should join");

        for rejected in [
            b"HTTP/1.1 099 Invalid\r\n\r\n".as_slice(),
            b"HTTP/1.1 600 Invalid\r\n\r\n".as_slice(),
            b"HTTP/2 200 Unsupported\r\n\r\n".as_slice(),
        ] {
            assert!(
                !valid_http_status_line(rejected),
                "invalid status line must be rejected: {}",
                String::from_utf8_lossy(rejected)
            );
        }
    }

    #[test]
    fn real_http_probe_accepts_a_fragmented_status_line_within_the_deadline() {
        let (address, server) = serve_chunks(
            vec![
                b"HTTP/1.".to_vec(),
                b"1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
            ],
            Duration::from_millis(10),
        );
        let provider = SocketReadinessProbeProvider;

        assert_eq!(
            provider.probe(
                ReadinessProbeTarget::Http(address),
                Duration::from_millis(200)
            ),
            ReadinessProbeObservation::Ready
        );
        server.join().expect("fragmented server should join");
    }

    #[test]
    fn real_http_probe_rejects_a_status_line_beyond_the_response_bound() {
        let oversized = vec![b'x'; 300];
        let (address, server) = serve_once(&oversized);
        let provider = SocketReadinessProbeProvider;

        assert!(matches!(
            provider.probe(ReadinessProbeTarget::Http(address), Duration::from_secs(1)),
            ReadinessProbeObservation::NotReady { .. }
        ));
        server.join().expect("oversized server should join");
    }

    #[test]
    fn real_http_probe_bounds_a_server_that_never_sends_a_response() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        listener
            .set_nonblocking(true)
            .expect("listener should become nonblocking");
        let address = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("accepted test stream should become blocking");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .expect("server read deadline");
                        let mut request = [0_u8; 256];
                        assert!(
                            stream.read(&mut request).expect("request should arrive") > 0,
                            "probe request must contain bytes"
                        );
                        thread::sleep(Duration::from_millis(100));
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "test listener did not receive the bounded probe"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("test listener accept failed: {error}"),
                }
            }
        });
        let provider = SocketReadinessProbeProvider;
        let started = Instant::now();

        assert!(matches!(
            provider.probe(
                ReadinessProbeTarget::Http(address),
                Duration::from_millis(20)
            ),
            ReadinessProbeObservation::NotReady { .. }
        ));
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "HTTP response wait must honor the probe deadline"
        );
        server.join().expect("bounded server should join");
    }

    #[test]
    fn real_tcp_probe_reports_ready_and_refused() {
        let provider = SocketReadinessProbeProvider;
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        assert_eq!(
            provider.probe(ReadinessProbeTarget::Tcp(address), Duration::from_secs(1)),
            ReadinessProbeObservation::Ready
        );
        drop(listener);
        assert!(matches!(
            provider.probe(
                ReadinessProbeTarget::Tcp(address),
                Duration::from_millis(100)
            ),
            ReadinessProbeObservation::NotReady { .. }
        ));
    }
}
