//! Crash-consistency proofs for the journal and the staged write.
//!
//! A crash during an append leaves a prefix of the bytes that the append
//! wrote. Truncating a journal at every byte offset therefore covers every
//! torn append that a crash can produce on a filesystem that does not reorder
//! writes within one file.

use std::fs;
use std::io;
use std::path::Path;

use nimbus_durable_record::{
    FRAME_HEADER_LEN, Journal, StagedWrite, WriteCheckpoint, WriteFailure, WriteStep,
};

const RECORDS: [&[u8]; 4] = [b"first", b"", b"third record", b"fourth"];

fn write_journal(path: &Path) -> Vec<usize> {
    let (mut journal, _) = Journal::open(path).expect("new journal should open");
    let mut boundaries = vec![0];
    for record in RECORDS {
        journal.append(record).expect("append should sync");
        boundaries.push(usize::try_from(journal.valid_len()).expect("length fits usize"));
    }
    boundaries
}

#[test]
fn journal_truncated_at_every_byte_offset_recovers_the_complete_prefix() {
    let root = tempfile::tempdir().expect("temporary directory should exist");
    let source = root.path().join("source.journal");
    let boundaries = write_journal(&source);
    let bytes = fs::read(&source).expect("journal bytes should read");
    assert_eq!(*boundaries.last().expect("boundaries"), bytes.len());
    assert_eq!(
        bytes.len(),
        RECORDS
            .iter()
            .map(|record| FRAME_HEADER_LEN + record.len())
            .sum::<usize>()
    );

    for cut in 0..=bytes.len() {
        let path = root.path().join(format!("cut-{cut}.journal"));
        fs::write(&path, &bytes[..cut]).expect("truncated journal should write");

        let complete = boundaries
            .iter()
            .rposition(|boundary| *boundary <= cut)
            .expect("offset zero is always a boundary");
        let boundary = boundaries[complete];
        let (mut journal, recovered) = Journal::open(&path)
            .unwrap_or_else(|error| panic!("cut at {cut} should recover: {error}"));

        let expected: Vec<Vec<u8>> = RECORDS[..complete]
            .iter()
            .map(|record| record.to_vec())
            .collect();
        assert_eq!(recovered.records, expected, "records after a cut at {cut}");
        assert_eq!(
            recovered.discarded_tail,
            (cut - boundary) as u64,
            "torn tail after a cut at {cut}"
        );
        assert_eq!(
            fs::metadata(&path).expect("journal metadata").len(),
            boundary as u64,
            "recovery must truncate the torn tail after a cut at {cut}"
        );

        journal
            .append(b"after recovery")
            .expect("a recovered journal should accept appends");
        drop(journal);
        let (_, reopened) = Journal::open(&path).expect("journal should reopen");
        let mut expected = expected;
        expected.push(b"after recovery".to_vec());
        assert_eq!(reopened.records, expected, "append after a cut at {cut}");
        assert_eq!(reopened.discarded_tail, 0);
    }
}

#[test]
fn zero_filled_torn_append_is_discarded() {
    let root = tempfile::tempdir().expect("temporary directory should exist");
    let path = root.path().join("zeroed.journal");
    let boundaries = write_journal(&path);
    let mut bytes = fs::read(&path).expect("journal bytes should read");
    // Some filesystems expose an allocated but unwritten extent as zeros.
    bytes.extend_from_slice(&[0; FRAME_HEADER_LEN + 32]);
    fs::write(&path, &bytes).expect("zero-filled tail should write");

    let (_, recovered) = Journal::open(&path).expect("zeroed tail should recover");
    assert_eq!(recovered.records.len(), RECORDS.len());
    assert_eq!(recovered.discarded_tail, (FRAME_HEADER_LEN + 32) as u64);
    assert_eq!(
        fs::metadata(&path).expect("journal metadata").len(),
        *boundaries.last().expect("boundaries") as u64
    );
}

#[test]
fn damaged_acknowledged_record_fails_closed_and_preserves_the_file() {
    let root = tempfile::tempdir().expect("temporary directory should exist");
    let path = root.path().join("damaged.journal");
    let boundaries = write_journal(&path);
    let mut bytes = fs::read(&path).expect("journal bytes should read");
    let damaged = boundaries[2] + FRAME_HEADER_LEN;
    bytes[damaged] ^= 0x01;
    fs::write(&path, &bytes).expect("damaged journal should write");

    let error = Journal::open(&path).expect_err("mid-journal damage must not be truncated");
    assert!(
        error
            .to_string()
            .contains(&format!("byte {}", boundaries[2])),
        "the damaged frame offset must be reported: {error}"
    );
    assert_eq!(
        fs::read(&path).expect("journal bytes should read"),
        bytes,
        "a corrupt journal must stay inspectable"
    );
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    Checkpoint(WriteCheckpoint),
    DirectorySync,
}

/// Fail a staged replace at each fault point. Every outcome leaves the
/// destination with either the complete old bytes or the complete new bytes,
/// and no stage file.
#[test]
fn staged_write_fault_at_every_step_leaves_old_or_new_bytes() {
    let faults = [
        Fault::Checkpoint(WriteCheckpoint::StageDurable),
        Fault::Checkpoint(WriteCheckpoint::Committed),
        Fault::DirectorySync,
    ];
    for fault in faults {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let destination = root.path().join("record.json");
        let stage = root.path().join("record.stage");
        fs::write(&destination, b"old").expect("previous record should write");

        let mut sync = |_: &Path| match fault {
            Fault::DirectorySync => Err(io::Error::other("injected directory sync failure")),
            Fault::Checkpoint(_) => Ok(()),
        };
        let mut observe = |checkpoint: WriteCheckpoint| match fault {
            Fault::Checkpoint(failing) if failing == checkpoint => Err("injected crash"),
            _ => Ok(()),
        };
        let error = StagedWrite::new(&stage, &destination)
            .directory_sync(&mut sync)
            .observer(&mut observe)
            .write(b"new")
            .expect_err("an injected fault should fail the write");

        let committed = match error.failure {
            WriteFailure::Observer { checkpoint, .. } => checkpoint == WriteCheckpoint::Committed,
            WriteFailure::Io { step, .. } => step == WriteStep::SyncDirectory,
        };
        let expected: &[u8] = if committed { b"new" } else { b"old" };
        assert_eq!(
            fs::read(&destination).expect("record should read"),
            expected,
            "destination after {fault:?}"
        );
        assert!(!stage.exists(), "no stage may remain after {fault:?}");
    }
}
