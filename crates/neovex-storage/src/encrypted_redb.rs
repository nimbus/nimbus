//! Encrypted storage backend for redb.
//!
//! This module provides a `StorageBackend` implementation that encrypts
//! data at rest using AES-256-GCM-SIV with per-page authenticated encryption.
//!
//! # Security Model
//!
//! - Each logical page is encrypted independently with a fresh random nonce
//! - Page position is included in the AAD to prevent page-swap attacks
//! - The format version is included in the AAD for upgrade safety
//! - The DEK is provided externally and managed through the key provider system
//! - All nonces are generated from OS entropy (`OsRng`), never `thread_rng`
//!
//! # Physical Layout
//!
//! The encrypted file maps logical pages to physical encrypted slots:
//!
//! ```text
//! Logical page i at offset (i * LOGICAL_PAGE_SIZE):
//!   Physical offset: (i * PHYSICAL_PAGE_SIZE)
//!   Physical layout: [ nonce (12) | ciphertext (LOGICAL_PAGE_SIZE) | tag (16) ]
//! ```
//!
//! # Thread Safety
//!
//! All mutable state is protected by a single mutex per backend instance.
//! This prevents TOCTOU races between bounds checks and I/O operations.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use aes_gcm_siv::aead::{AeadInPlace, KeyInit};
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Tag};
use parking_lot::Mutex;
use rand::RngCore;
use rand::rngs::OsRng;
use redb::StorageBackend;

/// Format version for the encrypted page layout.
/// Increment this when the encryption format changes.
pub const ENCRYPTED_FORMAT_VERSION: u32 = 1;

/// Logical page size matching redb's default.
pub const LOGICAL_PAGE_SIZE: usize = 4096;

/// Size of the AES-GCM-SIV nonce.
const NONCE_SIZE: usize = 12;

/// Size of the AES-GCM-SIV authentication tag.
const TAG_SIZE: usize = 16;

/// Total overhead per encrypted page (nonce + tag).
const PAGE_OVERHEAD: usize = NONCE_SIZE + TAG_SIZE;

/// Physical size of each encrypted page slot.
pub const PHYSICAL_PAGE_SIZE: usize = LOGICAL_PAGE_SIZE + PAGE_OVERHEAD;

/// Size of the page AAD payload: format version + page index + page size.
const PAGE_AAD_SIZE: usize = 16;

/// Builds the AAD for a page, binding it to its position and format.
fn build_aad(page_index: u64) -> [u8; PAGE_AAD_SIZE] {
    let mut aad = [0u8; PAGE_AAD_SIZE];
    aad[..4].copy_from_slice(&ENCRYPTED_FORMAT_VERSION.to_be_bytes());
    aad[4..12].copy_from_slice(&page_index.to_be_bytes());
    aad[12..].copy_from_slice(&(LOGICAL_PAGE_SIZE as u32).to_be_bytes());
    aad
}

/// Internal state protected by a single mutex to prevent TOCTOU races.
///
/// All operations that read or write file state must hold this lock for
/// the entire duration of the operation, including bounds checks.
struct FileState {
    file: File,
    logical_len: u64,
}

#[derive(Default)]
struct EncryptedReadProfile {
    read_calls: AtomicU64,
    bytes_requested: AtomicU64,
    page_reads: AtomicU64,
    file_read_nanos: AtomicU64,
    decrypt_nanos: AtomicU64,
}

