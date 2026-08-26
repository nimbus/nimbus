//! Deterministic framed AEAD primitive for content-addressed blobs.
//!
//! This module owns the byte-plane framed construction used by `nimbus-blob`.
//! Existing database/page encryption remains on the manifest/random-nonce path;
//! this API is only for blob frames that need deterministic ciphertext for
//! same-tenant content-addressed dedup.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use aes_gcm_siv::aead::{Aead, KeyInit};
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use nimbus_core::{Error, Result, StorageErrorKind};
use rand::RngCore;
use rand::rngs::OsRng;

use super::key::DataEncryptionKey;

/// Fixed plaintext bytes per frame.
pub const FRAME_PLAINTEXT_LEN: usize = 64 * 1024;

/// Per-frame nonce size for AES-GCM-SIV.
pub const NONCE_LEN: usize = 12;

/// Per-blob key seed size.
pub const KEY_SEED_LEN: usize = 32;

/// Maximum wrapped data keys accepted in a framed blob header.
pub const MAX_WRAPPED_DATA_KEYS: usize = 1;

/// Upper plaintext bound for a single framed blob.
pub const MAX_PLAINTEXT_BYTES: u64 = u64::MAX / 2;

const SUBKEY_CONTEXT: &str = "nimbus-blob 2026 per-blob subkey";
const MAGIC_PREFIX: &[u8; 3] = b"NBF";
const FORMAT_VERSION: u8 = 2;
const MAGIC: &[u8; 4] = b"NBF2";
const HEADER_LEN: usize = 4 /* magic */ + 1 /* seed_kind */ + KEY_SEED_LEN + 8 /* plaintext_len */;

/// Framed blob algorithm suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramedAlgorithmSuite {
    /// AES-256-GCM-SIV as specified by RFC 8452.
    Aes256GcmSiv,
}

/// Security class advertised by a framed AEAD backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramedAeadSecurity {
    /// Deterministic and nonce-misuse resistant for the accepted dedup path.
    SivDeterministic,
    /// Deterministic nonce over a non-SIV AEAD; rejected for dedup.
    NonSivDeterministic,
    /// Nondeterministic AEAD; rejected for dedup.
    Nondeterministic,
}

/// Object-safe per-frame AEAD backend.
pub trait FramedAead: Send + Sync {
    fn suite(&self) -> FramedAlgorithmSuite;
    fn security(&self) -> FramedAeadSecurity;
    fn seal_frame(
        &self,
        subkey: &[u8; 32],
        frame_index: u64,
        aad: &[u8],
        frame: &[u8],
    ) -> Result<Vec<u8>>;
    fn open_frame(
        &self,
        subkey: &[u8; 32],
        frame_index: u64,
        aad: &[u8],
        sealed: &[u8],
    ) -> Result<Vec<u8>>;
    fn sealed_frame_len(&self, plaintext_len: usize) -> usize;
}

/// AES-256-GCM-SIV frame backend.
#[derive(Debug, Default)]
pub struct Aes256GcmSivFramedAead;

impl Aes256GcmSivFramedAead {
    fn nonce(frame_index: u64) -> [u8; NONCE_LEN] {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[NONCE_LEN - 8..].copy_from_slice(&frame_index.to_be_bytes());
        nonce
    }
}

impl FramedAead for Aes256GcmSivFramedAead {
    fn suite(&self) -> FramedAlgorithmSuite {
        FramedAlgorithmSuite::Aes256GcmSiv
    }

    fn security(&self) -> FramedAeadSecurity {
        FramedAeadSecurity::SivDeterministic
    }

    fn seal_frame(
        &self,
        subkey: &[u8; 32],
        frame_index: u64,
        aad: &[u8],
        frame: &[u8],
    ) -> Result<Vec<u8>> {
        let cipher = Aes256GcmSiv::new_from_slice(subkey).map_err(|error| {
            Error::Internal(format!("failed to construct framed AEAD backend: {error}"))
        })?;
        let nonce_bytes = Self::nonce(frame_index);
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher
            .encrypt(nonce, aes_gcm_siv::aead::Payload { msg: frame, aad })
            .map_err(|_| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "framed AEAD seal failed for AES-256-GCM-SIV",
                )
            })
    }

    fn open_frame(
        &self,
        subkey: &[u8; 32],
        frame_index: u64,
        aad: &[u8],
        sealed: &[u8],
    ) -> Result<Vec<u8>> {
        let cipher = Aes256GcmSiv::new_from_slice(subkey).map_err(|error| {
            Error::Internal(format!("failed to construct framed AEAD backend: {error}"))
        })?;
        let nonce_bytes = Self::nonce(frame_index);
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher
            .decrypt(nonce, aes_gcm_siv::aead::Payload { msg: sealed, aad })
            .map_err(|_| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "framed AEAD authentication failed",
                )
            })
    }

    fn sealed_frame_len(&self, plaintext_len: usize) -> usize {
        plaintext_len + 16
    }
}

