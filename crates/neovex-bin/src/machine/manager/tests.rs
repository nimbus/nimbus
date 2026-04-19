use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::GzEncoder;
use libc::SIGKILL;
use oci_client::manifest::{OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE};
use tempfile::TempDir;

use super::guest::{ensure_guest_neovex_socket_shell_script, guest_neovex_archive_name};
use super::helpers::{
    bundled_helper_candidates_for_executable, known_helper_candidates, resolve_helper_binary,
    write_helper_stub,
};
use super::image::{
    attestation_repositories_for_reference, current_machine_oci_architectures,
    machine_artifact_metadata_from_annotations, materialize_cached_disk,
    resolve_bootable_image_path,
};
use super::launch::{MachineCommandLine, MachineLaunchPlan, build_virtio_vsock_listen_arg};
use super::ports::{
    load_machine_port_allocation_state, managed_machine_port_range_contains,
    with_port_allocation_lock, write_machine_port_allocation_state,
};
use super::readiness::{ssh_port_is_listening, wait_for_path, wait_for_ssh_ready};
use super::ssh::remote_shell_command;
use super::stop::{
    annotate_machine_start_error, cleanup_process, force_stop_pid, handle_start_machine_error,
    request_krunkit_state_change, send_signal, wait_for_pid_exit,
};
use super::*;
use crate::machine::bootstrap::GUEST_NEOVEX_SOCKET;
use crate::machine::{
    CURRENT_MACHINE_CONFIG_VERSION, DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY, MachineBootstrapMode,
    MachineGuestConfig, MachineImageFormat, MachineImageSource, MachineProvider, MachineResources,
    MachineRootLayout, MachineVolume, describe_machine_image_source,
    machine_image_reference_repository,
};

fn sample_config(image: &Path) -> MachineConfigRecord {
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

fn machine_lifecycle_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[test]
fn krunkit_provider_capabilities_match_podman_aligned_contract() {
    assert!(!MachineProvider::Krunkit.uses_provider_networking());
    assert!(MachineProvider::Krunkit.requires_exclusive_active());
    assert_eq!(
        MachineProvider::Krunkit.image_format(),
        MachineImageFormat::Raw
    );
    assert_eq!(
        MachineProvider::Krunkit.bootstrap_mode(),
        MachineBootstrapMode::Ignition
    );
    assert_eq!(MachineProvider::Krunkit.oci_artifact_disk_type(), "applehv");
    assert!(MachineProvider::Wsl2.uses_provider_networking());
    assert!(!MachineProvider::Wsl2.requires_exclusive_active());
    assert_eq!(
        MachineProvider::Wsl2.image_format(),
        MachineImageFormat::Tar
    );
    assert_eq!(
        MachineProvider::Wsl2.bootstrap_mode(),
        MachineBootstrapMode::ShellScript
    );
    assert_eq!(MachineProvider::Wsl2.oci_artifact_disk_type(), "wsl");
}

#[test]
fn machine_image_reference_repository_strips_tag_and_digest() {
    assert_eq!(
        machine_image_reference_repository("docker://quay.io/podman/machine-os:6.0"),
        "quay.io/podman/machine-os"
    );
    assert_eq!(
        machine_image_reference_repository("docker://quay.io/podman/machine-os@sha256:abc123"),
        "quay.io/podman/machine-os"
    );
}

#[test]
fn podman_machine_os_requires_host_guest_neovex_sync() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: "docker://quay.io/podman/machine-os:6.0".to_owned(),
    };

    assert_eq!(
        requires_host_guest_neovex_sync(&config),
        cfg!(target_os = "macos")
    );
}

#[test]
fn podman_machine_os_bootstrap_contract_requires_ssh_identity() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: "docker://quay.io/podman/machine-os:6.0".to_owned(),
    };

    if cfg!(target_os = "macos") {
        let error = validate_machine_bootstrap_contract(&config)
            .expect_err("podman machine-os should require ssh identity");
        assert!(error.to_string().contains("--identity"));
    } else {
        validate_machine_bootstrap_contract(&config)
            .expect("non-macOS hosts should not require macOS SSH bootstrapping");
    }
}

