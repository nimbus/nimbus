use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};

use super::{checked, checked_buf};
use crate::caps::capped_mount_backend;
use crate::{FsMountCaps, NimbusFs};

#[derive(Debug, Clone, Default)]
struct SpyBackend {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl SpyBackend {
    fn record_err<T>(&self, method: &'static str) -> FsResult<T> {
        self.calls.lock().unwrap().push(method);
        Err(io::Error::other(method).into())
    }

    fn record_bool(&self, method: &'static str) -> bool {
        self.calls.lock().unwrap().push(method);
        false
    }

    fn record_ok<T>(&self, method: &'static str, value: T) -> FsResult<T> {
        self.calls.lock().unwrap().push(method);
        Ok(value)
    }

    fn call_set(&self) -> BTreeSet<&'static str> {
        self.calls.lock().unwrap().iter().copied().collect()
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for SpyBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.record_err("cwd")
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.record_err("tmp_dir")
    }

    fn chdir(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("chdir")
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        self.record_err("umask")
    }

    fn open_sync(&self, _path: &CheckedPath<'_>, _options: OpenOptions) -> FsResult<Rc<dyn File>> {
        self.record_err("open_sync")
    }

    async fn open_async<'a>(
        &'a self,
        _path: CheckedPathBuf,
        _options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.record_err("open_async")
    }

    fn mkdir_sync(
        &self,
        _path: &CheckedPath<'_>,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("mkdir_sync")
    }

    async fn mkdir_async(
        &self,
        _path: CheckedPathBuf,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("mkdir_async")
    }

    #[cfg(unix)]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        self.record_err("chmod_sync")
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: i32) -> FsResult<()> {
        self.record_err("chmod_sync")
    }

    #[cfg(unix)]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.record_err("chmod_async")
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: i32) -> FsResult<()> {
        self.record_err("chmod_async")
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("chown_sync")
    }

    async fn chown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("chown_async")
    }

    fn lchmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        self.record_err("lchmod_sync")
    }

    async fn lchmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.record_err("lchmod_async")
    }

    fn lchown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("lchown_sync")
    }

    async fn lchown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("lchown_async")
    }

    fn remove_sync(&self, _path: &CheckedPath<'_>, _recursive: bool) -> FsResult<()> {
        self.record_err("remove_sync")
    }

    async fn remove_async(&self, _path: CheckedPathBuf, _recursive: bool) -> FsResult<()> {
        self.record_err("remove_async")
    }

    fn copy_file_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
    ) -> FsResult<()> {
        self.record_err("copy_file_sync")
    }

    async fn copy_file_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.record_err("copy_file_async")
    }

    fn cp_sync(&self, _path: &CheckedPath<'_>, _new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("cp_sync")
    }

    async fn cp_async(&self, _path: CheckedPathBuf, _new_path: CheckedPathBuf) -> FsResult<()> {
        self.record_err("cp_async")
    }

    fn stat_sync(&self, _path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.record_err("stat_sync")
    }

    async fn stat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        self.record_err("stat_async")
    }

    fn lstat_sync(&self, _path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.record_err("lstat_sync")
    }

    async fn lstat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        self.record_err("lstat_async")
    }

    fn statfs_sync(&self, _path: &CheckedPath<'_>, _bigint: bool) -> FsResult<FsStatFs> {
        self.record_err("statfs_sync")
    }

    async fn statfs_async(&self, _path: CheckedPathBuf, _bigint: bool) -> FsResult<FsStatFs> {
        self.record_err("statfs_async")
    }

    fn realpath_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.record_err("realpath_sync")
    }

    async fn realpath_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.record_err("realpath_async")
    }

    fn read_dir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.record_err("read_dir_sync")
    }

    async fn read_dir_async(&self, _path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        self.record_err("read_dir_async")
    }

    fn rename_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("rename_sync")
    }

    async fn rename_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.record_err("rename_async")
    }

    fn rmdir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("rmdir_sync")
    }

    async fn rmdir_async(&self, _path: CheckedPathBuf) -> FsResult<()> {
        self.record_err("rmdir_async")
    }

    fn link_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("link_sync")
    }

    async fn link_async(&self, _oldpath: CheckedPathBuf, _newpath: CheckedPathBuf) -> FsResult<()> {
        self.record_err("link_async")
    }

    fn symlink_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.record_err("symlink_sync")
    }

    async fn symlink_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.record_err("symlink_async")
    }

    fn read_link_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.record_err("read_link_sync")
    }

    async fn read_link_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.record_err("read_link_async")
    }

    fn truncate_sync(&self, _path: &CheckedPath<'_>, _len: u64) -> FsResult<()> {
        self.record_err("truncate_sync")
    }

    async fn truncate_async(&self, _path: CheckedPathBuf, _len: u64) -> FsResult<()> {
        self.record_err("truncate_async")
    }

    fn utime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("utime_sync")
    }

    async fn utime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("utime_async")
    }

    fn lutime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("lutime_sync")
    }

    async fn lutime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("lutime_async")
    }

    fn exists_sync(&self, _path: &CheckedPath<'_>) -> bool {
        self.record_bool("exists_sync")
    }

    async fn exists_async(&self, _path: CheckedPathBuf) -> FsResult<bool> {
        self.record_ok("exists_async", false)
    }
}

