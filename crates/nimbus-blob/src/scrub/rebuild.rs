//! Index rebuild and corrupt-index repair for local pack roots.
//!
//! Split from `scrub.rs` (modularity threshold): this module owns the two
//! rebuild paths — the in-state rebuild over an open store, and the
//! guard-held repair for a root whose `index.log` refuses to load — plus
//! their shared index encoding. Both publish the FULL rebuilt index as one
//! atomic durable replace and preserve quarantined claims; the resurrection
//! semantics (released-but-uncompacted blobs return as live claims for GC to
//! re-reclaim) are documented on the callers in [`super::LocalPackScrubber`].

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use nimbus_core::{Error, Result, StorageErrorKind, SystemWallClock, WallClock};

use crate::disk;
use crate::hash::BlobHash;
use crate::local::{self, INDEX_MAGIC, INDEX_PUT, LocalPackState, PACK_MAGIC, PackEntry};
use crate::root_guard::{self, LocalPackStoreOptions};

use super::{
    PacingTracker, ScannedRecord, ScrubFindingKind, ScrubPacing, ScrubReport, finding, scan_pack,
    verify_record_paced,
};

pub(super) fn is_index_corruption(err: &Error) -> bool {
    if err.storage_kind() != Some(StorageErrorKind::Corruption) {
        return false;
    }
    match err.storage_message() {
        Some(message) => message.starts_with("index "),
        None => false,
    }
}

