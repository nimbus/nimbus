//! Sidecar key manifest format.
//!
//! Each encrypted local database or artifact has an adjacent sidecar manifest
//! at `<protected-path>.nimbus-enc` that stores the wrapped DEK and metadata.
//!
//! # Wire Format (v1)
//!
//! ```text
//! [ magic (4 bytes) | payload ... | crc32 (4 bytes, over entire preceding content) ]
//! ```
//!
//! The magic bytes allow rapid file-type identification; the trailing CRC32
//! detects truncation or corruption independently of the cryptographic unwrap.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::key::{WrappedDatabaseKey, WrappingCipher};
use super::provider::KeyProviderKind;

/// The current manifest format version.
pub const MANIFEST_VERSION: u32 = 1;

/// Magic bytes at the start of every manifest file: "NVX\x01" (Nimbus v1).
const MANIFEST_MAGIC: [u8; 4] = [b'N', b'V', b'X', 0x01];

/// The file extension for sidecar manifests.
pub const MANIFEST_EXTENSION: &str = ".nimbus-enc";

/// Errors that can occur when reading a manifest.
#[derive(Debug)]
pub enum ManifestReadError {
    /// The manifest file could not be read.
    IoError { path: PathBuf, source: io::Error },
    /// The manifest data is invalid.
    ParseError { path: PathBuf, message: String },
    /// The manifest version is not supported.
    UnsupportedVersion { path: PathBuf, version: u32 },
}

impl std::fmt::Display for ManifestReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError { path, source } => {
                write!(f, "failed to read manifest {}: {source}", path.display())
            }
            Self::ParseError { path, message } => {
                write!(f, "invalid manifest at {}: {message}", path.display())
            }
            Self::UnsupportedVersion { path, version } => {
                write!(
                    f,
                    "unsupported manifest version {version} at {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ManifestReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Errors that can occur when writing a manifest.
#[derive(Debug)]
pub enum ManifestWriteError {
    /// The manifest file could not be written.
    IoError { path: PathBuf, source: io::Error },
    /// Serialization failed.
    SerializeError { message: String },
}

impl std::fmt::Display for ManifestWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError { path, source } => {
                write!(f, "failed to write manifest {}: {source}", path.display())
            }
            Self::SerializeError { message } => {
                write!(f, "manifest serialization failed: {message}")
            }
        }
    }
}

impl std::error::Error for ManifestWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Union error type for manifest operations.
#[derive(Debug)]
pub enum ManifestError {
    Read(ManifestReadError),
    Write(ManifestWriteError),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(e) => write!(f, "{e}"),
            Self::Write(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(e) => e.source(),
            Self::Write(e) => e.source(),
        }
    }
}

impl From<ManifestReadError> for ManifestError {
    fn from(e: ManifestReadError) -> Self {
        Self::Read(e)
    }
}

impl From<ManifestWriteError> for ManifestError {
    fn from(e: ManifestWriteError) -> Self {
        Self::Write(e)
    }
}

/// The cipher profile used for page encryption in the protected database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestCipher {
    /// AES-256-GCM-SIV for redb page encryption.
    RedbAes256GcmSiv,
    /// SQLCipher profile for embedded SQLite.
    SqlCipher,
    /// libsql native encryption for replica caches.
    LibsqlAes256Cbc,
}

impl ManifestCipher {
    /// Returns a stable string identifier for the cipher.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RedbAes256GcmSiv => "redb:aes-256-gcm-siv",
            Self::SqlCipher => "sqlite:sqlcipher",
            Self::LibsqlAes256Cbc => "libsql:aes-256-cbc",
        }
    }

    /// Parses a cipher from its string identifier.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "redb:aes-256-gcm-siv" => Some(Self::RedbAes256GcmSiv),
            "sqlite:sqlcipher" => Some(Self::SqlCipher),
            "libsql:aes-256-cbc" => Some(Self::LibsqlAes256Cbc),
            _ => None,
        }
    }
}

/// Header section of a key manifest, used as AAD during key wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyManifestHeader {
    /// Format version.
    pub version: u32,

    /// The cipher profile used for data encryption.
    pub cipher: ManifestCipher,

    /// Subject descriptor (role + tenant + name).
    pub subject_descriptor: String,

    /// Key provider kind descriptor.
    pub key_provider: KeyProviderKind,

    /// Creation timestamp (Unix epoch seconds).
    pub created_at: u64,

    /// Last KEK rotation timestamp (Unix epoch seconds).
    pub rotated_at: u64,
}

