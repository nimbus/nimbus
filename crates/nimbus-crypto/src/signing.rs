//! Workload/node identity signing seam.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const KEY_FILE_HEADER: &[u8] = b"nimbus-identity-key-v1\n";
const LOCK_SUFFIX: &str = ".lock";
const ROTATING_SUFFIX: &str = ".rotating";

/// Result type for identity signing operations.
pub type SigningResult<T> = Result<T, SigningError>;

/// Signing seam for workload/node identity.
///
/// This is deliberately separate from `LocalKeyProvider`, whose shape is
/// symmetric DEK wrapping. HS1 membership keys and FIPS-backed signers can
/// implement this trait later without changing callers.
pub trait IdentitySigner: Send + Sync {
    fn sign(&self, message: &[u8]) -> SigningResult<IdentitySignature>;
    fn verify(&self, message: &[u8], signature: &IdentitySignature) -> SigningResult<()>;
    fn public_key(&self) -> IdentityPublicKey;
    fn kind(&self) -> IdentitySignerKind;
}

/// A 32-byte Ed25519 identity public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdentityPublicKey {
    bytes: [u8; 32],
}

impl IdentityPublicKey {
    pub fn from_ed25519_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Returns the canonical node-key id: `nk_` plus 32 lowercase SHA-256 hex chars.
    pub fn fingerprint(&self) -> String {
        prefixed_fingerprint("nk_", &self.bytes)
    }

    pub fn to_hex(&self) -> String {
        lower_hex(&self.bytes)
    }
}

impl fmt::Debug for IdentityPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityPublicKey")
            .field("fingerprint", &self.fingerprint())
            .field("public_key", &self.to_hex())
            .finish()
    }
}

/// An identity signature plus the key id that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySignature {
    key_id: String,
    signature: Vec<u8>,
}

impl IdentitySignature {
    pub fn new(key_id: impl Into<String>, signature: Vec<u8>) -> Self {
        Self {
            key_id: key_id.into(),
            signature,
        }
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn signature_bytes(&self) -> &[u8] {
        &self.signature
    }
}

/// Diagnostics-safe signer descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentitySignerKind {
    FileBacked { path: String, fingerprint: String },
}

impl fmt::Display for IdentitySignerKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileBacked { path, fingerprint } => {
                write!(formatter, "file-backed:{path} ({fingerprint})")
            }
        }
    }
}

/// File-backed signer open mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    Existing,
    GenerateIfAbsent,
}

/// Local Ed25519 identity signer backed by a single 0600 key file.
pub struct FileBackedIdentitySigner {
    path: PathBuf,
    key_material: Zeroizing<Vec<u8>>,
    public_key: IdentityPublicKey,
}

impl FileBackedIdentitySigner {
    pub fn open(path: impl Into<PathBuf>, mode: OpenMode) -> SigningResult<Self> {
        let path = path.into();
        // Publication is serialized across processes by an advisory lock on
        // `<path>.lock`: concurrent GenerateIfAbsent opens cannot both
        // generate (the loser re-loads the winner's key), and an open cannot
        // discard the stage file out from under an in-flight rotation.
        let _lock = acquire_publish_lock(&path)?;
        discard_uncommitted_rotation_stage(&path)?;
        match load_existing(&path) {
            Ok(signer) => Ok(signer),
            Err(SigningError::KeyFileMissing { .. }) if mode == OpenMode::GenerateIfAbsent => {
                let key_material = generate_key_material()?;
                publish_key_file(&path, &key_material)?;
                Self::from_key_material(path, key_material)
            }
            Err(error) => Err(error),
        }
    }

    /// Rotates the live key atomically.
    ///
    /// If a process crashes after writing `<path>.rotating` but before the
    /// rename, the next `open` discards that uncommitted stage and keeps the
    /// live key. Without a commit marker there is no durable intent to adopt the
    /// staged key, so the live key remains canonical.
    ///
    /// The in-memory key is adopted the moment the rename succeeds — before
    /// the parent-directory fsync — so a failed fsync can never leave this
    /// signer on the old key while the live file (and future opens) hold the
    /// new one. An `Err` from a failed fsync therefore means "the rotation
    /// may not survive power loss", never "sign and verify are split-brained".
    pub fn rotate(&mut self) -> SigningResult<()> {
        // Same cross-process lock as `open` — rotations are serialized and
        // cannot interleave with another publisher's stage/rename.
        let _lock = acquire_publish_lock(&self.path)?;
        let key_material = generate_key_material()?;
        let public_key = public_key_from_pkcs8(&self.path, &key_material)?;
        replace_key_file(&self.path, &key_material)?;
        self.key_material = key_material;
        self.public_key = public_key;
        sync_parent_dir(&self.path)
    }

