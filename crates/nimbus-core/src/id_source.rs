//! Canonical injectable persistent-identity seam.
//!
//! Production document, table, and provider-writer IDs retain the existing
//! ULID representation, while simulations can replay independent deterministic
//! sequences without consulting ambient randomness. Keeping the streams
//! separate prevents provider lifecycle work from perturbing application IDs.

use std::sync::atomic::{AtomicU64, Ordering};

use ulid::Ulid;

use crate::{DocumentId, TableId};

/// Source of generated persistent identifiers.
pub trait IdSource: Send + Sync {
    /// Returns the next identifier for a document or document-backed job.
    fn next_document_id(&self) -> DocumentId;

    /// Returns the next stable identity for a logical table instance.
    fn next_table_id(&self) -> TableId;

    /// Returns the next process identity for provider committer ownership.
    fn next_committer_owner_id(&self) -> String;
}

/// [`IdSource`] backed by the production ULID generator.
#[derive(Default)]
pub struct SystemIdSource;

impl IdSource for SystemIdSource {
    fn next_document_id(&self) -> DocumentId {
        DocumentId::new()
    }

    fn next_table_id(&self) -> TableId {
        TableId::new()
    }

    fn next_committer_owner_id(&self) -> String {
        Ulid::new().to_string()
    }
}

/// Reproducible ULID source backed by a seed and monotonic counter.
pub struct SeededIdSource {
    seed: u64,
    next_document_counter: AtomicU64,
    next_table_counter: AtomicU64,
    next_committer_owner_counter: AtomicU64,
}

impl SeededIdSource {
    /// Creates a source whose first identifier uses counter zero.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            next_document_counter: AtomicU64::new(0),
            next_table_counter: AtomicU64::new(0),
            next_committer_owner_counter: AtomicU64::new(0),
        }
    }
}

impl IdSource for SeededIdSource {
    fn next_document_id(&self) -> DocumentId {
        let counter = self
            .next_document_counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |counter| {
                counter.checked_add(1)
            })
            .expect("seeded id source counter should not be exhausted");
        let seeded_counter = (u128::from(self.seed) << 64) | u128::from(counter);
        DocumentId::from_key(Ulid::from(seeded_counter).to_string())
            .expect("a generated ULID should be a valid document id")
    }

    fn next_table_id(&self) -> TableId {
        let counter = self
            .next_table_counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |counter| {
                counter.checked_add(1)
            })
            .expect("seeded table id source counter should not be exhausted");
        let seeded_counter = (u128::from(self.seed) << 64) | u128::from(counter);
        TableId::try_from(Ulid::from(seeded_counter).to_string())
            .expect("a generated ULID should be a valid table id")
    }

    fn next_committer_owner_id(&self) -> String {
        let counter = self
            .next_committer_owner_counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |counter| {
                counter.checked_add(1)
            })
            .expect("seeded committer-owner id source counter should not be exhausted");
        let seeded_counter = (u128::from(self.seed) << 64) | u128::from(counter);
        Ulid::from(seeded_counter).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_id_source_replays_independent_persistent_identity_sequences() {
        let left = SeededIdSource::new(0);
        let right = SeededIdSource::new(0);

        let left_ids = [left.next_document_id(), left.next_document_id()];
        let right_ids = [right.next_document_id(), right.next_document_id()];

        assert_eq!(left_ids, right_ids);
        assert_eq!(left_ids[0].as_str(), "00000000000000000000000000");
        assert_eq!(left_ids[1].as_str(), "00000000000000000000000001");
        let left_table_ids = [left.next_table_id(), left.next_table_id()];
        let right_table_ids = [right.next_table_id(), right.next_table_id()];

        assert_eq!(left_table_ids, right_table_ids);
        assert_eq!(left_table_ids[0].as_str(), "00000000000000000000000000");
        assert_eq!(left_table_ids[1].as_str(), "00000000000000000000000001");
        let left_owner_ids = [
            left.next_committer_owner_id(),
            left.next_committer_owner_id(),
        ];
        let right_owner_ids = [
            right.next_committer_owner_id(),
            right.next_committer_owner_id(),
        ];
        assert_eq!(left_owner_ids, right_owner_ids);
        assert_eq!(left_owner_ids[0], "00000000000000000000000000");
        assert_eq!(left_owner_ids[1], "00000000000000000000000001");

        assert_eq!(
            left.next_document_id().as_str(),
            "00000000000000000000000002",
            "provider-writer and table allocation must not consume the document stream"
        );
    }

    #[test]
    fn system_id_source_preserves_the_ulid_format() {
        let id = SystemIdSource.next_document_id();
        assert!(
            Ulid::from_string(id.as_str()).is_ok(),
            "production ids should remain parseable ULIDs"
        );
        assert!(Ulid::from_string(SystemIdSource.next_table_id().as_str()).is_ok());
        assert!(Ulid::from_string(&SystemIdSource.next_committer_owner_id()).is_ok());
    }
}