/// Opaque tenant data key for framed blobs.
pub struct FramedBlobKey {
    data_key: DataEncryptionKey,
}

impl FramedBlobKey {
    pub fn new(data_key: DataEncryptionKey) -> Self {
        Self { data_key }
    }

    fn subkey(&self, key_seed: &[u8; KEY_SEED_LEN]) -> [u8; 32] {
        let context_key = blake3::derive_key(SUBKEY_CONTEXT, key_seed);
        let mut hasher = blake3::Hasher::new_keyed(&context_key);
        hasher.update(self.data_key.as_bytes());
        *hasher.finalize().as_bytes()
    }
}

impl fmt::Debug for FramedBlobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FramedBlobKey").field(&"[REDACTED]").finish()
    }
}

/// How the framed key seed was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramedSeedKind {
    /// `key_seed = BLAKE3(plaintext)` for dedup-preserving buffered puts.
    Content,
    /// Random per-object salt for streamed puts.
    Salt,
}

impl FramedSeedKind {
    fn tag(self) -> u8 {
        match self {
            Self::Content => 0,
            Self::Salt => 1,
        }
    }

    fn from_tag(tag: u8) -> Result<Self> {
        match tag {
            0 => Ok(Self::Content),
            1 => Ok(Self::Salt),
            other => Err(Error::storage(
                StorageErrorKind::Corruption,
                format!("unknown framed-ciphertext seed kind {other}"),
            )),
        }
    }
}

/// Key seed policy for a seal operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramedBlobSeed {
    Content,
    Salt([u8; KEY_SEED_LEN]),
}

/// Parsed framed header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramedBlobHeader {
    pub seed_kind: FramedSeedKind,
    pub key_seed: [u8; KEY_SEED_LEN],
    pub plaintext_len: usize,
    pub frame_count: u64,
}

impl FramedBlobHeader {
    pub fn parse(framed: &[u8]) -> Result<(Self, &[u8])> {
        if framed.len() < HEADER_LEN {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "framed ciphertext shorter than header",
            ));
        }
        if &framed[..4] != MAGIC {
            if &framed[..3] == MAGIC_PREFIX && framed[3].is_ascii_digit() {
                return Err(Error::InvalidInput(format!(
                    "unsupported framed-ciphertext version {}; current version is {FORMAT_VERSION}",
                    framed[3] - b'0'
                )));
            }
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "bad framed-ciphertext magic",
            ));
        }
        let seed_kind = FramedSeedKind::from_tag(framed[4])?;
        let mut key_seed = [0u8; KEY_SEED_LEN];
        key_seed.copy_from_slice(&framed[5..5 + KEY_SEED_LEN]);
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&framed[5 + KEY_SEED_LEN..HEADER_LEN]);
        let plaintext_len_u64 = u64::from_be_bytes(len_bytes);
        if plaintext_len_u64 > MAX_PLAINTEXT_BYTES || plaintext_len_u64 > usize::MAX as u64 {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "framed plaintext bound exceeded",
            ));
        }
        let plaintext_len = plaintext_len_u64 as usize;
        Ok((
            Self {
                seed_kind,
                key_seed,
                plaintext_len,
                frame_count: frame_count(plaintext_len),
            },
            &framed[HEADER_LEN..],
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FramedSessionState {
    Ready,
    Finished,
    Failed,
}

/// Framed seal session with immutable header/materials after sealing starts.
pub struct FramedSealSession<'a> {
    key: &'a FramedBlobKey,
    aead: Arc<dyn FramedAead>,
    suite: FramedAlgorithmSuite,
    seed: FramedBlobSeed,
    state: FramedSessionState,
}

impl<'a> FramedSealSession<'a> {
    pub fn new(key: &'a FramedBlobKey, seed: FramedBlobSeed) -> Self {
        Self::with_aead(
            key,
            seed,
            FramedAlgorithmSuite::Aes256GcmSiv,
            Arc::new(Aes256GcmSivFramedAead),
        )
    }

    pub fn with_aead(
        key: &'a FramedBlobKey,
        seed: FramedBlobSeed,
        suite: FramedAlgorithmSuite,
        aead: Arc<dyn FramedAead>,
    ) -> Self {
        Self {
            key,
            aead,
            suite,
            seed,
            state: FramedSessionState::Ready,
        }
    }

    pub fn seal_all(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        if self.state != FramedSessionState::Ready {
            self.state = FramedSessionState::Failed;
            return Err(Error::conflict(
                "framed seal session is in terminal error state".to_string(),
            ));
        }
        validate_aead(self.suite, self.aead.as_ref()).inspect_err(|_| {
            self.state = FramedSessionState::Failed;
        })?;
        let plaintext_len = u64::try_from(plaintext.len()).map_err(|_| {
            self.state = FramedSessionState::Failed;
            Error::InvalidInput("framed plaintext length does not fit in u64".to_string())
        })?;
        if plaintext_len > MAX_PLAINTEXT_BYTES {
            self.state = FramedSessionState::Failed;
            return Err(Error::InvalidInput(format!(
                "framed plaintext exceeds {MAX_PLAINTEXT_BYTES} byte limit"
            )));
        }
        let (seed_kind, key_seed) = match self.seed {
            FramedBlobSeed::Content => (FramedSeedKind::Content, content_key_seed(plaintext)),
            FramedBlobSeed::Salt(seed) => (FramedSeedKind::Salt, seed),
        };
        let subkey = self.key.subkey(&key_seed);
        let frame_count = frame_count(plaintext.len());
        let mut out = Vec::with_capacity(HEADER_LEN + plaintext.len() + frame_count as usize * 16);
        out.extend_from_slice(MAGIC);
        out.push(seed_kind.tag());
        out.extend_from_slice(&key_seed);
        out.extend_from_slice(&plaintext_len.to_be_bytes());

        for (frame_index, frame) in plaintext.chunks(FRAME_PLAINTEXT_LEN).enumerate() {
            let frame_index = frame_index as u64;
            let aad = frame_aad(
                seed_kind,
                &key_seed,
                plaintext_len,
                frame_index,
                frame_count,
            );
            let sealed = self
                .aead
                .seal_frame(&subkey, frame_index, &aad, frame)
                .inspect_err(|_| {
                    self.state = FramedSessionState::Failed;
                })?;
            out.extend_from_slice(&sealed);
        }
        if plaintext.is_empty() {
            let aad = frame_aad(seed_kind, &key_seed, plaintext_len, 0, frame_count);
            let sealed = self
                .aead
                .seal_frame(&subkey, 0, &aad, &[])
                .inspect_err(|_| {
                    self.state = FramedSessionState::Failed;
                })?;
            out.extend_from_slice(&sealed);
        }
        self.state = FramedSessionState::Finished;
        Ok(out)
    }
}

/// Framed open session with terminal error state.
pub struct FramedOpenSession<'a> {
    key: &'a FramedBlobKey,
    aead: Arc<dyn FramedAead>,
    suite: FramedAlgorithmSuite,
    state: FramedSessionState,
}