    fn from_key_material(path: PathBuf, key_material: Zeroizing<Vec<u8>>) -> SigningResult<Self> {
        let public_key = public_key_from_pkcs8(&path, &key_material)?;
        Ok(Self {
            path,
            key_material,
            public_key,
        })
    }
}

impl IdentitySigner for FileBackedIdentitySigner {
    fn sign(&self, message: &[u8]) -> SigningResult<IdentitySignature> {
        let key_id = self.public_key.fingerprint();
        let key_pair = Ed25519KeyPair::from_pkcs8(&self.key_material).map_err(|_| {
            SigningError::MalformedInMemoryKey {
                key_id: key_id.clone(),
            }
        })?;
        Ok(IdentitySignature::new(
            key_id,
            key_pair.sign(message).as_ref().to_vec(),
        ))
    }

    fn verify(&self, message: &[u8], signature: &IdentitySignature) -> SigningResult<()> {
        let expected_key_id = self.public_key.fingerprint();
        if signature.key_id != expected_key_id {
            return Err(SigningError::StaleKey {
                expected_key_id,
                actual_key_id: signature.key_id.clone(),
            });
        }
        UnparsedPublicKey::new(&ED25519, self.public_key.as_bytes())
            .verify(message, signature.signature_bytes())
            .map_err(|_| SigningError::VerificationFailed {
                key_id: signature.key_id.clone(),
            })
    }

    fn public_key(&self) -> IdentityPublicKey {
        self.public_key
    }

    fn kind(&self) -> IdentitySignerKind {
        IdentitySignerKind::FileBacked {
            path: self.path.display().to_string(),
            fingerprint: self.public_key.fingerprint(),
        }
    }
}

impl fmt::Debug for FileBackedIdentitySigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileBackedIdentitySigner")
            .field("kind", &self.kind())
            .field("fingerprint", &self.public_key.fingerprint())
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

/// Errors produced by identity signing operations.
#[derive(Debug)]
pub enum SigningError {
    KeyFileMissing {
        path: PathBuf,
    },
    KeyFileRead {
        path: PathBuf,
        source: io::Error,
    },
    KeyFileWrite {
        path: PathBuf,
        source: io::Error,
    },
    KeyFileSync {
        path: PathBuf,
        source: io::Error,
    },
    KeyFilePermission {
        path: PathBuf,
        mode: u32,
    },
    KeyFileOwner {
        path: PathBuf,
        owner_uid: u32,
        current_uid: u32,
    },
    MalformedKeyFile {
        path: PathBuf,
        reason: &'static str,
    },
    RandomFailed {
        operation: &'static str,
    },
    MalformedInMemoryKey {
        key_id: String,
    },
    StaleKey {
        expected_key_id: String,
        actual_key_id: String,
    },
    VerificationFailed {
        key_id: String,
    },
}

impl fmt::Display for SigningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyFileMissing { path } => {
                write!(
                    formatter,
                    "identity signing key file missing at {}",
                    path.display()
                )
            }
            Self::KeyFileRead { path, source } => {
                write!(
                    formatter,
                    "failed to read identity signing key file {}: {source}",
                    path.display()
                )
            }
            Self::KeyFileWrite { path, source } => {
                write!(
                    formatter,
                    "failed to write identity signing key file {}: {source}",
                    path.display()
                )
            }
            Self::KeyFileSync { path, source } => {
                write!(
                    formatter,
                    "failed to sync identity signing key file {}: {source}",
                    path.display()
                )
            }
            Self::KeyFilePermission { path, mode } => {
                write!(
                    formatter,
                    "identity signing key file {} has insecure permissions {mode:o}; expected 600",
                    path.display()
                )
            }
            Self::KeyFileOwner {
                path,
                owner_uid,
                current_uid,
            } => {
                write!(
                    formatter,
                    "identity key file {} is owned by uid {owner_uid}, not the current uid {current_uid}",
                    path.display()
                )
            }
            Self::MalformedKeyFile { path, reason } => {
                write!(
                    formatter,
                    "identity signing key file {} is malformed: {reason}",
                    path.display()
                )
            }
            Self::RandomFailed { operation } => {
                write!(
                    formatter,
                    "identity signing random generation failed during {operation}"
                )
            }
            Self::MalformedInMemoryKey { key_id } => {
                write!(
                    formatter,
                    "identity signing key {key_id} could not be loaded from redacted memory"
                )
            }
            Self::StaleKey {
                expected_key_id,
                actual_key_id,
            } => {
                write!(
                    formatter,
                    "identity signature key id {actual_key_id} is stale; expected {expected_key_id}"
                )
            }
            Self::VerificationFailed { key_id } => {
                write!(
                    formatter,
                    "identity signature verification failed for key {key_id}"
                )
            }
        }
    }
}

