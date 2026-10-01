//! State-root filesystem classification.
//!
//! The authority accepts only same-host local filesystems. Each platform reports
//! its filesystem kind here, and known network-mounted kinds fail closed.

use std::path::Path;

use super::NetworkStateStoreError;

pub(super) fn ensure_supported_filesystem(
    path: &Path,
    filesystem_kind: &str,
) -> Result<(), NetworkStateStoreError> {
    let normalized = filesystem_kind.to_ascii_lowercase();
    let unsupported = [
        "nfs",
        "nfs4",
        "smb",
        "smb2",
        "smbfs",
        "cifs",
        "9p",
        "afs",
        "coda",
        "ncp",
        "ceph",
        "webdav",
        "davfs",
        "windows-no-root",
        "windows-unknown",
    ];
    if unsupported
        .iter()
        .any(|kind| normalized == *kind || normalized.starts_with(&format!("{kind}:")))
    {
        Err(NetworkStateStoreError::UnsupportedFilesystem {
            path: path.to_path_buf(),
            filesystem_kind: filesystem_kind.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(super) fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    use std::ffi::CString;
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let encoded = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        NetworkStateStoreError::UnsupportedFilesystem {
            path: path.to_path_buf(),
            filesystem_kind: "path contains a NUL byte".to_owned(),
        }
    })?;
    let mut stat = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `encoded` is a live NUL-terminated path and `stat` points to
    // writable storage of the exact structure libc expects.
    let result = unsafe { libc::statfs(encoded.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return Err(NetworkStateStoreError::Io {
            operation: "inspect state-root filesystem",
            path: path.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }
    // SAFETY: successful statfs initialized the structure.
    // Linux filesystem magic values are 32-bit bit patterns even when
    // `f_type` is a signed machine word. Normalize through `u32` before
    // widening so CIFS/SMB2 do not sign-extend on ILP32 targets.
    let magic = unsafe { stat.assume_init() }.f_type as u32;
    Ok(classify_linux_filesystem_magic(magic))
}

#[cfg(any(test, target_os = "linux"))]
fn classify_linux_filesystem_magic(magic: u32) -> String {
    match magic {
        0x0000_6969 => "nfs".to_owned(),
        0xff53_4d42 => "cifs".to_owned(),
        0xfe53_4d42 => "smb2".to_owned(),
        0x0102_1997 => "9p".to_owned(),
        0x5346_414f => "afs".to_owned(),
        0x7375_7245 => "coda".to_owned(),
        0x0000_564c => "ncp".to_owned(),
        0x00c3_6400 => "ceph".to_owned(),
        0x0000_ef53 => "ext".to_owned(),
        0x5846_5342 => "xfs".to_owned(),
        0x9123_683e => "btrfs".to_owned(),
        0x0102_1994 => "tmpfs".to_owned(),
        0x794c_7630 => "overlay".to_owned(),
        other => format!("linux-magic:0x{other:x}"),
    }
}

#[cfg(all(
    unix,
    any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )
))]
pub(super) fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    use std::ffi::{CStr, CString};
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let encoded = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        NetworkStateStoreError::UnsupportedFilesystem {
            path: path.to_path_buf(),
            filesystem_kind: "path contains a NUL byte".to_owned(),
        }
    })?;
    let mut stat = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `encoded` and `stat` satisfy libc::statfs's pointer contract.
    let result = unsafe { libc::statfs(encoded.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return Err(NetworkStateStoreError::Io {
            operation: "inspect state-root filesystem",
            path: path.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }
    // SAFETY: successful statfs initialized the structure and f_fstypename is
    // a kernel-provided NUL-terminated fixed buffer on supported BSD targets.
    let stat = unsafe { stat.assume_init() };
    let name = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    Ok(name.to_string_lossy().into_owned())
}

#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
pub(super) fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    Err(NetworkStateStoreError::UnsupportedFilesystem {
        path: path.to_path_buf(),
        filesystem_kind: format!("unsupported-unix-target:{}", std::env::consts::OS),
    })
}

#[cfg(windows)]
pub(super) fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    use std::fs;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_NO_ROOT_DIR, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };

    let canonical = fs::canonicalize(path).map_err(|source| NetworkStateStoreError::Io {
        operation: "canonicalize state-root filesystem",
        path: path.to_path_buf(),
        source,
    })?;
    let canonical_text = canonical.to_string_lossy();
    let root = match windows_classification_root(&canonical_text) {
        Some(WindowsClassificationRoot::Unc) => return Ok("smb".to_owned()),
        Some(WindowsClassificationRoot::Drive(root)) => root,
        None => {
            return Err(NetworkStateStoreError::UnsupportedFilesystem {
                path: canonical,
                filesystem_kind: "windows-unknown-root-shape".to_owned(),
            });
        }
    };
    let mut wide: Vec<u16> = std::ffi::OsStr::new(&root).encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a live NUL-terminated root path.
    let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
    Ok(match kind {
        DRIVE_REMOTE => "smb".to_owned(),
        DRIVE_FIXED => "windows-fixed".to_owned(),
        DRIVE_RAMDISK => "windows-ramdisk".to_owned(),
        DRIVE_REMOVABLE => "windows-removable".to_owned(),
        DRIVE_CDROM => "windows-cdrom".to_owned(),
        DRIVE_NO_ROOT_DIR => "windows-no-root".to_owned(),
        _ => "windows-unknown".to_owned(),
    })
}

