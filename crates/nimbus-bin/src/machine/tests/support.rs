use super::*;

#[derive(Debug, Parser)]
pub(super) struct RootCli {
    #[command(subcommand)]
    pub(super) command: Option<RootCommand>,
}

#[derive(Debug, Subcommand)]
pub(super) enum RootCommand {
    Machine(MachineCommand),
}

pub(super) fn expected_default_machine_image() -> String {
    if cfg!(target_os = "macos") {
        format!(
            "docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@{DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST}"
        )
    } else {
        format!(
            "docker://{DEFAULT_NIMBUS_MACHINE_IMAGE_REPOSITORY}:{}",
            current_machine_release_tag()
        )
    }
}

fn machine_guest_binary_override_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) fn lock_machine_guest_binary_override_env() -> std::sync::MutexGuard<'static, ()> {
    machine_guest_binary_override_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) struct GuestBinaryOverrideEnvGuard {
    previous: Option<std::ffi::OsString>,
}

impl GuestBinaryOverrideEnvGuard {
    pub(super) fn clear() -> Self {
        let previous = std::env::var_os("NIMBUS_MACHINE_GUEST_BINARY");
        unsafe { std::env::remove_var("NIMBUS_MACHINE_GUEST_BINARY") };
        Self { previous }
    }

    pub(super) fn set(path: &Path) -> Self {
        let previous = std::env::var_os("NIMBUS_MACHINE_GUEST_BINARY");
        unsafe { std::env::set_var("NIMBUS_MACHINE_GUEST_BINARY", path) };
        Self { previous }
    }
}

impl Drop for GuestBinaryOverrideEnvGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => unsafe { std::env::set_var("NIMBUS_MACHINE_GUEST_BINARY", value) },
            None => unsafe { std::env::remove_var("NIMBUS_MACHINE_GUEST_BINARY") },
        }
    }
}

pub(super) fn supported_stream_current_image_for_upgrade_test() -> String {
    if cfg!(target_os = "macos") {
        "docker://quay.io/podman/machine-os@sha256:abc123".to_owned()
    } else {
        "docker://ghcr.io/nimbus/nimbus-machine-os:v0.1.0".to_owned()
    }
}

pub(super) fn supported_stream_digest_image_for_upgrade_test() -> String {
    if cfg!(target_os = "macos") {
        format!("docker://{DEFAULT_PODMAN_MACHINE_IMAGE_REPOSITORY}@sha256:abc123")
    } else {
        "docker://ghcr.io/nimbus/nimbus-machine-os@sha256:abc123".to_owned()
    }
}

pub(super) fn expected_upgrade_target_version() -> String {
    if cfg!(target_os = "macos") {
        DEFAULT_PODMAN_MACHINE_IMAGE_DIGEST.to_owned()
    } else {
        current_machine_release_tag()
    }
}

pub(super) fn run_machine_command_for_test(
    command: MachineCommand,
    layout: &MachineRootLayout,
) -> Result<(), Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(run_machine_command_with_layout(command, layout))
}
