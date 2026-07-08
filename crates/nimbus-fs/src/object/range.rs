//! Bounded, windowed reads over the object byte plane.
//!
//! `get_range`-backed helpers that transfer only the bytes a read actually
//! spans, shared by the in-isolate [`ObjectFile`] reader and the external FUSE
//! face. Opening an object never transfers a body byte; only these helpers do,
//! and only the bytes a read requests.

use std::io;
use std::ops::Range;
use std::path::Path;

use bytes::Bytes;
use deno_io::fs::FsResult;
use nimbus_blob::BlobHash;
use nimbus_storage::{ObjectBlobLayout, ObjectManifest};

use crate::bridge::block_on_byte_plane;

use super::{ObjectFile, ObjectRwBackend, core_error, key_for_path, normalize_path};

impl ObjectRwBackend {
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

    pub(super) fn read_manifest_range(
        &self,
        manifest: &ObjectManifest,
        range: Range<u64>,
    ) -> FsResult<Bytes> {
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
}

impl ObjectFile {
    /// Serves `buf.len()` bytes starting at `start` out of `manifest` through
    /// bounded `get_range` windows (shared with the external FUSE face via
    /// `ObjectRwBackend::read_manifest_range` — no duplicated window math).
    /// Never transfers more than the manifest's remaining size.
    pub(super) fn read_window(
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
