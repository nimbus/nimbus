//! Persistent Linux network namespace lifecycle.

use std::path::Path;

#[cfg(unix)]
use cap_std::fs::MetadataExt as _;
#[cfg(unix)]
use cap_std::fs::OpenOptionsExt as _;
use cap_std::fs::{Dir, File as CapFile, OpenOptions as CapOpenOptions};
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::Read as _;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd as _;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
#[cfg(target_os = "linux")]
use std::thread;

use crate::error::{Result, SandboxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExactRegularArtifactObservation {
    Present,
    ExplicitlyAbsent,
}

/// Inspect one exact regular artifact without following its immediate parent
/// or final entry through a symlink.
///
/// Absence is authoritative only after the expected parent is proven to be a
/// readable, non-symlink directory and its complete entry set omits the exact
/// artifact name. This function performs no repair or filesystem mutation.
pub(super) fn inspect_exact_regular_artifact(
    expected_parent: &Path,
    path: &Path,
    label: &str,
) -> std::result::Result<ExactRegularArtifactObservation, String> {
    inspect_exact_regular_artifact_inner(expected_parent, path, label, || {}, || {})
}

#[cfg(test)]
pub(super) fn inspect_exact_regular_artifact_with_parent_open_hook(
    expected_parent: &Path,
    path: &Path,
    label: &str,
    after_parent_open: impl FnOnce(),
) -> std::result::Result<ExactRegularArtifactObservation, String> {
    inspect_exact_regular_artifact_inner(expected_parent, path, label, after_parent_open, || {})
}

#[cfg(test)]
pub(super) fn inspect_exact_regular_artifact_with_target_inspected_hook(
    expected_parent: &Path,
    path: &Path,
    label: &str,
    after_target_inspected: impl FnOnce(),
) -> std::result::Result<ExactRegularArtifactObservation, String> {
    inspect_exact_regular_artifact_inner(
        expected_parent,
        path,
        label,
        || {},
        after_target_inspected,
    )
}

fn inspect_exact_regular_artifact_inner(
    expected_parent: &Path,
    path: &Path,
    label: &str,
    after_parent_open: impl FnOnce(),
    after_target_inspected: impl FnOnce(),
) -> std::result::Result<ExactRegularArtifactObservation, String> {
    if path.parent() != Some(expected_parent) {
        return Err(format!(
            "cannot inspect {label} {}: parent crossed expected directory {}",
            path.display(),
            expected_parent.display()
        ));
    }
    let target_name = path.file_name().ok_or_else(|| {
        format!(
            "cannot inspect {label} {} without an exact artifact name",
            path.display()
        )
    })?;
    let parent = open_exact_parent(expected_parent, path, label)?;
    after_parent_open();
    let target = parent.inspect_target(expected_parent, path, target_name, label)?;
    after_target_inspected();
    parent.require_current_ambient_identity(expected_parent, path, label)?;
    parent.require_current_target_identity(path, target_name, label, &target)?;
    Ok(target.observation())
}

/// Read one exact regular artifact through the same pinned-parent inspection.
pub(super) fn read_exact_regular_artifact(
    expected_parent: &Path,
    path: &Path,
    label: &str,
) -> std::result::Result<Option<Vec<u8>>, String> {
    read_exact_regular_artifact_inner(expected_parent, path, label, || {})
}

#[cfg(test)]
pub(super) fn read_exact_regular_artifact_with_target_inspected_hook(
    expected_parent: &Path,
    path: &Path,
    label: &str,
    after_target_inspected: impl FnOnce(),
) -> std::result::Result<Option<Vec<u8>>, String> {
    read_exact_regular_artifact_inner(expected_parent, path, label, after_target_inspected)
}

fn read_exact_regular_artifact_inner(
    expected_parent: &Path,
    path: &Path,
    label: &str,
    after_target_inspected: impl FnOnce(),
) -> std::result::Result<Option<Vec<u8>>, String> {
    if path.parent() != Some(expected_parent) {
        return Err(format!(
            "cannot read {label} {}: parent crossed expected directory {}",
            path.display(),
            expected_parent.display()
        ));
    }
    let target_name = path.file_name().ok_or_else(|| {
        format!(
            "cannot read {label} {} without an exact artifact name",
            path.display()
        )
    })?;
    let parent = open_exact_parent(expected_parent, path, label)?;
    let mut target = parent.inspect_target(expected_parent, path, target_name, label)?;
    after_target_inspected();
    parent.require_current_ambient_identity(expected_parent, path, label)?;
    parent.require_current_target_identity(path, target_name, label, &target)?;
    if matches!(target, ExactRegularArtifactSnapshot::ExplicitlyAbsent) {
        return Ok(None);
    }
    let bytes = match &mut target {
        ExactRegularArtifactSnapshot::Present(target) => target.read(path, label)?,
        ExactRegularArtifactSnapshot::ExplicitlyAbsent => unreachable!(),
    };
    parent.require_current_ambient_identity(expected_parent, path, label)?;
    parent.require_current_target_identity(path, target_name, label, &target)?;
    Ok(Some(bytes))
}

enum ExactRegularArtifactSnapshot {
    Present(OpenExactRegularArtifact),
    ExplicitlyAbsent,
}

impl ExactRegularArtifactSnapshot {
    fn observation(&self) -> ExactRegularArtifactObservation {
        match self {
            Self::Present(_) => ExactRegularArtifactObservation::Present,
            Self::ExplicitlyAbsent => ExactRegularArtifactObservation::ExplicitlyAbsent,
        }
    }
}

struct OpenExactRegularArtifact {
    file: CapFile,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl OpenExactRegularArtifact {
    fn read(&mut self, path: &Path, label: &str) -> std::result::Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        self.file
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read exact {label} {}: {error}", path.display()))?;
        Ok(bytes)
    }
}

struct OpenExactParent {
    dir: Dir,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl OpenExactParent {
    fn inspect_target(
        &self,
        expected_parent: &Path,
        path: &Path,
        target_name: &std::ffi::OsStr,
        label: &str,
    ) -> std::result::Result<ExactRegularArtifactSnapshot, String> {
        let entries = self.dir.entries().map_err(|error| {
            format!(
                "cannot inspect {label} {}: cannot read parent {}: {error}",
                path.display(),
                expected_parent.display()
            )
        })?;
        for entry in entries {
            entry.map_err(|error| {
                format!(
                    "cannot inspect {label} {}: cannot read an entry in parent {}: {error}",
                    path.display(),
                    expected_parent.display()
                )
            })?;
        }
        let _entry_metadata = match self.dir.symlink_metadata(target_name) {
            Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => metadata,
            Ok(_) => {
                return Err(format!(
                    "{label} {} is not an exact regular provider artifact",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ExactRegularArtifactSnapshot::ExplicitlyAbsent);
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect exact {label} {} in parent {}: {error}",
                    path.display(),
                    expected_parent.display()
                ));
            }
        };
        let mut options = CapOpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let file = self
            .dir
            .open_with(target_name, &options)
            .map_err(|error| format!("cannot open exact {label} {}: {error}", path.display()))?;
        let opened_metadata = file
            .metadata()
            .map_err(|error| format!("cannot inspect exact {label} {}: {error}", path.display()))?;
        if !opened_metadata.is_file() {
            return Err(format!(
                "{label} {} is not an exact regular provider artifact",
                path.display()
            ));
        }
        #[cfg(unix)]
        if _entry_metadata.dev() != opened_metadata.dev()
            || _entry_metadata.ino() != opened_metadata.ino()
        {
            return Err(format!(
                "cannot inspect {label} {}: final artifact changed during inspection",
                path.display()
            ));
        }
        Ok(ExactRegularArtifactSnapshot::Present(
            OpenExactRegularArtifact {
                file,
                #[cfg(unix)]
                device: opened_metadata.dev(),
                #[cfg(unix)]
                inode: opened_metadata.ino(),
            },
        ))
    }

    fn require_current_target_identity(
        &self,
        path: &Path,
        target_name: &std::ffi::OsStr,
        label: &str,
        target: &ExactRegularArtifactSnapshot,
    ) -> std::result::Result<(), String> {
        match (target, self.dir.symlink_metadata(target_name)) {
            (ExactRegularArtifactSnapshot::ExplicitlyAbsent, Err(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(())
            }
            (ExactRegularArtifactSnapshot::ExplicitlyAbsent, _) => Err(format!(
                "cannot inspect {label} {}: final artifact changed during inspection",
                path.display()
            )),
            (ExactRegularArtifactSnapshot::Present(opened), Ok(current))
                if current.is_file() && !current.is_symlink() =>
            {
                #[cfg(unix)]
                if current.dev() != opened.device || current.ino() != opened.inode {
                    return Err(format!(
                        "cannot inspect {label} {}: final artifact changed during inspection",
                        path.display()
                    ));
                }
                Ok(())
            }
            (ExactRegularArtifactSnapshot::Present(_), _) => Err(format!(
                "cannot inspect {label} {}: final artifact changed during inspection",
                path.display()
            )),
        }
    }

    fn require_current_ambient_identity(
        &self,
        expected_parent: &Path,
        path: &Path,
        label: &str,
    ) -> std::result::Result<(), String> {
        let metadata = fs::symlink_metadata(expected_parent).map_err(|error| {
            format!(
                "cannot inspect {label} {}: parent {} changed during inspection: {error}",
                path.display(),
                expected_parent.display()
            )
        })?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "cannot inspect {label} {}: parent {} changed during inspection",
                path.display(),
                expected_parent.display()
            ));
        }
        #[cfg(unix)]
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(format!(
                "cannot inspect {label} {}: parent {} changed during inspection",
                path.display(),
                expected_parent.display()
            ));
        }
        Ok(())
    }
}

