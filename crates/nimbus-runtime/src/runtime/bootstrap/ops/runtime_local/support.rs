use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::backends::v8::embedder::JsErrorBox;
use crate::error::NimbusRuntimeError;

use super::types::RuntimeFsStatDescriptor;

pub(super) fn capability_denied_error(error: impl std::fmt::Display) -> JsErrorBox {
    JsErrorBox::generic(NimbusRuntimeError::CapabilityDenied(error.to_string()).to_string())
}

pub(super) fn runtime_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    let vendor = if cfg!(target_vendor = "apple") {
        "apple"
    } else if cfg!(target_vendor = "pc") {
        "pc"
    } else {
        "unknown"
    };
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        std::env::consts::OS
    };
    let env = if cfg!(target_env = "gnu") {
        Some("gnu")
    } else if cfg!(target_env = "musl") {
        Some("musl")
    } else if cfg!(target_env = "msvc") {
        Some("msvc")
    } else {
        None
    };
    match env {
        Some(env) => format!("{arch}-{vendor}-{os}-{env}"),
        None => format!("{arch}-{vendor}-{os}"),
    }
}

#[cfg(unix)]
pub(super) fn create_runtime_symlink(
    oldpath: &Path,
    newpath: &Path,
    _file_type: Option<&str>,
) -> std::io::Result<()> {
    std::os::unix::fs::symlink(oldpath, newpath)
}

#[cfg(windows)]
pub(super) fn create_runtime_symlink(
    oldpath: &Path,
    newpath: &Path,
    file_type: Option<&str>,
) -> std::io::Result<()> {
    match file_type {
        Some("dir") | Some("junction") => std::os::windows::fs::symlink_dir(oldpath, newpath),
        _ => std::os::windows::fs::symlink_file(oldpath, newpath),
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) fn create_runtime_symlink(
    _oldpath: &Path,
    _newpath: &Path,
    _file_type: Option<&str>,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink is not supported on this platform",
    ))
}

fn system_time_to_unix_millis(value: Option<SystemTime>) -> Option<i64> {
    value.and_then(|time| match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).ok(),
        Err(error) => {
            let millis = i64::try_from(error.duration().as_millis()).ok()?;
            Some(-millis)
        }
    })
}

fn system_time_from_unix_parts(seconds: i64, nanos: u32) -> std::io::Result<SystemTime> {
    if nanos >= 1_000_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "timestamp nanoseconds are out of range",
        ));
    }

    if seconds >= 0 {
        return SystemTime::UNIX_EPOCH
            .checked_add(Duration::new(seconds as u64, nanos))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "timestamp is out of range",
                )
            });
    }

    let seconds_before_epoch = seconds.unsigned_abs();
    let duration_before_epoch = if nanos == 0 {
        Duration::new(seconds_before_epoch, 0)
    } else {
        Duration::new(
            seconds_before_epoch.saturating_sub(1),
            1_000_000_000 - nanos,
        )
    };

    SystemTime::UNIX_EPOCH
        .checked_sub(duration_before_epoch)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "timestamp is out of range",
            )
        })
}

pub(super) fn apply_path_times(
    path: &Path,
    atime_secs: i64,
    atime_nanos: u32,
    mtime_secs: i64,
    mtime_nanos: u32,
) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let times = std::fs::FileTimes::new()
        .set_accessed(system_time_from_unix_parts(atime_secs, atime_nanos)?)
        .set_modified(system_time_from_unix_parts(mtime_secs, mtime_nanos)?);
    file.set_times(times)
}

#[cfg(unix)]
fn metadata_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn metadata_mode(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_ctime_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    let millis = (metadata.ctime() as i128)
        .checked_mul(1_000)?
        .checked_add((metadata.ctime_nsec() as i128) / 1_000_000)?;
    i64::try_from(millis).ok()
}

#[cfg(not(unix))]
fn metadata_ctime_ms(_metadata: &std::fs::Metadata) -> Option<i64> {
    None
}

#[cfg(unix)]
fn metadata_dev(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.dev())
}

#[cfg(not(unix))]
fn metadata_dev(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_ino(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino())
}

#[cfg(not(unix))]
fn metadata_ino(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_nlink(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.nlink())
}

#[cfg(not(unix))]
fn metadata_nlink(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_uid(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.uid())
}

#[cfg(not(unix))]
fn metadata_uid(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_gid(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.gid())
}

#[cfg(not(unix))]
fn metadata_gid(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_rdev(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.rdev())
}

#[cfg(not(unix))]
fn metadata_rdev(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_blksize(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.blksize())
}

#[cfg(not(unix))]
fn metadata_blksize(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_blocks(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.blocks())
}

#[cfg(not(unix))]
fn metadata_blocks(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_is_block_device(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_block_device()
}

#[cfg(not(unix))]
fn metadata_is_block_device(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata_is_char_device(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_char_device()
}

#[cfg(not(unix))]
fn metadata_is_char_device(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata_is_fifo(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_fifo()
}

#[cfg(not(unix))]
fn metadata_is_fifo(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata_is_socket(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_socket()
}

#[cfg(not(unix))]
fn metadata_is_socket(_metadata: &std::fs::Metadata) -> bool {
    false
}

pub(super) fn describe_metadata(metadata: &std::fs::Metadata) -> RuntimeFsStatDescriptor {
    RuntimeFsStatDescriptor {
        is_file: metadata.is_file(),
        is_directory: metadata.is_dir(),
        is_symlink: metadata.file_type().is_symlink(),
        size: metadata.len(),
        mtime_ms: system_time_to_unix_millis(metadata.modified().ok()),
        atime_ms: system_time_to_unix_millis(metadata.accessed().ok()),
        birthtime_ms: system_time_to_unix_millis(metadata.created().ok()),
        ctime_ms: metadata_ctime_ms(metadata),
        mode: metadata_mode(metadata),
        dev: metadata_dev(metadata),
        ino: metadata_ino(metadata),
        nlink: metadata_nlink(metadata),
        uid: metadata_uid(metadata),
        gid: metadata_gid(metadata),
        rdev: metadata_rdev(metadata),
        blksize: metadata_blksize(metadata),
        blocks: metadata_blocks(metadata),
        is_block_device: metadata_is_block_device(metadata),
        is_char_device: metadata_is_char_device(metadata),
        is_fifo: metadata_is_fifo(metadata),
        is_socket: metadata_is_socket(metadata),
    }
}

pub(super) fn runtime_fs_error_value(path: &Path, op: &str, error: &std::io::Error) -> Value {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        _ => match error.raw_os_error() {
            Some(20) => "ENOTDIR",
            Some(21) => "EISDIR",
            Some(1) => "EPERM",
            Some(39) => "ENOTEMPTY",
            #[cfg(windows)]
            Some(267) => "ENOTDIR",
            #[cfg(windows)]
            Some(145) => "ENOTEMPTY",
            _ => "EIO",
        },
    };
    json!({
        "code": code,
        "message": format!("{op} {} failed: {error}", path.display()),
    })
}

#[cfg(unix)]
pub(super) fn apply_directory_mode(path: &Path, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let permissions = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn apply_directory_mode(_path: &Path, _mode: Option<u32>) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn apply_fs_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
pub(super) fn apply_fs_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}