#[test]
fn ensure_machine_bootstrap_identity_generates_machine_owned_key_for_host_managed_contract() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: format!("docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@sha256:abc123"),
    };

    let paths = config.roots.paths("default");
    paths.ensure_directories().expect("paths should initialize");
    write_json_file(&paths.config_path, &config).expect("config should write");

    ensure_machine_bootstrap_identity(&paths, &mut config)
        .expect("bootstrap identity generation should succeed");

    if cfg!(target_os = "macos") {
        let identity_path = config
            .guest
            .ssh_identity_path
            .clone()
            .expect("macOS host-managed contract should record an identity path");
        let public_key_path = PathBuf::from(format!("{}.pub", identity_path.display()));
        assert_eq!(identity_path, paths.data_dir.join("machine"));
        assert!(identity_path.is_file());
        assert!(public_key_path.is_file());

        let stored: MachineConfigRecord = serde_json::from_slice(
            &fs::read(&paths.config_path).expect("config should still read"),
        )
        .expect("stored config should deserialize");
        assert_eq!(stored.guest.ssh_identity_path, Some(identity_path));
    } else {
        assert_eq!(config.guest.ssh_identity_path, None);
    }
}

#[test]
fn resolve_guest_neovex_binary_reuses_cached_release_asset() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");

    let archive_name = guest_neovex_archive_name().expect("archive name should resolve");
    let cached_binary = paths.guest_binary_cache_dir.join(format!(
        "{}-{}-neovex",
        super::super::current_machine_release_tag(),
        archive_name.trim_end_matches(".tar.gz")
    ));
    fs::write(&cached_binary, b"cached guest binary").expect("cached binary should write");

    assert_eq!(
        resolve_guest_neovex_binary(&paths).expect("cached guest binary should resolve"),
        cached_binary
    );
}

#[test]
fn converge_machine_image_contract_rebuilds_boot_artifacts_when_recorded_image_drifted() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.image_source = MachineImageSource::OciReference {
        reference: super::super::default_machine_image_for_provider(MachineProvider::Krunkit),
    };
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.materialized_image_path, b"old-image").expect("image should write");
    fs::write(&paths.efi_variable_store_path, b"old-efi").expect("efi store should write");

    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: paths.materialized_image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: "docker://quay.io/podman/machine-os@sha256:old-digest".to_owned(),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    converge_machine_image_contract(&paths, &mut config, &mut state)
        .expect("contract convergence should succeed");

    assert_eq!(
        config.guest.image_source,
        MachineImageSource::OciReference {
            reference: super::super::default_machine_image_for_provider(MachineProvider::Krunkit,),
        }
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("boot artifacts were reset")
    );
    assert!(!paths.materialized_image_path.exists());
    assert!(!paths.efi_variable_store_path.exists());
}

#[test]
fn machine_image_rebuild_reason_requires_rebuild_when_boot_artifacts_lack_identity() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.materialized_image_path, b"old-image").expect("image should write");

    let reason = machine_image_rebuild_reason(
        &paths,
        &MachineStateRecord::initialized(),
        "docker://quay.io/podman/machine-os@sha256:test",
    )
    .expect("boot artifacts without recorded identity should rebuild");

    assert!(reason.contains("without a recorded base-image identity"));
}

#[test]
fn launch_plan_requires_bootable_local_disk_image() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let state = MachineStateRecord::initialized();
    let plan = MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");

    assert!(
        plan.krunkit_command
            .args
            .iter()
            .any(|arg| arg.contains("virtio-blk,path="))
    );
    assert!(
        plan.krunkit_command
            .args
            .iter()
            .any(|arg| arg.contains("virtio-net,type=unixgram"))
    );
    assert!(plan.krunkit_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-vsock,port=1025,socketURL={},listen",
            paths.ready_socket_path.display()
        )
    }));
    assert!(plan.krunkit_command.args.iter().any(|arg| {
        arg == &format!(
            "virtio-vsock,port=1024,socketURL={},listen",
            paths.ignition_socket_path.display()
        )
    }));
    assert!(
        !plan
            .gvproxy_command
            .args
            .iter()
            .any(|arg| arg == "-forward-sock")
    );
    assert_eq!(
        plan.ignition_file_path,
        Some(paths.generated_ignition_path.clone())
    );
}

