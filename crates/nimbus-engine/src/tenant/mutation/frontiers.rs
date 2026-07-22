use nimbus_core::SequenceNumber;
use serde::Serialize;

/// Causal progress through one tenant's mutation pipeline.
///
/// Every head is a lower bound. A later phase proves that every earlier phase
/// reached at least the same sequence, so sampling reconciliation may raise an
/// earlier head but never lower a later one. `active_assigned_head` is the only
/// production frontier allowed to contract: a definitive non-commit discards
/// its provisional suffix while `assigned_high_water` preserves history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationFrontierStats {
    pub assigned_high_water: SequenceNumber,
    pub active_assigned_head: SequenceNumber,
    pub durable_head: SequenceNumber,
    pub storage_applied_head: SequenceNumber,
    pub published_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    /// Active assignments waiting for durable persistence.
    pub assignment_lag: u64,
    /// Durable records waiting for storage application.
    pub apply_lag: u64,
    /// Storage-applied records waiting behind the contiguous publish barrier.
    pub publication_lag: u64,
    /// Published records not yet exposed to applied-head waiters.
    pub visibility_lag: u64,
}

impl MutationFrontierStats {
    pub(crate) fn reconcile(
        write_log: WriteLogFrontierSample,
        journal_before: JournalFrontierSample,
        journal_after: JournalFrontierSample,
    ) -> Self {
        let journal = journal_before.max(journal_after);

        // A later-phase observation is causal proof for every earlier phase.
        // This also makes a legal shared-provider `applied > durable` read skew
        // truthful without mutating the production state from diagnostics.
        let applied_head = journal.applied_head;
        let published_head = write_log.published_head.max(applied_head);
        let storage_applied_head = write_log.storage_applied_head.max(published_head);
        let durable_head = journal.durable_head.max(storage_applied_head);
        let active_assigned_head = write_log.active_assigned_head.max(durable_head);
        let assigned_high_water = write_log.assigned_high_water.max(active_assigned_head);

        let stats = Self {
            assigned_high_water,
            active_assigned_head,
            durable_head,
            storage_applied_head,
            published_head,
            applied_head,
            assignment_lag: checked_lag(active_assigned_head, durable_head, "active assignment"),
            apply_lag: checked_lag(durable_head, storage_applied_head, "storage apply"),
            publication_lag: checked_lag(
                storage_applied_head,
                published_head,
                "write-log publication",
            ),
            visibility_lag: checked_lag(published_head, applied_head, "applied visibility"),
        };
        assert!(
            stats.is_causally_ordered(),
            "mutation frontier reconciliation must produce a causal ordering: {stats:?}"
        );
        stats
    }

    pub fn is_causally_ordered(&self) -> bool {
        self.assigned_high_water >= self.active_assigned_head
            && self.active_assigned_head >= self.durable_head
            && self.durable_head >= self.storage_applied_head
            && self.storage_applied_head >= self.published_head
            && self.published_head >= self.applied_head
    }
}

