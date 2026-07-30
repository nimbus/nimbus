//! Object-storage-backed `FileSystem`: the composition root.
//!
//! `ObjectRwBackend` maps a POSIX-shaped filesystem onto the object byte plane
//! (`BlobStore`) and manifest plane ([`ObjectManifestStore`]). This module owns
//! the shared vocabulary — the type definitions, the path algebra, and the
//! trait dispatch — and routes the actual work to concept-owned children:
//!
//! - [`manifests`]: the manifest-plane capability and its fencing contract.
//! - [`read`]: whole-object reads, `stat`, directory listing, the reader path.
//! - [`write`]: commits, directory mutation, copy, write sessions, the writer.
//! - [`range`]: bounded `get_range` windows shared by both faces.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use deno_core::{BufMutView, BufView, ResourceHandleFd, WriteOutcome};
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsError, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};
use nimbus_blob::{BlobHash, BlobStore};
use nimbus_storage::{ObjectManifest, ObjectManifestAttributes};

use crate::ObjectUnsupportedOperation;
use crate::PlatformStdio;

mod manifests;
mod range;
mod read;
mod write;

pub use manifests::ObjectManifestStore;

const OBJECT_FS_LIST_LIMIT: usize = 10_000;

#[derive(Clone)]
pub struct ObjectRwBackend {
    bucket: String,
    blobs: Arc<dyn BlobStore>,
    manifests: Arc<dyn ObjectManifestStore>,
    directories: Arc<Mutex<BTreeSet<PathBuf>>>,
}

pub struct ObjectWriteSession {
    backend: ObjectRwBackend,
    key: String,
    data: Vec<u8>,
}

#[derive(Clone)]
pub struct ExternalFuseObjectMount {
    backend: ObjectRwBackend,
}

pub struct ExternalFuseWrite {
    session: ObjectWriteSession,
}

#[derive(Debug, Clone)]
pub struct ObjectReadDir {
    entries: Arc<Mutex<Vec<FsDirEntry>>>,
}

#[derive(Debug)]
struct ObjectFile {
    backend: ObjectRwBackend,
    path: PathBuf,
    key: String,
    cursor: Mutex<u64>,
    state: Mutex<ObjectFileState>,
    readable: bool,
    writable: bool,
}

#[derive(Debug)]
enum ObjectFileState {
    /// Holds only the manifest — chunk hashes and lengths, no blob bytes.
    /// Each read serves its window through `ObjectRwBackend::read_manifest_range`,
    /// which issues one `BlobStore::get_range` per overlapping chunk. Opening
    /// a file never transfers a body byte; only reads do, and only the bytes
    /// a read actually spans.
    Reader(Box<ObjectManifest>),
    Writer(Vec<u8>),
}

impl ObjectRwBackend {
    pub fn new(
        bucket: impl Into<String>,
        blobs: Arc<dyn BlobStore>,
        manifests: Arc<dyn ObjectManifestStore>,
    ) -> FsResult<Self> {
        let bucket = bucket.into();
        validate_bucket(&bucket)?;
        let mut directories = BTreeSet::new();
        directories.insert(PathBuf::from("/"));
        Ok(Self {
            bucket,
            blobs,
            manifests,
            directories: Arc::new(Mutex::new(directories)),
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn unsupported_operations() -> &'static [ObjectUnsupportedOperation] {
        &[
            ObjectUnsupportedOperation::RandomWrite,
            ObjectUnsupportedOperation::Hardlink,
            ObjectUnsupportedOperation::Symlink,
            ObjectUnsupportedOperation::MutableOwnership,
            ObjectUnsupportedOperation::DirectoryRename,
        ]
    }

    pub fn reject_unsupported(operation: ObjectUnsupportedOperation) -> FsResult<()> {
        let label = match operation {
            ObjectUnsupportedOperation::RandomWrite => "random write",
            ObjectUnsupportedOperation::Hardlink => "hardlink",
            ObjectUnsupportedOperation::Symlink => "symlink",
            ObjectUnsupportedOperation::MutableOwnership => "mutable ownership",
            ObjectUnsupportedOperation::DirectoryRename => "directory rename",
        };
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("object-store backend unsupported POSIX operation: {label}"),
        )
        .into())
    }

