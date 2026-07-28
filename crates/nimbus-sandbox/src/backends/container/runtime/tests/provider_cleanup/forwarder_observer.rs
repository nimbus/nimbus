//! Semantic machine-forwarder request observation for provider-cleanup tests.

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::backends::oci::network::OciMachinePortForwarderConfig;

const FORWARDER_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);
static NEXT_COMPLETION_ID: AtomicU64 = AtomicU64::new(1);

pub(super) struct ForwarderObserver {
    address: SocketAddr,
    completion_request: Vec<u8>,
    expected_requests: usize,
    server: Option<thread::JoinHandle<Result<Vec<Vec<u8>>, String>>>,
}

impl ForwarderObserver {
    pub(super) fn spawn(
        listener: TcpListener,
        successful_responses: Vec<bool>,
        expected_requests: usize,
    ) -> Self {
        Self::spawn_with_provider(listener, None, successful_responses, expected_requests)
    }

    pub(super) fn spawn_authenticated(
        listener: TcpListener,
        provider: &OciMachinePortForwarderConfig,
        successful_responses: Vec<bool>,
        expected_requests: usize,
    ) -> Self {
        Self::spawn_with_provider(
            listener,
            Some(provider.clone()),
            successful_responses,
            expected_requests,
        )
    }

    fn spawn_with_provider(
        listener: TcpListener,
        provider: Option<OciMachinePortForwarderConfig>,
        successful_responses: Vec<bool>,
        expected_requests: usize,
    ) -> Self {
        assert!(
            expected_requests <= successful_responses.len(),
            "every expected request must have a scripted response"
        );
        let address = listener
            .local_addr()
            .expect("forwarder observer address should resolve");
        let completion_id = NEXT_COMPLETION_ID.fetch_add(1, Ordering::Relaxed);
        let completion_request = format!(
            "POST /__nimbus_test_forwarder_observer_complete/{completion_id} HTTP/1.0\r\n\
             Content-Length: 0\r\n\r\n"
        )
        .into_bytes();
        let server_completion_request = completion_request.clone();
        let server = thread::spawn(move || {
            observe_requests_until_completion(
                listener,
                &server_completion_request,
                provider.as_ref(),
                &successful_responses,
            )
        });
        Self {
            address,
            completion_request,
            expected_requests,
            server: Some(server),
        }
    }

    pub(super) fn finish_exact(mut self) -> Vec<Vec<u8>> {
        let signal = signal_completion(self.address, &self.completion_request);
        let observed = self
            .server
            .take()
            .expect("forwarder observer server should be owned")
            .join()
            .expect("forwarder observer thread should not panic")
            .unwrap_or_else(|error| panic!("forwarder observer failed: {error}"));
        signal.unwrap_or_else(|error| panic!("forwarder observer completion failed: {error}"));
        assert_eq!(
            observed.len(),
            self.expected_requests,
            "forwarder observer expected exactly {} request(s), observed {}: {:?}",
            self.expected_requests,
            observed.len(),
            observed
                .iter()
                .map(|request| String::from_utf8_lossy(request).into_owned())
                .collect::<Vec<_>>()
        );
        observed
    }
}

impl Drop for ForwarderObserver {
    fn drop(&mut self) {
        let Some(server) = self.server.take() else {
            return;
        };
        let _ = signal_completion(self.address, &self.completion_request);
        let _ = server.join();
    }
}

fn observe_requests_until_completion(
    listener: TcpListener,
    completion_request: &[u8],
    provider: Option<&OciMachinePortForwarderConfig>,
    successful_responses: &[bool],
) -> Result<Vec<Vec<u8>>, String> {
    let mut requests = Vec::new();
    let mut retained_publications = BTreeSet::new();
    loop {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("accept failed: {error}"))?;
        let request = read_complete_http_request_bytes(&mut stream)?;
        if request == completion_request {
            return Ok(requests);
        }
        if request
            .split(|byte| *byte == b'\n')
            .next()
            .is_some_and(|line| line.windows(5).any(|window| window == b"/all "))
        {
            let body = serde_json::to_vec(
                &retained_publications
                    .iter()
                    .map(|local| serde_json::json!({ "local": local }))
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| format!("inspection response encoding failed: {error}"))?;
            let response = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .and_then(|()| stream.write_all(&body))
                .map_err(|error| format!("inspection response write failed: {error}"))?;
            continue;
        }
        let successful = successful_responses
            .get(requests.len())
            .copied()
            .unwrap_or(false);
        let local = request_local(&request)?;
        if let Some(local) = local.as_ref() {
            if successful {
                retained_publications.remove(local);
            } else {
                retained_publications.insert(local.clone());
            }
        }
        requests.push(request);
        let response = if successful && let (Some(provider), Some(local)) = (provider, local) {
            let body = serde_json::to_vec(&serde_json::json!({
                "outcome": "withdrawn",
                "provider_instance": provider.provider_instance(),
                "provider_generation": provider.provider_generation(),
                "local": local,
                "protocol": "tcp",
            }))
            .map_err(|error| format!("typed withdrawal receipt encoding failed: {error}"))?;
            let mut response = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .into_bytes();
            response.extend_from_slice(&body);
            response
        } else if successful {
            b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()
        } else {
            b"HTTP/1.0 500 Internal Server Error\r\nContent-Length: 15\r\n\r\nproxy not found"
                .to_vec()
        };
        stream
            .set_write_timeout(Some(FORWARDER_OBSERVATION_TIMEOUT))
            .map_err(|error| format!("response write timeout failed: {error}"))?;
        stream
            .write_all(&response)
            .map_err(|error| format!("response write failed: {error}"))?;
    }
}