#[test]
fn launch_plan_adds_gvproxy_machine_api_forwarding_when_ssh_identity_exists() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    let ssh_identity_path = temp_dir.path().join("machine-key");
    let ssh_public_key_path = temp_dir.path().join("machine-key.pub");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&ssh_identity_path, "fake key").expect("identity should write");
    fs::write(
        &ssh_public_key_path,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example",
    )
    .expect("public key should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(ssh_identity_path.clone());

    let paths = config.roots.paths("default");
    let state = MachineStateRecord::initialized();
    let plan = MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");

    assert!(plan.gvproxy_command.args.windows(2).any(|pair| {
        pair[0] == "-forward-sock" && pair[1] == paths.api_socket_path.display().to_string()
    }));
    assert!(
        plan.gvproxy_command
            .args
            .windows(2)
            .any(|pair| { pair[0] == "-forward-dest" && pair[1] == GUEST_NEOVEX_SOCKET })
    );
    assert!(
        plan.gvproxy_command
            .args
            .windows(2)
            .any(|pair| { pair[0] == "-forward-user" && pair[1] == MACHINE_API_FORWARD_USER })
    );
    assert!(plan.gvproxy_command.args.windows(2).any(|pair| {
        pair[0] == "-forward-identity" && pair[1] == ssh_identity_path.display().to_string()
    }));
}

#[test]
fn build_virtio_vsock_listen_arg_matches_podman_listen_mode() {
    let socket_path = Path::new("/tmp/neovex-test.sock");

    assert_eq!(
        build_virtio_vsock_listen_arg(1024, socket_path),
        "virtio-vsock,port=1024,socketURL=/tmp/neovex-test.sock,listen"
    );
}

#[test]
fn remote_shell_command_single_quotes_guest_scripts_for_ssh() {
    let script = "if [ -x '/usr/local/bin/neovex' ]; then printf '%s' ok; fi";

    assert_eq!(
        remote_shell_command(script),
        "sh -lc 'if [ -x '\"'\"'/usr/local/bin/neovex'\"'\"' ]; then printf '\"'\"'%s'\"'\"' ok; fi'"
    );
}

#[test]
fn ensure_guest_neovex_socket_shell_repairs_first_boot_failures() {
    let script = ensure_guest_neovex_socket_shell_script();

    assert!(script.contains("systemctl daemon-reload"), "{script}");
    assert!(
        script.contains("systemctl stop neovex.service neovex.socket"),
        "{script}"
    );
    assert!(
        script.contains("systemctl reset-failed neovex.service neovex.socket"),
        "{script}"
    );
    assert!(script.contains("systemctl start neovex.socket"), "{script}");
    assert!(script.contains(GUEST_NEOVEX_SOCKET), "{script}");
}

#[test]
fn annotate_machine_start_error_hints_when_guest_reaches_login_prompt() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.machine_log_path, "Fedora Linux 42\nfedora login:\n")
        .expect("machine log should write");

    let error = annotate_machine_start_error(
        &paths,
        &config,
        Error::Internal(
            "gvproxy exited before machine readiness with status exit status: 0".to_owned(),
        ),
    );

    let message = error.to_string();
    assert!(message.contains("gvproxy exited before machine readiness"));
    assert!(message.contains("guest reached a console login prompt"));
    assert!(message.contains("generic fedora-bootc raw images"));
}

#[test]
fn annotate_machine_start_error_leaves_unrelated_failures_unchanged() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.machine_log_path, "Fedora Linux 42\nfedora login:\n")
        .expect("machine log should write");

    let error = annotate_machine_start_error(
        &paths,
        &config,
        Error::Internal("failed to resolve machine guest OCI reference".to_owned()),
    );

    assert_eq!(
        error.to_string(),
        "internal error: failed to resolve machine guest OCI reference"
    );
}

#[test]
fn registry_image_reference_materializes_raw_disk_from_oci_registry() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let raw_payload = b"raw-disk-oci-bytes".to_vec();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&raw_payload)
        .expect("gzip payload should write");
    let gzip_payload = encoder.finish().expect("gzip payload should finish");
    let reference = serve_fake_oci_registry(gzip_payload);

    let materialized = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::OciReference { reference },
        MachineProvider::Krunkit,
    )
    .expect("registry image should materialize");

    assert_eq!(materialized, paths.materialized_image_path);
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should read"),
        raw_payload
    );
}

