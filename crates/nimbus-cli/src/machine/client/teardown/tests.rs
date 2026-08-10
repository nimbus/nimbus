use std::io::{Read, Write};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::Path;
use std::thread::JoinHandle;
use std::time::Duration;

use nimbus_machine::api::{
    MachineApiWorkloadTeardownExecuteObservation, MachineApiWorkloadTeardownInspectObservation,
    MachineApiWorkloadTeardownObservation, MachineApiWorkloadTeardownPhaseRequest,
    MachineApiWorkloadTeardownPhaseResponse,
};
use nimbus_workloads::{WorkloadTeardownCommandMode, WorkloadTeardownStep};
use serde_json::json;

use super::*;
use crate::machine::api::{teardown_wire_fixture, teardown_wire_fixture_for_forwarder};

enum ScriptedReply {
    Bytes(Vec<u8>),
    Close,
    Delay(Duration),
}

#[test]
fn teardown_client_accepts_one_fully_correlated_execute_and_inspect_response() {
    let execute = exact_request(WorkloadTeardownCommandMode::Execute);
    let inspect = exact_request(WorkloadTeardownCommandMode::Inspect);
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let server = spawn_scripted_server(
        &socket_path,
        vec![
            ScriptedReply::Bytes(exact_response_bytes(&execute)),
            ScriptedReply::Bytes(exact_response_bytes(&inspect)),
        ],
    );
    let client = MachineApiClient::new_for_test(&socket_path)
        .with_forwarder_authority(execute.forwarder_authority().clone());

    for request in [&execute, &inspect] {
        let outcome = client
            .teardown_workload_phase(request)
            .expect("pre-send request validation should pass");
        let MachineApiWorkloadTeardownTransportOutcome::Correlated(response) = outcome else {
            panic!("exact response must correlate: {outcome:?}");
        };
        response
            .validate_for_request(request)
            .expect("client must retain every response fence");
    }

    assert_request_sequence(server, &[&execute, &inspect]);
}

#[test]
fn teardown_client_rejects_missing_or_crossed_authority_before_socket_io() {
    let request = exact_request(WorkloadTeardownCommandMode::Execute);
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("must-not-exist.sock");

    let missing = MachineApiClient::new_for_test(&socket_path)
        .teardown_workload_phase(&request)
        .expect_err("missing authority must fail before transport");
    assert!(
        missing.to_string().contains("forwarder authority"),
        "{missing}"
    );

    let (foreign_authority, _) = teardown_wire_fixture_for_forwarder(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
        "foreign-client-forwarder",
        request.forwarder_authority().generation().as_u64(),
    );
    let crossed = MachineApiClient::new_for_test(&socket_path)
        .with_forwarder_authority(foreign_authority)
        .teardown_workload_phase(&request)
        .expect_err("crossed authority must fail before transport");
    assert!(crossed.to_string().contains("crossed"), "{crossed}");
    assert!(
        !socket_path.exists(),
        "deterministic pre-send rejection must open no socket"
    );
}

#[test]
fn teardown_client_response_loss_decode_timeout_and_size_failures_are_ambiguous_and_one_shot() {
    let request = exact_request(WorkloadTeardownCommandMode::Execute);
    let malformed = http_response(b"{", None);
    let truncated = http_response(b"{", Some(100));
    let non_success =
        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec();
    let oversized_body = vec![b' '; MAX_WORKLOAD_TEARDOWN_RESPONSE_BODY_BYTES + 1];
    let oversized_content_length = http_response(&oversized_body, None);
    let oversized_chunked = chunked_response(&oversized_body);

    for reply in [
        ScriptedReply::Close,
        ScriptedReply::Bytes(malformed),
        ScriptedReply::Bytes(truncated),
        ScriptedReply::Bytes(non_success),
        ScriptedReply::Bytes(oversized_content_length),
        ScriptedReply::Bytes(oversized_chunked),
    ] {
        assert_one_shot_ambiguous(&request, reply, Duration::from_secs(2));
    }
    assert_one_shot_ambiguous(
        &request,
        ScriptedReply::Delay(Duration::from_millis(150)),
        Duration::from_millis(50),
    );
}