impl EncryptedReadProfile {
    fn record_read_call(&self, len: usize) {
        self.read_calls.fetch_add(1, Ordering::Relaxed);
        self.bytes_requested
            .fetch_add(len.try_into().unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    fn record_page_read(&self, file_read: Duration, decrypt: Duration) {
        self.page_reads.fetch_add(1, Ordering::Relaxed);
        self.file_read_nanos
            .fetch_add(file_read.as_nanos() as u64, Ordering::Relaxed);
        self.decrypt_nanos
            .fetch_add(decrypt.as_nanos() as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> EncryptedReadProfileSnapshot {
        EncryptedReadProfileSnapshot {
            read_calls: self.read_calls.load(Ordering::Relaxed),
            bytes_requested: self.bytes_requested.load(Ordering::Relaxed),
            page_reads: self.page_reads.load(Ordering::Relaxed),
            file_read: Duration::from_nanos(self.file_read_nanos.load(Ordering::Relaxed)),
            decrypt: Duration::from_nanos(self.decrypt_nanos.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Clone)]
pub(crate) struct EncryptedReadProfileHandle {
    stats: Arc<EncryptedReadProfile>,
}

impl EncryptedReadProfileHandle {
    pub(crate) fn snapshot(&self) -> EncryptedReadProfileSnapshot {
        self.stats.snapshot()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EncryptedReadProfileSnapshot {
    pub(crate) read_calls: u64,
    pub(crate) bytes_requested: u64,
    pub(crate) page_reads: u64,
    pub(crate) file_read: Duration,
    pub(crate) decrypt: Duration,
}

/// An encrypted storage backend for redb databases.
///
/// This backend provides transparent encryption for redb by:
/// - Encrypting each logical page with AES-256-GCM-SIV
/// - Using fresh random nonces per page write from OS entropy
/// - Including page index in AAD to prevent reordering attacks
///
/// # Thread Safety
///
/// All file operations are serialized through a single mutex to prevent
/// time-of-check-to-time-of-use (TOCTOU) races between bounds checks
/// and I/O operations.
pub struct EncryptedFileBackend {
    /// File state and logical length under a single lock.
    state: Mutex<FileState>,

    /// The cipher instance for encryption/decryption.
    cipher: Aes256GcmSiv,

    read_profile: Option<Arc<EncryptedReadProfile>>,
}

impl std::fmt::Debug for EncryptedFileBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let logical_len = self.state.lock().logical_len;
        f.debug_struct("EncryptedFileBackend")
            .field("logical_len", &logical_len)
            .finish_non_exhaustive()
    }
}

impl EncryptedFileBackend {
    /// Creates a new encrypted backend, creating the file if it doesn't exist.
    pub fn create(path: impl AsRef<Path>, dek: &[u8; 32]) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        let cipher = Aes256GcmSiv::new_from_slice(dek)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let physical_len = file.metadata()?.len();
        let logical_len = physical_to_logical_len(physical_len);

        Ok(Self {
            state: Mutex::new(FileState { file, logical_len }),
            cipher,
            read_profile: maybe_create_read_profile(),
        })
    }

    /// Opens an existing encrypted backend.
    pub fn open(path: impl AsRef<Path>, dek: &[u8; 32]) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;

        let cipher = Aes256GcmSiv::new_from_slice(dek)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let physical_len = file.metadata()?.len();
        let logical_len = physical_to_logical_len(physical_len);

        Ok(Self {
            state: Mutex::new(FileState { file, logical_len }),
            cipher,
            read_profile: maybe_create_read_profile(),
        })
    }

    pub(crate) fn read_profile_handle(&self) -> Option<EncryptedReadProfileHandle> {
        self.read_profile
            .as_ref()
            .map(|stats| EncryptedReadProfileHandle {
                stats: Arc::clone(stats),
            })
    }

    /// Encrypts a page and writes it to the physical offset.
    ///
    /// Caller must hold the state lock and pass the file reference.
    fn write_encrypted_page(
        &self,
        file: &mut File,
        page_index: u64,
        plaintext: &[u8],
    ) -> io::Result<()> {
        let mut page_data = [0u8; LOGICAL_PAGE_SIZE];
        let copy_len = plaintext.len().min(LOGICAL_PAGE_SIZE);
        page_data[..copy_len].copy_from_slice(&plaintext[..copy_len]);

        // Generate random nonce from OS entropy
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let aad = build_aad(page_index);

        let tag = self
            .cipher
            .encrypt_in_place_detached(nonce, &aad, &mut page_data)
            .map_err(|e| io::Error::other(format!("encryption failed: {e}")))?;

        let mut physical_page = [0u8; PHYSICAL_PAGE_SIZE];
        physical_page[..NONCE_SIZE].copy_from_slice(&nonce_bytes);
        physical_page[NONCE_SIZE..NONCE_SIZE + LOGICAL_PAGE_SIZE].copy_from_slice(&page_data);
        physical_page[NONCE_SIZE + LOGICAL_PAGE_SIZE..].copy_from_slice(tag.as_slice());

        // Write to physical offset as one slot: nonce || ciphertext || tag
        let physical_offset = page_index * PHYSICAL_PAGE_SIZE as u64;
        file.seek(SeekFrom::Start(physical_offset))?;
        file.write_all(&physical_page)?;

        Ok(())
    }

    /// Reads and decrypts a page from the physical offset.
    ///
    /// Caller must hold the state lock and pass the file reference.
    fn read_encrypted_page(
        &self,
        file: &mut File,
        page_index: u64,
    ) -> io::Result<[u8; LOGICAL_PAGE_SIZE]> {
        let physical_offset = page_index * PHYSICAL_PAGE_SIZE as u64;
        let mut physical_page = [0u8; PHYSICAL_PAGE_SIZE];
        let read_started = Instant::now();
        file.seek(SeekFrom::Start(physical_offset))?;
        file.read_exact(&mut physical_page)?;
        let file_read_elapsed = read_started.elapsed();

        let (nonce_bytes, encrypted_page) = physical_page.split_at(NONCE_SIZE);
        let (ciphertext, tag_bytes) = encrypted_page.split_at(LOGICAL_PAGE_SIZE);
        let mut page_data = [0u8; LOGICAL_PAGE_SIZE];
        page_data.copy_from_slice(ciphertext);

        let nonce = Nonce::from_slice(&nonce_bytes);
        let aad = build_aad(page_index);

        let decrypt_started = Instant::now();
        self.cipher
            .decrypt_in_place_detached(nonce, &aad, &mut page_data, Tag::from_slice(&tag_bytes))
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "decryption failed: wrong key or corrupted page",
                )
            })?;
        let decrypt_elapsed = decrypt_started.elapsed();

        if let Some(profile) = &self.read_profile {
            profile.record_page_read(file_read_elapsed, decrypt_elapsed);
        }

        Ok(page_data)
    }
}

impl StorageBackend for EncryptedFileBackend {
    fn len(&self) -> io::Result<u64> {
        Ok(self.state.lock().logical_len)
    }