#[test]
fn registry_image_reference_reuses_materialized_disk_when_present() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    fs::create_dir_all(&paths.image_cache_dir).expect("image cache dir should exist");
    fs::create_dir_all(
        paths
            .materialized_image_path
            .parent()
            .expect("materialized image parent should exist"),
    )
    .expect("materialized image parent should exist");
    fs::write(&paths.materialized_image_path, []).expect("materialized image should write");

    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: "default".to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::OciReference {
                reference: format!(
                    "docker://ghcr.io/agentstation/neovex-machine-os:v{}",
                    env!("CARGO_PKG_VERSION")
                ),
            },
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
        volumes: Vec::new(),
        roots: layout.clone(),
    };

    let plan = MachineLaunchPlan::build(&paths, &config, &MachineStateRecord::initialized())
        .expect("materialized disk should satisfy launch plan");

    assert_eq!(plan.runtime.image_path, paths.materialized_image_path);
    assert!(
        plan.krunkit_command
            .args
            .iter()
            .any(|arg| arg.contains(&format!(
                "virtio-blk,path={}",
                paths.materialized_image_path.display()
            )))
    );
}

#[test]
fn http_image_source_materializes_raw_disk_into_reserved_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let payload = b"raw-disk-bytes".to_vec();
    let url = serve_single_http_response(payload.clone(), None);

    let materialized = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::HttpUrl { url: url.clone() },
        MachineProvider::Krunkit,
    )
    .expect("http source should materialize");

    assert_eq!(materialized, paths.materialized_image_path);
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should read"),
        payload
    );
}

#[test]
fn cached_zstd_machine_image_materializes_into_reserved_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let source_path = temp_dir.path().join("disk.raw.zst");
    let output_path = temp_dir.path().join("disk.raw");
    let payload = b"raw-disk-zstd-bytes".to_vec();
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload), 1)
        .expect("zstd payload should encode");
    fs::write(&source_path, compressed).expect("compressed source should write");

    materialize_cached_disk(&source_path, &output_path, "test zstd image")
        .expect("zstd image should materialize");

    assert_eq!(
        fs::read(&output_path).expect("materialized image should read"),
        payload
    );
}

#[test]
fn http_gzip_image_source_materializes_decompressed_disk_into_reserved_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    let payload = b"raw-disk-gzip-bytes".to_vec();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&payload)
        .expect("gzip payload should write");
    let gzip_payload = encoder.finish().expect("gzip payload should finish");
    let url = serve_single_http_response(gzip_payload, Some("/disk.raw.gz"));

    let materialized = resolve_bootable_image_path(
        &paths,
        &MachineImageSource::HttpUrl { url: url.clone() },
        MachineProvider::Krunkit,
    )
    .expect("gzip http source should materialize");

    assert_eq!(materialized, paths.materialized_image_path);
    assert_eq!(
        fs::read(&paths.materialized_image_path).expect("materialized image should read"),
        payload
    );
}

#[test]
fn helper_resolution_honors_environment_overrides() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let krunkit_path = temp_dir.path().join("krunkit");
    let gvproxy_path = temp_dir.path().join("gvproxy");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let resolved =
        MachineHelperBinaryPaths::resolve().expect("helper binaries should resolve via env");

    assert_eq!(resolved.krunkit, krunkit_path);
    assert_eq!(resolved.gvproxy, gvproxy_path);
}

#[test]
fn bundled_helper_candidates_cover_root_and_bin_layouts() {
    let root_layout = bundled_helper_candidates_for_executable(
        Path::new("/opt/homebrew/Caskroom/neovex/0.1.10/neovex"),
        "gvproxy",
    );
    assert_eq!(
        root_layout,
        vec![PathBuf::from(
            "/opt/homebrew/Caskroom/neovex/0.1.10/libexec/gvproxy"
        )]
    );

    let bin_layout =
        bundled_helper_candidates_for_executable(Path::new("/opt/homebrew/bin/neovex"), "gvproxy");
    assert_eq!(
        bin_layout,
        vec![
            PathBuf::from("/opt/homebrew/bin/libexec/gvproxy"),
            PathBuf::from("/opt/homebrew/libexec/gvproxy"),
        ]
    );
}

