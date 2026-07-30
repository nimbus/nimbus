//! Object creation, mutation, and write sessions.
//!
//! Takes bytes in and mutates the namespace: commits (blob put + manifest
//! write), directory creation and removal, copy, the agent/FUSE write
//! sessions, and the in-isolate [`ObjectFile`] writer path. Every published
//! byte flows through `commit_key`; uncommitted writes stay invisible.

use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use bytes::Bytes;
use deno_fs::OpenOptions;
use deno_io::fs::{File, FsResult};
use deno_permissions::CheckedPath;
use nimbus_blob::BlobHash;
use nimbus_storage::{ObjectManifest, ObjectManifestAttributes};

use crate::ObjectUnsupportedOperation;
use crate::bridge::block_on_byte_plane;

use super::{
    ExternalFuseObjectMount, ExternalFuseWrite, OBJECT_FS_LIST_LIMIT, ObjectFile, ObjectFileState,
    ObjectRwBackend, ObjectWriteSession, core_error, key_for_path, normalize_path,
    prefix_for_directory, validate_key,
};

impl ObjectRwBackend {
    pub fn begin_agent_write(&self, path: impl AsRef<Path>) -> FsResult<ObjectWriteSession> {
        let path = normalize_path(path.as_ref())?;
        let key = key_for_path(&path)?;
        Ok(ObjectWriteSession {
            backend: self.clone(),
            key,
            data: Vec::new(),
        })
    }

    pub(crate) fn commit_path(&self, path: &Path, bytes: Bytes) -> FsResult<ObjectManifest> {
        let path = normalize_path(path)?;
        let key = key_for_path(&path)?;
        self.commit_key(&key, bytes)
    }

    pub(super) fn commit_key(&self, key: &str, bytes: Bytes) -> FsResult<ObjectManifest> {
        validate_key(key)?;
        let size = bytes.len() as u64;
        let hash = self.put_blob(bytes)?;
        let hash_hex = hash.to_hex();
        let attrs = ObjectManifestAttributes::new(format!("\"{hash_hex}\""), now_millis()?);
        let manifest =
            ObjectManifest::whole(&self.bucket, key, size, hash_hex, attrs).map_err(core_error)?;
        self.manifests.put_manifest(&manifest).map_err(core_error)?;
        self.record_parent_dirs_for_key(key)?;
        Ok(manifest)
    }

    fn put_blob(&self, bytes: Bytes) -> FsResult<BlobHash> {
        let blobs = self.blobs.clone();
        block_on_byte_plane(async move {
            let hash = blobs.put(bytes).await.map_err(core_error)?;
            Ok(hash)
        })
    }

    pub(super) fn create_dir(&self, path: &Path, recursive: bool) -> FsResult<()> {
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

    pub(super) fn remove_path(&self, path: &Path, recursive: bool) -> FsResult<()> {
        let path = normalize_path(path)?;
        if path == Path::new("/") {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if let Some(manifest) = self.manifest_for_path(&path)? {
            self.manifests
                .delete_manifest(&manifest.bucket, &manifest.key)
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
                    .delete_manifest(&manifest.bucket, &manifest.key)
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

    pub(super) fn copy_object(&self, oldpath: &Path, newpath: &Path) -> FsResult<()> {
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

    pub(super) fn write_file(
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

    /// Builds a writable [`ObjectFile`] handle for `path`. Prepares an
    /// in-memory buffer (reading the existing object only for `append`); no
    /// bytes are published until a write commits through `commit_key`.
    pub(super) fn open_writer(
        &self,
        path: PathBuf,
        key: String,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
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
        Ok(Rc::new(ObjectFile {
            backend: self.clone(),
            path,
            key,
            cursor: Mutex::new(cursor),
            state: Mutex::new(ObjectFileState::Writer(data)),
            readable: options.read,
            writable: true,
        }))
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

impl ObjectFile {
    pub(super) fn write_current(self: Rc<Self>, buf: &[u8]) -> FsResult<usize> {
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

    pub(super) fn write_at(self: Rc<Self>, buf: &[u8], position: u64) -> FsResult<usize> {
        let current = *self.cursor.lock().unwrap();
        if position != current {
            return reject_unsupported_value(ObjectUnsupportedOperation::RandomWrite);
        }
        self.write_sync(buf)
    }

    pub(super) fn sync_current(self: Rc<Self>) -> FsResult<()> {
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

    pub(super) fn truncate_current(self: Rc<Self>, len: u64) -> FsResult<()> {
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

fn now_millis() -> FsResult<u64> {
    Ok(nimbus_core::clock::system_now_millis())
}

fn reject_unsupported_value<T>(operation: ObjectUnsupportedOperation) -> FsResult<T> {
    ObjectRwBackend::reject_unsupported(operation)?;
    unreachable!("reject_unsupported always returns an error")
}
