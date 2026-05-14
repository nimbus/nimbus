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

use super::guest::{ensure_guest_nimbus_socket_shell_script, guest_nimbus_archive_name};
use super::helpers::{
    bundled_helper_candidates_for_executable, known_helper_candidates, resolve_helper_binary,
    write_helper_stub,
};
use super::image::{
    attestation_repositories_for_reference, build_digest_reference,
    current_machine_oci_architectures, machine_artifact_metadata_from_annotations,
    materialize_cached_disk, resolve_bootable_image_path,
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
use crate::machine::bootstrap::GUEST_NIMBUS_SOCKET;
use crate::machine::{
    CURRENT_MACHINE_CONFIG_VERSION, DEFAULT_BOOTC_MACHINE_SSH_USER,
    DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY, MachineBootstrapMode, MachineGuestConfig,
    MachineGuestProvisioning, MachineImageFormat, MachineImageSource, MachineProvider,
    MachineResources, MachineRootLayout, MachineVolume, current_machine_release_tag,
    default_machine_image_for_provider, describe_machine_image_source,
    machine_image_reference_repository,
};

mod attestation;
mod helper_resolution;
mod launch_image;
mod ports_state;
mod provider_bootstrap;
mod readiness_startup;
mod ssh_scp;
mod stop_cleanup;
mod support;

use self::support::*;
