//! Per-hash quarantine lifecycle for local pack roots, plus the repair
//! support the scrubber leans on (index-prefix salvage, checkpoint
//! invalidation). Split from `local.rs` per the modularity thresholds.
//!
//! Quarantine semantics: entries carry a [`QuarantineReason`]; every
//! location-bound insertion re-verifies on-disk ground truth under the store
//! lock, a header-discredited ACTIVE pack retires (rolls off) at quarantine
//! time, and reads consult the set only for LIVE claims.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use nimbus_core::Result;

use crate::disk::{self, SyncObserver};
use crate::hash::BlobHash;

use super::{
    INDEX_MAGIC, IndexRecord, LocalPackState, PACK_MAGIC, PackEntry, corruption, ensure_pack_file,
    io_error, pack_path, parse_index_record, read_pack_entry,
};

pub(crate) const QUARANTINE_FILE: &str = "quarantine.nblq";
const QUARANTINE_MAGIC: &[u8] = b"NBLQ2\n";
const QUARANTINE_ENTRY_LEN: usize = crate::BLAKE3_HASH_LEN + 1;

/// Best-effort salvage of a (possibly corrupt) index log: parses records
/// from the front and stops at the FIRST structural failure of any kind,
/// returning the last-wins map of the parseable prefix. Used by corrupt-index
/// repair to recover offsets for records a sequential pack scan cannot reach;
/// never used on the normal open path (which distinguishes torn tails from
/// corruption and fails closed accordingly).
pub(crate) fn salvage_index_prefix(index_path: &Path) -> HashMap<BlobHash, PackEntry> {
    let mut bytes = Vec::new();
    let readable = File::open(index_path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .is_ok();
    let mut salvaged = HashMap::new();
    if !readable || !bytes.starts_with(INDEX_MAGIC) {
        return salvaged;
    }
    let mut cursor = INDEX_MAGIC.len();
    while cursor < bytes.len() {
        match parse_index_record(&bytes, &mut cursor) {
            Ok((IndexRecord::Put(entry), hash)) => {
                salvaged.insert(hash, entry);
            }
            Ok((IndexRecord::Release, hash)) => {
                salvaged.remove(&hash);
            }
            Err(_) => break,
        }
    }
    salvaged
}

/// Why a hash is quarantined — determines what may lift the quarantine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuarantineReason {
    /// The on-disk RECORD is corrupt. A re-upload of the content-addressed
    /// bytes publishes a fresh verified record and lifts the quarantine.
    Record = 1,
    /// The CONTENT itself is bad (e.g. framed ciphertext failing AEAD): a
    /// raw re-upload of the identical bytes reproduces the identical
    /// failure, so only release/repair lifts it — never a local put.
    Content = 2,
}

impl QuarantineReason {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Record),
            2 => Some(Self::Content),
            _ => None,
        }
    }
}

/// How a quarantine request is revalidated against on-disk ground truth
/// under the store lock, immediately before insertion. Entry equality alone
/// is not identity (empty-store compaction reuses pack id 0, and a
/// release+compact+reupload can reproduce identical coordinates), so every
/// location-bound arm re-checks the disk: only what is corrupt RIGHT NOW is
/// quarantined.
#[derive(Clone, Copy, Debug)]
pub(crate) enum QuarantineCheck {
    /// Content-level corruption (e.g. AEAD open failure): content-addressed
    /// bytes are identical wherever the record lives — no location check.
    Unconditional,
    /// Record-level corruption: quarantine only while the hash still maps to
    /// exactly this record AND re-reading that record still fails.
    CorruptRecord(PackEntry),
    /// Pack-header corruption: quarantine only while the hash still maps to
    /// exactly this record AND the pack's header still fails to validate
    /// (individual records behind a corrupt header may verify — the header
    /// is the discrediting fact).
    CorruptPackHeader(PackEntry),
}

pub(crate) fn pack_header_is_valid(packs_dir: &Path, pack_id: u64) -> bool {
    let path = pack_path(packs_dir, pack_id);
    let mut magic = vec![0u8; PACK_MAGIC.len()];
    match File::open(&path).and_then(|mut file| file.read_exact(&mut magic)) {
        Ok(()) => magic == PACK_MAGIC,
        Err(_) => false,
    }
}

pub(crate) fn load_quarantine(path: &Path) -> Result<HashMap<BlobHash, QuarantineReason>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(err) => return Err(io_error(err, format!("open quarantine {}", path.display()))),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| io_error(err, format!("read quarantine {}", path.display())))?;
    if !bytes.starts_with(QUARANTINE_MAGIC) {
        return Err(corruption(format!(
            "quarantine {} has invalid magic",
            path.display()
        )));
    }
    let payload_len = bytes.len() - QUARANTINE_MAGIC.len();
    if payload_len % QUARANTINE_ENTRY_LEN != 0 {
        return Err(corruption(format!(
            "quarantine {} has truncated entry",
            path.display()
        )));
    }

    let mut quarantined = HashMap::new();
    let mut cursor = QUARANTINE_MAGIC.len();
    while cursor < bytes.len() {
        let mut raw = [0u8; crate::BLAKE3_HASH_LEN];
        raw.copy_from_slice(&bytes[cursor..cursor + crate::BLAKE3_HASH_LEN]);
        let reason_byte = bytes[cursor + crate::BLAKE3_HASH_LEN];
        let reason = QuarantineReason::from_byte(reason_byte).ok_or_else(|| {
            corruption(format!(
                "quarantine {} has invalid reason byte {reason_byte}",
                path.display()
            ))
        })?;
        quarantined.insert(BlobHash::from_bytes(raw), reason);
        cursor += QUARANTINE_ENTRY_LEN;
    }
    Ok(quarantined)
}