impl<'a> FramedOpenSession<'a> {
    pub fn new(key: &'a FramedBlobKey) -> Self {
        Self::with_aead(
            key,
            FramedAlgorithmSuite::Aes256GcmSiv,
            Arc::new(Aes256GcmSivFramedAead),
        )
    }

    pub fn with_aead(
        key: &'a FramedBlobKey,
        suite: FramedAlgorithmSuite,
        aead: Arc<dyn FramedAead>,
    ) -> Self {
        Self {
            key,
            aead,
            suite,
            state: FramedSessionState::Ready,
        }
    }

    pub fn open_all(&mut self, framed: &[u8]) -> Result<Vec<u8>> {
        self.open_range(framed, 0..u64::MAX)
    }

    pub fn open_range(&mut self, framed: &[u8], range: Range<u64>) -> Result<Vec<u8>> {
        if self.state != FramedSessionState::Ready {
            self.state = FramedSessionState::Failed;
            return Err(Error::conflict(
                "framed open session is in terminal error state".to_string(),
            ));
        }
        validate_aead(self.suite, self.aead.as_ref()).inspect_err(|_| {
            self.state = FramedSessionState::Failed;
        })?;
        let result = open_range_with_aead(self.key, self.aead.as_ref(), framed, range);
        match result {
            Ok(bytes) => {
                self.state = FramedSessionState::Finished;
                Ok(bytes)
            }
            Err(error) => {
                self.state = FramedSessionState::Failed;
                Err(error)
            }
        }
    }
}

pub fn seal_framed_blob(
    key: &FramedBlobKey,
    seed: FramedBlobSeed,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    FramedSealSession::new(key, seed).seal_all(plaintext)
}

pub fn open_framed_blob(key: &FramedBlobKey, framed: &[u8]) -> Result<Vec<u8>> {
    FramedOpenSession::new(key).open_all(framed)
}

pub fn open_framed_blob_range(
    key: &FramedBlobKey,
    framed: &[u8],
    range: Range<u64>,
) -> Result<Vec<u8>> {
    FramedOpenSession::new(key).open_range(framed, range)
}

/// Fixed byte length of a framed blob header (magic + seed_kind + key_seed +
/// plaintext_len).
///
/// A byte-plane store that wants to avoid a whole-blob fetch reads exactly
/// this many bytes first (a bounded probe) to get enough bytes for
/// [`FramedBlobHeader::parse`], then uses [`framed_span_for_plaintext_range`]
/// to size its second, targeted fetch.
pub const FRAMED_HEADER_LEN: usize = HEADER_LEN;

