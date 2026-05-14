use super::*;

pub(super) fn sample_config(image: &Path) -> MachineConfigRecord {
    let base_root = image
        .parent()
        .expect("test image path should have a parent directory");
    MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: "default".to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::LocalDisk {
                path: image.to_path_buf(),
            },
            provisioning: MachineGuestProvisioning::Ignition,
            ssh_user: "core".to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: 2,
            memory_mib: 2048,
            disk_gib: 20,
        },
        volumes: vec![MachineVolume {
            source: PathBuf::from("/Users"),
            target: PathBuf::from("/Users"),
        }],
        roots: MachineRootLayout::new(
            base_root.join("config-root"),
            base_root.join("state-root"),
            base_root.join("runtime-root"),
        ),
    }
}

pub(super) fn machine_lifecycle_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

pub(super) fn serve_single_http_response(body: Vec<u8>, path: Option<&str>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should resolve");
    let request_path = path.unwrap_or("/disk.raw").to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept one request");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("response header should write");
        stream.write_all(&body).expect("response body should write");
    });
    format!("http://{}:{}{}", address.ip(), address.port(), request_path)
}

pub(super) fn spawn_reaped_process(command: &str) -> (i32, thread::JoinHandle<()>) {
    let mut child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), command.to_owned()],
    }
    .spawn()
    .expect("managed process should spawn");
    let pid = child.id() as i32;
    let reaper = thread::spawn(move || {
        let _ = child.wait();
    });
    (pid, reaper)
}

pub(super) fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should resolve");
    let repository = "example/nimbus-machine-os";
    let tag = "test";
    let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
    let ignored_layer_body = b"ignored-raw-disk-artifact-bytes".to_vec();
    let ignored_layer_digest = format!("sha256:{:x}", Sha256::digest(&ignored_layer_body));
    let current_arch = current_machine_oci_architectures()[0];
    let child_manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_IMAGE_MEDIA_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "size": 2,
            "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        },
        "layers": [{
            "mediaType": "application/vnd.nimbus.machine.disk.layer.v1.tar+gzip",
            "size": layer_body.len(),
            "digest": layer_digest,
            "annotations": {
                "org.opencontainers.image.title": "nimbus-machine-os.raw.gz"
            }
        }]
    });
    let child_manifest_bytes =
        serde_json::to_vec(&child_manifest).expect("child manifest should serialize");
    let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
    let ignored_manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_IMAGE_MEDIA_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "size": 2,
            "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        },
        "layers": [{
            "mediaType": "application/vnd.nimbus.machine.disk.layer.v1.tar+gzip",
            "size": ignored_layer_body.len(),
            "digest": ignored_layer_digest,
            "annotations": {
                "org.opencontainers.image.title": "ignored.raw.gz"
            }
        }]
    });
    let ignored_manifest_bytes =
        serde_json::to_vec(&ignored_manifest).expect("ignored manifest should serialize");
    let ignored_manifest_digest = format!("sha256:{:x}", Sha256::digest(&ignored_manifest_bytes));
    let index_manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_IMAGE_INDEX_MEDIA_TYPE,
        "manifests": [
            {
                "mediaType": OCI_IMAGE_MEDIA_TYPE,
                "size": ignored_manifest_bytes.len(),
                "digest": ignored_manifest_digest,
                "platform": {
                    "architecture": current_arch,
                    "os": OCI_MACHINE_OS
                },
                "annotations": {
                    "disktype": "raw"
                }
            },
            {
                "mediaType": OCI_IMAGE_MEDIA_TYPE,
                "size": child_manifest_bytes.len(),
                "digest": child_manifest_digest,
                "platform": {
                    "architecture": current_arch,
                    "os": OCI_MACHINE_OS
                },
                "annotations": {
                    "disktype": MachineProvider::Krunkit.oci_artifact_disk_type(),
                    "org.opencontainers.image.source": "https://github.com/nimbus/nimbus-machine-os",
                    "io.nimbus.machine.attestation.repository": "nimbus/nimbus-machine-os",
                    "io.nimbus.machine.nimbus.version": "v1.2.3"
                }
            }
        ]
    });
    let index_manifest_bytes =
        serde_json::to_vec(&index_manifest).expect("index manifest should serialize");

    thread::spawn(move || {
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).expect("request should read");
            let request = String::from_utf8_lossy(&buffer[..read]);
            let mut parts = request
                .lines()
                .next()
                .unwrap_or_default()
                .split_whitespace();
            let method = parts.next().unwrap_or("GET");
            let path = parts.next().unwrap_or("/");
            let (status, content_type, body) = match path {
                "/v2/" | "/v2" => (200, "text/plain", Vec::new()),
                _ if path == format!("/v2/{repository}/manifests/{tag}") => (
                    200,
                    OCI_IMAGE_INDEX_MEDIA_TYPE,
                    index_manifest_bytes.clone(),
                ),
                _ if path == format!("/v2/{repository}/manifests/{ignored_manifest_digest}") => {
                    (200, OCI_IMAGE_MEDIA_TYPE, ignored_manifest_bytes.clone())
                }
                _ if path == format!("/v2/{repository}/manifests/{child_manifest_digest}") => {
                    (200, OCI_IMAGE_MEDIA_TYPE, child_manifest_bytes.clone())
                }
                _ if path == format!("/v2/{repository}/blobs/{ignored_layer_digest}") => {
                    (200, "application/octet-stream", ignored_layer_body.clone())
                }
                _ if path == format!("/v2/{repository}/blobs/{layer_digest}") => {
                    (200, "application/octet-stream", layer_body.clone())
                }
                _ => (404, "text/plain", b"not found".to_vec()),
            };
            let status_line = if status == 200 {
                "HTTP/1.1 200 OK"
            } else {
                "HTTP/1.1 404 Not Found"
            };
            let response = format!(
                "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("response header should write");
            if method != "HEAD" {
                stream.write_all(&body).expect("response body should write");
            }
        }
    });

    format!("docker://127.0.0.1:{}/{repository}:{tag}", address.port())
}