    fn read(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }

        if let Some(profile) = &self.read_profile {
            profile.record_read_call(len);
        }

        let mut state = self.state.lock();

        if offset + len as u64 > state.logical_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "read beyond end: offset={offset}, len={len}, logical_len={}",
                    state.logical_len
                ),
            ));
        }

        let mut result = Vec::with_capacity(len);
        let start_page = offset / LOGICAL_PAGE_SIZE as u64;
        let end_page = (offset + len as u64 - 1) / LOGICAL_PAGE_SIZE as u64;

        for page_index in start_page..=end_page {
            let page_start = page_index * LOGICAL_PAGE_SIZE as u64;
            let plaintext = self.read_encrypted_page(&mut state.file, page_index)?;

            let page_offset = if page_index == start_page {
                (offset - page_start) as usize
            } else {
                0
            };

            let page_end = if page_index == end_page {
                let end_in_page = (offset + len as u64 - page_start) as usize;
                end_in_page.min(LOGICAL_PAGE_SIZE)
            } else {
                LOGICAL_PAGE_SIZE
            };

            result.extend_from_slice(&plaintext[page_offset..page_end]);
        }

        Ok(result)
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        let mut state = self.state.lock();

        if len > state.logical_len {
            let current_pages = state.logical_len.div_ceil(LOGICAL_PAGE_SIZE as u64);
            let new_pages = len.div_ceil(LOGICAL_PAGE_SIZE as u64);

            // If current length is not page-aligned, re-encrypt the last page
            // to ensure the extended portion is zero-filled
            if state.logical_len > 0 && !state.logical_len.is_multiple_of(LOGICAL_PAGE_SIZE as u64)
            {
                let last_page = current_pages - 1;
                let last_page_start = last_page * LOGICAL_PAGE_SIZE as u64;
                let last_page_used = (state.logical_len - last_page_start) as usize;

                let mut full_page = self.read_encrypted_page(&mut state.file, last_page)?;
                full_page[last_page_used..].fill(0);

                self.write_encrypted_page(&mut state.file, last_page, &full_page)?;
            }

            // Create new zero pages
            let zero_page = [0u8; LOGICAL_PAGE_SIZE];
            for page_index in current_pages..new_pages {
                self.write_encrypted_page(&mut state.file, page_index, &zero_page)?;
            }

            // Update physical file length
            let physical_len = new_pages * PHYSICAL_PAGE_SIZE as u64;
            state.file.set_len(physical_len)?;
        } else if len < state.logical_len {
            let new_pages = len.div_ceil(LOGICAL_PAGE_SIZE as u64);
            let physical_len = new_pages * PHYSICAL_PAGE_SIZE as u64;
            state.file.set_len(physical_len)?;
        }

        state.logical_len = len;
        Ok(())
    }

    fn sync_data(&self, _eventual: bool) -> io::Result<()> {
        self.state.lock().file.sync_data()
    }

    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let mut state = self.state.lock();

        if offset + data.len() as u64 > state.logical_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "write beyond end: offset={offset}, len={}, logical_len={}",
                    data.len(),
                    state.logical_len
                ),
            ));
        }

        let start_page = offset / LOGICAL_PAGE_SIZE as u64;
        let end_page = (offset + data.len() as u64 - 1) / LOGICAL_PAGE_SIZE as u64;

        let mut data_offset = 0usize;

        for page_index in start_page..=end_page {
            let page_start = page_index * LOGICAL_PAGE_SIZE as u64;

            let page_offset = if page_index == start_page {
                (offset - page_start) as usize
            } else {
                0
            };

            let write_len = {
                let remaining = data.len() - data_offset;
                let available = LOGICAL_PAGE_SIZE - page_offset;
                remaining.min(available)
            };

            // Read existing page content for partial writes (read-modify-write)
            let mut page_data = if page_offset > 0 || write_len < LOGICAL_PAGE_SIZE {
                self.read_encrypted_page(&mut state.file, page_index)
                    .unwrap_or([0u8; LOGICAL_PAGE_SIZE])
            } else {
                [0u8; LOGICAL_PAGE_SIZE]
            };

            page_data[page_offset..page_offset + write_len]
                .copy_from_slice(&data[data_offset..data_offset + write_len]);

            self.write_encrypted_page(&mut state.file, page_index, &page_data)?;

            data_offset += write_len;
        }

        Ok(())
    }
}