/// Computes the framed-blob byte span (relative to byte 0 of the framed blob,
/// i.e. including the header) that covers every frame overlapping
/// `plaintext_range`.
///
/// Pairs with [`open_framed_span`]: a byte-plane store fetches exactly this
/// span from the underlying substrate (in addition to the `FRAMED_HEADER_LEN`
/// header probe used to obtain `header`), then passes the fetched bytes to
/// `open_framed_span` to recover the exact plaintext slice. This is what lets
/// `EncryptedBlobStore::get_range` transfer only the overlapping frames
/// instead of the whole ciphertext.
pub fn framed_span_for_plaintext_range(
    header: &FramedBlobHeader,
    plaintext_range: Range<u64>,
) -> Result<Range<u64>> {
    let len = header.plaintext_len as u64;
    if plaintext_range.start > plaintext_range.end || plaintext_range.end > len {
        return Err(Error::InvalidInput(format!(
            "range {}..{} out of bounds for framed plaintext of {len} bytes",
            plaintext_range.start, plaintext_range.end
        )));
    }
    let header_len = HEADER_LEN as u64;
    if plaintext_range.start == plaintext_range.end {
        return Ok(header_len..header_len);
    }
    let aead = Aes256GcmSivFramedAead;
    let first_frame = plaintext_range.start as usize / FRAME_PLAINTEXT_LEN;
    let last_frame = (plaintext_range.end as usize - 1) / FRAME_PLAINTEXT_LEN;
    let start_offset = sealed_offset_for_frame(&aead, first_frame as u64)?;
    let last_frame_plaintext_len = frame_plaintext_len(header.plaintext_len, last_frame as u64);
    let last_frame_sealed_len = aead.sealed_frame_len(last_frame_plaintext_len);
    let end_offset = sealed_offset_for_frame(&aead, last_frame as u64)?
        .checked_add(last_frame_sealed_len)
        .ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "framed ciphertext offset overflow",
            )
        })?;
    Ok(header_len + start_offset as u64..header_len + end_offset as u64)
}

/// Decrypts exactly `plaintext_range` from a pre-fetched framed span.
///
/// `framed_span` must be exactly the bytes at the range returned by
/// [`framed_span_for_plaintext_range`] for the same `header` and
/// `plaintext_range`: sealed frame bytes only (the header prefix already
/// stripped), aligned to full frame boundaries, containing every frame that
/// overlaps `plaintext_range` and nothing else. AEAD semantics are identical
/// to a full [`open_framed_blob`]: the true absolute frame index is used for
/// both the nonce and the AAD, so a frame decrypted out of its positional
/// context still authenticates exactly as it would in a whole-blob open.
///
/// This does not perform the whole-blob body-length or content-seed checks
/// that a full-range [`open_framed_blob_range`] call performs (those require
/// having fetched the entire ciphertext); callers that need those checks for
/// a full-range request should keep using the whole-fetch path instead.
pub fn open_framed_span(
    key: &FramedBlobKey,
    header: &FramedBlobHeader,
    framed_span: &[u8],
    plaintext_range: Range<u64>,
) -> Result<Vec<u8>> {
    let len = header.plaintext_len as u64;
    if plaintext_range.start > plaintext_range.end || plaintext_range.end > len {
        return Err(Error::InvalidInput(format!(
            "range {}..{} out of bounds for framed plaintext of {len} bytes",
            plaintext_range.start, plaintext_range.end
        )));
    }
    if plaintext_range.start == plaintext_range.end {
        return Ok(Vec::new());
    }
    let aead = Aes256GcmSivFramedAead;
    let subkey = key.subkey(&header.key_seed);
    let first_frame = plaintext_range.start as usize / FRAME_PLAINTEXT_LEN;
    let last_frame = (plaintext_range.end as usize - 1) / FRAME_PLAINTEXT_LEN;
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for frame_index in first_frame..=last_frame {
        let plaintext_len = frame_plaintext_len(header.plaintext_len, frame_index as u64);
        let sealed_len = aead.sealed_frame_len(plaintext_len);
        let end = cursor.checked_add(sealed_len).ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "sealed frame range overflows framed span",
            )
        })?;
        if end > framed_span.len() {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "truncated framed span",
            ));
        }
        let aad = frame_aad(
            header.seed_kind,
            &header.key_seed,
            header.plaintext_len as u64,
            frame_index as u64,
            header.frame_count,
        );
        let frame =
            aead.open_frame(&subkey, frame_index as u64, &aad, &framed_span[cursor..end])?;
        if frame.len() != plaintext_len {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "framed AEAD returned unexpected plaintext length",
            ));
        }
        let frame_start = (frame_index as u64)
            .checked_mul(FRAME_PLAINTEXT_LEN as u64)
            .ok_or_else(|| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "frame plaintext range overflows blob length",
                )
            })?;
        let frame_end = frame_start.checked_add(frame.len() as u64).ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "frame plaintext range overflows blob length",
            )
        })?;
        let copy_start = plaintext_range.start.saturating_sub(frame_start) as usize;
        let copy_end = (plaintext_range.end.min(frame_end) - frame_start) as usize;
        out.extend_from_slice(&frame[copy_start..copy_end]);
        cursor = end;
    }
    Ok(out)
}