#[test]
fn helper_resolution_prefers_packaged_candidates_before_fallbacks() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let packaged_dir = temp_dir.path().join("libexec");
    let fallback_dir = temp_dir.path().join("fallback");
    fs::create_dir_all(&packaged_dir).expect("packaged helper dir should exist");
    fs::create_dir_all(&fallback_dir).expect("fallback helper dir should exist");
    let packaged_gvproxy = packaged_dir.join("gvproxy");
    let fallback_gvproxy = fallback_dir.join("gvproxy");
    write_helper_stub(&packaged_gvproxy, "gvproxy");
    write_helper_stub(&fallback_gvproxy, "gvproxy");

    let resolved = resolve_helper_binary(
        "NEOVEX_TEST_GVPROXY",
        "gvproxy-does-not-exist",
        std::slice::from_ref(&packaged_gvproxy),
        &[fallback_gvproxy],
    )
    .expect("packaged helper should resolve");

    assert_eq!(resolved, packaged_gvproxy);
}

#[test]
fn helper_resolution_honors_helper_binary_directory_override() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let helper_dir = temp_dir.path().join("helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should exist");
    let helper_gvproxy = helper_dir.join("gvproxy");
    write_helper_stub(&helper_gvproxy, "gvproxy");
    let _guard = MachineHelperEnvGuard::with_helper_binary_dir(&helper_dir);

    let resolved = resolve_helper_binary("NEOVEX_TEST_GVPROXY", "gvproxy", &[], &[])
        .expect("helper dir override should resolve");

    assert_eq!(resolved, helper_gvproxy);
}

#[test]
fn known_helper_candidates_mirror_podman_darwin_defaults() {
    assert_eq!(
        known_helper_candidates("gvproxy"),
        vec![
            PathBuf::from("/usr/local/opt/podman/libexec/podman/gvproxy"),
            PathBuf::from("/opt/homebrew/opt/podman/libexec/podman/gvproxy"),
            PathBuf::from("/opt/homebrew/bin/gvproxy"),
            PathBuf::from("/usr/local/bin/gvproxy"),
            PathBuf::from("/opt/homebrew/libexec/podman/gvproxy"),
            PathBuf::from("/usr/local/libexec/podman/gvproxy"),
            PathBuf::from("/usr/local/lib/podman/gvproxy"),
            PathBuf::from("/usr/libexec/podman/gvproxy"),
            PathBuf::from("/usr/lib/podman/gvproxy"),
        ]
    );
}

#[test]
fn helper_resolution_does_not_fall_back_to_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let helper_dir = temp_dir.path().join("path-only");
    fs::create_dir_all(&helper_dir).expect("path-only helper dir should exist");
    let helper_gvproxy = helper_dir.join("gvproxy");
    write_helper_stub(&helper_gvproxy, "gvproxy");
    let _guard = MachineHelperEnvGuard::with_path_only(&helper_dir);

    let error = resolve_helper_binary("NEOVEX_TEST_GVPROXY", "gvproxy", &[], &[])
        .expect_err("PATH-only helpers should be ignored");

    assert!(
        error
            .to_string()
            .contains("supported packaged or Homebrew helper directory"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn machine_command_spawn_detaches_helpers_into_new_session() {
    let command = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    };
    let mut child = command.spawn().expect("helper process should spawn");
    let child_pid = child.id() as i32;
    let parent_sid = unsafe { libc::getsid(0) };
    let child_sid = unsafe { libc::getsid(child_pid) };

    assert!(parent_sid > 0, "parent sid should resolve");
    assert_eq!(child_sid, child_pid, "child should lead its own session");
    assert_ne!(
        child_sid, parent_sid,
        "child session should differ from parent"
    );

    cleanup_process(&mut child).expect("child should clean up");
}

#[test]
fn ssh_port_is_listening_detects_local_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();

    assert!(ssh_port_is_listening(port));
}

#[test]
fn wait_for_ssh_ready_accepts_listening_port_without_identity_probe() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let mut gvproxy_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("gvproxy probe child should spawn");
    let mut krunkit_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("krunkit probe child should spawn");

    let result = wait_for_ssh_ready(
        &config,
        port,
        Duration::from_secs(1),
        &mut krunkit_child,
        &mut gvproxy_child,
        &StartupSignalMonitor::inactive_for_test(),
    );

    cleanup_process(&mut krunkit_child).expect("krunkit probe child should clean up");
    cleanup_process(&mut gvproxy_child).expect("gvproxy probe child should clean up");
    drop(listener);

    assert!(result.is_ok(), "listener-backed SSH readiness should pass");
}