    fn directories(&self) -> FsResult<std::sync::MutexGuard<'_, BTreeSet<PathBuf>>> {
        self.directories
            .lock()
            .map_err(|_| io::Error::other("ObjectRwBackend directory lock poisoned").into())
    }
}

impl std::fmt::Debug for ObjectRwBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectRwBackend")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ExternalFuseObjectMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalFuseObjectMount")
            .field("bucket", &self.backend.bucket())
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for ObjectRwBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/"))
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/tmp"))
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        if self.stat_sync(path)?.is_directory {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotADirectory, "path is not a directory").into())
        }
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        Ok(0)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        let path = normalize_path(path)?;
        if path == Path::new("/") {
            return Err(io::Error::new(io::ErrorKind::IsADirectory, "path is a directory").into());
        }
        let key = key_for_path(&path)?;
        if options.write
            || options.create
            || options.truncate
            || options.append
            || options.create_new
        {
            return self.open_writer(path, key, options);
        }
        self.open_reader(path, key)
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.open_sync(&path.as_checked_path(), options)
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.create_dir(path, recursive)
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.mkdir_sync(&path.as_checked_path(), recursive, mode)
    }

    #[cfg(unix)]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: i32) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        self.chmod_sync(&path.as_checked_path(), mode)
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.chown_sync(&path.as_checked_path(), uid, gid)
    }

    // deno's lchmod keeps mode: u32 on every target while chmod is
    // platform-split to i32 off unix (interface.rs:186-209), so the
    // forward casts there.
    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        #[cfg(unix)]
        {
            self.chmod_sync(path, mode)
        }
        #[cfg(not(unix))]
        {
            self.chmod_sync(path, mode as i32)
        }
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.lchmod_sync(&path.as_checked_path(), mode)
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.chown_sync(path, uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.chown_sync(&path.as_checked_path(), uid, gid)
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        self.remove_path(path, recursive)
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.remove_sync(&path.as_checked_path(), recursive)
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.copy_object(oldpath, newpath)
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.copy_file_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.copy_file_sync(path, new_path)
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        self.copy_file_sync(&path.as_checked_path(), &new_path.as_checked_path())
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.stat_path(path)
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.stat_sync(path)
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn statfs_sync(&self, _path: &CheckedPath<'_>, _bigint: bool) -> FsResult<FsStatFs> {
        Ok(FsStatFs {
            typ: 0x4f424a46,
            bsize: 4096,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            files: self.list_prefix("", OBJECT_FS_LIST_LIMIT)?.len() as u64,
            ffree: 0,
        })
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        self.statfs_sync(&path.as_checked_path(), bigint)
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let path = normalize_path(path)?;
        self.stat_path(&path)?;
        Ok(path)
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.realpath_sync(&path.as_checked_path())
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.read_dir_entries(path)
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        let entries = self.read_dir_sync(&path.as_checked_path())?;
        Ok(MaybeArc::new(ObjectReadDir {
            entries: Arc::new(Mutex::new(entries)),
        }))
    }

    fn rename_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::DirectoryRename)
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.rename_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        self.remove_path(path, false)
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        self.rmdir_sync(&path.as_checked_path())
    }

    fn link_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::Hardlink)
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.link_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn symlink_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::Symlink)
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.symlink_sync(
            &oldpath.as_checked_path(),
            &newpath.as_checked_path(),
            file_type,
        )
    }

    fn read_link_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        Err(FsError::NotSupported)
    }

    async fn read_link_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        Err(FsError::NotSupported)
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        if len != 0 {
            return ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::RandomWrite);
        }
        self.commit_path(path, Bytes::new())?;
        Ok(())
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        self.truncate_sync(&path.as_checked_path(), len)
    }

    fn utime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    async fn utime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(
            &path.as_checked_path(),
            atime_secs,
            atime_nanos,
            mtime_secs,
            mtime_nanos,
        )
    }

    fn lutime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    async fn lutime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(
            &path.as_checked_path(),
            atime_secs,
            atime_nanos,
            mtime_secs,
            mtime_nanos,
        )
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        self.stat_sync(path).is_ok()
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        Ok(self.exists_sync(&path.as_checked_path()))
    }

    fn write_file_sync(
        &self,
        path: &CheckedPath<'_>,
        options: OpenOptions,
        data: &[u8],
    ) -> FsResult<()> {
        self.write_file(path, options, data)
    }

    async fn write_file_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
        data: Box<[u8]>,
    ) -> FsResult<()> {
        self.write_file_sync(&path.as_checked_path(), options, &data)
    }

    fn read_file_sync(
        &self,
        path: &CheckedPath<'_>,
        _options: OpenOptions,
    ) -> FsResult<Cow<'static, [u8]>> {
        Ok(Cow::Owned(self.read_path(path)?.to_vec()))
    }

    async fn read_file_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Cow<'static, [u8]>> {
        self.read_file_sync(&path.as_checked_path(), options)
    }
}

