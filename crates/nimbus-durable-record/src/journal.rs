//! Append-only journal of checksummed records with torn-tail recovery.
//!
//! Each record is one frame:
//!
//! ```text
//! magic "NDR1" | payload length (u32 LE) | CRC-32 (u32 LE) | payload
//! ```
//!
//! The CRC covers the length field and the payload. [`Journal::append`]
//! returns only after the frame is synced, so a crash can tear only the last
//! unacknowledged append. [`Journal::open`] keeps the longest prefix of valid
//! frames and truncates the torn tail durably.
//!
//! A damaged frame that a later valid frame follows is not a torn tail: an
//! acknowledged record would be lost. Open fails with
//! [`JournalError::Corrupt`] in that case and leaves the file unchanged. A
//! torn final record whose payload embeds a complete frame also fails closed
//! this way, because the scan cannot tell it from mid-journal damage.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use crate::directory::sync_directory;

const FRAME_MAGIC: [u8; 4] = *b"NDR1";

/// Bytes in a frame before its payload.
pub const FRAME_HEADER_LEN: usize = 12;

/// The records that survived recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// Every acknowledged record, in append order.
    pub records: Vec<Vec<u8>>,
    /// Bytes of a torn tail that open removed. Zero after a clean shutdown.
    pub discarded_tail: u64,
}

/// A journal failure.
#[derive(Debug)]
pub enum JournalError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// A damaged frame starts at `offset`, and a valid frame follows it.
    Corrupt { path: PathBuf, offset: u64 },
    /// A record payload exceeds the `u32` length field.
    RecordTooLarge { len: usize },
    /// An earlier append failed after it could have written bytes. Reopen
    /// the journal to recover a consistent tail.
    Poisoned { path: PathBuf },
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} {}: {source}",
                path.display()
            ),
            Self::Corrupt { path, offset } => write!(
                formatter,
                "journal {} has a damaged record at byte {offset} before later valid records",
                path.display()
            ),
            Self::RecordTooLarge { len } => {
                write!(
                    formatter,
                    "journal record of {len} bytes exceeds the u32 frame limit"
                )
            }
            Self::Poisoned { path } => write!(
                formatter,
                "journal {} had a failed append; reopen it to recover",
                path.display()
            ),
        }
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// An open journal positioned after its last valid record.
#[derive(Debug)]
pub struct Journal {
    file: File,
    path: PathBuf,
    valid_len: u64,
    poisoned: bool,
}

impl Journal {
    /// Open or create the journal at `path` and recover its records.
    ///
    /// A new journal file and its directory entry are synced before this
    /// returns. A torn tail is truncated and the truncation is synced.
    pub fn open(path: &Path) -> Result<(Self, Recovered), JournalError> {
        let io_error = |operation: &'static str| {
            move |source: io::Error| JournalError::Io {
                operation,
                path: path.to_path_buf(),
                source,
            }
        };
        let mut file = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(io_error("create journal"))?;
                file.sync_all().map_err(io_error("sync new journal"))?;
                let parent = match path.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => parent,
                    _ => Path::new("."),
                };
                sync_directory(parent).map_err(|source| JournalError::Io {
                    operation: "sync journal directory",
                    path: parent.to_path_buf(),
                    source,
                })?;
                file
            }
            Err(error) => return Err(io_error("open journal")(error)),
        };

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(io_error("read journal"))?;
        let scan = scan(&bytes).map_err(|offset| JournalError::Corrupt {
            path: path.to_path_buf(),
            offset: offset as u64,
        })?;
        let file_len = bytes.len() as u64;
        let valid_len = scan.valid_len as u64;
        if valid_len < file_len {
            file.set_len(valid_len)
                .map_err(io_error("truncate torn journal tail"))?;
            file.sync_all()
                .map_err(io_error("sync truncated journal"))?;
        }
        let journal = Self {
            file,
            path: path.to_path_buf(),
            valid_len,
            poisoned: false,
        };
        let recovered = Recovered {
            records: scan.records,
            discarded_tail: file_len - valid_len,
        };
        Ok((journal, recovered))
    }

    /// Append one record and sync it before returning.
    pub fn append(&mut self, record: &[u8]) -> Result<(), JournalError> {
        if self.poisoned {
            return Err(JournalError::Poisoned {
                path: self.path.clone(),
            });
        }
        let frame = encode_frame(record)?;
        let written = self
            .file
            .seek(SeekFrom::Start(self.valid_len))
            .and_then(|_| self.file.write_all(&frame))
            .and_then(|()| self.file.sync_all());
        match written {
            Ok(()) => {
                self.valid_len += frame.len() as u64;
                Ok(())
            }
            Err(source) => {
                self.poisoned = true;
                Err(JournalError::Io {
                    operation: "append journal record",
                    path: self.path.clone(),
                    source,
                })
            }
        }
    }

    /// Bytes of acknowledged records.
    pub fn valid_len(&self) -> u64 {
        self.valid_len
    }
}

