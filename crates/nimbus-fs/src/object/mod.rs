use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use deno_core::{BufMutView, BufView, ResourceHandleFd, WriteOutcome};
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDir, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsError, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};
use nimbus_blob::{BlobHash, BlobStore};
use nimbus_storage::{ObjectBlobLayout, ObjectManifest, ObjectManifestAttributes, ObjectMetaStore};

use crate::ObjectUnsupportedOperation;
use crate::PlatformStdio;
use crate::bridge::block_on_byte_plane;

const OBJECT_FS_LIST_LIMIT: usize = 10_000;

#[derive(Clone)]
pub struct ObjectRwBackend {
    bucket: String,
    blobs: Arc<dyn BlobStore>,
    manifests: Arc<dyn ObjectMetaStore + Send + Sync>,
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
        manifests: Arc<dyn ObjectMetaStore + Send + Sync>,
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

    pub fn begin_agent_write(&self, path: impl AsRef<Path>) -> FsResult<ObjectWriteSession> {
        let path = normalize_path(path.as_ref())?;
        let key = key_for_path(&path)?;
        Ok(ObjectWriteSession {
            backend: self.clone(),
            key,
            data: Vec::new(),
        })
    }

    pub fn read_path(&self, path: impl AsRef<Path>) -> FsResult<Bytes> {
        let path = normalize_path(path.as_ref())?;
        self.read_key(&key_for_path(&path)?)
    }

    pub(crate) fn commit_path(&self, path: &Path, bytes: Bytes) -> FsResult<ObjectManifest> {
        let path = normalize_path(path)?;
        let key = key_for_path(&path)?;
        self.commit_key(&key, bytes)
    }

    fn commit_key(&self, key: &str, bytes: Bytes) -> FsResult<ObjectManifest> {
        validate_key(key)?;
        let size = bytes.len() as u64;
        let hash = self.put_blob(bytes)?;
        let hash_hex = hash.to_hex();
        let attrs = ObjectManifestAttributes::new(format!("\"{hash_hex}\""), now_millis()?);
        let manifest =
            ObjectManifest::whole(&self.bucket, key, size, hash_hex, attrs).map_err(core_error)?;
        self.manifests
            .put_object_manifest(&manifest)
            .map_err(core_error)?;
        self.record_parent_dirs_for_key(key)?;
        Ok(manifest)
    }

