//! Durable committer-lease capability for shared provider tenant namespaces.

use std::time::Duration;

use nimbus_core::{Error, Result, SequenceNumber, Timestamp};
/// The durable lease fencing a provider tenant namespace's committer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitterLease {
    pub owner_id: String,
    pub epoch: u64,
    pub expires_at: Timestamp,
    pub durable_sequence: SequenceNumber,
}

/// A typed refusal from a committer-lease state transition.
#[derive(Debug, Clone)]
pub enum CommitterLeaseError {
    Held,
    Fenced { owner_id: String, epoch: u64 },
    Unsupported,
    Storage(Error),
}

impl std::fmt::Display for CommitterLeaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held => formatter.write_str("committer lease is held by another owner"),
            Self::Fenced { owner_id, epoch } => write!(
                formatter,
                "committer lease owner {owner_id} at epoch {epoch} has been fenced"
            ),
            Self::Unsupported => {
                formatter.write_str("fenced durable apply requires a provider committer lease")
            }
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CommitterLeaseError {}

impl From<Error> for CommitterLeaseError {
    fn from(error: Error) -> Self {
        Self::Storage(error)
    }
}

pub type CommitterLeaseResult<T> = std::result::Result<T, CommitterLeaseError>;

/// Lease operations supported by networked SQL provider tenant stores.
///
/// Implementations evaluate expiry using the provider's clock. Acquisition
/// and renewal are atomic provider-side state transitions.
pub trait CommitterLeaseStore {
    fn read_committer_lease(&self) -> Result<Option<CommitterLease>>;

    fn acquire_committer_lease(
        &self,
        owner_id: &str,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease>;

    fn renew_committer_lease(
        &self,
        owner_id: &str,
        epoch: u64,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease>;
}