pub fn random_framed_salt() -> [u8; KEY_SEED_LEN] {
    let mut salt = [0u8; KEY_SEED_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

fn validate_aead(suite: FramedAlgorithmSuite, aead: &dyn FramedAead) -> Result<()> {
    if aead.suite() != suite {
        return Err(Error::InvalidInput(format!(
            "framed AEAD backend suite {:?} does not match requested {:?}",
            aead.suite(),
            suite
        )));
    }
    if aead.security() != FramedAeadSecurity::SivDeterministic {
        return Err(Error::InvalidInput(format!(
            "framed AEAD backend {:?} is rejected for the dedup path; RFC 8452 AES-256-GCM-SIV or an explicit synthetic-IV construction is required",
            aead.security()
        )));
    }
    Ok(())
}

fn open_range_with_aead(
    key: &FramedBlobKey,
    aead: &dyn FramedAead,
    framed: &[u8],
    range: Range<u64>,
) -> Result<Vec<u8>> {
    let (header, body) = FramedBlobHeader::parse(framed)?;
    let len = header.plaintext_len as u64;
    let requested = if range.start == 0 && range.end == u64::MAX {
        0..len
    } else {
        range
    };
    if requested.start > requested.end || requested.end > len {
        return Err(Error::InvalidInput(format!(
            "range {}..{} out of bounds for framed plaintext of {len} bytes",
            requested.start, requested.end
        )));
    }
    if requested.start == 0 && requested.end == len {
        let expected_body_len =
            sealed_body_len_for_header(aead, header.plaintext_len, header.frame_count)?;
        if body.len() != expected_body_len {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "framed ciphertext body length mismatch",
            ));
        }
    }
    if requested.start == requested.end {
        return Ok(Vec::new());
    }
    let subkey = key.subkey(&header.key_seed);
    let first_frame = requested.start as usize / FRAME_PLAINTEXT_LEN;
    let last_frame = (requested.end as usize - 1) / FRAME_PLAINTEXT_LEN;
    let mut out = Vec::new();
    for frame_index in first_frame..=last_frame {
        let plaintext_len = frame_plaintext_len(header.plaintext_len, frame_index as u64);
        let offset = sealed_offset_for_frame(aead, frame_index as u64)?;
        let sealed_len = aead.sealed_frame_len(plaintext_len);
        let end = offset.checked_add(sealed_len).ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "sealed frame range overflows framed ciphertext length",
            )
        })?;
        if end > body.len() {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "truncated framed ciphertext",
            ));
        }
        let aad = frame_aad(
            header.seed_kind,
            &header.key_seed,
            header.plaintext_len as u64,
            frame_index as u64,
            header.frame_count,
        );
        let frame = aead.open_frame(&subkey, frame_index as u64, &aad, &body[offset..end])?;
        if frame.len() != plaintext_len {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "framed AEAD returned unexpected plaintext length",
            ));
        }
        let frame_start = (frame_index as u64)
            .checked_mul(FRAME_PLAINTEXT_LEN as u64)
            .ok_or_else(|| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "frame plaintext range overflows blob length",
                )
            })?;
        let frame_end = frame_start.checked_add(frame.len() as u64).ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "frame plaintext range overflows blob length",
            )
        })?;
        let copy_start = requested.start.saturating_sub(frame_start) as usize;
        let copy_end = (requested.end.min(frame_end) - frame_start) as usize;
        out.extend_from_slice(&frame[copy_start..copy_end]);
    }
    if header.seed_kind == FramedSeedKind::Content && requested.start == 0 && requested.end == len {
        let recomputed = blake3::hash(&out);
        if recomputed.as_bytes() != &header.key_seed {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "content seed mismatch: decrypted plaintext does not match key_seed",
            ));
        }
    }
    Ok(out)
}

fn content_key_seed(plaintext: &[u8]) -> [u8; KEY_SEED_LEN] {
    *blake3::hash(plaintext).as_bytes()
}

fn frame_count(plaintext_len: usize) -> u64 {
    if plaintext_len == 0 {
        1
    } else {
        plaintext_len.div_ceil(FRAME_PLAINTEXT_LEN) as u64
    }
}

fn frame_plaintext_len(total_len: usize, frame_index: u64) -> usize {
    if total_len == 0 {
        return 0;
    }
    let start = frame_index as usize * FRAME_PLAINTEXT_LEN;
    let remaining = total_len.saturating_sub(start);
    remaining.min(FRAME_PLAINTEXT_LEN)
}