    fn read_key(&self, key: &str) -> FsResult<Bytes> {
        let manifest = self
            .manifests
            .get_object_manifest(&self.bucket, key)
            .map_err(core_error)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("object {key}")))?;
        self.read_manifest(&manifest)
    }

    fn read_manifest(&self, manifest: &ObjectManifest) -> FsResult<Bytes> {
        match &manifest.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } => {
                let hash = BlobHash::from_hex(blob_hash).map_err(core_error)?;
                self.get_blob(&hash)
            }
            ObjectBlobLayout::Chunked { chunks } => {
                let mut bytes = Vec::with_capacity(manifest.size as usize);
                for chunk in chunks {
                    let hash = BlobHash::from_hex(&chunk.blob_hash).map_err(core_error)?;
                    let chunk_bytes = self.get_blob(&hash)?;
                    if chunk_bytes.len() as u64 != chunk.len {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "object chunk {} length mismatch: manifest={} actual={}",
                                chunk.blob_hash,
                                chunk.len,
                                chunk_bytes.len()
                            ),
                        )
                        .into());
                    }
                    bytes.extend_from_slice(&chunk_bytes);
                }
                Ok(Bytes::from(bytes))
            }
        }
    }

    fn put_blob(&self, bytes: Bytes) -> FsResult<BlobHash> {
        let blobs = self.blobs.clone();
        block_on_byte_plane(async move {
            let hash = blobs.put(bytes).await.map_err(core_error)?;
            Ok(hash)
        })
    }

    fn get_blob(&self, hash: &BlobHash) -> FsResult<Bytes> {
        let blobs = self.blobs.clone();
        let hash = *hash;
        block_on_byte_plane(async move {
            let bytes = blobs.get(&hash).await.map_err(core_error)?;
            Ok(bytes)
        })
    }

    /// Reads a bounded byte `range` of the blob addressed by `hash` through
    /// `BlobStore::get_range`, instead of materializing the whole blob and
    /// slicing in memory.
    fn get_blob_range(&self, hash: &BlobHash, range: Range<u64>) -> FsResult<Bytes> {
        let blobs = self.blobs.clone();
        let hash = *hash;
        block_on_byte_plane(async move {
            blobs
                .get_range(&hash, range)
                .await
                .map_err(|error| core_error(error).into())
        })
    }

    /// Reads a bounded byte `range` of the object at `path` (used by the
    /// external FUSE face, which previously materialized the entire object
    /// and sliced the requested window in memory — replaced under FCW2 with
    /// `BlobStore::get_range`, which transfers only the requested bytes).
    pub fn read_range(&self, path: impl AsRef<Path>, range: Range<u64>) -> FsResult<Bytes> {
        let path = normalize_path(path.as_ref())?;
        let key = key_for_path(&path)?;
        let manifest = self
            .manifests
            .get_object_manifest(&self.bucket, &key)
            .map_err(core_error)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("object {key}")))?;
        self.read_manifest_range(&manifest, range)
    }

    fn read_manifest_range(&self, manifest: &ObjectManifest, range: Range<u64>) -> FsResult<Bytes> {
        let start = range.start.min(manifest.size);
        let end = range.end.min(manifest.size);
        if start >= end {
            return Ok(Bytes::new());
        }
        match &manifest.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } => {
                let hash = BlobHash::from_hex(blob_hash).map_err(core_error)?;
                self.get_blob_range(&hash, start..end)
            }
            ObjectBlobLayout::Chunked { chunks } => {
                let mut bytes = Vec::with_capacity((end - start) as usize);
                let mut chunk_start = 0u64;
                for chunk in chunks {
                    let chunk_end = chunk_start + chunk.len;
                    if start < chunk_end && end > chunk_start {
                        let local_start = start.saturating_sub(chunk_start);
                        let local_end = end.min(chunk_end) - chunk_start;
                        let hash = BlobHash::from_hex(&chunk.blob_hash).map_err(core_error)?;
                        let window = self.get_blob_range(&hash, local_start..local_end)?;
                        bytes.extend_from_slice(&window);
                    }
                    chunk_start = chunk_end;
                    if chunk_start >= end {
                        break;
                    }
                }
                Ok(Bytes::from(bytes))
            }
        }
    }

    fn manifest_for_path(&self, path: &Path) -> FsResult<Option<ObjectManifest>> {
        if path == Path::new("/") {
            return Ok(None);
        }
        let key = key_for_path(path)?;
        Ok(self
            .manifests
            .get_object_manifest(&self.bucket, &key)
            .map_err(core_error)?)
    }

    fn list_prefix(&self, prefix: &str, limit: usize) -> FsResult<Vec<ObjectManifest>> {
        Ok(self
            .manifests
            .list_object_manifests(&self.bucket, prefix, limit)
            .map_err(core_error)?)
    }

    fn stat_path(&self, path: &Path) -> FsResult<FsStat> {
        let path = normalize_path(path)?;
        if path == Path::new("/") {
            return Ok(stat_dir(0o755));
        }
        if let Some(manifest) = self.manifest_for_path(&path)? {
            return Ok(stat_file(manifest.size, 0o644));
        }
        if self.directory_exists(&path)? {
            return Ok(stat_dir(0o755));
        }
        Err(io::ErrorKind::NotFound.into())
    }

    fn directory_exists(&self, path: &Path) -> FsResult<bool> {
        if path == Path::new("/") {
            return Ok(true);
        }
        if self.directories()?.contains(path) {
            return Ok(true);
        }
        let prefix = prefix_for_directory(path)?;
        Ok(!self.list_prefix(&prefix, 1)?.is_empty())
    }

    fn read_dir_entries(&self, path: &Path) -> FsResult<Vec<FsDirEntry>> {
        let path = normalize_path(path)?;
        if self.manifest_for_path(&path)?.is_some() {
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "path is an object").into());
        }
        if !self.directory_exists(&path)? {
            return Err(io::ErrorKind::NotFound.into());
        }
        let prefix = if path == Path::new("/") {
            String::new()
        } else {
            prefix_for_directory(&path)?
        };
        let mut entries = BTreeMap::<String, FsDirEntry>::new();
        for manifest in self.list_prefix(&prefix, OBJECT_FS_LIST_LIMIT)? {
            let Some(relative) = manifest.key.strip_prefix(&prefix) else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }
            if let Some((dir, _)) = relative.split_once('/') {
                entries
                    .entry(dir.to_string())
                    .or_insert_with(|| FsDirEntry {
                        name: dir.to_string(),
                        is_file: false,
                        is_directory: true,
                        is_symlink: false,
                    });
            } else {
                entries.insert(
                    relative.to_string(),
                    FsDirEntry {
                        name: relative.to_string(),
                        is_file: true,
                        is_directory: false,
                        is_symlink: false,
                    },
                );
            }
        }
        for dir in self.directories()?.iter() {
            if dir == &path {
                continue;
            }
            let Ok(relative) = dir.strip_prefix(&path) else {
                continue;
            };
            let mut components = relative.components();
            let Some(Component::Normal(name)) = components.next() else {
                continue;
            };
            if components.next().is_some() {
                continue;
            }
            let name = name.to_string_lossy().into_owned();
            entries.entry(name.clone()).or_insert(FsDirEntry {
                name,
                is_file: false,
                is_directory: true,
                is_symlink: false,
            });
        }
        Ok(entries.into_values().collect())
    }

    fn create_dir(&self, path: &Path, recursive: bool) -> FsResult<()> {
        let path = normalize_path(path)?;
        if path == Path::new("/") {
            return Ok(());
        }
        if self.manifest_for_path(&path)?.is_some() {
            return Err(
                io::Error::new(io::ErrorKind::AlreadyExists, "object exists at path").into(),
            );
        }
        if !recursive {
            let parent = path.parent().unwrap_or_else(|| Path::new("/"));
            if !self.directory_exists(parent)? {
                return Err(
                    io::Error::new(io::ErrorKind::NotFound, "parent directory not found").into(),
                );
            }
        }
        let mut dirs = self.directories()?;
        if recursive {
            insert_parent_dirs(&mut dirs, &path);
        }
        dirs.insert(path);
        Ok(())
    }

    fn remove_path(&self, path: &Path, recursive: bool) -> FsResult<()> {
        let path = normalize_path(path)?;
        if path == Path::new("/") {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if let Some(manifest) = self.manifest_for_path(&path)? {
            self.manifests
                .delete_object_manifest(&manifest.bucket, &manifest.key)
                .map_err(core_error)?;
            return Ok(());
        }
        if !self.directory_exists(&path)? {
            return Err(io::ErrorKind::NotFound.into());
        }
        let prefix = prefix_for_directory(&path)?;
        let manifests = self.list_prefix(&prefix, OBJECT_FS_LIST_LIMIT)?;
        if !recursive && !manifests.is_empty() {
            return Err(io::ErrorKind::DirectoryNotEmpty.into());
        }
        if recursive {
            for manifest in manifests {
                self.manifests
                    .delete_object_manifest(&manifest.bucket, &manifest.key)
                    .map_err(core_error)?;
            }
        }
        let mut dirs = self.directories()?;
        let removing: Vec<_> = dirs
            .iter()
            .filter(|dir| *dir == &path || dir.starts_with(&path))
            .cloned()
            .collect();
        for dir in removing {
            dirs.remove(&dir);
        }
        Ok(())
    }

    fn copy_object(&self, oldpath: &Path, newpath: &Path) -> FsResult<()> {
        let bytes = self.read_path(oldpath)?;
        self.commit_path(newpath, bytes)?;
        Ok(())
    }

    fn record_parent_dirs_for_key(&self, key: &str) -> FsResult<()> {
        let path = path_for_key(key);
        let mut dirs = self.directories()?;
        insert_parent_dirs(&mut dirs, &path);
        Ok(())
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

impl ObjectWriteSession {
    pub fn write_sequential(&mut self, offset: u64, bytes: &[u8]) -> FsResult<usize> {
        if offset != self.data.len() as u64 {
            return reject_unsupported_value(ObjectUnsupportedOperation::RandomWrite);
        }
        self.data.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    pub fn len(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn commit(&self) -> FsResult<ObjectManifest> {
        self.backend
            .commit_key(&self.key, Bytes::copy_from_slice(&self.data))
    }
}

impl ExternalFuseObjectMount {
    pub fn new(backend: ObjectRwBackend) -> Self {
        Self { backend }
    }

    pub fn read(&self, path: impl AsRef<Path>, offset: u64, size: u32) -> FsResult<Vec<u8>> {
        let end = offset.saturating_add(size as u64);
        let bytes = self.backend.read_range(path, offset..end)?;
        Ok(bytes.to_vec())
    }

    pub fn begin_write(&self, path: impl AsRef<Path>) -> FsResult<ExternalFuseWrite> {
        Ok(ExternalFuseWrite {
            session: self.backend.begin_agent_write(path)?,
        })
    }
}

impl std::fmt::Debug for ExternalFuseObjectMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalFuseObjectMount")
            .field("bucket", &self.backend.bucket())
            .finish_non_exhaustive()
    }
}

impl ExternalFuseWrite {
    pub fn write_at(&mut self, offset: u64, bytes: &[u8]) -> FsResult<usize> {
        self.session.write_sequential(offset, bytes)
    }

    pub fn flush(&self) -> FsResult<ObjectManifest> {
        self.session.commit()
    }

    pub fn len(&self) -> u64 {
        self.session.len()
    }

    pub fn is_empty(&self) -> bool {
        self.session.is_empty()
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
            if options.create_new && self.manifest_for_path(&path)?.is_some() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            let existing = self.manifest_for_path(&path)?;
            if existing.is_none() && !(options.create || options.create_new) {
                return Err(io::ErrorKind::NotFound.into());
            }
            if existing.is_some() && !options.truncate && !options.append {
                return reject_unsupported_value(ObjectUnsupportedOperation::RandomWrite);
            }
            let mut data = if options.append {
                existing
                    .as_ref()
                    .map(|manifest| self.read_manifest(manifest))
                    .transpose()?
                    .map(|bytes| bytes.to_vec())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let cursor = data.len() as u64;
            if options.truncate && existing.is_some() {
                data.clear();
            }
            return Ok(Rc::new(ObjectFile {
                backend: self.clone(),
                path,
                key,
                cursor: Mutex::new(cursor),
                state: Mutex::new(ObjectFileState::Writer(data)),
                readable: options.read,
                writable: true,
            }));
        }

        let manifest = self
            .manifest_for_path(&path)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("object {key}")))?;
        Ok(Rc::new(ObjectFile {
            backend: self.clone(),
            path,
            key,
            cursor: Mutex::new(0),
            state: Mutex::new(ObjectFileState::Reader(Box::new(manifest))),
            readable: true,
            writable: false,
        }))
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
        if options.create_new && self.manifest_for_path(&normalize_path(path)?)?.is_some() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let existing = self.manifest_for_path(&normalize_path(path)?)?;
        let bytes = if options.append {
            let mut current = existing
                .as_ref()
                .map(|manifest| self.read_manifest(manifest))
                .transpose()?
                .map(|bytes| bytes.to_vec())
                .unwrap_or_default();
            current.extend_from_slice(data);
            Bytes::from(current)
        } else {
            if existing.is_some() && !options.truncate {
                return ObjectRwBackend::reject_unsupported(
                    ObjectUnsupportedOperation::RandomWrite,
                );
            }
            if existing.is_none() && !(options.create || options.create_new) {
                return Err(io::ErrorKind::NotFound.into());
            }
            Bytes::copy_from_slice(data)
        };
        self.commit_path(path, bytes)?;
        Ok(())
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
impl FsReadDir for ObjectReadDir {
    async fn next(&self) -> FsResult<Option<FsDirEntry>> {
        Ok(self.entries.lock().unwrap().pop())
    }
}

impl ObjectFile {
    /// Serves `buf.len()` bytes starting at `start` out of `manifest` through
    /// bounded `get_range` windows (shared with the external FUSE face via
    /// `ObjectRwBackend::read_manifest_range` — no duplicated window math).
    /// Never transfers more than the manifest's remaining size.
    fn read_window(
        &self,
        manifest: &ObjectManifest,
        start: u64,
        buf: &mut [u8],
    ) -> FsResult<usize> {
        if start >= manifest.size || buf.is_empty() {
            return Ok(0);
        }
        let end = start.saturating_add(buf.len() as u64).min(manifest.size);
        let window = self.backend.read_manifest_range(manifest, start..end)?;
        let nread = window.len();
        buf[..nread].copy_from_slice(&window);
        Ok(nread)
    }
}

#[async_trait::async_trait(?Send)]
impl File for ObjectFile {
    fn maybe_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn read_sync(self: Rc<Self>, buf: &mut [u8]) -> FsResult<usize> {
        if !self.readable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut cursor = self.cursor.lock().unwrap();
        let state = self.state.lock().unwrap();
        let nread = match &*state {
            ObjectFileState::Reader(manifest) => self.read_window(manifest, *cursor, buf)?,
            ObjectFileState::Writer(bytes) => {
                let start = usize::try_from(*cursor).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "cursor overflows usize")
                })?;
                if start >= bytes.len() {
                    0
                } else {
                    let nread = (bytes.len() - start).min(buf.len());
                    buf[..nread].copy_from_slice(&bytes[start..start + nread]);
                    nread
                }
            }
        };
        drop(state);
        *cursor += nread as u64;
        Ok(nread)
    }

    async fn read_byob(self: Rc<Self>, mut buf: BufMutView) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_sync(&mut buf)?;
        Ok((nread, buf))
    }

    fn write_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<usize> {
        if !self.writable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let commit = {
            let mut cursor = self.cursor.lock().unwrap();
            let mut state = self.state.lock().unwrap();
            let ObjectFileState::Writer(bytes) = &mut *state else {
                return Err(io::ErrorKind::PermissionDenied.into());
            };
            if *cursor != bytes.len() as u64 {
                return reject_unsupported_value(ObjectUnsupportedOperation::RandomWrite);
            }
            bytes.extend_from_slice(buf);
            *cursor = bytes.len() as u64;
            Bytes::copy_from_slice(bytes)
        };
        self.backend.commit_key(&self.key, commit)?;
        Ok(buf.len())
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
        if !self.readable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let cursor = *self.cursor.lock().unwrap();
        let state = self.state.lock().unwrap();
        match &*state {
            ObjectFileState::Reader(manifest) => {
                if cursor >= manifest.size {
                    return Ok(Cow::Owned(Vec::new()));
                }
                let bytes = self
                    .backend
                    .read_manifest_range(manifest, cursor..manifest.size)?;
                Ok(Cow::Owned(bytes.to_vec()))
            }
            ObjectFileState::Writer(bytes) => {
                let start = usize::try_from(cursor).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "cursor overflows usize")
                })?;
                if start >= bytes.len() {
                    return Ok(Cow::Owned(Vec::new()));
                }
                Ok(Cow::Owned(bytes[start..].to_vec()))
            }
        }
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
        let len = {
            let state = self.state.lock().unwrap();
            match &*state {
                ObjectFileState::Reader(manifest) => manifest.size,
                ObjectFileState::Writer(bytes) => bytes.len() as u64,
            }
        };
        let current = *self.cursor.lock().unwrap();
        let next = match pos {
            io::SeekFrom::Start(pos) => pos as i128,
            io::SeekFrom::End(offset) => len as i128 + offset as i128,
            io::SeekFrom::Current(offset) => current as i128 + offset as i128,
        };
        if next < 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let next = u64::try_from(next)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "seek overflows u64"))?;
        *self.cursor.lock().unwrap() = next;
        Ok(next)
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
        let commit = {
            let state = self.state.lock().unwrap();
            match &*state {
                ObjectFileState::Reader(_) => return Ok(()),
                ObjectFileState::Writer(bytes) => Bytes::copy_from_slice(bytes),
            }
        };
        self.backend.commit_key(&self.key, commit)?;
        Ok(())
    }

    async fn sync_async(self: Rc<Self>) -> FsResult<()> {
        self.sync_sync()
    }

    fn stat_sync(self: Rc<Self>) -> FsResult<FsStat> {
        let len = {
            let state = self.state.lock().unwrap();
            match &*state {
                ObjectFileState::Reader(manifest) => manifest.size,
                ObjectFileState::Writer(bytes) => bytes.len() as u64,
            }
        };
        Ok(stat_file(len, 0o644))
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
        if !self.writable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if len != 0 {
            return ObjectRwBackend::reject_unsupported(ObjectUnsupportedOperation::RandomWrite);
        }
        {
            let mut cursor = self.cursor.lock().unwrap();
            let mut state = self.state.lock().unwrap();
            let ObjectFileState::Writer(bytes) = &mut *state else {
                return Err(io::ErrorKind::PermissionDenied.into());
            };
            bytes.clear();
            *cursor = 0;
        }
        self.backend.commit_key(&self.key, Bytes::new())?;
        Ok(())
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
        let state = self.state.lock().unwrap();
        match &*state {
            ObjectFileState::Reader(manifest) => self.read_window(manifest, position, buf),
            ObjectFileState::Writer(bytes) if self.readable => {
                let start = usize::try_from(position).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "position overflows usize")
                })?;
                if start >= bytes.len() {
                    return Ok(0);
                }
                let nread = (bytes.len() - start).min(buf.len());
                buf[..nread].copy_from_slice(&bytes[start..start + nread]);
                Ok(nread)
            }
            ObjectFileState::Writer(_) => Err(io::ErrorKind::PermissionDenied.into()),
        }
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
        let current = *self.cursor.lock().unwrap();
        if position != current {
            return reject_unsupported_value(ObjectUnsupportedOperation::RandomWrite);
        }
        self.write_sync(buf)
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