#[async_trait::async_trait(?Send)]
impl File for ObjectFile {
    fn maybe_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn read_sync(self: Rc<Self>, buf: &mut [u8]) -> FsResult<usize> {
        self.read_current(buf)
    }

    async fn read_byob(self: Rc<Self>, mut buf: BufMutView) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_sync(&mut buf)?;
        Ok((nread, buf))
    }

    fn write_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<usize> {
        self.write_current(buf)
    }

    async fn write(self: Rc<Self>, view: BufView) -> FsResult<WriteOutcome> {
        let nwritten = self.clone().write_sync(&view)?;
        Ok(WriteOutcome::Partial { nwritten, view })
    }

    fn write_all_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<()> {
        self.write_sync(buf)?;
        Ok(())
    }

    async fn write_all(self: Rc<Self>, buf: BufView) -> FsResult<()> {
        self.write_all_sync(&buf)
    }

    fn read_all_sync(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        self.read_all_current()
    }

    async fn read_all_async(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        self.read_all_sync()
    }

    fn chmod_sync(self: Rc<Self>, _pathmode: u32) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    async fn chmod_async(self: Rc<Self>, mode: u32) -> FsResult<()> {
        self.chmod_sync(mode)
    }

    fn chown_sync(self: Rc<Self>, _uid: Option<u32>, _gid: Option<u32>) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    async fn chown_async(self: Rc<Self>, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.chown_sync(uid, gid)
    }

    fn seek_sync(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        self.seek_to(pos)
    }

    async fn seek_async(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        self.seek_sync(pos)
    }

    fn datasync_sync(self: Rc<Self>) -> FsResult<()> {
        self.sync_sync()
    }

    async fn datasync_async(self: Rc<Self>) -> FsResult<()> {
        self.datasync_sync()
    }

    fn sync_sync(self: Rc<Self>) -> FsResult<()> {
        self.sync_current()
    }

    async fn sync_async(self: Rc<Self>) -> FsResult<()> {
        self.sync_sync()
    }

    fn stat_sync(self: Rc<Self>) -> FsResult<FsStat> {
        self.stat_current()
    }

    async fn stat_async(self: Rc<Self>) -> FsResult<FsStat> {
        self.stat_sync()
    }

    fn lock_sync(self: Rc<Self>, _exclusive: bool) -> FsResult<()> {
        Ok(())
    }

    async fn lock_async(self: Rc<Self>, exclusive: bool) -> FsResult<()> {
        self.lock_sync(exclusive)
    }

    fn try_lock_sync(self: Rc<Self>, _exclusive: bool) -> FsResult<bool> {
        Ok(true)
    }

    async fn try_lock_async(self: Rc<Self>, exclusive: bool) -> FsResult<bool> {
        self.try_lock_sync(exclusive)
    }

    fn unlock_sync(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    async fn unlock_async(self: Rc<Self>) -> FsResult<()> {
        self.unlock_sync()
    }

    fn truncate_sync(self: Rc<Self>, len: u64) -> FsResult<()> {
        self.truncate_current(len)
    }

    async fn truncate_async(self: Rc<Self>, len: u64) -> FsResult<()> {
        self.truncate_sync(len)
    }

    fn utime_sync(
        self: Rc<Self>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::MutableOwnership)
    }

    async fn utime_async(
        self: Rc<Self>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    fn read_at_sync(self: Rc<Self>, buf: &mut [u8], position: u64) -> FsResult<usize> {
        self.read_at(buf, position)
    }

    async fn read_at_async(
        self: Rc<Self>,
        mut buf: BufMutView,
        position: u64,
    ) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_at_sync(&mut buf, position)?;
        Ok((nread, buf))
    }

    fn write_at_sync(self: Rc<Self>, buf: &[u8], position: u64) -> FsResult<usize> {
        self.write_at(buf, position)
    }

    fn as_stdio(self: Rc<Self>) -> FsResult<PlatformStdio> {
        Err(FsError::NotSupported)
    }

    fn backing_fd(self: Rc<Self>) -> Option<ResourceHandleFd> {
        None
    }

    fn try_clone_inner(self: Rc<Self>) -> FsResult<Rc<dyn File>> {
        let state = self.state.lock().unwrap();
        let cloned_state = match &*state {
            ObjectFileState::Reader(manifest) => ObjectFileState::Reader(manifest.clone()),
            ObjectFileState::Writer(bytes) => ObjectFileState::Writer(bytes.clone()),
        };
        Ok(Rc::new(ObjectFile {
            backend: self.backend.clone(),
            path: self.path.clone(),
            key: self.key.clone(),
            cursor: Mutex::new(*self.cursor.lock().unwrap()),
            state: Mutex::new(cloned_state),
            readable: self.readable,
            writable: self.writable,
        }))
    }
}