fn open_exact_parent(
    expected_parent: &Path,
    path: &Path,
    label: &str,
) -> std::result::Result<OpenExactParent, String> {
    let metadata = fs::symlink_metadata(expected_parent).map_err(|error| {
        format!(
            "cannot inspect {label} {}: cannot inspect parent {}: {error}",
            path.display(),
            expected_parent.display()
        )
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "cannot inspect {label} {}: parent {} is not a non-symlink directory",
            path.display(),
            expected_parent.display()
        ));
    }

    #[cfg(unix)]
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(expected_parent)
        .map_err(|error| {
            format!(
                "cannot inspect {label} {}: cannot open exact parent {}: {error}",
                path.display(),
                expected_parent.display()
            )
        })?;
    #[cfg(not(unix))]
    let file = Dir::open_ambient_dir(expected_parent, cap_std::ambient_authority())
        .map_err(|error| {
            format!(
                "cannot inspect {label} {}: cannot open exact parent {}: {error}",
                path.display(),
                expected_parent.display()
            )
        })?
        .into_std_file();
    let opened_metadata = file.metadata().map_err(|error| {
        format!(
            "cannot inspect {label} {}: cannot inspect opened parent {}: {error}",
            path.display(),
            expected_parent.display()
        )
    })?;
    if !opened_metadata.is_dir() {
        return Err(format!(
            "cannot inspect {label} {}: parent {} is not a non-symlink directory",
            path.display(),
            expected_parent.display()
        ));
    }
    Ok(OpenExactParent {
        dir: Dir::from_std_file(file),
        #[cfg(unix)]
        device: opened_metadata.dev(),
        #[cfg(unix)]
        inode: opened_metadata.ino(),
    })
}

