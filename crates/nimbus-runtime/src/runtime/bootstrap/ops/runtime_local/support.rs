use std::path::Path;

use deno_io::fs::{FsError, FsStat};
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

pub(super) fn describe_fs_stat(stat: &FsStat) -> RuntimeFsStatDescriptor {
    RuntimeFsStatDescriptor {
        is_file: stat.is_file,
        is_directory: stat.is_directory,
        is_symlink: stat.is_symlink,
        size: stat.size,
        mtime_ms: stat.mtime,
        atime_ms: stat.atime,
        birthtime_ms: stat.birthtime,
        ctime_ms: stat.ctime,
        mode: unix_u32(stat.mode),
        dev: unix_u64(stat.dev),
        ino: stat.ino,
        nlink: stat.nlink,
        uid: unix_u32(stat.uid),
        gid: unix_u32(stat.gid),
        rdev: unix_u64(stat.rdev),
        blksize: unix_u64(stat.blksize),
        blocks: stat.blocks,
        is_block_device: stat.is_block_device,
        is_char_device: stat.is_char_device,
        is_fifo: stat.is_fifo,
        is_socket: stat.is_socket,
    }
}

#[cfg(unix)]
fn unix_u32(value: u32) -> Option<u32> {
    Some(value)
}

#[cfg(not(unix))]
fn unix_u32(_value: u32) -> Option<u32> {
    None
}

#[cfg(unix)]
fn unix_u64(value: u64) -> Option<u64> {
    Some(value)
}

#[cfg(not(unix))]
fn unix_u64(_value: u64) -> Option<u64> {
    None
}

pub(super) fn runtime_fs_error_value(path: &Path, op: &str, error: &FsError) -> Value {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        _ => match raw_os_error(error) {
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

fn raw_os_error(error: &FsError) -> Option<i32> {
    match error {
        FsError::Io(error) => error.raw_os_error(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_fs_stat_preserves_pre_epoch_timestamps() {
        let stat = FsStat {
            is_file: true,
            is_directory: false,
            is_symlink: false,
            size: 17,
            mtime: Some(-4_000),
            atime: Some(-3_000),
            birthtime: Some(-2_000),
            ctime: Some(-1_000),
            dev: 1,
            ino: Some(2),
            mode: 0o644,
            nlink: Some(1),
            uid: 3,
            gid: 4,
            rdev: 5,
            blksize: 4_096,
            blocks: Some(1),
            is_block_device: false,
            is_char_device: false,
            is_fifo: false,
            is_socket: false,
        };

        let descriptor = describe_fs_stat(&stat);

        assert_eq!(descriptor.mtime_ms, Some(-4_000));
        assert_eq!(descriptor.atime_ms, Some(-3_000));
        assert_eq!(descriptor.birthtime_ms, Some(-2_000));
        assert_eq!(descriptor.ctime_ms, Some(-1_000));
    }
}
