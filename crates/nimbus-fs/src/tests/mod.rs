use std::borrow::Cow;
use std::path::{Path, PathBuf};

use deno_fs::sync::MaybeArc;
use deno_io::fs::{FsError, FsResult, FsStat};
use deno_permissions::{CheckedPath, CheckedPathBuf};

use crate::{MemFsBackend, MountTable, NimbusFs};

mod caps;
mod cas_ro;
mod delegation;
mod grants;
mod mount;
mod object;
mod passthrough;

fn checked(path: &Path) -> CheckedPath<'_> {
    CheckedPath::unsafe_new(Cow::Borrowed(path))
}

fn checked_buf(path: impl Into<PathBuf>) -> CheckedPathBuf {
    CheckedPathBuf::unsafe_new(path.into())
}

fn memfs_rc() -> deno_fs::FileSystemRc {
    MaybeArc::new(MemFsBackend::new())
}

fn fs_with_mounts(table: MountTable) -> NimbusFs {
    NimbusFs::with_mount_table(table, "/")
}

fn expect_stat_error(result: FsResult<FsStat>, message: &str) -> FsError {
    match result {
        Ok(_) => panic!("{message}"),
        Err(error) => error,
    }
}