#[test]
fn teardown_client_rejects_every_crossed_response_fence_as_ambiguous() {
    let request = exact_request(WorkloadTeardownCommandMode::Execute);
    let foreign = foreign_request(WorkloadTeardownCommandMode::Execute);
    let stop = request_for_step(
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let detach = request_for_step(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
    );
    let exact = response_value(&request);
    let foreign = response_value(&foreign);
    let stop = response_value(&stop);
    let detach = response_value(&detach);
    let mut crossed_responses = Vec::new();

    for (pointer, source) in [
        ("/requestDigest", &foreign),
        ("/forwarderAuthority", &foreign),
        ("/commandId", &stop),
        ("/issuingTransitionId", &stop),
        ("/confirmedTransitionId", &stop),
        ("/attemptId", &stop),
        ("/providerTarget", &detach),
        ("/subjects", &detach),
    ] {
        let mut crossed = exact.clone();
        *crossed
            .pointer_mut(pointer)
            .expect("response fence should exist") = source
            .pointer(pointer)
            .expect("crossed response fence should exist")
            .clone();
        crossed_responses.push(crossed);
    }
    for (field, replacement) in [
        ("issuingRevision", json!("999")),
        ("confirmedRevision", json!("999")),
        ("dispatchEpoch", json!("1")),
        ("providerTranslation", json!("guest_container_attachment")),
        ("step", json!("stop_execution")),
        ("mode", json!("inspect")),
    ] {
        let mut crossed = exact.clone();
        crossed[field] = replacement;
        crossed_responses.push(crossed);
    }
    let mut unknown = exact;
    unknown["unknown"] = json!(true);
    crossed_responses.push(unknown);

    for crossed in crossed_responses {
        assert_one_shot_ambiguous(
            &request,
            ScriptedReply::Bytes(http_response(
                &serde_json::to_vec(&crossed).expect("crossed response should encode"),
                None,
            )),
            Duration::from_secs(2),
        );
    }
}

#[test]
fn teardown_client_sends_exact_inspect_after_ambiguous_execute_without_automatic_retry() {
    let execute = exact_request(WorkloadTeardownCommandMode::Execute);
    let inspect = exact_request(WorkloadTeardownCommandMode::Inspect);
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let server = spawn_scripted_server(
        &socket_path,
        vec![
            ScriptedReply::Close,
            ScriptedReply::Bytes(exact_response_bytes(&inspect)),
        ],
    );
    let client = MachineApiClient::new_for_test(&socket_path)
        .with_forwarder_authority(execute.forwarder_authority().clone());

    let first = client
        .teardown_workload_phase(&execute)
        .expect("execute is valid before transport");
    assert_ambiguous(first);
    let second = client
        .teardown_workload_phase(&inspect)
        .expect("inspect is valid before transport");
    assert!(matches!(
        second,
        MachineApiWorkloadTeardownTransportOutcome::Correlated(_)
    ));

    assert_request_sequence(server, &[&execute, &inspect]);
}

fn exact_request(mode: WorkloadTeardownCommandMode) -> MachineApiWorkloadTeardownPhaseRequest {
    request_for_step(WorkloadTeardownStep::DrainExecution, mode)
}

fn request_for_step(
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
) -> MachineApiWorkloadTeardownPhaseRequest {
    let (authority, command) = teardown_wire_fixture(step, mode);
    MachineApiWorkloadTeardownPhaseRequest::new(authority, command)
        .expect("exact client request should validate")
}

fn foreign_request(mode: WorkloadTeardownCommandMode) -> MachineApiWorkloadTeardownPhaseRequest {
    let (authority, command) = teardown_wire_fixture_for_forwarder(
        WorkloadTeardownStep::DrainExecution,
        mode,
        "foreign-response-forwarder",
        7,
    );
    MachineApiWorkloadTeardownPhaseRequest::new(authority, command)
        .expect("foreign response request should remain internally correlated")
}

fn response_value(request: &MachineApiWorkloadTeardownPhaseRequest) -> serde_json::Value {
    serde_json::to_value(exact_response(request)).expect("response should serialize")
}

fn exact_response(
    request: &MachineApiWorkloadTeardownPhaseRequest,
) -> MachineApiWorkloadTeardownPhaseResponse {
    let observation = match request.command().mode() {
        WorkloadTeardownCommandMode::Execute => MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
        ),
        WorkloadTeardownCommandMode::Inspect => MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Ambiguous,
        ),
    };
    MachineApiWorkloadTeardownPhaseResponse::for_request(request, observation)
        .expect("exact response should validate")
}