#[test]
fn wait_for_path_returns_cancelled_when_startup_signal_is_set() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let path = temp_dir.path().join("gvproxy.sock");
    let mut child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("probe child should spawn");

    let result = wait_for_path(
        &path,
        Duration::from_secs(1),
        &mut child,
        &StartupSignalMonitor::interrupted_for_test(),
    );

    cleanup_process(&mut child).expect("probe child should clean up");

    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn interrupted_start_transitions_to_stopped_and_cleans_runtime_artifacts() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");

    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
    ] {
        fs::write(path, b"artifact").expect("runtime artifact should write");
    }
    for path in [
        &paths.machine_log_path,
        &paths.krunkit_log_path,
        &paths.gvproxy_log_path,
    ] {
        fs::write(path, b"non-empty").expect("log artifact should write");
    }

    let mut krunkit_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("krunkit child should spawn");
    let mut gvproxy_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("gvproxy child should spawn");

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Starting;
    state.manager = MachineManagerState::Launching;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let result = handle_start_machine_error(
        &paths,
        &config,
        &mut state,
        Error::Cancelled,
        Some(&mut krunkit_child),
        Some(&mut gvproxy_child),
    );

    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::HelpersResolved);
    assert_eq!(state.last_error, None);
    assert!(
        krunkit_child
            .try_wait()
            .expect("krunkit child status should resolve")
            .is_some(),
        "krunkit child should be reaped on interrupted startup"
    );
    assert!(
        gvproxy_child
            .try_wait()
            .expect("gvproxy child status should resolve")
            .is_some(),
        "gvproxy child should be reaped on interrupted startup"
    );
    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
    ] {
        assert!(
            !path.exists(),
            "runtime artifact {} should be removed",
            path.display()
        );
    }
    for path in [
        &paths.machine_log_path,
        &paths.krunkit_log_path,
        &paths.gvproxy_log_path,
    ] {
        assert_eq!(
            fs::read(path).expect("log artifact should remain readable"),
            Vec::<u8>::new(),
            "log artifact {} should be truncated",
            path.display()
        );
    }
}

#[test]
fn stop_machine_uses_graceful_krunkit_stop_before_cleaning_up_helpers() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");

    let (krunkit_pid, krunkit_reaper) = spawn_reaped_process("exec sleep 30");
    let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.krunkit_pid_path, krunkit_pid.to_string()).expect("krunkit pid should write");
    fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string()).expect("gvproxy pid should write");

    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_for_server = std::sync::Arc::clone(&requests);
    let endpoint_path = paths.krunkit_endpoint_path.clone();
    let request_path = endpoint_path.clone();
    let server = thread::spawn(move || {
        let listener = UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
        let (mut stream, _) = listener.accept().expect("endpoint should accept request");
        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let state = if request.contains("\"HardStop\"") {
            "HardStop"
        } else {
            "Stop"
        };
        requests_for_server
            .lock()
            .expect("request log should lock")
            .push(state.to_owned());
        let _ = send_signal(krunkit_pid, SIGKILL);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("response should write");
        stream.flush().expect("response should flush");
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while !request_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(request_path.exists(), "endpoint should appear before stop");

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    stop_machine(&paths, &config, &mut state).expect("machine stop should succeed");
    server.join().expect("endpoint server should finish");

    assert_eq!(
        requests.lock().expect("request log should lock").clone(),
        vec!["Stop".to_owned()]
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::HelpersResolved);
    assert_eq!(state.last_error, None);
    assert!(
        wait_for_pid_exit(krunkit_pid, Duration::from_secs(2))
            .expect("krunkit pid should become not alive"),
        "krunkit process should exit during graceful provider stop"
    );
    assert!(
        wait_for_pid_exit(gvproxy_pid, Duration::from_secs(2))
            .expect("gvproxy pid should become not alive"),
        "gvproxy process should be stopped during cleanup"
    );
    krunkit_reaper
        .join()
        .expect("krunkit reaper should observe process exit");
    gvproxy_reaper
        .join()
        .expect("gvproxy reaper should observe process exit");
}

#[test]
fn request_krunkit_state_change_sends_hard_stop_payload() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let endpoint_path = temp_dir.path().join("krunkit.sock");
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_for_server = std::sync::Arc::clone(&requests);
    let request_path = endpoint_path.clone();
    let server = thread::spawn(move || {
        let listener = UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
        let (mut stream, _) = listener.accept().expect("endpoint should accept request");
        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let state = if request.contains("\"HardStop\"") {
            "HardStop"
        } else {
            "Stop"
        };
        requests_for_server
            .lock()
            .expect("request log should lock")
            .push(state.to_owned());
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("response should write");
        stream.flush().expect("response should flush");
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while !request_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        request_path.exists(),
        "endpoint should appear before request"
    );

    request_krunkit_state_change(&request_path, "HardStop")
        .expect("hard-stop request should succeed");
    server.join().expect("endpoint server should finish");

    assert_eq!(
        requests.lock().expect("request log should lock").clone(),
        vec!["HardStop".to_owned()]
    );
}

#[test]
fn wait_for_pid_exit_reports_timeout_while_process_is_still_running() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (pid, reaper) = spawn_reaped_process("exec sleep 30");

    assert!(
        !wait_for_pid_exit(pid, Duration::from_millis(50))
            .expect("wait should report timeout for a running process")
    );

    force_stop_pid(pid, Duration::from_secs(2)).expect("force stop should succeed");
    reaper
        .join()
        .expect("process reaper should observe process exit");
}

