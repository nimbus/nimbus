use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::OciDockerfileBuilder;
use crate::backends::oci::buildah::OciExposedPortProtocol;
use crate::instance::SandboxId;
use crate::spec::SandboxProcessSpec;

const CHILD_STATE_ROOT_ENV: &str = "NIMBUS_BUILD_RECOVERY_STATE_ROOT";
const CHILD_DOCKERFILE_ENV: &str = "NIMBUS_BUILD_RECOVERY_DOCKERFILE";
const CHILD_CONTEXT_ENV: &str = "NIMBUS_BUILD_RECOVERY_CONTEXT";
const CHILD_SANDBOX_ID_ENV: &str = "NIMBUS_BUILD_RECOVERY_SANDBOX_ID";
const CHILD_IMAGE_NAME_ENV: &str = "NIMBUS_BUILD_RECOVERY_IMAGE_NAME";

#[test]
fn builder_builds_from_scratch_with_copy_and_runtime_metadata() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(context_dir.join("bin")).expect("context dir should build");
    fs::write(context_dir.join("bin/server"), b"#!/bin/sh\nexit 0\n")
        .expect("server fixture should write");
    let dockerfile_path = context_dir.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        r#"
FROM scratch
WORKDIR /app
ENV APP_ENV=dev LOG_LEVEL=info
COPY ./bin/server ./server
ENTRYPOINT ["/app/server"]
EXPOSE 8080
USER 1000:1000
STOPSIGNAL SIGQUIT
LABEL com.example.role=edge
HEALTHCHECK CMD ["/app/server", "--healthcheck"]
"#,
    )
    .expect("dockerfile should write");

    let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
    let prepared = builder
        .prepare_built_image_launch(
            &SandboxId::new("build-01"),
            "demo-build",
            &dockerfile_path,
            &context_dir,
            &SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .expect("scratch build should succeed");

    assert!(prepared.artifact.rootfs_path.join("app/server").is_file());
    assert_eq!(
        prepared.launch_defaults.process.args,
        vec!["/app/server".to_owned()]
    );
    assert_eq!(prepared.launch_defaults.process.cwd, PathBuf::from("/app"));
    assert_eq!(
        prepared.launch_defaults.process.env,
        vec!["APP_ENV=dev".to_owned(), "LOG_LEVEL=info".to_owned()]
    );
    assert_eq!(prepared.launch_defaults.user.as_deref(), Some("1000:1000"));
    assert_eq!(
        prepared.launch_defaults.stop_signal.as_deref(),
        Some("SIGQUIT")
    );
    assert_eq!(
        prepared
            .launch_defaults
            .labels
            .get("com.example.role")
            .map(String::as_str),
        Some("edge")
    );
    assert_eq!(prepared.launch_defaults.exposed_ports.len(), 1);
    assert_eq!(prepared.launch_defaults.exposed_ports[0].raw, "8080/tcp");
    assert_eq!(
        prepared.launch_defaults.exposed_ports[0].protocol,
        OciExposedPortProtocol::Tcp
    );
    assert_eq!(
        prepared
            .launch_defaults
            .healthcheck
            .as_ref()
            .expect("healthcheck should exist")
            .test,
        vec![
            "CMD".to_owned(),
            "/app/server".to_owned(),
            "--healthcheck".to_owned(),
        ]
    );
}