fn checked_lag(upper: SequenceNumber, lower: SequenceNumber, phase: &str) -> u64 {
    upper.0.checked_sub(lower.0).unwrap_or_else(|| {
        panic!("{phase} frontier is causally inverted: upper={upper}, lower={lower}")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WriteLogFrontierSample {
    pub(crate) assigned_high_water: SequenceNumber,
    pub(crate) active_assigned_head: SequenceNumber,
    pub(crate) storage_applied_head: SequenceNumber,
    pub(crate) published_head: SequenceNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JournalFrontierSample {
    pub(crate) durable_head: SequenceNumber,
    pub(crate) applied_head: SequenceNumber,
}

impl JournalFrontierSample {
    fn max(self, other: Self) -> Self {
        Self {
            durable_head: self.durable_head.max(other.durable_head),
            applied_head: self.applied_head.max(other.applied_head),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_progress_applied_observation_implies_durable_lower_bound() {
        let stats = MutationFrontierStats::reconcile(
            WriteLogFrontierSample {
                assigned_high_water: SequenceNumber(4),
                active_assigned_head: SequenceNumber(4),
                storage_applied_head: SequenceNumber(5),
                published_head: SequenceNumber(5),
            },
            JournalFrontierSample {
                durable_head: SequenceNumber(4),
                applied_head: SequenceNumber(5),
            },
            JournalFrontierSample {
                durable_head: SequenceNumber(4),
                applied_head: SequenceNumber(5),
            },
        );

        assert_eq!(stats.assigned_high_water, SequenceNumber(5));
        assert_eq!(stats.active_assigned_head, SequenceNumber(5));
        assert_eq!(stats.durable_head, SequenceNumber(5));
        assert_eq!(stats.storage_applied_head, SequenceNumber(5));
        assert_eq!(stats.published_head, SequenceNumber(5));
        assert_eq!(stats.applied_head, SequenceNumber(5));
        assert_eq!(stats.assignment_lag, 0);
        assert_eq!(stats.apply_lag, 0);
        assert_eq!(stats.publication_lag, 0);
        assert_eq!(stats.visibility_lag, 0);
    }

    #[test]
    fn zero_frontiers_are_ordered_and_have_no_lag() {
        let zero = SequenceNumber(0);
        let stats = MutationFrontierStats::reconcile(
            WriteLogFrontierSample {
                assigned_high_water: zero,
                active_assigned_head: zero,
                storage_applied_head: zero,
                published_head: zero,
            },
            JournalFrontierSample {
                durable_head: zero,
                applied_head: zero,
            },
            JournalFrontierSample {
                durable_head: zero,
                applied_head: zero,
            },
        );
        assert!(stats.is_causally_ordered());
        assert_eq!(
            (
                stats.assignment_lag,
                stats.apply_lag,
                stats.publication_lag,
                stats.visibility_lag,
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn provider_publisher_frontiers_are_monotonic_and_contiguous() {
        let samples = [
            (
                WriteLogFrontierSample {
                    assigned_high_water: SequenceNumber(1),
                    active_assigned_head: SequenceNumber(1),
                    storage_applied_head: SequenceNumber(0),
                    published_head: SequenceNumber(0),
                },
                JournalFrontierSample {
                    durable_head: SequenceNumber(0),
                    applied_head: SequenceNumber(0),
                },
            ),
            (
                WriteLogFrontierSample {
                    assigned_high_water: SequenceNumber(1),
                    active_assigned_head: SequenceNumber(1),
                    storage_applied_head: SequenceNumber(0),
                    published_head: SequenceNumber(0),
                },
                JournalFrontierSample {
                    durable_head: SequenceNumber(1),
                    applied_head: SequenceNumber(0),
                },
            ),
            (
                WriteLogFrontierSample {
                    assigned_high_water: SequenceNumber(1),
                    active_assigned_head: SequenceNumber(1),
                    storage_applied_head: SequenceNumber(1),
                    published_head: SequenceNumber(0),
                },
                JournalFrontierSample {
                    durable_head: SequenceNumber(1),
                    applied_head: SequenceNumber(0),
                },
            ),
            (
                WriteLogFrontierSample {
                    assigned_high_water: SequenceNumber(1),
                    active_assigned_head: SequenceNumber(1),
                    storage_applied_head: SequenceNumber(1),
                    published_head: SequenceNumber(1),
                },
                JournalFrontierSample {
                    durable_head: SequenceNumber(1),
                    applied_head: SequenceNumber(0),
                },
            ),
            (
                WriteLogFrontierSample {
                    assigned_high_water: SequenceNumber(1),
                    active_assigned_head: SequenceNumber(1),
                    storage_applied_head: SequenceNumber(1),
                    published_head: SequenceNumber(1),
                },
                JournalFrontierSample {
                    durable_head: SequenceNumber(1),
                    applied_head: SequenceNumber(1),
                },
            ),
        ];
        let mut previous: Option<MutationFrontierStats> = None;
        for (write_log, journal) in samples {
            let stats = MutationFrontierStats::reconcile(write_log, journal, journal);
            assert!(stats.is_causally_ordered());
            if let Some(previous) = previous {
                assert!(stats.assigned_high_water >= previous.assigned_high_water);
                assert!(stats.active_assigned_head >= previous.active_assigned_head);
                assert!(stats.durable_head >= previous.durable_head);
                assert!(stats.storage_applied_head >= previous.storage_applied_head);
                assert!(stats.published_head >= previous.published_head);
                assert!(stats.applied_head >= previous.applied_head);
            }
            previous = Some(stats);
        }
    }
}
