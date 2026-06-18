//! Content-addressed store for deployed source packages — the read-artifact
//! behind the console Source view. See the Function Source Visibility plan
//! (FSV2).
//!
//! The digest is the lowercase-hex SHA-256 of the package bytes. [`put`] is
//! idempotent: identical bytes produce the same digest and a single stored
//! object, which is the deduplication mechanism. [`get`] re-hashes and verifies
//! before returning, so a corrupted or tampered object fails closed instead of
//! silently serving the wrong source.
//!
//! The disk implementation is a stopgap behind a digest-keyed trait so the byte
//! plane can swap to `nimbus-blob` later with no change to the `source_packages`
//! record schema — `storageKey` is the indirection point.
//!
//! [`put`]: SourcePackageStore::put
//! [`get`]: SourcePackageStore::get

use std::path::PathBuf;

use nimbus_core::{Error, Result};
use sha2::{Digest, Sha256};

/// Reference to a stored source package: its content digest, the location key
/// recorded in `source_packages.storageKey`, and its packed size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSourcePackage {
    pub digest: String,
    pub storage_key: String,
    pub size_bytes: u64,
}

/// A content-addressed byte store for source packages.
pub trait SourcePackageStore: Send + Sync {
    /// Store `bytes`, returning their content reference. Idempotent: storing the
    /// same bytes twice stores one object and returns the same digest.
    fn put(&self, bytes: &[u8]) -> Result<StoredSourcePackage>;
    /// Fetch the bytes for `digest`, verifying them against it. Fails closed on
    /// a missing object or a digest mismatch.
    fn get(&self, digest: &str) -> Result<Vec<u8>>;
    /// Whether an object for `digest` is already stored.
    fn contains(&self, digest: &str) -> bool;
}

/// Lowercase-hex SHA-256 of `bytes` — the canonical source-package digest.
pub fn source_package_digest(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit(u32::from(b >> 4), 16).expect("nibble is < 16"));
        out.push(char::from_digit(u32::from(b & 0x0f), 16).expect("nibble is < 16"));
    }
    out
}

fn is_hex_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Disk-backed CAS rooted at a directory. Objects are sharded by the first two
/// hex characters of the digest to avoid one enormous flat directory.
pub struct DiskSourcePackageStore {
    root: PathBuf,
}

impl DiskSourcePackageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn object_path(&self, digest: &str) -> PathBuf {
        self.root.join(&digest[0..2]).join(digest)
    }

    fn storage_key(digest: &str) -> String {
        format!("source-packages/{}/{}", &digest[0..2], digest)
    }
}

impl SourcePackageStore for DiskSourcePackageStore {
    fn put(&self, bytes: &[u8]) -> Result<StoredSourcePackage> {
        let digest = source_package_digest(bytes);
        let path = self.object_path(&digest);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io_err("create source-package dir"))?;
            }
            // Write a temp sibling then rename so a concurrent reader never sees
            // a partially written object.
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, bytes).map_err(io_err("write source package"))?;
            std::fs::rename(&tmp, &path).map_err(io_err("commit source package"))?;
        }
        Ok(StoredSourcePackage {
            storage_key: Self::storage_key(&digest),
            size_bytes: bytes.len() as u64,
            digest,
        })
    }

    fn get(&self, digest: &str) -> Result<Vec<u8>> {
        if !is_hex_digest(digest) {
            return Err(Error::Internal(format!(
                "invalid source-package digest: {digest}"
            )));
        }
        let bytes = std::fs::read(self.object_path(digest)).map_err(|error| {
            Error::Internal(format!("source package {digest} not found: {error}"))
        })?;
        let actual = source_package_digest(&bytes);
        if actual != digest {
            return Err(Error::Internal(format!(
                "source package {digest} failed integrity check (stored bytes hash to {actual})"
            )));
        }
        Ok(bytes)
    }

    fn contains(&self, digest: &str) -> bool {
        is_hex_digest(digest) && self.object_path(digest).exists()
    }
}

fn io_err(context: &'static str) -> impl Fn(std::io::Error) -> Error {
    move |error| Error::Internal(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, DiskSourcePackageStore) {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = DiskSourcePackageStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn put_then_get_round_trips_and_reports_size() {
        let (_dir, store) = store();
        let bytes = b"export const list = query({});\n";
        let stored = store.put(bytes).expect("put should succeed");
        assert_eq!(stored.size_bytes, bytes.len() as u64);
        assert_eq!(stored.digest, source_package_digest(bytes));
        assert!(stored.storage_key.ends_with(&stored.digest));
        assert!(store.contains(&stored.digest));
        assert_eq!(
            store.get(&stored.digest).expect("get should succeed"),
            bytes
        );
    }

    #[test]
    fn put_is_idempotent_and_dedupes_identical_bytes() {
        let (dir, store) = store();
        let bytes = b"identical source bytes";
        let first = store.put(bytes).expect("first put");
        let second = store.put(bytes).expect("second put");
        assert_eq!(first.digest, second.digest);
        // Exactly one object on disk for the shared digest.
        let shard = std::fs::read_dir(dir.path().join(&first.digest[0..2]))
            .expect("shard dir should read")
            .filter_map(std::result::Result::ok)
            .count();
        assert_eq!(shard, 1, "identical bytes must store a single object");
    }

    #[test]
    fn distinct_bytes_get_distinct_digests() {
        let (_dir, store) = store();
        let alpha = store.put(b"alpha").expect("alpha");
        let beta = store.put(b"beta").expect("beta");
        assert_ne!(alpha.digest, beta.digest);
    }

    #[test]
    fn get_fails_closed_on_tampered_object() {
        let (dir, store) = store();
        let stored = store.put(b"trusted source").expect("put");
        let object = dir.path().join(&stored.digest[0..2]).join(&stored.digest);
        std::fs::write(&object, b"tampered bytes").expect("overwrite stored object");
        let error = store
            .get(&stored.digest)
            .expect_err("tampered read must fail closed");
        assert!(
            format!("{error}").contains("integrity check"),
            "expected integrity failure, got: {error}"
        );
    }

    #[test]
    fn get_missing_digest_errors() {
        let (_dir, store) = store();
        let missing = source_package_digest(b"never stored");
        assert!(!store.contains(&missing));
        assert!(store.get(&missing).is_err());
    }

    #[test]
    fn get_rejects_malformed_digest() {
        let (_dir, store) = store();
        assert!(store.get("not-a-valid-digest").is_err());
        assert!(!store.contains("XYZ"));
        assert!(!store.contains("ABCDEF"));
    }
}