#[test]
fn capped_backend_delegates_cp_without_collapsing_to_copy_file() {
    let spy = SpyBackend::default();
    let (backend, _) = capped_mount_backend(MaybeArc::new(spy.clone()), FsMountCaps::read_write());
    let path = checked(Path::new("/from"));
    let other = checked(Path::new("/to"));

    let error = backend
        .cp_sync(&path, &other)
        .expect_err("spy backend returns its delegated method name");
    assert_eq!(error.to_string(), "cp_sync");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let error = backend
            .cp_async(checked_buf("from"), checked_buf("to"))
            .await
            .expect_err("spy backend returns its delegated method name");
        assert_eq!(error.to_string(), "cp_async");
    });

    let calls = spy.call_set();
    assert!(calls.contains("cp_sync"));
    assert!(calls.contains("cp_async"));
    assert!(!calls.contains("copy_file_sync"));
    assert!(!calls.contains("copy_file_async"));
}

#[test]
fn delegates_filesystem_trait_methods_to_backend_or_composes_them() {
    let root = tempfile::tempdir().unwrap();
    let spy = SpyBackend::default();
    let fs = NimbusFs::with_cwd(spy.clone(), root.path());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let path = checked(Path::new("a"));
    let other = checked(Path::new("b"));

    assert_eq!(fs.cwd().unwrap(), root.path());
    let _ = fs.chdir(&path);
    let _ = fs.tmp_dir();
    let _ = fs.umask(None);
    let _ = fs.open_sync(&path, OpenOptions::read());
    let _ = fs.mkdir_sync(&path, false, None);
    let _ = fs.chmod_sync(&path, 0o600);
    let _ = fs.chown_sync(&path, Some(1), Some(2));
    let _ = fs.lchmod_sync(&path, 0o600);
    let _ = fs.lchown_sync(&path, Some(1), Some(2));
    let _ = fs.remove_sync(&path, false);
    let _ = fs.copy_file_sync(&path, &other);
    let _ = fs.cp_sync(&path, &other);
    let _ = fs.stat_sync(&path);
    let _ = fs.lstat_sync(&path);
    let _ = fs.statfs_sync(&path, false);
    let _ = fs.realpath_sync(&path);
    let _ = fs.read_dir_sync(&path);
    let _ = fs.rename_sync(&path, &other);
    let _ = fs.rmdir_sync(&path);
    let _ = fs.link_sync(&path, &other);
    let _ = fs.symlink_sync(&path, &other, None);
    let _ = fs.read_link_sync(&path);
    let _ = fs.truncate_sync(&path, 1);
    let _ = fs.utime_sync(&path, 1, 2, 3, 4);
    let _ = fs.lutime_sync(&path, 1, 2, 3, 4);
    assert!(!fs.exists_sync(&path));
    let _ = fs.write_file_sync(&path, OpenOptions::write(true, false, false, None), b"x");
    let _ = fs.read_file_sync(&path, OpenOptions::read());
    assert!(!fs.is_file_sync(&path));
    assert!(!fs.is_dir_sync(&path));
    let _ = fs.read_text_file_lossy_sync(&path);

    runtime.block_on(async {
        let _ = fs.open_async(checked_buf("a"), OpenOptions::read()).await;
        let _ = fs.mkdir_async(checked_buf("a"), false, None).await;
        let _ = fs.chmod_async(checked_buf("a"), 0o600).await;
        let _ = fs.chown_async(checked_buf("a"), Some(1), Some(2)).await;
        let _ = fs.lchmod_async(checked_buf("a"), 0o600).await;
        let _ = fs.lchown_async(checked_buf("a"), Some(1), Some(2)).await;
        let _ = fs.remove_async(checked_buf("a"), false).await;
        let _ = fs.copy_file_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs.cp_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs.stat_async(checked_buf("a")).await;
        let _ = fs.lstat_async(checked_buf("a")).await;
        let _ = fs.statfs_async(checked_buf("a"), false).await;
        let _ = fs.realpath_async(checked_buf("a")).await;
        let _ = fs.read_dir_async(checked_buf("a")).await;
        let _ = fs.rename_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs.rmdir_async(checked_buf("a")).await;
        let _ = fs.link_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs
            .symlink_async(checked_buf("a"), checked_buf("b"), None)
            .await;
        let _ = fs.read_link_async(checked_buf("a")).await;
        let _ = fs.truncate_async(checked_buf("a"), 1).await;
        let _ = fs.utime_async(checked_buf("a"), 1, 2, 3, 4).await;
        let _ = fs.lutime_async(checked_buf("a"), 1, 2, 3, 4).await;
        let _ = fs.exists_async(checked_buf("a")).await;
        let _ = fs
            .write_file_async(
                checked_buf("a"),
                OpenOptions::write(true, false, false, None),
                Box::from(*b"x"),
            )
            .await;
        let _ = fs
            .read_file_async(checked_buf("a"), OpenOptions::read())
            .await;
        let _ = fs.read_text_file_lossy_async(checked_buf("a")).await;
    });

    let calls = spy.call_set();
    let required = BTreeSet::from([
        "tmp_dir",
        "umask",
        "open_sync",
        "open_async",
        "mkdir_sync",
        "mkdir_async",
        "chmod_sync",
        "chmod_async",
        "chown_sync",
        "chown_async",
        "lchmod_sync",
        "lchmod_async",
        "lchown_sync",
        "lchown_async",
        "remove_sync",
        "remove_async",
        "copy_file_sync",
        "copy_file_async",
        "cp_sync",
        "cp_async",
        "stat_sync",
        "stat_async",
        "lstat_sync",
        "lstat_async",
        "statfs_sync",
        "statfs_async",
        "realpath_sync",
        "realpath_async",
        "read_dir_sync",
        "read_dir_async",
        "rename_sync",
        "rename_async",
        "rmdir_sync",
        "rmdir_async",
        "link_sync",
        "link_async",
        "symlink_sync",
        "symlink_async",
        "read_link_sync",
        "read_link_async",
        "truncate_sync",
        "truncate_async",
        "utime_sync",
        "utime_async",
        "lutime_sync",
        "lutime_async",
        "exists_sync",
        "exists_async",
    ]);
    assert_eq!(&required - &calls, BTreeSet::new());
    assert!(
        !calls.contains("cwd"),
        "NimbusFS owns cwd instead of delegating to backend process cwd"
    );
    assert!(
        !calls.contains("chdir"),
        "NimbusFS chdir must compose over stat without mutating backend process cwd"
    );

    let backend_rc: deno_fs::FileSystemRc = MaybeArc::new(spy);
    let _ = NimbusFs::with_backend_rc(backend_rc, root.path());
}