fn path_for_key(key: &str) -> PathBuf {
    let mut path = PathBuf::from("/");
    for part in key.split('/') {
        if !part.is_empty() {
            path.push(part);
        }
    }
    path
}

fn prefix_for_directory(path: &Path) -> FsResult<String> {
    let path = normalize_path(path)?;
    if path == Path::new("/") {
        return Ok(String::new());
    }
    Ok(format!("{}/", key_for_path(&path)?))
}

fn insert_parent_dirs(dirs: &mut BTreeSet<PathBuf>, path: &Path) {
    dirs.insert(PathBuf::from("/"));
    let mut current = PathBuf::from("/");
    for component in path.parent().unwrap_or_else(|| Path::new("/")).components() {
        if let Component::Normal(part) = component {
            current.push(part);
            dirs.insert(current.clone());
        }
    }
}

fn stat_file(size: u64, mode: u32) -> FsStat {
    FsStat {
        is_file: true,
        is_directory: false,
        is_symlink: false,
        size,
        mtime: None,
        atime: None,
        birthtime: None,
        ctime: None,
        dev: 0,
        ino: None,
        mode,
        nlink: Some(1),
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 4096,
        blocks: Some(size.div_ceil(512)),
        is_block_device: false,
        is_char_device: false,
        is_fifo: false,
        is_socket: false,
    }
}

fn stat_dir(mode: u32) -> FsStat {
    FsStat {
        is_file: false,
        is_directory: true,
        is_symlink: false,
        size: 0,
        mtime: None,
        atime: None,
        birthtime: None,
        ctime: None,
        dev: 0,
        ino: None,
        mode,
        nlink: Some(1),
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 4096,
        blocks: Some(0),
        is_block_device: false,
        is_char_device: false,
        is_fifo: false,
        is_socket: false,
    }
}

fn now_millis() -> FsResult<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::other(format!("system clock before epoch: {error}")))?;
    u64::try_from(duration.as_millis())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "timestamp overflows u64").into())
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

fn reject_unsupported_value<T>(operation: ObjectUnsupportedOperation) -> FsResult<T> {
    ObjectRwBackend::reject_unsupported(operation)?;
    unreachable!("reject_unsupported always returns an error")
}