#[test]
fn launch_plan_reuses_recorded_managed_ssh_port_when_available() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let plan = MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");
    let allocation_state = load_machine_port_allocation_state(&config.roots)
        .expect("port allocation state should load");

    assert_eq!(plan.runtime.ssh_port, 20022);
    assert_eq!(allocation_state.machine_ports.get("default"), Some(&20022));
}

#[test]
fn launch_plan_reassigns_recorded_ssh_port_when_it_is_busy() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let listener = TcpListener::bind("127.0.0.1:20023")
        .or_else(|_| TcpListener::bind("127.0.0.1:0"))
        .expect("listener should bind");
    let busy_port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: busy_port,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let plan = MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");
    let allocation_state = load_machine_port_allocation_state(&config.roots)
        .expect("port allocation state should load");

    assert_ne!(plan.runtime.ssh_port, busy_port);
    assert!(managed_machine_port_range_contains(plan.runtime.ssh_port));
    assert_eq!(
        allocation_state.machine_ports.get("default"),
        Some(&plan.runtime.ssh_port)
    );
}

#[test]
fn release_machine_ssh_port_removes_reserved_port() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    with_port_allocation_lock(&roots, || {
        let mut state = load_machine_port_allocation_state(&roots)?;
        state.machine_ports.insert("default".to_owned(), 20024);
        write_machine_port_allocation_state(&roots, &state)
    })
    .expect("reserved machine port should write");

    release_machine_ssh_port(&roots, "default").expect("port release should succeed");

    let allocation_state =
        load_machine_port_allocation_state(&roots).expect("allocation state should load");
    assert!(allocation_state.machine_ports.is_empty());
}

#[test]
fn refresh_machine_state_marks_missing_pids_as_stale() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    paths
        .ensure_runtime_directories()
        .expect("runtime directories should exist");

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: PathBuf::from("/tmp/disk.raw"),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: "docker://quay.io/podman/machine-os@sha256:test".to_owned(),
        ssh_port: 2222,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    refresh_machine_state(&paths, &mut state).expect("refresh should succeed");

    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .expect("stale error should be present")
            .contains("krunkit_alive=false")
    );
}

#[test]
fn ssh_command_requires_running_machine_and_identity() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = None;

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let error = build_ssh_command(&config, &state).expect_err("missing identity should fail");
    assert!(error.to_string().contains("no SSH identity configured"));
}

#[test]
fn ssh_command_applies_localhost_machine_safety_options() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let identity_path = temp_dir.path().join("machine");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&identity_path, "fake-private-key").expect("identity should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(identity_path.clone());

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let command = build_ssh_command(&config, &state).expect("ssh command should build");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "BatchMode=yes"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "StrictHostKeyChecking=no"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "UserKnownHostsFile=/dev/null"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-i", identity_path.to_string_lossy().as_ref()])
    );
    assert!(args.windows(2).any(|window| window == ["-p", "2222"]));
    assert_eq!(args.last().map(String::as_str), Some("core@127.0.0.1"));
}