#[test]
fn builder_binds_digest_and_rootfs_to_one_private_context_snapshot() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(&context_dir).expect("context dir should build");
    let payload_path = context_dir.join("payload");
    fs::write(&payload_path, b"snapshot-payload").expect("payload fixture should write");
    let dockerfile_path = context_dir.join("Dockerfile");
    let dockerfile_source = b"FROM scratch\nCOPY payload /payload\nCMD [\"/payload\"]\n";
    fs::write(&dockerfile_path, dockerfile_source).expect("dockerfile should write");
    let snapshot_digest = std::cell::RefCell::new(None);

    let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
    let prepared = builder
        .prepare_built_image_launch_with_snapshot_observer(
            &SandboxId::new("snapshot-bound-build"),
            "snapshot-bound-image",
            &dockerfile_path,
            &context_dir,
            &SandboxProcessSpec::new(Vec::<String>::new()),
            |snapshot| {
                let recipe = super::DockerfileRecipe::parse(dockerfile_source, &dockerfile_path)
                    .expect("snapshot fixture Dockerfile should parse");
                snapshot_digest.replace(Some(
                    super::artifact::context_sha256(&recipe, snapshot)
                        .expect("private context snapshot should hash"),
                ));
                fs::write(&payload_path, b"mutated-live-context")
                    .expect("live context should mutate after snapshot capture");
                Ok(())
            },
        )
        .expect("snapshot-bound build should succeed");

    assert_eq!(
        fs::read(prepared.artifact.rootfs_path.join("payload"))
            .expect("published payload should read"),
        b"snapshot-payload",
        "rootfs materialization must consume the same immutable snapshot that was hashed"
    );
    assert_eq!(
        fs::read(&payload_path).expect("mutated live payload should read"),
        b"mutated-live-context",
        "the test must cross the old hash-then-live-copy race window"
    );
    let artifact_dir = prepared
        .artifact
        .rootfs_path
        .parent()
        .expect("published rootfs should have an artifact directory");
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(artifact_dir.join("build.json")).expect("build receipt should read"),
    )
    .expect("build receipt should decode");
    assert_eq!(
        receipt["provenance"]["context_sha256"].as_str(),
        snapshot_digest.borrow().as_deref(),
        "published provenance must name the digest of the materialized snapshot"
    );
}

#[test]
fn builder_layers_runtime_metadata_over_a_registry_base_image() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(&context_dir).expect("context dir should build");
    let registry = serve_fake_oci_registry(build_layer_archive());
    let dockerfile_path = context_dir.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        format!("FROM {registry}\nENV PORT=9090 APP_MODE=dev\nCMD [\"--custom\"]\nEXPOSE 9090\n"),
    )
    .expect("dockerfile should write");

    let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
    let prepared = builder
        .prepare_built_image_launch(
            &SandboxId::new("build-02"),
            "demo-base",
            &dockerfile_path,
            &context_dir,
            &SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .expect("registry-backed build should succeed");

    assert!(prepared.artifact.rootfs_path.join("usr/bin/demo").is_file());
    assert_eq!(
        prepared.launch_defaults.process.args,
        vec!["/usr/bin/demo".to_owned(), "--custom".to_owned()]
    );
    assert_eq!(
        prepared.launch_defaults.process.env,
        vec![
            "PATH=/usr/bin".to_owned(),
            "PORT=9090".to_owned(),
            "APP_MODE=dev".to_owned(),
        ]
    );
    assert_eq!(
        prepared.launch_defaults.process.cwd,
        PathBuf::from("/workspace")
    );
    assert_eq!(prepared.launch_defaults.user.as_deref(), Some("1000:1000"));
    assert_eq!(
        prepared
            .launch_defaults
            .labels
            .get("app")
            .map(String::as_str),
        Some("demo")
    );
    assert_eq!(
        prepared
            .launch_defaults
            .exposed_ports
            .iter()
            .map(|port| port.raw.as_str())
            .collect::<Vec<_>>(),
        vec!["8080/tcp", "9090/tcp"]
    );
}

#[test]
fn builder_rejects_run_instructions_cleanly() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(&context_dir).expect("context dir should build");
    let dockerfile_path = context_dir.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        "FROM scratch\nRUN echo nope\nCMD [\"/bin/true\"]\n",
    )
    .expect("dockerfile should write");

    let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
    let error = builder
        .prepare_built_image_launch(
            &SandboxId::new("build-03"),
            "demo-run",
            &dockerfile_path,
            &context_dir,
            &SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .expect_err("RUN should be rejected");
    assert!(
        error
            .to_string()
            .contains("Dockerfile instruction \"RUN\" is not supported"),
        "{error}"
    );
}

#[test]
fn fresh_process_lost_manifest_adopts_exact_scratch_build() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(&context_dir).expect("context dir should build");
    fs::write(context_dir.join("payload"), b"scratch-payload")
        .expect("context payload should write");
    let dockerfile_path = context_dir.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        "FROM scratch\nCOPY payload /payload\nCMD [\"/payload\"]\n",
    )
    .expect("dockerfile should write");

    assert_fresh_process_adoption(
        temp_dir.path(),
        &dockerfile_path,
        &context_dir,
        "fresh-scratch",
        "fresh-scratch-image",
    );
}