/// Repairs a corrupt `index.log` by scanning packs and publishing the FULL
/// rebuilt index as one atomic durable replace, all under the root guard.
///
/// There is deliberately no intermediate durable state: a crash at any point
/// leaves either the old (corrupt, still refusing to open) index or the
/// complete rebuilt one — never a valid-but-empty index that would hide every
/// existing pack record from a subsequent open.
pub(super) fn rebuild_corrupt_index_under_guard(
    root: PathBuf,
    options: LocalPackStoreOptions,
    pacing: ScrubPacing,
) -> Result<ScrubReport> {
    root_guard::check_writable_root_shape(&root, &options)?;
    let canonical = root.canonicalize().map_err(|err| {
        local::io_error(err, format!("canonicalize blob root {}", root.display()))
    })?;
    let packs_dir = canonical.join("packs");
    let observer = disk::NoopSyncObserver;
    let _guard = root_guard::guard_writable_root(
        &canonical,
        &packs_dir,
        &options,
        SystemWallClock.now_millis(),
        &observer,
    )?;

    let mut report = ScrubReport::default();
    let mut pacing = PacingTracker::new(pacing);
    let mut rebuilt = HashMap::new();
    let mut header_corrupt_packs = BTreeSet::new();
    let mut corrupt_record_index: HashMap<BlobHash, ScannedRecord> = HashMap::new();
    let written_at_millis = SystemWallClock.now_millis();
    for pack_id in local::pack_ids_on_disk(&packs_dir)? {
        let pack_scan = scan_pack(&packs_dir, pack_id, None, &mut pacing)?;
        report.packs_scanned += 1;
        if report.first_scanned_pack_id.is_none() {
            report.first_scanned_pack_id = Some(pack_id);
        }
        report.last_scanned_pack_id = Some(pack_id);
        report.records_scanned += pack_scan.records_scanned;
        report.bytes_scanned = report.bytes_scanned.saturating_add(pack_scan.bytes_scanned);
        report.corrupt_records += pack_scan.findings.len();
        report.findings.extend(pack_scan.findings);
        if !pack_scan.pack_header_valid {
            header_corrupt_packs.insert(pack_id);
        }
        for (record, body_hash) in &pack_scan.corrupt_records {
            // Index the evidence under BOTH hashes: when the hash FIELD was
            // the corrupted bytes, the quarantine side file knows the blob by
            // its true (body) hash, not the garbage stored one.
            corrupt_record_index.insert(record.hash, record.clone());
            corrupt_record_index.insert(*body_hash, record.clone());
        }
        for record in pack_scan.valid_records {
            rebuilt.insert(
                record.hash,
                PackEntry {
                    pack_id: record.pack_id,
                    offset: record.offset,
                    len: record.len,
                    written_at_millis,
                },
            );
            report.records_verified += 1;
        }
    }

    let index_path = canonical.join("index.log");

    // Mirror the in-state rebuild's second pass: the corrupt index's
    // parseable PREFIX still knows offsets the sequential pack scan could
    // not reach (records past a structurally corrupt segment). Direct-verify
    // each salvaged entry and carry it forward; carry quarantined claims
    // (the quarantine side file is separate and intact) unconditionally so
    // their bytes stay claim-tracked. Without this, corrupt-index repair
    // silently drops live, readable blobs the normal rebuild preserves.
    let salvaged = local::salvage_index_prefix(&index_path);
    let quarantine_path = canonical.join(local::QUARANTINE_FILE);
    let mut quarantined = local::load_quarantine(&quarantine_path)?;
    let mut new_quarantines: HashMap<BlobHash, local::QuarantineReason> = HashMap::new();
    // Quarantined claims must stay locatable (claim-tracked, pack-retained)
    // even when the corrupt index prefix cannot supply their entry: recover
    // coordinates from the pack scan's corrupt records; report the ones that
    // are genuinely unlocatable instead of silently dropping the claim.
    let mut unlocatable: Vec<BlobHash> = Vec::new();
    for hash in quarantined.keys() {
        if rebuilt.contains_key(hash) || salvaged.contains_key(hash) {
            continue;
        }
        if let Some(record) = corrupt_record_index.get(hash) {
            rebuilt.insert(
                *hash,
                PackEntry {
                    pack_id: record.pack_id,
                    offset: record.offset,
                    len: record.len,
                    written_at_millis,
                },
            );
        } else {
            unlocatable.push(*hash);
        }
    }
    // Fail CLOSED: never publish an index that silently drops a quarantined
    // claim. If a claim cannot be relocated (its pack was destroyed and the
    // corrupt index had no salvageable entry) the bytes are gone, but the
    // claim is retention-tracked — turning it into NotFound would let
    // compaction reclaim the pack and erase the audit trail. Refuse the
    // repair and name the unrecoverable claims so an operator explicitly
    // releases them before repair proceeds.
    if !unlocatable.is_empty() {
        unlocatable.sort();
        return Err(Error::storage(
            // Busy, not Corruption: a precondition refusal (operator must
            // release the unrecoverable claims), not a disk fault, so it must
            // not poison the store.
            StorageErrorKind::Busy,
            format!(
                "corrupt-index repair aborted: {} quarantined claim(s) unrecoverable \
                 (pack destroyed + no salvageable index entry); release them explicitly to \
                 proceed: {}",
                unlocatable.len(),
                unlocatable
                    .iter()
                    .map(BlobHash::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    for (hash, entry) in salvaged {
        if rebuilt.contains_key(&hash) {
            continue;
        }
        if quarantined.contains_key(&hash) {
            rebuilt.insert(hash, entry);
            continue;
        }
        if header_corrupt_packs.contains(&entry.pack_id) {
            // The pack is discredited wholesale, but the claim is live:
            // retain it quarantined (fail-closed) rather than drop it to
            // NotFound and let compaction delete the bytes.
            rebuilt.insert(hash, entry);
            new_quarantines.insert(hash, local::QuarantineReason::Record);
            continue;
        }
        let mut direct_bytes = 0u64;
        match verify_record_paced(&packs_dir, &hash, entry, &mut pacing, &mut direct_bytes) {
            Ok(()) => {
                rebuilt.insert(hash, entry);
                report.records_verified += 1;
            }
            Err(err) => {
                // Retain the LIVE claim fail-closed instead of silently
                // dropping it: keep the entry, quarantine it, and name it in
                // the report — silent removal would turn a corruption read
                // into NotFound and let compaction delete the bytes.
                rebuilt.insert(hash, entry);
                new_quarantines.insert(hash, local::QuarantineReason::Record);
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::HashMismatch,
                    Some(entry.pack_id),
                    Some(entry.offset),
                    Some(hash),
                    Some(hash),
                    None,
                    format!(
                        "direct verification of live claim {hash} failed during rebuild: {err}"
                    ),
                ));
            }
        }
        report.bytes_scanned = report.bytes_scanned.saturating_add(direct_bytes);
    }
    // Persist newly discovered quarantines BEFORE the rebuilt index so a
    // crash between the two leaves the claim either absent (old corrupt
    // index) or quarantined — never live-and-unguarded.
    if !new_quarantines.is_empty() {
        quarantined.extend(
            new_quarantines
                .iter()
                .map(|(hash, reason)| (*hash, *reason)),
        );
        disk::write_replace_durable(
            &quarantine_path,
            &local::encode_quarantine(&quarantined),
            &observer,
        )
        .map_err(|err| {
            local::io_error(
                err,
                format!("persist rebuild quarantines {}", quarantine_path.display()),
            )
        })?;
        // Surface the rebuild-created quarantines to operator automation,
        // matching the normal scrub quarantine path.
        report.quarantined_hashes.extend(new_quarantines.keys());
        report.quarantined_hashes.sort();
        report.quarantined_hashes.dedup();
    }
    // Invalidate any resume checkpoint BEFORE publishing the rebuilt index:
    // its evidence was gathered against the pre-rebuild index/scan state, and
    // a crash between the two must never leave a stale checkpoint alongside a
    // rebuilt index. (Checkpoint gone + still-corrupt index is safe: the next
    // open refuses and repair reruns.)
    let checkpoint_path = canonical.join(crate::scrub::SCRUB_CHECKPOINT_FILE);
    match std::fs::remove_file(&checkpoint_path) {
        Ok(()) => {
            disk::fsync_dir(&canonical, &observer).map_err(|err| {
                local::io_error(err, format!("sync root dir {}", canonical.display()))
            })?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(local::io_error(
                err,
                format!(
                    "remove stale scrub checkpoint {}",
                    checkpoint_path.display()
                ),
            ));
        }
    }
    disk::write_replace_durable(&index_path, &encode_index(&rebuilt), &observer).map_err(
        |err| {
            local::io_error(
                err,
                format!("publish rebuilt index {}", index_path.display()),
            )
        },
    )?;
    report.completed = true;
    report.pacing = pacing.finish();
    Ok(report)
}

pub(super) fn rebuild_index_locked(
    state: &mut LocalPackState,
    pacing: ScrubPacing,
    written_at_millis: u64,
) -> Result<ScrubReport> {
    let mut report = ScrubReport::default();
    let mut pacing = PacingTracker::new(pacing);
    let mut rebuilt = HashMap::new();
    let mut new_quarantines: HashMap<BlobHash, local::QuarantineReason> = HashMap::new();
    let mut header_corrupt_packs = BTreeSet::new();
    let mut corrupt_record_index: HashMap<BlobHash, ScannedRecord> = HashMap::new();
    let pack_ids = local::pack_ids_on_disk(&state.packs_dir)?;

    for pack_id in pack_ids.iter().copied() {
        let pack_scan = scan_pack(&state.packs_dir, pack_id, None, &mut pacing)?;
        report.packs_scanned += 1;
        if report.first_scanned_pack_id.is_none() {
            report.first_scanned_pack_id = Some(pack_id);
        }
        report.last_scanned_pack_id = Some(pack_id);
        report.records_scanned += pack_scan.records_scanned;
        report.bytes_scanned = report.bytes_scanned.saturating_add(pack_scan.bytes_scanned);
        report.corrupt_records += pack_scan.findings.len();
        report.findings.extend(pack_scan.findings);
        if !pack_scan.pack_header_valid {
            header_corrupt_packs.insert(pack_id);
        }
        for (record, body_hash) in &pack_scan.corrupt_records {
            corrupt_record_index.insert(record.hash, record.clone());
            corrupt_record_index.insert(*body_hash, record.clone());
        }
        for record in pack_scan.valid_records {
            rebuilt.insert(
                record.hash,
                PackEntry {
                    pack_id: record.pack_id,
                    offset: record.offset,
                    len: record.len,
                    written_at_millis,
                },
            );
            report.records_verified += 1;
        }
    }

    // The sequential scan stops at the first structural corruption, but the
    // CURRENT index still knows the offsets of records past that segment.
    // Direct-verify each such entry and carry it forward — publishing only
    // the scanned prefix would make healthy, currently-readable blobs
    // NotFound and let a later compaction delete their bytes. Quarantined
    // claims are carried unconditionally: their bytes stay claim-tracked
    // (and pack-retained) until an explicit release/repair decision.
    let observer = Arc::clone(&state.observer);
    for (hash, entry) in &state.index {
        if rebuilt.contains_key(hash) {
            continue;
        }
        if state.quarantined.contains_key(hash) {
            rebuilt.insert(*hash, *entry);
            continue;
        }
        if header_corrupt_packs.contains(&entry.pack_id) {
            // Live claim behind a discredited pack: retain quarantined.
            rebuilt.insert(*hash, *entry);
            new_quarantines.insert(*hash, local::QuarantineReason::Record);
            continue;
        }
        let mut direct_bytes = 0u64;
        match verify_record_paced(
            &state.packs_dir,
            hash,
            *entry,
            &mut pacing,
            &mut direct_bytes,
        ) {
            Ok(()) => {
                rebuilt.insert(*hash, *entry);
                report.records_verified += 1;
            }
            Err(err) => {
                // Retain fail-closed instead of silently dropping (see the
                // guard-path second pass).
                rebuilt.insert(*hash, *entry);
                new_quarantines.insert(*hash, local::QuarantineReason::Record);
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::HashMismatch,
                    Some(entry.pack_id),
                    Some(entry.offset),
                    Some(*hash),
                    Some(*hash),
                    None,
                    format!(
                        "direct verification of live claim {hash} failed during rebuild: {err}"
                    ),
                ));
            }
        }
        report.bytes_scanned = report.bytes_scanned.saturating_add(direct_bytes);
    }

    // Recover quarantined claims that have NO current index entry (e.g. the
    // index log was lost entirely, so `state.index` is empty). Their bytes
    // are corrupt but claim-tracked: relocate via the pack scan's corrupt
    // records; a genuinely unrecoverable claim fails the repair closed
    // rather than being silently dropped.
    let mut unlocatable: Vec<BlobHash> = Vec::new();
    for hash in state.quarantined.keys() {
        if rebuilt.contains_key(hash) {
            continue;
        }
        if let Some(record) = corrupt_record_index.get(hash) {
            rebuilt.insert(
                *hash,
                PackEntry {
                    pack_id: record.pack_id,
                    offset: record.offset,
                    len: record.len,
                    written_at_millis,
                },
            );
        } else {
            unlocatable.push(*hash);
        }
    }
    if !unlocatable.is_empty() {
        // Fail closed WITHOUT leaving a provisional empty index behind: if
        // this open created `index.log` from missing, remove it DURABLY so
        // the next open sees the index as still-missing (needs repair) rather
        // than as an authoritative empty index that would prune the
        // quarantine claims we are refusing to drop. The unlink is fsynced
        // (parent dir) and its error propagated — a swallowed error could
        // leave the empty index durable across a crash.
        if state.index_provisional && state.index.is_empty() {
            if let Some(root) = state.index_path.parent() {
                match std::fs::remove_file(&state.index_path) {
                    Ok(()) => {
                        disk::fsync_dir(root, &*observer).map_err(|err| {
                            local::io_error(
                                err,
                                format!(
                                    "sync root after provisional index unlink {}",
                                    root.display()
                                ),
                            )
                        })?;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(local::io_error(
                            err,
                            format!("remove provisional index {}", state.index_path.display()),
                        ));
                    }
                }
            }
        }
        unlocatable.sort();
        return Err(Error::storage(
            // Busy, not Corruption: precondition refusal, must not poison.
            StorageErrorKind::Busy,
            format!(
                "index rebuild aborted: {} quarantined claim(s) unrecoverable (pack destroyed \
                 + no index entry); release them explicitly to proceed: {}",
                unlocatable.len(),
                unlocatable
                    .iter()
                    .map(BlobHash::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    // Persist newly discovered quarantines BEFORE the rebuilt index (see the
    // guard-path ordering rationale).
    if !new_quarantines.is_empty() {
        let mut merged = state.quarantined.clone();
        merged.extend(
            new_quarantines
                .iter()
                .map(|(hash, reason)| (*hash, *reason)),
        );
        local::write_quarantine_locked(state, &merged, &*observer)?;
        state.quarantined = merged;
        // Surface the rebuild-created quarantines to operator automation,
        // matching the normal scrub quarantine path.
        report.quarantined_hashes.extend(new_quarantines.keys());
        report.quarantined_hashes.sort();
        report.quarantined_hashes.dedup();
    }

    let index_bytes = encode_index(&rebuilt);
    // Invalidate any resume checkpoint BEFORE publishing the rebuilt index:
    // its evidence was gathered against the pre-rebuild index/scan state,
    // and a crash between the two must never leave a stale checkpoint
    // alongside a rebuilt index. (Checkpoint gone + old index is safe: the
    // next scrub simply full-scans.)
    local::invalidate_scrub_checkpoint(state, &*observer)?;
    disk::write_replace_durable(&state.index_path, &index_bytes, &*observer).map_err(|err| {
        local::io_error(err, format!("rebuild index {}", state.index_path.display()))
    })?;
    state.index = rebuilt;
    // Never select a header-corrupt pack (its records were quarantine-
    // carried) as the active append target: roll to a fresh id past
    // everything on disk instead.
    let index_max = state
        .index
        .values()
        .map(|entry| entry.pack_id)
        .max()
        .unwrap_or(0);
    let disk_max = pack_ids.iter().max().copied().unwrap_or(0);
    let candidate = index_max.max(disk_max);
    state.active_pack_id = if header_corrupt_packs.contains(&candidate) {
        candidate.saturating_add(1)
    } else {
        candidate
    };
    state.active_pack_bytes =
        local::ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
    if state.active_pack_bytes >= state.pack_target_bytes
        && state.active_pack_bytes > PACK_MAGIC.len() as u64
    {
        state.active_pack_id = state.active_pack_id.saturating_add(1);
        state.active_pack_bytes =
            local::ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
    }
    report.completed = true;
    report.pacing = pacing.finish();
    Ok(report)
}

pub(super) fn encode_index(index: &HashMap<BlobHash, PackEntry>) -> Vec<u8> {
    let mut entries = index.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(_, entry)| (entry.pack_id, entry.offset));
    let mut bytes = Vec::with_capacity(INDEX_MAGIC.len() + entries.len() * 65);
    bytes.extend_from_slice(INDEX_MAGIC);
    for (hash, entry) in entries {
        bytes.push(INDEX_PUT);
        bytes.extend_from_slice(hash.as_bytes());
        bytes.extend_from_slice(&entry.pack_id.to_le_bytes());
        bytes.extend_from_slice(&entry.offset.to_le_bytes());
        bytes.extend_from_slice(&entry.len.to_le_bytes());
        bytes.extend_from_slice(&entry.written_at_millis.to_le_bytes());
    }
    bytes
}