fn validate_bucket(bucket: &str) -> FsResult<()> {
    let hash = BlobHash::of(&[]);
    let attrs = ObjectManifestAttributes::new("\"bucket-probe\"", 1);
    Ok(
        ObjectManifest::whole(bucket, "__nimbus_bucket_probe", 0, hash.to_hex(), attrs)
            .map(drop)
            .map_err(core_error)?,
    )
}

fn validate_key(key: &str) -> FsResult<()> {
    if key.is_empty() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "object key cannot be empty").into(),
        );
    }
    Ok(())
}

fn normalize_path(path: &Path) -> FsResult<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        PathBuf::from("/").join(path)
    };
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_owned()),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "path escapes object filesystem root",
                    )
                    .into());
                }
            }
            Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "platform prefixes are unsupported in object filesystem paths",
                )
                .into());
            }
        }
    }
    let mut normalized = PathBuf::from("/");
    for part in parts {
        normalized.push(part);
    }
    Ok(normalized)
}

fn key_for_path(path: &Path) -> FsResult<String> {
    let path = normalize_path(path)?;
    if path == Path::new("/") {
        return Err(io::Error::new(io::ErrorKind::IsADirectory, "root is not an object").into());
    }
    let key = path
        .strip_prefix("/")
        .unwrap_or(&path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    validate_key(&key)?;
    Ok(key)
}

fn prefix_for_directory(path: &Path) -> FsResult<String> {
    let path = normalize_path(path)?;
    if path == Path::new("/") {
        return Ok(String::new());
    }
    Ok(format!("{}/", key_for_path(&path)?))
}

fn core_error(error: nimbus_core::Error) -> io::Error {
    match error {
        nimbus_core::Error::NotFound(message) => io::Error::new(io::ErrorKind::NotFound, message),
        nimbus_core::Error::InvalidInput(message) => {
            io::Error::new(io::ErrorKind::InvalidInput, message)
        }
        other => io::Error::other(other.to_string()),
    }
}