fn sealed_offset_for_frame(aead: &dyn FramedAead, frame_index: u64) -> Result<usize> {
    let full_frame_len = aead.sealed_frame_len(FRAME_PLAINTEXT_LEN);
    let frame_index = usize::try_from(frame_index).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "framed ciphertext frame index exceeds host address width",
        )
    })?;
    frame_index.checked_mul(full_frame_len).ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            "framed ciphertext offset overflow",
        )
    })
}

fn sealed_body_len_for_header(
    aead: &dyn FramedAead,
    plaintext_len: usize,
    frame_count: u64,
) -> Result<usize> {
    if frame_count == 0 {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            "framed ciphertext has zero frames",
        ));
    }
    if frame_count == 1 {
        return Ok(aead.sealed_frame_len(frame_plaintext_len(plaintext_len, 0)));
    }
    let full_frames = usize::try_from(frame_count - 1).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "framed ciphertext frame count exceeds host address width",
        )
    })?;
    let full_frame_len = aead.sealed_frame_len(FRAME_PLAINTEXT_LEN);
    let prefix_len = full_frames.checked_mul(full_frame_len).ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            "framed ciphertext body length overflow",
        )
    })?;
    prefix_len
        .checked_add(aead.sealed_frame_len(frame_plaintext_len(plaintext_len, frame_count - 1)))
        .ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "framed ciphertext body length overflow",
            )
        })
}