fn maybe_create_read_profile() -> Option<Arc<EncryptedReadProfile>> {
    if std::env::var_os("NEOVEX_REDB_OPEN_PROFILE").is_some()
        || std::env::var_os("NEOVEX_REDB_IO_PROFILE").is_some()
    {
        Some(Arc::new(EncryptedReadProfile::default()))
    } else {
        None
    }
}

/// Converts physical file length to logical length.
fn physical_to_logical_len(physical_len: u64) -> u64 {
    if physical_len == 0 {
        return 0;
    }

    let physical_pages = physical_len / PHYSICAL_PAGE_SIZE as u64;
    physical_pages * LOGICAL_PAGE_SIZE as u64
}

/// Internal state for the in-memory backend, protected by a single mutex.
struct MemoryState {
    data: Vec<u8>,
    logical_len: u64,
}

/// An in-memory encrypted backend for testing.
///
/// This wraps an in-memory buffer with encryption, useful for tests that
/// need to verify encryption behavior without touching the filesystem.
pub struct EncryptedMemoryBackend {
    /// All mutable state under a single lock to prevent TOCTOU races.
    state: Mutex<MemoryState>,

    /// The cipher instance for encryption/decryption.
    cipher: Aes256GcmSiv,
}

impl std::fmt::Debug for EncryptedMemoryBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let logical_len = self.state.lock().logical_len;
        f.debug_struct("EncryptedMemoryBackend")
            .field("logical_len", &logical_len)
            .finish_non_exhaustive()
    }
}

impl EncryptedMemoryBackend {
    /// Creates a new in-memory encrypted backend.
    pub fn new(dek: &[u8; 32]) -> io::Result<Self> {
        let cipher = Aes256GcmSiv::new_from_slice(dek)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        Ok(Self {
            state: Mutex::new(MemoryState {
                data: Vec::new(),
                logical_len: 0,
            }),
            cipher,
        })
    }