#[test]
fn scp_command_applies_localhost_machine_safety_options() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let identity_path = temp_dir.path().join("machine");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&identity_path, "fake-private-key").expect("identity should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(identity_path.clone());

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let command = build_scp_command(&config, &state, false, "/tmp/remote.txt", "./local.txt")
        .expect("scp command should build");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "BatchMode=yes"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "StrictHostKeyChecking=no"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "UserKnownHostsFile=/dev/null"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-i", identity_path.to_string_lossy().as_ref()])
    );
    assert!(args.windows(2).any(|window| window == ["-P", "2222"]));
    assert!(args.iter().any(|arg| arg == "-r"));
    assert!(
        args.iter()
            .any(|arg| arg == "core@127.0.0.1:/tmp/remote.txt")
    );
    assert_eq!(
        args.last().map(String::as_str),
        Some("core@127.0.0.1:/tmp/remote.txt")
    );
}

#[test]
fn scp_command_formats_guest_source_for_downloads() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let identity_path = temp_dir.path().join("machine");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&identity_path, "fake-private-key").expect("identity should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(identity_path);

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let command = build_scp_command(&config, &state, true, "/tmp/remote.txt", "./local.txt")
        .expect("scp command should build");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    let src_index = args
        .iter()
        .position(|arg| arg == "core@127.0.0.1:/tmp/remote.txt")
        .expect("remote source should exist");
    let dst_index = args
        .iter()
        .position(|arg| arg == "./local.txt")
        .expect("local destination should exist");
    assert!(src_index < dst_index);
}

fn serve_single_http_response(body: Vec<u8>, path: Option<&str>) -> String {
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

fn spawn_reaped_process(command: &str) -> (i32, thread::JoinHandle<()>) {
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

fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should resolve");
    let repository = "example/neovex-machine-os";
    let tag = "test";
    let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
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
            "mediaType": "application/vnd.neovex.machine.disk.layer.v1.tar+gzip",
            "size": layer_body.len(),
            "digest": layer_digest,
            "annotations": {
                "org.opencontainers.image.title": "neovex-machine-os.raw.gz"
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
            "mediaType": "application/vnd.neovex.machine.disk.layer.v1.tar+gzip",
            "size": layer_body.len(),
            "digest": layer_digest,
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
                    "org.opencontainers.image.source": "https://github.com/agentstation/neovex-machine-os",
                    "io.neovex.machine.attestation.repository": "agentstation/neovex-machine-os",
                    "io.neovex.machine.neovex.version": "v1.2.3"
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

#[test]
fn attestation_repository_prefers_explicit_metadata() {
    assert_eq!(
        attestation_repositories_for_reference(
            "agentstation/neovex-machine-os",
            Some("agentstation/neovex")
        ),
        vec!["agentstation/neovex".to_owned()]
    );
}

#[test]
fn attestation_repository_falls_back_to_known_repo_order() {
    assert_eq!(
        attestation_repositories_for_reference("agentstation/neovex-machine-os", None),
        vec![
            "agentstation/neovex-machine-os".to_owned(),
            "agentstation/neovex".to_owned()
        ]
    );
}

#[test]
fn machine_artifact_metadata_uses_primary_then_fallback_annotations() {
    let mut primary = BTreeMap::new();
    primary.insert(
        OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY.to_owned(),
        "agentstation/neovex".to_owned(),
    );
    let mut fallback = BTreeMap::new();
    fallback.insert(
        OCI_ANNOTATION_SOURCE.to_owned(),
        "https://github.com/agentstation/neovex-machine-os".to_owned(),
    );
    fallback.insert(
        OCI_ANNOTATION_MACHINE_NEOVEX_VERSION.to_owned(),
        "v1.2.3".to_owned(),
    );

    let metadata = machine_artifact_metadata_from_annotations(Some(&primary), Some(&fallback));

    assert_eq!(
        metadata.attestation_repository.as_deref(),
        Some("agentstation/neovex")
    );
    assert_eq!(
        metadata.source_repository_url.as_deref(),
        Some("https://github.com/agentstation/neovex-machine-os")
    );
    assert_eq!(metadata.neovex_version.as_deref(), Some("v1.2.3"));
}