pub(super) fn create_persistent_network_namespace(path: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(SandboxError::BackendUnavailable {
            message: "persistent OCI network namespaces require Linux".to_owned(),
        })
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create network-namespace parent {}: {error}",
                    parent.display()
                ),
            })?;
        }
        if path.exists() {
            remove_persistent_network_namespace(path)?;
        }
        fs::File::create(path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create network-namespace file {}: {error}",
                path.display()
            ),
        })?;

        let target = path.to_path_buf();
        let join = thread::spawn(move || -> Result<()> {
            let target_c = cstring_path(&target)?;
            let source = CString::new("/proc/thread-self/ns/net").map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to encode network-namespace source path: {error}"),
                }
            })?;
            // SAFETY: unshare and mount are called with validated constant flags and
            // NUL-terminated C strings owned for the duration of the calls.
            unsafe {
                if libc::unshare(libc::CLONE_NEWNET) != 0 {
                    return Err(last_os_error("failed to unshare network namespace"));
                }
                if libc::mount(
                    source.as_ptr(),
                    target_c.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND as libc::c_ulong,
                    std::ptr::null(),
                ) != 0
                {
                    return Err(last_os_error("failed to persist network namespace"));
                }
            }
            Ok(())
        });
        join.join().map_err(|_| SandboxError::OperationFailed {
            message: format!(
                "network-namespace setup thread panicked for {}",
                path.display()
            ),
        })?
    }
}