fn exact_response_bytes(request: &MachineApiWorkloadTeardownPhaseRequest) -> Vec<u8> {
    http_response(
        &serde_json::to_vec(&exact_response(request)).expect("response should encode"),
        None,
    )
}

fn assert_one_shot_ambiguous(
    request: &MachineApiWorkloadTeardownPhaseRequest,
    reply: ScriptedReply,
    timeout: Duration,
) {
    let temp_dir = short_socket_tempdir();
    let socket_path = temp_dir.path().join("nimbus.sock");
    let server = spawn_scripted_server(&socket_path, vec![reply]);
    let client = MachineApiClient::new_for_test(&socket_path)
        .with_forwarder_authority(request.forwarder_authority().clone())
        .with_mutation_io_timeout_for_test(timeout);

    let outcome = client
        .teardown_workload_phase(request)
        .expect("pre-send validation should pass");
    assert_ambiguous(outcome);
    assert_request_sequence(server, &[request]);
}

fn assert_ambiguous(outcome: MachineApiWorkloadTeardownTransportOutcome) {
    let MachineApiWorkloadTeardownTransportOutcome::Ambiguous { reason } = outcome else {
        panic!("transport uncertainty must remain ambiguous: {outcome:?}");
    };
    assert!(reason.contains("ambiguous outcome"), "{reason}");
}

fn spawn_scripted_server(
    socket_path: &Path,
    replies: Vec<ScriptedReply>,
) -> JoinHandle<Vec<Vec<u8>>> {
    let listener = StdUnixListener::bind(socket_path).expect("scripted server should bind");
    std::thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            requests.push(read_http_request_body(&mut stream));
            match reply {
                ScriptedReply::Bytes(response) => {
                    let _ = stream.write_all(&response);
                }
                ScriptedReply::Close => {}
                ScriptedReply::Delay(delay) => std::thread::sleep(delay),
            }
        }
        requests
    })
}

fn read_http_request_body(stream: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("request read timeout should set");
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream
            .read(&mut chunk)
            .expect("request should remain readable");
        assert!(read > 0, "request must contain HTTP bytes");
        request.extend_from_slice(&chunk[..read]);
        let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let body_start = header_end + 4;
        let headers =
            std::str::from_utf8(&request[..header_end]).expect("request headers should be UTF-8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length: ")
                    .or_else(|| line.strip_prefix("Content-Length: "))
            })
            .expect("teardown POST must declare content length")
            .parse::<usize>()
            .expect("content length should be numeric");
        if request.len() >= body_start + content_length {
            return request[body_start..body_start + content_length].to_vec();
        }
    }
}

fn assert_request_sequence(
    server: JoinHandle<Vec<Vec<u8>>>,
    expected: &[&MachineApiWorkloadTeardownPhaseRequest],
) {
    let requests = server.join().expect("scripted server should join");
    assert_eq!(requests.len(), expected.len());
    for (body, expected) in requests.iter().zip(expected) {
        let decoded: MachineApiWorkloadTeardownPhaseRequest =
            serde_json::from_slice(body).expect("client must send the strict request");
        assert_eq!(&decoded, *expected);
    }
}

fn http_response(body: &[u8], declared_content_length: Option<usize>) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        declared_content_length.unwrap_or(body.len())
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn chunked_response(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response.extend_from_slice(b"\r\n0\r\n\r\n");
    response
}

fn short_socket_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("nimbus-teardown-")
        .tempdir_in("/tmp")
        .expect("short socket tempdir should create")
}