impl KeyManifestHeader {
    /// Serializes the header to bytes for use as AAD.
    ///
    /// The format is stable and must not change within a version.
    pub fn to_aad(&self) -> Vec<u8> {
        let mut aad = Vec::new();

        // Version (4 bytes, big-endian)
        aad.extend_from_slice(&self.version.to_be_bytes());

        // Cipher string (length-prefixed)
        let cipher_str = self.cipher.as_str();
        aad.extend_from_slice(&(cipher_str.len() as u32).to_be_bytes());
        aad.extend_from_slice(cipher_str.as_bytes());

        // Subject descriptor (length-prefixed)
        aad.extend_from_slice(&(self.subject_descriptor.len() as u32).to_be_bytes());
        aad.extend_from_slice(self.subject_descriptor.as_bytes());

        // Key provider descriptor (length-prefixed)
        let provider_str = self.key_provider.to_string();
        aad.extend_from_slice(&(provider_str.len() as u32).to_be_bytes());
        aad.extend_from_slice(provider_str.as_bytes());

        // Timestamps (8 bytes each, big-endian)
        aad.extend_from_slice(&self.created_at.to_be_bytes());
        aad.extend_from_slice(&self.rotated_at.to_be_bytes());

        aad
    }
}

/// A complete key manifest stored in a sidecar file.
#[derive(Debug, Clone)]
pub struct KeyManifest {
    /// The manifest header (also used as AAD).
    pub header: KeyManifestHeader,

    /// The wrapped data-encryption key.
    pub wrapped_key: WrappedDatabaseKey,
}