    /// Encrypts a page and writes it into the buffer.
    ///
    /// Caller must hold the state lock and pass a mutable reference to `data`.
    fn write_encrypted_page(
        &self,
        data: &mut Vec<u8>,
        page_index: u64,
        plaintext: &[u8],
    ) -> io::Result<()> {
        let mut page_data = [0u8; LOGICAL_PAGE_SIZE];
        let copy_len = plaintext.len().min(LOGICAL_PAGE_SIZE);
        page_data[..copy_len].copy_from_slice(&plaintext[..copy_len]);

        // Generate random nonce from OS entropy
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let aad = build_aad(page_index);

        let tag = self
            .cipher
            .encrypt_in_place_detached(nonce, &aad, &mut page_data)
            .map_err(|e| io::Error::other(format!("encryption failed: {e}")))?;

        let physical_offset = (page_index as usize) * PHYSICAL_PAGE_SIZE;
        let physical_end = physical_offset + PHYSICAL_PAGE_SIZE;

        // Ensure buffer is large enough
        if data.len() < physical_end {
            data.resize(physical_end, 0);
        }

        data[physical_offset..physical_offset + NONCE_SIZE].copy_from_slice(&nonce_bytes);
        data[physical_offset + NONCE_SIZE..physical_offset + NONCE_SIZE + LOGICAL_PAGE_SIZE]
            .copy_from_slice(&page_data);
        data[physical_offset + NONCE_SIZE + LOGICAL_PAGE_SIZE..physical_end]
            .copy_from_slice(tag.as_slice());

        Ok(())
    }

    /// Reads and decrypts a page from the buffer.
    ///
    /// Caller must hold the state lock and pass a reference to `data`.
    fn read_encrypted_page(
        &self,
        data: &[u8],
        page_index: u64,
    ) -> io::Result<[u8; LOGICAL_PAGE_SIZE]> {
        let physical_offset = (page_index as usize) * PHYSICAL_PAGE_SIZE;
        let physical_end = physical_offset + PHYSICAL_PAGE_SIZE;

        if data.len() < physical_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read beyond end of buffer",
            ));
        }

        let nonce = Nonce::from_slice(&data[physical_offset..physical_offset + NONCE_SIZE]);
        let mut page_data = [0u8; LOGICAL_PAGE_SIZE];
        page_data.copy_from_slice(
            &data[physical_offset + NONCE_SIZE..physical_offset + NONCE_SIZE + LOGICAL_PAGE_SIZE],
        );
        let tag =
            Tag::from_slice(&data[physical_offset + NONCE_SIZE + LOGICAL_PAGE_SIZE..physical_end]);

        let aad = build_aad(page_index);

        self.cipher
            .decrypt_in_place_detached(nonce, &aad, &mut page_data, tag)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "decryption failed: wrong key or corrupted page",
                )
            })?;

        Ok(page_data)
    }
}

impl StorageBackend for EncryptedMemoryBackend {
    fn len(&self) -> io::Result<u64> {
        Ok(self.state.lock().logical_len)
    }

    fn read(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }

        let state = self.state.lock();