#[test]
fn fresh_process_lost_manifest_adopts_exact_registry_base_build() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(&context_dir).expect("context dir should build");
    fs::write(context_dir.join("overlay"), b"registry-overlay")
        .expect("context payload should write");
    let registry = serve_fake_oci_registry(build_layer_archive());
    let dockerfile_path = context_dir.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        format!("FROM {registry}\nCOPY overlay /overlay\nCMD [\"--recovered\"]\n"),
    )
    .expect("dockerfile should write");

    assert_fresh_process_adoption(
        temp_dir.path(),
        &dockerfile_path,
        &context_dir,
        "fresh-registry",
        "fresh-registry-image",
    );
}

#[test]
fn crossed_build_context_fails_before_final_artifact_mutation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let context_dir = temp_dir.path().join("context");
    fs::create_dir_all(&context_dir).expect("context dir should build");
    let payload_path = context_dir.join("payload");
    fs::write(&payload_path, b"first-payload").expect("context payload should write");
    let dockerfile_path = context_dir.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        "FROM scratch\nCOPY payload /payload\nCMD [\"/payload\"]\n",
    )
    .expect("dockerfile should write");
    let sandbox_id = SandboxId::new("crossed-build");
    let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
    builder
        .prepare_built_image_launch(
            &sandbox_id,
            "crossed-image",
            &dockerfile_path,
            &context_dir,
            &SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .expect("initial build should publish");
    let final_artifact = temp_dir
        .path()
        .join("materialized-rootfs")
        .join(sandbox_id.as_str());
    let before = snapshot_tree(&final_artifact);

    fs::write(&payload_path, b"crossed-payload").expect("crossed payload should write");
    let error = builder
        .prepare_built_image_launch(
            &sandbox_id,
            "crossed-image",
            &dockerfile_path,
            &context_dir,
            &SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .expect_err("crossed build provenance should fail closed");
    assert!(
        error
            .to_string()
            .contains("without the exact requested build provenance"),
        "{error}"
    );
    assert_eq!(snapshot_tree(&final_artifact), before);
}

#[test]
#[ignore = "subprocess entry point; parent supplies exact durable build inputs"]
fn builder_fresh_process_entry() {
    let state_root = PathBuf::from(
        std::env::var_os(CHILD_STATE_ROOT_ENV)
            .expect("child state root should be supplied by parent"),
    );
    let dockerfile = PathBuf::from(
        std::env::var_os(CHILD_DOCKERFILE_ENV)
            .expect("child Dockerfile should be supplied by parent"),
    );
    let context = PathBuf::from(
        std::env::var_os(CHILD_CONTEXT_ENV).expect("child context should be supplied by parent"),
    );
    let sandbox_id = SandboxId::new(
        std::env::var(CHILD_SANDBOX_ID_ENV).expect("child sandbox id should be supplied by parent"),
    );
    let image_name =
        std::env::var(CHILD_IMAGE_NAME_ENV).expect("child image name should be supplied by parent");

    OciDockerfileBuilder::under_state_root(state_root)
        .prepare_built_image_launch(
            &sandbox_id,
            &image_name,
            &dockerfile,
            &context,
            &SandboxProcessSpec::new(Vec::<String>::new()),
        )
        .expect("fresh child process should publish or adopt the exact build");
}

fn assert_fresh_process_adoption(
    state_root: &Path,
    dockerfile: &Path,
    context: &Path,
    sandbox_id: &str,
    image_name: &str,
) {
    run_build_child(state_root, dockerfile, context, sandbox_id, image_name);
    let final_artifact = state_root.join("materialized-rootfs").join(sandbox_id);
    let before = snapshot_tree(&final_artifact);
    assert!(final_artifact.join("build.json").is_file());
    assert!(!final_artifact.join("materialization.json").exists());

    run_build_child(state_root, dockerfile, context, sandbox_id, image_name);
    assert_eq!(snapshot_tree(&final_artifact), before);
}

fn run_build_child(
    state_root: &Path,
    dockerfile: &Path,
    context: &Path,
    sandbox_id: &str,
    image_name: &str,
) {
    let output = Command::new(
        std::env::current_exe().expect("current sandbox test executable should resolve"),
    )
    .arg("builder_fresh_process_entry")
    .arg("--ignored")
    .arg("--nocapture")
    .env(CHILD_STATE_ROOT_ENV, state_root)
    .env(CHILD_DOCKERFILE_ENV, dockerfile)
    .env(CHILD_CONTEXT_ENV, context)
    .env(CHILD_SANDBOX_ID_ENV, sandbox_id)
    .env(CHILD_IMAGE_NAME_ENV, image_name)
    .output()
    .expect("fresh build subprocess should start");
    assert!(
        output.status.success(),
        "fresh build subprocess failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let relative = path
            .strip_prefix(root)
            .expect("snapshot path should remain below root")
            .to_owned();
        if path.is_dir() {
            snapshot.insert(relative, b"directory".to_vec());
            let mut entries = fs::read_dir(path)
                .expect("snapshot directory should read")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("snapshot entries should read");
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                visit(root, &entry.path(), snapshot);
            }
        } else {
            snapshot.insert(relative, fs::read(path).expect("snapshot file should read"));
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn build_layer_archive() -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);

    write_tar_file(
        &mut builder,
        "etc/passwd",
        b"demo:x:1000:1000:demo:/home/demo:/bin/sh\n",
        0o644,
    );
    write_tar_file(&mut builder, "etc/group", b"demo:x:1000:\n", 0o644);
    write_tar_file(
        &mut builder,
        "usr/bin/demo",
        b"#!/bin/sh\nexec sleep 60\n",
        0o755,
    );

    let encoder = builder.into_inner().expect("tar encoder should finish");
    encoder.finish().expect("gzip layer should finish")
}

fn write_tar_file(
    builder: &mut tar::Builder<GzEncoder<Vec<u8>>>,
    path: &str,
    body: &[u8],
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    header.set_mode(mode);
    header.set_size(body.len() as u64);
    header.set_cksum();
    builder
        .append_data(&mut header, path, Cursor::new(body))
        .expect("layer entry should append");
}

fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("registry listener should bind");
    let address = listener
        .local_addr()
        .expect("registry listener should report local addr");

    let config = serde_json::json!({
        "config": {
            "Entrypoint": ["/usr/bin/demo"],
            "Cmd": ["--serve"],
            "Env": ["PATH=/usr/bin", "PORT=8080"],
            "User": "demo",
            "WorkingDir": "/workspace",
            "ExposedPorts": {
                "8080/tcp": {}
            },
            "Labels": {
                "app": "demo"
            }
        }
    });
    let config_bytes = serde_json::to_vec(&config).expect("config should serialize");
    let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));
    let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
    let child_manifest = serde_json::json!({
        "schemaVersion": 2,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "size": config_bytes.len(),
            "digest": config_digest
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "size": layer_body.len(),
            "digest": layer_digest
        }]
    });
    let child_manifest_bytes =
        serde_json::to_vec(&child_manifest).expect("child manifest should serialize");
    let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
    let index = serde_json::json!({
        "schemaVersion": 2,
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": child_manifest_digest,
            "platform": {
                "os": "linux",
                "architecture": std::env::consts::ARCH
            }
        }]
    });
    let index_bytes = serde_json::to_vec(&index).expect("index should serialize");

    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.expect("registry connection should succeed");
            let mut request = [0_u8; 2048];
            let bytes_read = stream
                .read(&mut request)
                .expect("registry request should read");
            let request = String::from_utf8_lossy(&request[..bytes_read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let (status, body) = match path {
                "/v2/" => (200, Vec::new()),
                "/v2/library/demo/manifests/latest" => (200, index_bytes.clone()),
                path if path == format!("/v2/library/demo/manifests/{child_manifest_digest}") => {
                    (200, child_manifest_bytes.clone())
                }
                path if path == format!("/v2/library/demo/blobs/{config_digest}") => {
                    (200, config_bytes.clone())
                }
                path if path == format!("/v2/library/demo/blobs/{layer_digest}") => {
                    (200, layer_body.clone())
                }
                _ => (404, Vec::new()),
            };

            let response = format!(
                "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                if status == 200 { "OK" } else { "Not Found" },
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("fake OCI registry response head should write");
            stream
                .write_all(&body)
                .expect("fake OCI registry response body should write");
        }
    });

    format!("docker://localhost:{}/library/demo:latest", address.port())
}