fn frame_aad(
    seed_kind: FramedSeedKind,
    key_seed: &[u8; KEY_SEED_LEN],
    plaintext_len: u64,
    frame_index: u64,
    frame_count: u64,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(4 + 1 + KEY_SEED_LEN + 8 + 8 + 8);
    aad.extend_from_slice(MAGIC);
    aad.push(seed_kind.tag());
    aad.extend_from_slice(key_seed);
    aad.extend_from_slice(&plaintext_len.to_be_bytes());
    aad.extend_from_slice(&frame_index.to_be_bytes());
    aad.extend_from_slice(&frame_count.to_be_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn key(seed: &str) -> FramedBlobKey {
        FramedBlobKey::new(DataEncryptionKey::new(
            *blake3::hash(seed.as_bytes()).as_bytes(),
        ))
    }

    #[test]
    fn framed_blob_round_trips() {
        let key = key("tenant");
        let plaintext = b"top secret payload";

        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, plaintext).unwrap();
        let opened = open_framed_blob(&key, &framed).unwrap();

        assert_eq!(opened, plaintext);
    }

    #[test]
    fn framed_blob_reports_future_format_as_version_skew() {
        let key = key("tenant");
        let mut framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"payload").unwrap();
        framed[3] = b'3';

        let error = open_framed_blob(&key, &framed)
            .expect_err("future framed-ciphertext version must fail closed");

        assert_eq!(error.storage_kind(), None);
        assert!(
            matches!(error, Error::InvalidInput(ref message)
                if message.contains("unsupported framed-ciphertext version 3")
                    && message.contains("current version is 2")),
            "version skew must not report as corruption: {error}"
        );
    }

    #[test]
    fn framed_blob_keeps_unrecognized_magic_as_corruption() {
        let key = key("tenant");
        let mut framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"payload").unwrap();
        framed[..4].copy_from_slice(b"NOPE");

        let error = open_framed_blob(&key, &framed)
            .expect_err("unrecognized framed ciphertext must fail closed");

        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[test]
    fn seed_kind_is_authenticated() {
        let key = key("tenant");
        let mut framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"payload").unwrap();
        framed[4] = FramedSeedKind::Salt.tag();

        let error = open_framed_blob(&key, &framed).unwrap_err();

        assert!(matches!(error, Error::Storage { .. }));
    }

    #[test]
    fn full_open_rejects_extra_trailing_bytes() {
        let key = key("tenant");
        let mut framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"payload").unwrap();
        framed.extend_from_slice(b"trailing");

        let error = open_framed_blob(&key, &framed).unwrap_err();

        assert!(matches!(error, Error::Storage { .. }));
    }

    #[test]
    fn range_end_sentinel_only_applies_to_open_all() {
        let key = key("tenant");
        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"payload").unwrap();

        let error = open_framed_blob_range(&key, &framed, 1..u64::MAX).unwrap_err();

        assert!(matches!(error, Error::InvalidInput(_)));
    }

    #[test]
    fn range_open_rejects_truncated_claim_before_attacker_sized_preallocation() {
        let key = key("tenant");
        let claimed_len = (usize::MAX as u64).min(MAX_PLAINTEXT_BYTES);
        let mut framed = Vec::with_capacity(HEADER_LEN);
        framed.extend_from_slice(MAGIC);
        framed.push(FramedSeedKind::Salt.tag());
        framed.extend_from_slice(&[0x42; KEY_SEED_LEN]);
        framed.extend_from_slice(&claimed_len.to_be_bytes());
        assert_eq!(framed.len(), HEADER_LEN);

        let error = open_framed_blob_range(&key, &framed, 1..claimed_len).unwrap_err();

        assert!(matches!(error, Error::Storage { .. }));
    }

    #[test]
    fn deterministic_content_seed_preserves_dedup() {
        let key = key("tenant");
        let plaintext = b"dedup me";

        let first = seal_framed_blob(&key, FramedBlobSeed::Content, plaintext).unwrap();
        let second = seal_framed_blob(&key, FramedBlobSeed::Content, plaintext).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn crypto_salt_path_does_not_dedup() {
        let key = key("tenant");
        let plaintext = b"streamed bytes";

        let first =
            seal_framed_blob(&key, FramedBlobSeed::Salt(random_framed_salt()), plaintext).unwrap();
        let second =
            seal_framed_blob(&key, FramedBlobSeed::Salt(random_framed_salt()), plaintext).unwrap();

        assert_ne!(first, second);
        assert_eq!(open_framed_blob(&key, &first).unwrap(), plaintext);
        assert_eq!(open_framed_blob(&key, &second).unwrap(), plaintext);
    }

    #[test]
    fn algorithm_suite_rejects_nondeterministic_aead_backend() {
        let key = key("tenant");
        let mut session = FramedSealSession::with_aead(
            &key,
            FramedBlobSeed::Content,
            FramedAlgorithmSuite::Aes256GcmSiv,
            Arc::new(FakeAead {
                security: FramedAeadSecurity::Nondeterministic,
                opened: Mutex::new(0),
            }),
        );

        let error = session.seal_all(b"payload").unwrap_err();

        assert!(matches!(error, Error::InvalidInput(_)));
    }

    #[test]
    fn algorithm_suite_rejects_deterministic_nonce_over_gcm_without_siv_aead() {
        let key = key("tenant");
        let mut session = FramedSealSession::with_aead(
            &key,
            FramedBlobSeed::Content,
            FramedAlgorithmSuite::Aes256GcmSiv,
            Arc::new(FakeAead {
                security: FramedAeadSecurity::NonSivDeterministic,
                opened: Mutex::new(0),
            }),
        );

        let error = session.seal_all(b"payload").unwrap_err();

        assert!(matches!(error, Error::InvalidInput(_)));
    }

    #[test]
    fn session_rejects_reuse_after_finish() {
        let key = key("tenant");
        let mut session = FramedSealSession::new(&key, FramedBlobSeed::Content);

        session.seal_all(b"payload").unwrap();
        let error = session.seal_all(b"again").unwrap_err();

        assert!(matches!(error, Error::Conflict { .. }));
    }

    #[test]
    fn cross_backend_decrypts_when_construction_matches() {
        let key = key("tenant");
        let mut seal = FramedSealSession::with_aead(
            &key,
            FramedBlobSeed::Content,
            FramedAlgorithmSuite::Aes256GcmSiv,
            Arc::new(Aes256GcmSivFramedAead),
        );
        let framed = seal.seal_all(b"portable").unwrap();
        let mut open = FramedOpenSession::with_aead(
            &key,
            FramedAlgorithmSuite::Aes256GcmSiv,
            Arc::new(Aes256GcmSivFramedAead),
        );

        assert_eq!(open.open_all(&framed).unwrap(), b"portable");
    }

    #[test]
    fn range_open_decrypts_only_overlapping_frames() {
        let key = key("tenant");
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let aead = Arc::new(FakeAead {
            security: FramedAeadSecurity::SivDeterministic,
            opened: Mutex::new(0),
        });
        let mut seal = FramedSealSession::with_aead(
            &key,
            FramedBlobSeed::Content,
            FramedAlgorithmSuite::Aes256GcmSiv,
            aead.clone(),
        );
        let framed = seal.seal_all(&plaintext).unwrap();
        let mut open =
            FramedOpenSession::with_aead(&key, FramedAlgorithmSuite::Aes256GcmSiv, aead.clone());
        let start = FRAME_PLAINTEXT_LEN as u64 + 10;
        let end = FRAME_PLAINTEXT_LEN as u64 * 2 + 20;

        let range = open.open_range(&framed, start..end).unwrap();

        assert_eq!(range, plaintext[start as usize..end as usize]);
        assert_eq!(
            *aead.opened.lock().expect("counter should lock"),
            2,
            "only overlapping frames should decrypt"
        );
    }

    #[test]
    fn span_and_open_span_round_trip_across_frame_boundaries() {
        let key = key("tenant");
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, &plaintext).unwrap();
        let (header, _) = FramedBlobHeader::parse(&framed).unwrap();

        let start = FRAME_PLAINTEXT_LEN as u64 + 10;
        let end = FRAME_PLAINTEXT_LEN as u64 * 2 + 20;
        let span = framed_span_for_plaintext_range(&header, start..end).unwrap();
        let span_bytes = &framed[span.start as usize..span.end as usize];

        let opened = open_framed_span(&key, &header, span_bytes, start..end).unwrap();

        assert_eq!(opened, plaintext[start as usize..end as usize]);
        assert!(
            span.end - span.start < (FRAME_PLAINTEXT_LEN as u64) * 3,
            "span should cover only the overlapping frames, not the whole ciphertext"
        );
    }

    #[test]
    fn span_covers_single_frame_in_final_partial_frame() {
        let key = key("tenant");
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 2 + 100))
            .map(|index| (index % 251) as u8)
            .collect();
        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, &plaintext).unwrap();
        let (header, _) = FramedBlobHeader::parse(&framed).unwrap();

        let start = FRAME_PLAINTEXT_LEN as u64 * 2 + 5;
        let end = FRAME_PLAINTEXT_LEN as u64 * 2 + 50;
        let span = framed_span_for_plaintext_range(&header, start..end).unwrap();
        let span_bytes = &framed[span.start as usize..span.end as usize];

        let opened = open_framed_span(&key, &header, span_bytes, start..end).unwrap();

        assert_eq!(opened, plaintext[start as usize..end as usize]);
    }

    #[test]
    fn span_at_exact_frame_edge_returns_single_frame() {
        let key = key("tenant");
        let plaintext: Vec<u8> = (0..(FRAME_PLAINTEXT_LEN * 2))
            .map(|index| (index % 251) as u8)
            .collect();
        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, &plaintext).unwrap();
        let (header, _) = FramedBlobHeader::parse(&framed).unwrap();

        let start = 0u64;
        let end = FRAME_PLAINTEXT_LEN as u64;
        let span = framed_span_for_plaintext_range(&header, start..end).unwrap();
        let span_bytes = &framed[span.start as usize..span.end as usize];

        let opened = open_framed_span(&key, &header, span_bytes, start..end).unwrap();

        assert_eq!(opened, plaintext[start as usize..end as usize]);
        assert_eq!(
            span.end - span.start,
            Aes256GcmSivFramedAead.sealed_frame_len(FRAME_PLAINTEXT_LEN) as u64,
            "a request landing exactly on a frame boundary should fetch exactly one frame"
        );
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn span_out_of_bounds_is_invalid_input() {
        let key = key("tenant");
        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"short").unwrap();
        let (header, _) = FramedBlobHeader::parse(&framed).unwrap();

        let error = framed_span_for_plaintext_range(&header, 0..1000).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)));

        let error = open_framed_span(&key, &header, &[], 3..1).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)));
    }

    #[test]
    fn empty_span_request_returns_empty_without_frames() {
        let key = key("tenant");
        let framed = seal_framed_blob(&key, FramedBlobSeed::Content, b"payload").unwrap();
        let (header, _) = FramedBlobHeader::parse(&framed).unwrap();

        let span = framed_span_for_plaintext_range(&header, 2..2).unwrap();
        assert_eq!(span.start, span.end);

        let opened = open_framed_span(&key, &header, &[], 2..2).unwrap();
        assert!(opened.is_empty());
    }

    #[derive(Debug)]
    struct FakeAead {
        security: FramedAeadSecurity,
        opened: Mutex<usize>,
    }

    impl FramedAead for FakeAead {
        fn suite(&self) -> FramedAlgorithmSuite {
            FramedAlgorithmSuite::Aes256GcmSiv
        }

        fn security(&self) -> FramedAeadSecurity {
            self.security
        }

        fn seal_frame(
            &self,
            _subkey: &[u8; 32],
            _frame_index: u64,
            aad: &[u8],
            frame: &[u8],
        ) -> Result<Vec<u8>> {
            let mut sealed = Vec::new();
            sealed.extend_from_slice(&(aad.len() as u64).to_be_bytes());
            sealed.extend_from_slice(aad);
            sealed.extend_from_slice(frame);
            Ok(sealed)
        }

        fn open_frame(
            &self,
            _subkey: &[u8; 32],
            _frame_index: u64,
            aad: &[u8],
            sealed: &[u8],
        ) -> Result<Vec<u8>> {
            *self.opened.lock().expect("counter should lock") += 1;
            let (len_bytes, rest) = sealed.split_at(8);
            let len = u64::from_be_bytes(len_bytes.try_into().expect("len bytes")) as usize;
            let (sealed_aad, frame) = rest.split_at(len);
            if sealed_aad != aad {
                return Err(Error::storage(
                    StorageErrorKind::Corruption,
                    "fake AEAD authentication failed",
                ));
            }
            Ok(frame.to_vec())
        }

        fn sealed_frame_len(&self, plaintext_len: usize) -> usize {
            8 + frame_aad(FramedSeedKind::Content, &[0u8; 32], 0, 0, 0).len() + plaintext_len
        }
    }
}