fn encode_frame(record: &[u8]) -> Result<Vec<u8>, JournalError> {
    let len = u32::try_from(record.len())
        .map_err(|_| JournalError::RecordTooLarge { len: record.len() })?;
    let len_bytes = len.to_le_bytes();
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + record.len());
    frame.extend_from_slice(&FRAME_MAGIC);
    frame.extend_from_slice(&len_bytes);
    frame.extend_from_slice(&checksum(&len_bytes, record).to_le_bytes());
    frame.extend_from_slice(record);
    Ok(frame)
}

fn checksum(len_bytes: &[u8; 4], payload: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(len_bytes);
    hasher.update(payload);
    hasher.finalize()
}

struct Scan {
    records: Vec<Vec<u8>>,
    valid_len: usize,
}

/// Return the valid prefix, or the offset of a damaged frame that a later
/// valid frame follows.
fn scan(bytes: &[u8]) -> Result<Scan, usize> {
    let mut records = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        match frame_at(bytes, offset) {
            Some(payload) => {
                records.push(payload.to_vec());
                offset += FRAME_HEADER_LEN + payload.len();
            }
            None => {
                if (offset + 1..bytes.len()).any(|later| frame_at(bytes, later).is_some()) {
                    return Err(offset);
                }
                break;
            }
        }
    }
    Ok(Scan {
        records,
        valid_len: offset,
    })
}

fn frame_at(bytes: &[u8], offset: usize) -> Option<&[u8]> {
    let header = bytes.get(offset..offset.checked_add(FRAME_HEADER_LEN)?)?;
    if header[..4] != FRAME_MAGIC {
        return None;
    }
    let len_bytes: [u8; 4] = header[4..8].try_into().ok()?;
    let expected = u32::from_le_bytes(header[8..12].try_into().ok()?);
    let start = offset + FRAME_HEADER_LEN;
    let end = start.checked_add(usize::try_from(u32::from_le_bytes(len_bytes)).ok()?)?;
    let payload = bytes.get(start..end)?;
    (checksum(&len_bytes, payload) == expected).then_some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_records_reopen_in_order() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let path = root.path().join("journal");
        let (mut journal, recovered) = Journal::open(&path).expect("new journal should open");
        assert!(recovered.records.is_empty());
        for record in [&b"alpha"[..], b"", b"gamma"] {
            journal.append(record).expect("append should sync");
        }
        drop(journal);

        let (journal, recovered) = Journal::open(&path).expect("journal should reopen");
        assert_eq!(
            recovered,
            Recovered {
                records: vec![b"alpha".to_vec(), Vec::new(), b"gamma".to_vec()],
                discarded_tail: 0,
            }
        );
        assert_eq!(
            journal.valid_len(),
            std::fs::metadata(&path).expect("journal metadata").len()
        );
    }

    #[test]
    fn damaged_frame_before_a_valid_frame_fails_closed() {
        let first = encode_frame(b"first").expect("frame should encode");
        let second = encode_frame(b"second").expect("frame should encode");
        let mut bytes = [first.clone(), second].concat();
        bytes[FRAME_HEADER_LEN] ^= 0xff;

        assert_eq!(scan(&bytes).err(), Some(0));
        bytes[FRAME_HEADER_LEN] ^= 0xff;
        bytes[first.len() + FRAME_HEADER_LEN] ^= 0xff;
        let scanned = scan(&bytes).expect("a damaged final frame is a torn tail");
        assert_eq!(scanned.records, [b"first".to_vec()]);
        assert_eq!(scanned.valid_len, first.len());
    }

    #[test]
    fn oversized_length_field_is_a_torn_tail_without_allocation() {
        let mut bytes = encode_frame(b"kept").expect("frame should encode");
        let kept = bytes.len();
        bytes.extend_from_slice(&FRAME_MAGIC);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);

        let scanned = scan(&bytes).expect("an unfinished frame is a torn tail");
        assert_eq!(scanned.records, [b"kept".to_vec()]);
        assert_eq!(scanned.valid_len, kept);
    }
}