fn request_local(request: &[u8]) -> Result<Option<String>, String> {
    let Some(body_start) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(None);
    };
    let body = &request[body_start + 4..];
    if body.is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| format!("forwarder request body was not JSON: {error}"))?;
    Ok(value
        .get("local")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned))
}

fn signal_completion(address: SocketAddr, completion_request: &[u8]) -> Result<(), String> {
    let mut stream = TcpStream::connect_timeout(&address, FORWARDER_OBSERVATION_TIMEOUT)
        .map_err(|error| format!("completion connection to {address} failed: {error}"))?;
    stream
        .set_write_timeout(Some(FORWARDER_OBSERVATION_TIMEOUT))
        .map_err(|error| format!("completion write timeout failed: {error}"))?;
    stream
        .write_all(completion_request)
        .map_err(|error| format!("completion marker write failed: {error}"))?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| format!("completion marker shutdown failed: {error}"))
}

fn read_complete_http_request_bytes(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    stream
        .set_read_timeout(Some(FORWARDER_OBSERVATION_TIMEOUT))
        .map_err(|error| format!("request read timeout failed: {error}"))?;
    let mut request = Vec::new();
    let mut expected_len = None;
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("request read failed: {error}"))?;
        if read == 0 {
            return Err("request closed before its complete body arrived".to_owned());
        }
        request.extend_from_slice(&chunk[..read]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..header_end])
                .map_err(|error| format!("request headers were not UTF-8: {error}"))?;
            let content_len = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then_some(value.trim())
                })
                .map(str::parse::<usize>)
                .transpose()
                .map_err(|error| format!("request content length was invalid: {error}"))?
                .unwrap_or(0);
            expected_len = Some(header_end + 4 + content_len);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            return Ok(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_marker_finishes_exact_zero_request_observation() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("observer listener should bind");
        let observer = ForwarderObserver::spawn(listener, Vec::new(), 0);

        assert!(
            observer.finish_exact().is_empty(),
            "semantic completion should prove an exact zero-request interval"
        );
    }

    #[test]
    fn scripted_observer_preserves_request_order_and_response_sequence() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("observer listener should bind");
        let address = listener
            .local_addr()
            .expect("observer address should resolve");
        let observer = ForwarderObserver::spawn(listener, vec![false, true], 2);

        let first = send_request(address, "/first");
        let second = send_request(address, "/second");
        let observed = observer.finish_exact();

        assert!(
            first.starts_with(b"HTTP/1.0 500") && second.starts_with(b"HTTP/1.0 200"),
            "scripted responses must retain their exact order"
        );
        assert!(observed[0].starts_with(b"POST /first "));
        assert!(observed[1].starts_with(b"POST /second "));
    }

    #[test]
    fn finish_exact_reports_unexpected_requests_with_counts() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("observer listener should bind");
        let address = listener
            .local_addr()
            .expect("observer address should resolve");
        let observer = ForwarderObserver::spawn(listener, Vec::new(), 0);
        let _ = send_request(address, "/unexpected");

        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer.finish_exact()))
                .expect_err("an unexpected request must fail exact observation");
        let diagnostic = if let Some(message) = panic.downcast_ref::<String>() {
            message.clone()
        } else if let Some(message) = panic.downcast_ref::<&str>() {
            (*message).to_owned()
        } else {
            String::new()
        };
        assert!(
            diagnostic.contains("expected exactly 0 request(s), observed 1")
                && diagnostic.contains("/unexpected"),
            "the mismatch must report exact counts and request evidence: {diagnostic}"
        );
    }

    fn send_request(address: SocketAddr, path: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).expect("request should connect");
        write!(stream, "POST {path} HTTP/1.0\r\nContent-Length: 0\r\n\r\n")
            .expect("request should write");
        stream
            .shutdown(Shutdown::Write)
            .expect("request write should finish");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .expect("response should read");
        response
    }
}