impl std::error::Error for SigningError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::KeyFileRead { source, .. }
            | Self::KeyFileWrite { source, .. }
            | Self::KeyFileSync { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn load_existing(path: &Path) -> SigningResult<FileBackedIdentitySigner> {
    // Open once with no-follow semantics, validate the HANDLE's metadata,
    // and read from the same handle — the permission check and the bytes
    // are tied to one inode, closing the symlink/path-swap TOCTOU window.
    let mut file = open_key_file_no_follow(path)?;
    let metadata = file
        .metadata()
        .map_err(|source| SigningError::KeyFileRead {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(SigningError::MalformedKeyFile {
            path: path.to_path_buf(),
            reason: "identity key path is not a regular file",
        });
    }
    validate_metadata_permissions(path, &metadata)?;
    let mut bytes = Zeroizing::new(Vec::new());
    io::Read::read_to_end(&mut file, &mut bytes).map_err(|source| SigningError::KeyFileRead {
        path: path.to_path_buf(),
        source,
    })?;
    if !bytes.starts_with(KEY_FILE_HEADER) {
        return Err(SigningError::MalformedKeyFile {
            path: path.to_path_buf(),
            reason: "missing Nimbus identity key version tag",
        });
    }
    let key_material = Zeroizing::new(bytes[KEY_FILE_HEADER.len()..].to_vec());
    bytes.zeroize();
    if key_material.is_empty() {
        return Err(SigningError::MalformedKeyFile {
            path: path.to_path_buf(),
            reason: "missing Ed25519 PKCS#8 private key",
        });
    }
    FileBackedIdentitySigner::from_key_material(path.to_path_buf(), key_material)
}

#[cfg(unix)]
fn open_key_file_no_follow(path: &Path) -> SigningResult<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    // O_NOFOLLOW: a symlink at the key path is refused outright instead of
    // being resolved to whatever it currently points at.
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| map_key_open_error(path, source))
}

#[cfg(not(unix))]
fn open_key_file_no_follow(path: &Path) -> SigningResult<fs::File> {
    fs::File::open(path).map_err(|source| map_key_open_error(path, source))
}

fn map_key_open_error(path: &Path, source: io::Error) -> SigningError {
    if source.kind() == io::ErrorKind::NotFound {
        SigningError::KeyFileMissing {
            path: path.to_path_buf(),
        }
    } else {
        SigningError::KeyFileRead {
            path: path.to_path_buf(),
            source,
        }
    }
}

#[cfg(unix)]
fn validate_metadata_permissions(path: &Path, metadata: &fs::Metadata) -> SigningResult<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    // Bind acceptance to the owner, not just the mode: a 0600 key file owned
    // by another uid is someone else's key material, not ours.
    let owner_uid = metadata.uid();
    let current_uid = current_effective_uid();
    if owner_uid != current_uid {
        return Err(SigningError::KeyFileOwner {
            path: path.to_path_buf(),
            owner_uid,
            current_uid,
        });
    }

    let mode = metadata.permissions().mode() & 0o777;
    if mode == 0o600 {
        Ok(())
    } else {
        Err(SigningError::KeyFilePermission {
            path: path.to_path_buf(),
            mode,
        })
    }
}

#[cfg(unix)]
fn current_effective_uid() -> u32 {
    // SAFETY: geteuid has no failure modes and touches no memory.
    unsafe { libc::geteuid() }
}