impl KeyManifest {
    /// Creates a new manifest for a freshly encrypted database.
    pub fn new(
        cipher: ManifestCipher,
        subject_descriptor: String,
        key_provider: KeyProviderKind,
        wrapped_key: WrappedDatabaseKey,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            header: KeyManifestHeader {
                version: MANIFEST_VERSION,
                cipher,
                subject_descriptor,
                key_provider,
                created_at: now,
                rotated_at: now,
            },
            wrapped_key,
        }
    }

    /// Returns the path to the sidecar manifest for a given protected path.
    pub fn manifest_path(protected_path: &Path) -> PathBuf {
        let mut manifest_path = protected_path.as_os_str().to_owned();
        manifest_path.push(MANIFEST_EXTENSION);
        PathBuf::from(manifest_path)
    }

    /// Reads a manifest from a sidecar file.
    pub fn read(manifest_path: &Path) -> Result<Self, ManifestReadError> {
        let bytes = fs::read(manifest_path).map_err(|source| ManifestReadError::IoError {
            path: manifest_path.to_path_buf(),
            source,
        })?;

        Self::parse(&bytes, manifest_path)
    }

    /// Reads a manifest for a protected path (appending `.nimbus-enc`).
    pub fn read_for(protected_path: &Path) -> Result<Self, ManifestReadError> {
        Self::read(&Self::manifest_path(protected_path))
    }

    /// Parses a manifest from bytes.
    fn parse(bytes: &[u8], path: &Path) -> Result<Self, ManifestReadError> {
        let parse_err = |msg: &str| ManifestReadError::ParseError {
            path: path.to_path_buf(),
            message: msg.to_string(),
        };

        // Minimum size: magic (4) + version (4) + CRC32 trailer (4) = 12
        if bytes.len() < 12 {
            return Err(parse_err("manifest too short"));
        }

        // Validate magic bytes
        if bytes[..4] != MANIFEST_MAGIC {
            return Err(parse_err("invalid magic bytes (not a nimbus-enc manifest)"));
        }

        // Validate trailing CRC32 (covers everything except the checksum itself)
        let payload = &bytes[..bytes.len() - 4];
        let stored_checksum = u32::from_be_bytes(
            bytes[bytes.len() - 4..]
                .try_into()
                .map_err(|_| parse_err("invalid checksum bytes"))?,
        );
        let computed_checksum = crc32fast::hash(payload);
        if stored_checksum != computed_checksum {
            return Err(parse_err(
                "checksum mismatch (manifest is corrupted or truncated)",
            ));
        }

        // Parse the payload (between magic and CRC32)
        let mut cursor = 4; // skip magic

        // Version (4 bytes)
        let version = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| parse_err("invalid version bytes"))?,
        );
        cursor += 4;

        if version != MANIFEST_VERSION {
            return Err(ManifestReadError::UnsupportedVersion {
                path: path.to_path_buf(),
                version,
            });
        }

        // Helper to read a length-prefixed string
        let read_string = |cursor: &mut usize| -> Result<String, ManifestReadError> {
            if *cursor + 4 > bytes.len() {
                return Err(parse_err("unexpected end of manifest"));
            }
            let len = u32::from_be_bytes(
                bytes[*cursor..*cursor + 4]
                    .try_into()
                    .map_err(|_| parse_err("invalid string length"))?,
            ) as usize;
            *cursor += 4;
            if *cursor + len > bytes.len() {
                return Err(parse_err("string extends past end of manifest"));
            }
            let s = String::from_utf8(bytes[*cursor..*cursor + len].to_vec())
                .map_err(|_| parse_err("invalid UTF-8 in string"))?;
            *cursor += len;
            Ok(s)
        };

        // Cipher
        let cipher_str = read_string(&mut cursor)?;
        let cipher =
            ManifestCipher::parse(&cipher_str).ok_or_else(|| parse_err("unknown cipher"))?;

        // Subject descriptor
        let subject_descriptor = read_string(&mut cursor)?;

        // Key provider descriptor (we parse it back from the string)
        let provider_str = read_string(&mut cursor)?;
        let key_provider = parse_key_provider_kind(&provider_str)
            .ok_or_else(|| parse_err("invalid key provider descriptor"))?;

        // Timestamps
        if cursor + 16 > bytes.len() {
            return Err(parse_err("unexpected end of manifest (timestamps)"));
        }
        let created_at = u64::from_be_bytes(
            bytes[cursor..cursor + 8]
                .try_into()
                .map_err(|_| parse_err("invalid created_at"))?,
        );
        cursor += 8;
        let rotated_at = u64::from_be_bytes(
            bytes[cursor..cursor + 8]
                .try_into()
                .map_err(|_| parse_err("invalid rotated_at"))?,
        );
        cursor += 8;

        // Wrapped key cipher
        let wrapping_cipher_str = read_string(&mut cursor)?;
        let wrapping_cipher = WrappingCipher::parse(&wrapping_cipher_str)
            .ok_or_else(|| parse_err("unknown wrapping cipher"))?;

        // Wrapped key ciphertext
        if cursor + 4 > bytes.len() {
            return Err(parse_err("unexpected end of manifest (ciphertext length)"));
        }
        let ciphertext_len = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| parse_err("invalid ciphertext length"))?,
        ) as usize;
        cursor += 4;
        if cursor + ciphertext_len > bytes.len() {
            return Err(parse_err("ciphertext extends past end of manifest"));
        }
        let ciphertext = bytes[cursor..cursor + ciphertext_len].to_vec();

        Ok(Self {
            header: KeyManifestHeader {
                version,
                cipher,
                subject_descriptor,
                key_provider,
                created_at,
                rotated_at,
            },
            wrapped_key: WrappedDatabaseKey::new(wrapping_cipher, ciphertext),
        })
    }

    /// Serializes the manifest to bytes.
    ///
    /// Format: `MAGIC (4) | payload | CRC32 (4)`
    fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Magic bytes for file-type identification
        bytes.extend_from_slice(&MANIFEST_MAGIC);

        // Version
        bytes.extend_from_slice(&self.header.version.to_be_bytes());

        // Cipher (length-prefixed)
        let cipher_str = self.header.cipher.as_str();
        bytes.extend_from_slice(&(cipher_str.len() as u32).to_be_bytes());
        bytes.extend_from_slice(cipher_str.as_bytes());

        // Subject descriptor (length-prefixed)
        bytes.extend_from_slice(&(self.header.subject_descriptor.len() as u32).to_be_bytes());
        bytes.extend_from_slice(self.header.subject_descriptor.as_bytes());

        // Key provider (length-prefixed)
        let provider_str = self.header.key_provider.to_string();
        bytes.extend_from_slice(&(provider_str.len() as u32).to_be_bytes());
        bytes.extend_from_slice(provider_str.as_bytes());

        // Timestamps
        bytes.extend_from_slice(&self.header.created_at.to_be_bytes());
        bytes.extend_from_slice(&self.header.rotated_at.to_be_bytes());

        // Wrapped key cipher (length-prefixed)
        let wrapping_cipher_str = self.wrapped_key.cipher.as_str();
        bytes.extend_from_slice(&(wrapping_cipher_str.len() as u32).to_be_bytes());
        bytes.extend_from_slice(wrapping_cipher_str.as_bytes());

        // Wrapped key ciphertext (length-prefixed)
        bytes.extend_from_slice(&(self.wrapped_key.ciphertext.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.wrapped_key.ciphertext);

        // Trailing CRC32 over all preceding content (detects truncation/corruption)
        let checksum = crc32fast::hash(&bytes);
        bytes.extend_from_slice(&checksum.to_be_bytes());

        bytes
    }

    /// Writes the manifest atomically to a sidecar file.
    ///
    /// This uses write-to-temp-then-rename for crash safety.
    pub fn write(&self, manifest_path: &Path) -> Result<(), ManifestWriteError> {
        let bytes = self.serialize();
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ManifestWriteError::IoError {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        // Write to a temporary file first
        let temp_path = manifest_path.with_extension("nimbus-enc.tmp");
        {
            let mut file =
                File::create(&temp_path).map_err(|source| ManifestWriteError::IoError {
                    path: temp_path.clone(),
                    source,
                })?;
            file.write_all(&bytes)
                .map_err(|source| ManifestWriteError::IoError {
                    path: temp_path.clone(),
                    source,
                })?;
            file.sync_all()
                .map_err(|source| ManifestWriteError::IoError {
                    path: temp_path.clone(),
                    source,
                })?;
        }

        // Move the temp file into place, replacing any previous manifest.
        move_temp_file_into_place(&temp_path, manifest_path).map_err(|source| {
            ManifestWriteError::IoError {
                path: manifest_path.to_path_buf(),
                source,
            }
        })?;

        // Fsync the parent directory to ensure the rename is durable.
        // Without this, a crash after rename could lose the manifest on Linux/macOS.
        if let Some(Ok(dir)) = manifest_path.parent().map(File::open) {
            let _ = dir.sync_all();
        }

        Ok(())
    }

    /// Writes the manifest for a protected path (appending `.nimbus-enc`).
    pub fn write_for(&self, protected_path: &Path) -> Result<(), ManifestWriteError> {
        self.write(&Self::manifest_path(protected_path))
    }

    /// Returns a diagnostics-safe summary of the manifest.
    pub fn summary(&self) -> ManifestSummary {
        ManifestSummary {
            cipher: self.header.cipher,
            subject_descriptor: self.header.subject_descriptor.clone(),
            key_provider: self.header.key_provider.clone(),
            created_at: self.header.created_at,
            rotated_at: self.header.rotated_at,
        }
    }
}

