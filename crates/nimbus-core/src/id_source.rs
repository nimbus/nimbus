//! Canonical injectable document-identity seam.
//!
//! Production IDs retain the existing ULID representation, while simulations
//! can replay a deterministic sequence without consulting ambient randomness.

use std::sync::atomic::{AtomicU64, Ordering};

use ulid::Ulid;

use crate::DocumentId;

/// Source of generated document identifiers.
pub trait IdSource: Send + Sync {
    /// Returns the next identifier for a document or document-backed job.
    fn next_document_id(&self) -> DocumentId;
}

/// [`IdSource`] backed by the production ULID generator.
#[derive(Default)]
pub struct SystemIdSource;

impl IdSource for SystemIdSource {
    fn next_document_id(&self) -> DocumentId {
        DocumentId::new()
    }
}

/// Reproducible ULID source backed by a seed and monotonic counter.
pub struct SeededIdSource {
    seed: u64,
    next_counter: AtomicU64,
}

impl SeededIdSource {
    /// Creates a source whose first identifier uses counter zero.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            next_counter: AtomicU64::new(0),
        }
    }
}

impl IdSource for SeededIdSource {
    fn next_document_id(&self) -> DocumentId {
        let counter = self
            .next_counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |counter| {
                counter.checked_add(1)
            })
            .expect("seeded id source counter should not be exhausted");
        let seeded_counter = (u128::from(self.seed) << 64) | u128::from(counter);
        DocumentId::from_key(Ulid::from(seeded_counter).to_string())
            .expect("a generated ULID should be a valid document id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_id_source_replays_the_same_sequence() {
        let left = SeededIdSource::new(0);
        let right = SeededIdSource::new(0);

        let left_ids = [left.next_document_id(), left.next_document_id()];
        let right_ids = [right.next_document_id(), right.next_document_id()];

        assert_eq!(left_ids, right_ids);
        assert_eq!(left_ids[0].as_str(), "00000000000000000000000000");
        assert_eq!(left_ids[1].as_str(), "00000000000000000000000001");
    }

    #[test]
    fn system_id_source_preserves_the_ulid_format() {
        let id = SystemIdSource.next_document_id();
        assert!(
            Ulid::from_string(id.as_str()).is_ok(),
            "production ids should remain parseable ULIDs"
        );
    }
}