fn acquire_publish_lock(path: &Path) -> SigningResult<File> {
    use fs2::FileExt;

    let lock_path = publish_lock_path(path);
    if let Some(parent) = nonempty_parent(&lock_path) {
        fs::create_dir_all(parent).map_err(|source| SigningError::KeyFileWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock_file = options
        .open(&lock_path)
        .map_err(|source| SigningError::KeyFileWrite {
            path: lock_path.clone(),
            source,
        })?;
    lock_file
        .lock_exclusive()
        .map_err(|source| SigningError::KeyFileWrite {
            path: lock_path,
            source,
        })?;
    Ok(lock_file)
}

#[cfg(not(unix))]
fn validate_metadata_permissions(_path: &Path, _metadata: &fs::Metadata) -> SigningResult<()> {
    Ok(())
}

fn generate_key_material() -> SigningResult<Zeroizing<Vec<u8>>> {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| SigningError::RandomFailed {
        operation: "ed25519 key generation",
    })?;
    Ok(Zeroizing::new(pkcs8.as_ref().to_vec()))
}

fn public_key_from_pkcs8(path: &Path, key_material: &[u8]) -> SigningResult<IdentityPublicKey> {
    let key_pair =
        Ed25519KeyPair::from_pkcs8(key_material).map_err(|_| SigningError::MalformedKeyFile {
            path: path.to_path_buf(),
            reason: "invalid Ed25519 PKCS#8 private key",
        })?;
    let public_key = key_pair.public_key().as_ref();
    let public_key: [u8; 32] =
        public_key
            .try_into()
            .map_err(|_| SigningError::MalformedKeyFile {
                path: path.to_path_buf(),
                reason: "invalid Ed25519 public key length",
            })?;
    Ok(IdentityPublicKey::from_ed25519_bytes(public_key))
}

fn publish_key_file(path: &Path, key_material: &[u8]) -> SigningResult<()> {
    replace_key_file(path, key_material)?;
    sync_parent_dir(path)
}

fn replace_key_file(path: &Path, key_material: &[u8]) -> SigningResult<()> {
    let stage_path = rotating_stage_path(path);
    remove_file_if_exists(&stage_path)?;
    write_staged_key_file(&stage_path, key_material)?;
    replace_file(&stage_path, path).map_err(|source| SigningError::KeyFileWrite {
        path: path.to_path_buf(),
        source,
    })
}

fn write_staged_key_file(path: &Path, key_material: &[u8]) -> SigningResult<()> {
    if let Some(parent) = nonempty_parent(path) {
        fs::create_dir_all(parent).map_err(|source| SigningError::KeyFileWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|source| SigningError::KeyFileWrite {
            path: path.to_path_buf(),
            source,
        })?;
    set_owner_only_permissions(path, &file)?;
    file.write_all(KEY_FILE_HEADER)
        .and_then(|_| file.write_all(key_material))
        .map_err(|source| SigningError::KeyFileWrite {
            path: path.to_path_buf(),
            source,
        })?;
    file.sync_all()
        .map_err(|source| SigningError::KeyFileSync {
            path: path.to_path_buf(),
            source,
        })?;
    drop(file);
    sync_parent_dir(path)
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path, file: &File) -> SigningResult<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| SigningError::KeyFileWrite {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path, _file: &File) -> SigningResult<()> {
    Ok(())
}

fn discard_uncommitted_rotation_stage(path: &Path) -> SigningResult<()> {
    let stage_path = rotating_stage_path(path);
    if try_exists(&stage_path)? {
        remove_file_if_exists(&stage_path)?;
        sync_parent_dir(&stage_path)?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> SigningResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SigningError::KeyFileWrite {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn try_exists(path: &Path) -> SigningResult<bool> {
    path.try_exists()
        .map_err(|source| SigningError::KeyFileRead {
            path: path.to_path_buf(),
            source,
        })
}

fn sync_parent_dir(path: &Path) -> SigningResult<()> {
    // A bare relative path like `identity.key` has an empty parent — that is
    // the current directory, and it still needs the durability fsync.
    let parent = nonempty_parent(path).unwrap_or_else(|| Path::new("."));
    let dir = File::open(parent).map_err(|source| SigningError::KeyFileSync {
        path: parent.to_path_buf(),
        source,
    })?;
    dir.sync_all().map_err(|source| SigningError::KeyFileSync {
        path: parent.to_path_buf(),
        source,
    })
}

fn nonempty_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut source_wide: Vec<u16> = source.as_os_str().encode_wide().collect();
    source_wide.push(0);
    let mut destination_wide: Vec<u16> = destination.as_os_str().encode_wide().collect();
    destination_wide.push(0);
    let ok = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn rotating_stage_path(path: &Path) -> PathBuf {
    append_suffix(path, ROTATING_SUFFIX)
}

fn publish_lock_path(path: &Path) -> PathBuf {
    append_suffix(path, LOCK_SUFFIX)
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn prefixed_fingerprint(prefix: &str, key_bytes: &[u8; 32]) -> String {
    let digest = Sha256::digest(key_bytes);
    let mut id = String::with_capacity(prefix.len() + 32);
    id.push_str(prefix);
    push_lower_hex(&mut id, &digest[..16]);
    id
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    push_lower_hex(&mut encoded, bytes);
    encoded
}

fn push_lower_hex(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn sign_verify_round_trips_and_rejects_tampered_message() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("identity.key");
        let signer = FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should open");

        let signature = signer.sign(b"message").expect("message should sign");

        signer
            .verify(b"message", &signature)
            .expect("signature should verify");
        assert!(matches!(
            signer.verify(b"tampered", &signature),
            Err(SigningError::VerificationFailed { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn open_refuses_symlinked_key_path() {
        let dir = tempdir().expect("tempdir should create");
        let real = dir.path().join("real.key");
        FileBackedIdentitySigner::open(&real, OpenMode::GenerateIfAbsent)
            .expect("real key should generate");

        let link = dir.path().join("link.key");
        std::os::unix::fs::symlink(&real, &link).expect("symlink should create");

        // O_NOFOLLOW: the open itself must refuse the symlink even though the
        // target is a valid 0600 key file — path-swap attacks cannot route the
        // permission check and the read through different inodes.
        assert!(matches!(
            FileBackedIdentitySigner::open(&link, OpenMode::Existing),
            Err(SigningError::KeyFileRead { .. })
        ));
    }

    #[test]
    fn existing_mode_fails_closed_on_missing_malformed_and_truncated_files() {
        let dir = tempdir().expect("tempdir should create");
        let missing = dir.path().join("missing.key");

        assert!(matches!(
            FileBackedIdentitySigner::open(&missing, OpenMode::Existing),
            Err(SigningError::KeyFileMissing { .. })
        ));

        let malformed = dir.path().join("malformed.key");
        write_raw_key_file(&malformed, b"not-a-nimbus-key");
        assert!(matches!(
            FileBackedIdentitySigner::open(&malformed, OpenMode::Existing),
            Err(SigningError::MalformedKeyFile { .. })
        ));

        let truncated = dir.path().join("truncated.key");
        write_raw_key_file(&truncated, KEY_FILE_HEADER);
        assert!(matches!(
            FileBackedIdentitySigner::open(&truncated, OpenMode::Existing),
            Err(SigningError::MalformedKeyFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn existing_mode_fails_closed_on_group_or_other_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("identity.key");
        FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should create key file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("permissions should update");

        assert!(matches!(
            FileBackedIdentitySigner::open(&path, OpenMode::Existing),
            Err(SigningError::KeyFilePermission { mode: 0o644, .. })
        ));
    }

    #[test]
    fn generate_if_absent_creates_owner_only_file_and_reopens_same_fingerprint() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("identity.key");
        let signer = FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should create key file");
        let fingerprint = signer.public_key().fingerprint();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = fs::metadata(&path)
                .expect("key file should stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        let reopened = FileBackedIdentitySigner::open(&path, OpenMode::Existing)
            .expect("signer should reopen");
        assert_eq!(reopened.public_key().fingerprint(), fingerprint);
    }

    #[test]
    fn generate_if_absent_creates_nested_key_directories() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("state").join("identity").join("node.key");
        assert!(
            !path
                .parent()
                .expect("nested path should have parent")
                .exists(),
            "nested key parent should start absent"
        );

        let signer = FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should create nested key directories");
        assert!(path.exists(), "nested key file should exist after open");

        let signature = signer.sign(b"message").expect("message should sign");
        signer
            .verify(b"message", &signature)
            .expect("signature should verify");
    }

    #[test]
    fn lock_path_appends_suffix_instead_of_replacing_extension() {
        let dir = tempdir().expect("tempdir should create");
        let key_path = dir.path().join("identity.key");
        let lock_path = dir.path().join("identity.key.lock");
        assert_eq!(publish_lock_path(&key_path), lock_path);

        let signer = FileBackedIdentitySigner::open(&key_path, OpenMode::GenerateIfAbsent)
            .expect("signer should create key file");
        let signature = signer.sign(b"message").expect("message should sign");
        signer
            .verify(b"message", &signature)
            .expect("signature should verify");
        assert!(lock_path.exists(), "lock path should append .lock suffix");
        assert!(
            !dir.path().join("identity.lock").exists(),
            "lock path should not replace the key extension"
        );

        let lock_named_key_path = dir.path().join("x.lock");
        let lock_named_signer =
            FileBackedIdentitySigner::open(&lock_named_key_path, OpenMode::GenerateIfAbsent)
                .expect("key named x.lock should not collide with its lock file");
        assert!(
            lock_named_key_path.exists(),
            "key named x.lock should be created"
        );
        assert!(
            dir.path().join("x.lock.lock").exists(),
            "key named x.lock should lock via x.lock.lock"
        );
        let signature = lock_named_signer
            .sign(b"message")
            .expect("x.lock key should sign");
        lock_named_signer
            .verify(b"message", &signature)
            .expect("x.lock key signature should verify");
    }

    #[test]
    fn rotation_changes_key_verifies_new_signatures_and_rejects_stale_key_id() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("identity.key");
        let mut signer = FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should create key file");
        let old_key_id = signer.public_key().fingerprint();
        let old_signature = signer.sign(b"message").expect("old signature should sign");

        signer.rotate().expect("rotation should complete");
        let new_key_id = signer.public_key().fingerprint();
        assert_ne!(new_key_id, old_key_id);
        let new_signature = signer.sign(b"message").expect("new signature should sign");
        signer
            .verify(b"message", &new_signature)
            .expect("new signature should verify");

        assert!(matches!(
            signer.verify(b"message", &old_signature),
            Err(SigningError::StaleKey {
                expected_key_id,
                actual_key_id,
            }) if expected_key_id == new_key_id && actual_key_id == old_key_id
        ));
    }

    #[test]
    fn interrupted_rotation_stage_is_discarded_and_live_key_survives() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("identity.key");
        let signer = FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should create key file");
        let live_key_id = signer.public_key().fingerprint();
        let stage_path = rotating_stage_path(&path);
        let staged_key = generate_key_material().expect("staged key should generate");
        write_staged_key_file(&stage_path, &staged_key).expect("stage should write");
        assert!(stage_path.exists());

        let reopened = FileBackedIdentitySigner::open(&path, OpenMode::Existing)
            .expect("signer should reopen");

        assert_eq!(reopened.public_key().fingerprint(), live_key_id);
        assert!(
            !stage_path.exists(),
            "uncommitted stage should be discarded"
        );
        let signature = reopened.sign(b"message").expect("message should sign");
        reopened
            .verify(b"message", &signature)
            .expect("surviving key should verify");
    }

    #[test]
    fn debug_and_errors_redact_private_key_material_and_file_contents() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("identity.key");
        let mut signer = FileBackedIdentitySigner::open(&path, OpenMode::GenerateIfAbsent)
            .expect("signer should create key file");
        let key_file_hex = lower_hex(&fs::read(&path).expect("key file should read"));
        let old_signature = signer.sign(b"message").expect("message should sign");
        let old_key_id = old_signature.key_id().to_string();

        let debug = format!("{signer:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(debug.contains(&old_key_id));
        assert!(!debug.contains(&key_file_hex));

        signer.rotate().expect("rotation should complete");
        let stale_error = signer
            .verify(b"message", &old_signature)
            .expect_err("old signature should be stale");
        let stale_error = stale_error.to_string();
        assert!(stale_error.contains(&old_key_id));
        assert!(stale_error.contains(&signer.public_key().fingerprint()));
        assert!(!stale_error.contains(&key_file_hex));

        let malformed = dir.path().join("malformed.key");
        let malformed_bytes = b"nimbus-identity-key-v1\nprivate-file-bytes";
        write_raw_key_file(&malformed, malformed_bytes);
        let malformed_error = FileBackedIdentitySigner::open(&malformed, OpenMode::Existing)
            .expect_err("malformed key should be rejected")
            .to_string();
        assert!(!malformed_error.contains("private-file-bytes"));
        assert!(!malformed_error.contains(&lower_hex(malformed_bytes)));
    }

    fn write_raw_key_file(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).expect("raw key file should write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .expect("permissions should update");
        }
    }
}