#[cfg(any(test, windows))]
#[derive(Debug, PartialEq, Eq)]
enum WindowsClassificationRoot {
    /// A UNC path is network-mounted by definition; no drive-type lookup can
    /// make it a supported node-local authority root.
    Unc,
    /// Plain drive root accepted by `GetDriveTypeW`, including the trailing
    /// backslash required by that API.
    Drive(String),
}

/// Convert Rust's canonical Windows path shape into the root form accepted by
/// `GetDriveTypeW`.
///
/// `std::fs::canonicalize` returns verbatim paths (`\\?\C:\...` or
/// `\\?\UNC\server\share\...`). `GetDriveTypeW` requires a plain `C:\` root;
/// UNC paths are classified as remote without calling it. Unknown device or
/// volume shapes fail closed at the caller.
#[cfg(any(test, windows))]
fn windows_classification_root(path: &str) -> Option<WindowsClassificationRoot> {
    if path
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        return Some(WindowsClassificationRoot::Unc);
    }
    if path.starts_with(r"\\") && !path.starts_with(r"\\?\") {
        return Some(WindowsClassificationRoot::Unc);
    }
    let drive_path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = drive_path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return Some(WindowsClassificationRoot::Drive(format!(
            "{}:\\",
            bytes[0] as char
        )));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_network_filesystems_are_rejected_and_local_types_are_accepted() {
        let root = Path::new("/authority");
        for unsupported in [
            "nfs", "nfs4", "smbfs", "cifs", "9p", "afs", "coda", "ncp", "ceph", "webdav",
        ] {
            assert!(
                matches!(
                    ensure_supported_filesystem(root, unsupported),
                    Err(NetworkStateStoreError::UnsupportedFilesystem { .. })
                ),
                "{unsupported} must fail closed"
            );
        }
        for supported in ["apfs", "ext", "xfs", "btrfs", "tmpfs", "overlay"] {
            ensure_supported_filesystem(root, supported)
                .unwrap_or_else(|error| panic!("{supported} should be accepted: {error}"));
        }
    }

    #[test]
    fn signed_ilp32_linux_magic_preserves_cifs_and_smb2_bit_patterns() {
        for (magic, expected) in [(0xff53_4d42_u32, "cifs"), (0xfe53_4d42_u32, "smb2")] {
            let signed = magic as i32;
            assert!(
                signed.is_negative(),
                "fixture must exercise the ILP32 sign bit"
            );
            assert_eq!(
                classify_linux_filesystem_magic(signed as u32),
                expected,
                "normalizing through u32 must prevent signed-word extension"
            );
            assert!(matches!(
                ensure_supported_filesystem(
                    Path::new("/authority"),
                    &classify_linux_filesystem_magic(signed as u32)
                ),
                Err(NetworkStateStoreError::UnsupportedFilesystem { .. })
            ));
        }
    }

    #[test]
    fn windows_verbatim_drive_and_unc_roots_are_classified_fail_closed() {
        assert_eq!(
            windows_classification_root(r"\\?\C:\Users\Nimbus\state"),
            Some(WindowsClassificationRoot::Drive("C:\\".to_owned()))
        );
        assert_eq!(
            windows_classification_root(r"D:\state"),
            Some(WindowsClassificationRoot::Drive("D:\\".to_owned()))
        );
        assert_eq!(
            windows_classification_root(r"\\?\UNC\server\share\state"),
            Some(WindowsClassificationRoot::Unc)
        );
        assert_eq!(
            windows_classification_root(r"\\server\share\state"),
            Some(WindowsClassificationRoot::Unc)
        );
        assert_eq!(
            windows_classification_root(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\state"),
            None,
            "unknown device/volume shapes must fail closed instead of bypassing classification"
        );
    }
}