/// Remove one exact namespace artifact while its caller holds the provider
/// command and sandbox lifecycle serialization for this trusted state root.
pub(super) fn remove_persistent_network_namespace(path: &Path) -> Result<()> {
    remove_persistent_network_namespace_inner(path, || {})
}

#[cfg(test)]
pub(super) fn remove_persistent_network_namespace_with_target_inspected_hook(
    path: &Path,
    after_target_inspected: impl FnOnce(),
) -> Result<()> {
    remove_persistent_network_namespace_inner(path, after_target_inspected)
}

fn remove_persistent_network_namespace_inner(
    path: &Path,
    after_target_inspected: impl FnOnce(),
) -> Result<()> {
    let expected_parent = path.parent().ok_or_else(|| SandboxError::OperationFailed {
        message: format!(
            "network-namespace artifact {} has no exact parent",
            path.display()
        ),
    })?;
    let target_name = path
        .file_name()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "network-namespace artifact {} has no exact name",
                path.display()
            ),
        })?;
    let parent = open_exact_parent(expected_parent, path, "namespace")
        .map_err(|message| SandboxError::OperationFailed { message })?;
    let target = parent
        .inspect_target(expected_parent, path, target_name, "namespace")
        .map_err(|message| SandboxError::OperationFailed { message })?;
    after_target_inspected();
    parent
        .require_current_ambient_identity(expected_parent, path, "namespace")
        .map_err(|message| SandboxError::OperationFailed { message })?;
    parent
        .require_current_target_identity(path, target_name, "namespace", &target)
        .map_err(|message| SandboxError::OperationFailed { message })?;
    if matches!(target, ExactRegularArtifactSnapshot::ExplicitlyAbsent) {
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        // OCI namespace creation is Linux-only, but cleanup must still remove
        // an exact stale namespace artifact created by deterministic recovery
        // tests or copied state. A no-op would falsely report absence while
        // the durable path remains present.
        parent
            .dir
            .remove_file(target_name)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to delete network-namespace artifact {}: {error}",
                    path.display()
                ),
            })?;
    }

    #[cfg(target_os = "linux")]
    {
        let target_fd = match &target {
            ExactRegularArtifactSnapshot::Present(target) => target.file.as_raw_fd(),
            ExactRegularArtifactSnapshot::ExplicitlyAbsent => unreachable!(),
        };
        let pinned_target = Path::new("/proc/self/fd").join(target_fd.to_string());
        let target_c = cstring_path(&pinned_target)?;
        // SAFETY: `umount2` receives a NUL-terminated procfs path for the
        // retained exact target descriptor, not a mutable directory name.
        unsafe {
            if libc::umount2(target_c.as_ptr(), libc::MNT_DETACH) != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EINVAL) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove network namespace {}: {error}",
                            path.display()
                        ),
                    });
                }
            }
        }
        parent
            .require_current_ambient_identity(expected_parent, path, "namespace")
            .map_err(|message| SandboxError::OperationFailed { message })?;
        parent
            .require_current_target_identity(path, target_name, "namespace", &target)
            .map_err(|message| SandboxError::OperationFailed { message })?;
        parent
            .dir
            .remove_file(target_name)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to delete network-namespace file {}: {error}",
                    path.display()
                ),
            })?;
    }
    parent
        .require_current_ambient_identity(expected_parent, path, "namespace")
        .map_err(|message| SandboxError::OperationFailed { message })?;
    parent
        .require_current_target_identity(
            path,
            target_name,
            "namespace",
            &ExactRegularArtifactSnapshot::ExplicitlyAbsent,
        )
        .map_err(|message| SandboxError::OperationFailed { message })?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn cstring_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to encode filesystem path {}: {error}",
            path.display()
        ),
    })
}

#[cfg(target_os = "linux")]
fn last_os_error(context: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("{context}: {}", std::io::Error::last_os_error()),
    }
}