#[cfg(not(windows))]
fn move_temp_file_into_place(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn move_temp_file_into_place(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    fn encode(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let source = encode(source);
    let destination = encode(destination);
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// A diagnostics-safe summary of a key manifest.
#[derive(Debug, Clone)]
pub struct ManifestSummary {
    pub cipher: ManifestCipher,
    pub subject_descriptor: String,
    pub key_provider: KeyProviderKind,
    pub created_at: u64,
    pub rotated_at: u64,
}

/// Parses a key provider kind from its display string.
fn parse_key_provider_kind(s: &str) -> Option<KeyProviderKind> {
    if let Some(path) = s.strip_prefix("master-key-file:") {
        Some(KeyProviderKind::MasterKeyFile {
            path: path.to_string(),
        })
    } else if let Some(path) = s.strip_prefix("key-dir:") {
        Some(KeyProviderKind::KeyDirectory {
            path: path.to_string(),
        })
    } else if let Some(rest) = s.strip_prefix("aws-kms:") {
        // Parse "key_id (region=region)" or just "key_id"
        if let Some(paren_pos) = rest.find(" (region=") {
            let key_id = rest[..paren_pos].to_string();
            let region_part = &rest[paren_pos + 9..];
            let region = region_part.strip_suffix(')').map(|s| s.to_string());
            Some(KeyProviderKind::AwsKms { key_id, region })
        } else {
            Some(KeyProviderKind::AwsKms {
                key_id: rest.to_string(),
                region: None,
            })
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_through_serialization() {
        let manifest = KeyManifest::new(
            ManifestCipher::SqlCipher,
            "db:sqlite:tenant:demo:demo.sqlite3".to_string(),
            KeyProviderKind::MasterKeyFile {
                path: "/secure/nimbus.key".to_string(),
            },
            WrappedDatabaseKey::new(WrappingCipher::Aes256GcmSiv, vec![1, 2, 3, 4]),
        );

        let bytes = manifest.serialize();
        let parsed =
            KeyManifest::parse(&bytes, Path::new("test.nimbus-enc")).expect("parse should succeed");

        assert_eq!(parsed.header.version, manifest.header.version);
        assert_eq!(parsed.header.cipher, manifest.header.cipher);
        assert_eq!(
            parsed.header.subject_descriptor,
            manifest.header.subject_descriptor
        );
        assert_eq!(parsed.header.key_provider, manifest.header.key_provider);
        assert_eq!(parsed.wrapped_key, manifest.wrapped_key);
    }

    #[test]
    fn manifest_path_appends_extension() {
        let protected = Path::new("/data/demo.sqlite3");
        let manifest = KeyManifest::manifest_path(protected);
        assert_eq!(manifest, PathBuf::from("/data/demo.sqlite3.nimbus-enc"));
    }

    #[test]
    fn header_aad_is_deterministic() {
        let header = KeyManifestHeader {
            version: 1,
            cipher: ManifestCipher::SqlCipher,
            subject_descriptor: "test".to_string(),
            key_provider: KeyProviderKind::MasterKeyFile {
                path: "/key".to_string(),
            },
            created_at: 1000,
            rotated_at: 2000,
        };

        let aad1 = header.to_aad();
        let aad2 = header.to_aad();
        assert_eq!(aad1, aad2);
    }

    #[test]
    fn key_provider_kind_round_trips_through_string() {
        let kinds = [
            KeyProviderKind::MasterKeyFile {
                path: "/secure/key".to_string(),
            },
            KeyProviderKind::KeyDirectory {
                path: "/keys".to_string(),
            },
            KeyProviderKind::AwsKms {
                key_id: "arn:aws:kms:us-east-1:123:key/abc".to_string(),
                region: Some("us-east-1".to_string()),
            },
            KeyProviderKind::AwsKms {
                key_id: "alias/my-key".to_string(),
                region: None,
            },
        ];

        for kind in kinds {
            let s = kind.to_string();
            let parsed = parse_key_provider_kind(&s).expect("should parse");
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn manifest_rejects_bad_magic() {
        let manifest = KeyManifest::new(
            ManifestCipher::SqlCipher,
            "test".to_string(),
            KeyProviderKind::MasterKeyFile {
                path: "/key".to_string(),
            },
            WrappedDatabaseKey::new(WrappingCipher::Aes256GcmSiv, vec![0u8; 60]),
        );

        let mut bytes = manifest.serialize();
        // Corrupt magic bytes
        bytes[0] = 0xFF;

        let result = KeyManifest::parse(&bytes, Path::new("test.nimbus-enc"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("magic bytes"));
    }

    #[test]
    fn manifest_rejects_corrupted_checksum() {
        let manifest = KeyManifest::new(
            ManifestCipher::SqlCipher,
            "test".to_string(),
            KeyProviderKind::MasterKeyFile {
                path: "/key".to_string(),
            },
            WrappedDatabaseKey::new(WrappingCipher::Aes256GcmSiv, vec![0u8; 60]),
        );

        let mut bytes = manifest.serialize();
        // Corrupt a byte in the payload (not the checksum)
        bytes[10] ^= 0xFF;

        let result = KeyManifest::parse(&bytes, Path::new("test.nimbus-enc"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"));
    }

    #[test]
    fn manifest_rejects_truncation() {
        let manifest = KeyManifest::new(
            ManifestCipher::SqlCipher,
            "test".to_string(),
            KeyProviderKind::MasterKeyFile {
                path: "/key".to_string(),
            },
            WrappedDatabaseKey::new(WrappingCipher::Aes256GcmSiv, vec![0u8; 60]),
        );

        let bytes = manifest.serialize();
        // Truncate to just past magic — too short for valid checksum
        let truncated = &bytes[..8];

        let result = KeyManifest::parse(truncated, Path::new("test.nimbus-enc"));
        // Either "too short" or "checksum mismatch" depending on length
        assert!(result.is_err());
    }
}
