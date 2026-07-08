//! Whole-object reads, stat, and directory listing.
//!
//! Serves bytes and metadata out of the object byte and manifest planes:
//! eager whole-object reads, `stat`, directory existence and enumeration, and
//! the in-isolate [`ObjectFile`] reader path (which leans on `range.rs` for its
//! bounded windows).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use bytes::Bytes;
use deno_fs::{FsDirEntry, FsReadDir};
use deno_io::fs::{File, FsResult, FsStat};
use nimbus_blob::BlobHash;
use nimbus_storage::{ObjectBlobLayout, ObjectManifest};

use crate::bridge::block_on_byte_plane;

use super::{
    OBJECT_FS_LIST_LIMIT, ObjectFile, ObjectFileState, ObjectReadDir, ObjectRwBackend, core_error,
    key_for_path, normalize_path, prefix_for_directory,
};

impl ObjectRwBackend {
    pub fn read_path(&self, path: impl AsRef<Path>) -> FsResult<Bytes> {
        let path = normalize_path(path.as_ref())?;
        self.read_key(&key_for_path(&path)?)
    }

    fn read_key(&self, key: &str) -> FsResult<Bytes> {
        let manifest = self
            .manifests
            .get_object_manifest(&self.bucket, key)
            .map_err(core_error)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("object {key}")))?;
        self.read_manifest(&manifest)
    }

    pub(super) fn read_manifest(&self, manifest: &ObjectManifest) -> FsResult<Bytes> {
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

    fn get_blob(&self, hash: &BlobHash) -> FsResult<Bytes> {
        let blobs = self.blobs.clone();
        let hash = *hash;
        block_on_byte_plane(async move {
            let bytes = blobs.get(&hash).await.map_err(core_error)?;
            Ok(bytes)
        })
    }

    pub(super) fn manifest_for_path(&self, path: &Path) -> FsResult<Option<ObjectManifest>> {
        if path == Path::new("/") {
            return Ok(None);
        }
        let key = key_for_path(path)?;
        Ok(self
            .manifests
            .get_object_manifest(&self.bucket, &key)
            .map_err(core_error)?)
    }

    pub(super) fn list_prefix(&self, prefix: &str, limit: usize) -> FsResult<Vec<ObjectManifest>> {
        Ok(self
            .manifests
            .list_object_manifests(&self.bucket, prefix, limit)
            .map_err(core_error)?)
    }

    pub(super) fn stat_path(&self, path: &Path) -> FsResult<FsStat> {
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

    pub(super) fn directory_exists(&self, path: &Path) -> FsResult<bool> {
        if path == Path::new("/") {
            return Ok(true);
        }
        if self.directories()?.contains(path) {
            return Ok(true);
        }
        let prefix = prefix_for_directory(path)?;
        Ok(!self.list_prefix(&prefix, 1)?.is_empty())
    }

    pub(super) fn read_dir_entries(&self, path: &Path) -> FsResult<Vec<FsDirEntry>> {
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

    /// Builds a read-only [`ObjectFile`] handle for `path`. Opening never
    /// transfers a body byte: the handle carries only the manifest, and each
    /// read serves its window through the bounded `get_range` path in
    /// `range.rs`.
    pub(super) fn open_reader(&self, path: PathBuf, key: String) -> FsResult<Rc<dyn File>> {
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
}

impl ObjectFile {
    pub(super) fn read_current(self: Rc<Self>, buf: &mut [u8]) -> FsResult<usize> {
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

    pub(super) fn read_all_current(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
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

    pub(super) fn seek_to(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
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

    pub(super) fn stat_current(self: Rc<Self>) -> FsResult<FsStat> {
        let len = {
            let state = self.state.lock().unwrap();
            match &*state {
                ObjectFileState::Reader(manifest) => manifest.size,
                ObjectFileState::Writer(bytes) => bytes.len() as u64,
            }
        };
        Ok(stat_file(len, 0o644))
    }

    pub(super) fn read_at(self: Rc<Self>, buf: &mut [u8], position: u64) -> FsResult<usize> {
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
}

#[async_trait::async_trait(?Send)]
impl FsReadDir for ObjectReadDir {
    async fn next(&self) -> FsResult<Option<FsDirEntry>> {
        Ok(self.entries.lock().unwrap().pop())
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
