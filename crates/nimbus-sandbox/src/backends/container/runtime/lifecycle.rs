use super::support::*;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use tempfile::TempDir;

use crate::backends::oci::command::CommandSpec;

#[test]
fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args(["-c", "exit 1"]);
    std::fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

    assert_eq!(
        backend
            .detect_runtime_status(&manifest)
            .expect("status should resolve"),
        SandboxStatus::Failed
    );
}

#[test]
fn release_execution_artifacts_ignores_machine_forwarder_unexpose_failures() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection should arrive");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        stream
            .write_all(
                b"HTTP/1.0 500 Internal Server Error\r\nContent-Length: 16\r\n\r\nproxy not found",
            )
            .expect("response should write");
    });

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("cleanup should ignore unexpose failures");
    server.join().expect("server thread should join");
}