pub(crate) fn encode_quarantine(quarantined: &HashMap<BlobHash, QuarantineReason>) -> Vec<u8> {
    let mut entries = quarantined.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(hash, _)| **hash);
    let mut bytes =
        Vec::with_capacity(QUARANTINE_MAGIC.len() + entries.len() * QUARANTINE_ENTRY_LEN);
    bytes.extend_from_slice(QUARANTINE_MAGIC);
    for (hash, reason) in entries {
        bytes.extend_from_slice(hash.as_bytes());
        bytes.push(*reason as u8);
    }
    bytes
}

/// Result of a quarantine batch: the hashes actually inserted plus the
/// ground-truth revalidation I/O performed (bytes), so callers can fold it
/// into their pacing/accounting contract.
pub(crate) struct QuarantineOutcome {
    pub(crate) inserted: Vec<BlobHash>,
    pub(crate) revalidation_bytes: u64,
}

pub(crate) fn quarantine_hashes_locked(
    state: &mut LocalPackState,
    requests: &[(BlobHash, QuarantineCheck)],
) -> Result<QuarantineOutcome> {
    let mut next = state.quarantined.clone();
    let mut inserted = Vec::new();
    let mut revalidation_bytes = 0u64;
    let mut retire_packs: BTreeSet<u64> = BTreeSet::new();
    for (hash, check) in requests {
        match check {
            QuarantineCheck::Unconditional => {
                // Content-level findings still require a LIVE claim: a
                // release that raced the scrub already dropped the claim,
                // and a stale side-file entry would poison a future
                // reintroduction of the same content hash.
                if !state.index.contains_key(hash) {
                    continue;
                }
            }
            QuarantineCheck::CorruptRecord(expected) => {
                if state.index.get(hash) != Some(expected) {
                    continue;
                }
                // The re-read walks header + up to `len` body bytes either
                // way; account it so the scrub's pacing/reporting contract
                // covers revalidation I/O too.
                revalidation_bytes = revalidation_bytes
                    .saturating_add(expected.len)
                    .saturating_add(4 + crate::BLAKE3_HASH_LEN as u64 + 8);
                if read_pack_entry(&state.packs_dir, hash, *expected).is_ok() {
                    continue;
                }
            }
            QuarantineCheck::CorruptPackHeader(expected) => {
                if state.index.get(hash) != Some(expected) {
                    continue;
                }
                revalidation_bytes = revalidation_bytes.saturating_add(PACK_MAGIC.len() as u64);
                if pack_header_is_valid(&state.packs_dir, expected.pack_id) {
                    continue;
                }
                retire_packs.insert(expected.pack_id);
            }
        }
        let reason = match check {
            QuarantineCheck::Unconditional => QuarantineReason::Content,
            QuarantineCheck::CorruptRecord(_) | QuarantineCheck::CorruptPackHeader(_) => {
                QuarantineReason::Record
            }
        };
        if next.insert(*hash, reason).is_none() {
            inserted.push(*hash);
        }
    }
    let observer = Arc::clone(&state.observer);
    if !inserted.is_empty() {
        write_quarantine_locked(state, &next, &*observer)?;
        state.quarantined = next;
    }

    // Retire a header-discredited ACTIVE pack: roll to a fresh validated
    // pack under this same lock so (a) unrelated new puts never land behind
    // the bad header and (b) reopen selects the fresh pack as active instead
    // of re-validating (and refusing) the corrupt one. This runs even when
    // every hash was ALREADY quarantined (a repeat scrub of the same corrupt
    // header must still retire the pack — e.g. after a crash lost the
    // original retirement roll).
    if retire_packs.contains(&state.active_pack_id) {
        state.active_pack_id = state.active_pack_id.saturating_add(1);
        state.active_pack_bytes =
            ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
    }
    Ok(QuarantineOutcome {
        inserted,
        revalidation_bytes,
    })
}

pub(crate) fn write_quarantine_locked(
    state: &LocalPackState,
    next: &HashMap<BlobHash, QuarantineReason>,
    observer: &dyn SyncObserver,
) -> Result<()> {
    disk::write_replace_durable(&state.quarantine_path, &encode_quarantine(next), observer).map_err(
        |err| {
            io_error(
                err,
                format!("write quarantine {}", state.quarantine_path.display()),
            )
        },
    )
}

/// Removes the scrub checkpoint (if any) and persists the removal.
pub(crate) fn invalidate_scrub_checkpoint(
    state: &LocalPackState,
    observer: &dyn SyncObserver,
) -> Result<()> {
    let Some(root) = state.index_path.parent() else {
        return Ok(());
    };
    let path = root.join(crate::scrub::SCRUB_CHECKPOINT_FILE);
    match fs::remove_file(&path) {
        Ok(()) => disk::fsync_dir(root, observer)
            .map_err(|err| io_error(err, format!("sync root dir {}", root.display()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io_error(
            err,
            format!("remove stale scrub checkpoint {}", path.display()),
        )),
    }
}