        if offset + len as u64 > state.logical_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read beyond end",
            ));
        }

        let mut result = Vec::with_capacity(len);
        let start_page = offset / LOGICAL_PAGE_SIZE as u64;
        let end_page = (offset + len as u64 - 1) / LOGICAL_PAGE_SIZE as u64;

        for page_index in start_page..=end_page {
            let page_start = page_index * LOGICAL_PAGE_SIZE as u64;
            let plaintext = self.read_encrypted_page(&state.data, page_index)?;

            let page_offset = if page_index == start_page {
                (offset - page_start) as usize
            } else {
                0
            };

            let page_end = if page_index == end_page {
                let end_in_page = (offset + len as u64 - page_start) as usize;
                end_in_page.min(LOGICAL_PAGE_SIZE)
            } else {
                LOGICAL_PAGE_SIZE
            };

            result.extend_from_slice(&plaintext[page_offset..page_end]);
        }

        Ok(result)
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        let mut state = self.state.lock();

        if len > state.logical_len {
            let current_pages = state.logical_len.div_ceil(LOGICAL_PAGE_SIZE as u64);
            let new_pages = len.div_ceil(LOGICAL_PAGE_SIZE as u64);

            // Handle partial last page
            if state.logical_len > 0 && !state.logical_len.is_multiple_of(LOGICAL_PAGE_SIZE as u64)
            {
                let last_page = current_pages - 1;
                let last_page_start = last_page * LOGICAL_PAGE_SIZE as u64;
                let last_page_used = (state.logical_len - last_page_start) as usize;

                let mut full_page = self.read_encrypted_page(&state.data, last_page)?;
                full_page[last_page_used..].fill(0);
                self.write_encrypted_page(&mut state.data, last_page, &full_page)?;
            }

            // Create new zero pages
            let zero_page = [0u8; LOGICAL_PAGE_SIZE];
            for page_index in current_pages..new_pages {
                self.write_encrypted_page(&mut state.data, page_index, &zero_page)?;
            }
        } else if len < state.logical_len {
            let new_pages = len.div_ceil(LOGICAL_PAGE_SIZE as u64);
            let physical_len = (new_pages as usize) * PHYSICAL_PAGE_SIZE;
            state.data.truncate(physical_len);
        }

        state.logical_len = len;
        Ok(())
    }

    fn sync_data(&self, _eventual: bool) -> io::Result<()> {
        Ok(())
    }

    fn write(&self, offset: u64, data_to_write: &[u8]) -> io::Result<()> {
        if data_to_write.is_empty() {
            return Ok(());
        }

        let mut state = self.state.lock();

        if offset + data_to_write.len() as u64 > state.logical_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write beyond end",
            ));
        }

        let start_page = offset / LOGICAL_PAGE_SIZE as u64;
        let end_page = (offset + data_to_write.len() as u64 - 1) / LOGICAL_PAGE_SIZE as u64;

        let mut data_offset = 0usize;

        for page_index in start_page..=end_page {
            let page_start = page_index * LOGICAL_PAGE_SIZE as u64;

            let page_offset = if page_index == start_page {
                (offset - page_start) as usize
            } else {
                0
            };

            let write_len = {
                let remaining = data_to_write.len() - data_offset;
                let available = LOGICAL_PAGE_SIZE - page_offset;
                remaining.min(available)
            };

            let mut page_data = if page_offset > 0 || write_len < LOGICAL_PAGE_SIZE {
                self.read_encrypted_page(&state.data, page_index)
                    .unwrap_or([0u8; LOGICAL_PAGE_SIZE])
            } else {
                [0u8; LOGICAL_PAGE_SIZE]
            };

            page_data[page_offset..page_offset + write_len]
                .copy_from_slice(&data_to_write[data_offset..data_offset + write_len]);

            self.write_encrypted_page(&mut state.data, page_index, &page_data)?;

            data_offset += write_len;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encrypted_memory_backend_basic_operations() {
        let dek = [0x42u8; 32];
        let backend = EncryptedMemoryBackend::new(&dek).expect("backend should create");

        // Set initial length
        backend.set_len(8192).expect("set_len should work");
        assert_eq!(backend.len().unwrap(), 8192);

        // Write some data
        let data = b"Hello, encrypted world!";
        backend.write(100, data).expect("write should work");

        // Read it back
        let read = backend.read(100, data.len()).expect("read should work");
        assert_eq!(&read, data);
    }

    #[test]
    fn encrypted_memory_backend_cross_page_read_write() {
        let dek = [0x42u8; 32];
        let backend = EncryptedMemoryBackend::new(&dek).expect("backend should create");

        // Set length for 3 pages
        backend
            .set_len(LOGICAL_PAGE_SIZE as u64 * 3)
            .expect("set_len should work");

        // Write data that spans page boundary
        let offset = (LOGICAL_PAGE_SIZE - 10) as u64;
        let data = vec![0xAB; 30]; // Spans from page 0 into page 1
        backend.write(offset, &data).expect("write should work");

        // Read it back
        let read = backend.read(offset, data.len()).expect("read should work");
        assert_eq!(read, data);
    }

    #[test]
    fn encrypted_memory_backend_wrong_key_fails() {
        let dek1 = [0x42u8; 32];
        let dek2 = [0x43u8; 32];

        let backend1 = EncryptedMemoryBackend::new(&dek1).expect("backend should create");
        backend1.set_len(4096).expect("set_len should work");
        backend1
            .write(0, b"secret data")
            .expect("write should work");

        // Read the raw encrypted data
        let raw_data = backend1.state.lock().data.clone();

        // Create a new backend with different key and inject the raw data
        let backend2 = EncryptedMemoryBackend::new(&dek2).expect("backend should create");
        {
            let mut state = backend2.state.lock();
            state.data = raw_data;
            state.logical_len = 4096;
        }

        // Reading should fail due to authentication
        let result = backend2.read(0, 11);
        assert!(result.is_err());
    }

    #[test]
    fn encrypted_file_backend_create_and_reopen() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("test.redb.enc");
        let dek = [0x42u8; 32];

        // Create and write
        {
            let backend = EncryptedFileBackend::create(&path, &dek).expect("backend should create");
            backend.set_len(8192).expect("set_len should work");
            backend
                .write(0, b"persistent data")
                .expect("write should work");
            backend.sync_data(false).expect("sync should work");
        }

        // Reopen and read
        {
            let backend = EncryptedFileBackend::open(&path, &dek).expect("backend should open");
            assert_eq!(backend.len().unwrap(), 8192);
            let read = backend.read(0, 15).expect("read should work");
            assert_eq!(&read, b"persistent data");
        }
    }

    #[test]
    fn encrypted_file_backend_with_redb() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("test.redb");
        let dek = [0x42u8; 32];

        // Create database with encrypted backend
        let backend = EncryptedFileBackend::create(&path, &dek).expect("backend should create");
        let db = redb::Database::builder()
            .create_with_backend(backend)
            .expect("database should create");

        // Write some data
        {
            let table_def: redb::TableDefinition<&str, &str> =
                redb::TableDefinition::new("test_table");
            let write_txn = db.begin_write().expect("begin_write should work");
            {
                let mut table = write_txn
                    .open_table(table_def)
                    .expect("open_table should work");
                table.insert("key1", "value1").expect("insert should work");
            }
            write_txn.commit().expect("commit should work");
        }

        // Read it back
        {
            let table_def: redb::TableDefinition<&str, &str> =
                redb::TableDefinition::new("test_table");
            let read_txn = db.begin_read().expect("begin_read should work");
            let table = read_txn
                .open_table(table_def)
                .expect("open_table should work");
            let value = table.get("key1").expect("get should work");
            assert_eq!(value.unwrap().value(), "value1");
        }

        drop(db);

        // Reopen with same key
        let backend = EncryptedFileBackend::open(&path, &dek).expect("backend should open");
        let db = redb::Database::builder()
            .create_with_backend(backend)
            .expect("database should open");

        // Verify data persisted
        {
            let table_def: redb::TableDefinition<&str, &str> =
                redb::TableDefinition::new("test_table");
            let read_txn = db.begin_read().expect("begin_read should work");
            let table = read_txn
                .open_table(table_def)
                .expect("open_table should work");
            let value = table.get("key1").expect("get should work");
            assert_eq!(value.unwrap().value(), "value1");
        }
    }

    #[test]
    fn encrypted_file_backend_wrong_key_fails_reopen() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("test.redb");
        let dek1 = [0x42u8; 32];
        let dek2 = [0x43u8; 32];

        // Create database with one key
        {
            let backend =
                EncryptedFileBackend::create(&path, &dek1).expect("backend should create");
            let db = redb::Database::builder()
                .create_with_backend(backend)
                .expect("database should create");

            let table_def: redb::TableDefinition<&str, &str> =
                redb::TableDefinition::new("test_table");
            let write_txn = db.begin_write().expect("begin_write should work");
            {
                let mut table = write_txn
                    .open_table(table_def)
                    .expect("open_table should work");
                table.insert("key1", "value1").expect("insert should work");
            }
            write_txn.commit().expect("commit should work");
        }

        // Try to open with wrong key
        let backend = EncryptedFileBackend::open(&path, &dek2).expect("backend should open");
        let result = redb::Database::builder().create_with_backend(backend);

        // Should fail during database open (when it tries to read the header)
        assert!(result.is_err());
    }

    #[test]
    fn page_swap_attack_is_detected() {
        let dek = [0x42u8; 32];
        let backend = EncryptedMemoryBackend::new(&dek).expect("backend should create");

        // Create two pages with different content
        backend
            .set_len(LOGICAL_PAGE_SIZE as u64 * 2)
            .expect("set_len should work");
        backend
            .write(0, b"page 0 content")
            .expect("write should work");
        backend
            .write(LOGICAL_PAGE_SIZE as u64, b"page 1 content")
            .expect("write should work");

        // Swap the encrypted pages
        {
            let mut state = backend.state.lock();
            let page0 = state.data[..PHYSICAL_PAGE_SIZE].to_vec();
            let page1 = state.data[PHYSICAL_PAGE_SIZE..PHYSICAL_PAGE_SIZE * 2].to_vec();
            state.data[..PHYSICAL_PAGE_SIZE].copy_from_slice(&page1);
            state.data[PHYSICAL_PAGE_SIZE..PHYSICAL_PAGE_SIZE * 2].copy_from_slice(&page0);
        }

        // Reading should fail because AAD includes page index
        let result = backend.read(0, 14);
        assert!(result.is_err());
    }
}
