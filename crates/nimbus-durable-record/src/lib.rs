//! Crash-consistent local records on a POSIX-style filesystem.
//!
//! This leaf crate owns the three durability protocols that host-local
//! authority stores share:
//!
//! - [`sync_directory`] makes a directory entry change durable.
//! - [`StagedWrite`] publishes one complete file through a stage file in the
//!   same directory. It syncs the stage, commits it by rename (or by a
//!   no-replace link), and then syncs the directory.
//! - [`Journal`] appends checksummed records and, on open, recovers the
//!   longest valid prefix by discarding a torn tail.
//!
//! The crate has no workspace dependency. Callers keep their own envelopes,
//! locks, and error vocabulary, and map the typed step errors here onto them.

mod directory;
mod journal;
mod staged_write;

pub use directory::sync_directory;
pub use journal::{FRAME_HEADER_LEN, Journal, JournalError, Recovered};
pub use staged_write::{
    CleanupError, CleanupStep, Commit, StagedWrite, WriteCheckpoint, WriteError, WriteFailure,
    WriteStep,
};
