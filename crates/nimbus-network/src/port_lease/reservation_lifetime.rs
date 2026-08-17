//! Process-lifetime fencing for pre-publication launch reservations.
//!
//! A launch coordinator holds this lock from atomic port reservation until the
//! canonical workload manifest publishes the exact request set. A fresh
//! process may therefore compensate claim-only orphan records only after the
//! original coordinator lifetime has ended.

use std::fmt;
use std::fs::File;

use fs2::FileExt;
use sha2::{Digest, Sha256};

use super::{LocalPortLeaseAuthority, PortLeaseError};
use crate::NetworkReservationClaim;
use crate::state_store::{create_dir_all_owner_only, is_lock_contended, open_owner_file};

const RESERVATION_LIFETIME_LOCK_DIRECTORY: &str = "port-reservation-lifetimes";
const RESERVATION_LIFETIME_KEY_DOMAIN: &[u8] = b"nimbus.network.port-reservation-lifetime.v1";

/// Result of nonblocking launch-reservation lifetime acquisition.
#[derive(Debug)]
pub enum NetworkReservationLifetimeAttempt {
    /// The caller exclusively owns the exact coordinator lifetime.
    Acquired(NetworkReservationLifetimeGuard),
    /// Another live process still owns the same attempt-unique claim.
    LiveOwner,
}

/// Non-cloneable process lifetime for one attempt-unique launch reservation.
pub struct NetworkReservationLifetimeGuard {
    claim: NetworkReservationClaim,
    _file: File,
}

impl NetworkReservationLifetimeGuard {
    /// Exact durable claim whose vulnerable pre-publication interval is held.
    pub fn claim(&self) -> &NetworkReservationClaim {
        &self.claim
    }
}

impl fmt::Debug for NetworkReservationLifetimeGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NetworkReservationLifetimeGuard")
            .field(
                "provider_id",
                self.claim.coordinator_attempt().provider_id(),
            )
            .field("opaque_value", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl LocalPortLeaseAuthority {
    /// Try to own one launch claim until its exact request set is durable.
    ///
    /// Lock-file existence is not evidence. Only live exclusive ownership of
    /// the claim-derived OS lock distinguishes an in-flight coordinator from a
    /// dead one. The attempt-unique claim is the durable generation fence.
    pub fn try_acquire_reservation_lifetime(
        &self,
        claim: &NetworkReservationClaim,
    ) -> Result<NetworkReservationLifetimeAttempt, PortLeaseError> {
        let directory = self
            .store
            .state_root()
            .join("networks")
            .join("control-plane")
            .join(RESERVATION_LIFETIME_LOCK_DIRECTORY);
        create_dir_all_owner_only(&directory).map_err(PortLeaseError::Store)?;
        let path = directory.join(format!("{}.lock", reservation_lifetime_key(claim)));
        let file = open_owner_file(&path, false).map_err(PortLeaseError::Store)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(NetworkReservationLifetimeAttempt::Acquired(
                NetworkReservationLifetimeGuard {
                    claim: claim.clone(),
                    _file: file,
                },
            )),
            Err(source) if is_lock_contended(&source) => {
                Ok(NetworkReservationLifetimeAttempt::LiveOwner)
            }
            Err(source) => Err(PortLeaseError::Store(crate::NetworkStateStoreError::Io {
                operation: "acquire launch-reservation lifetime lock",
                path,
                source,
            })),
        }
    }
}

fn reservation_lifetime_key(claim: &NetworkReservationClaim) -> String {
    let attempt = claim.coordinator_attempt();
    let mut digest = Sha256::new();
    digest.update(RESERVATION_LIFETIME_KEY_DOMAIN);
    digest.update([0]);
    digest.update(attempt.provider_id().as_str().as_bytes());
    digest.update([0]);
    digest.update(attempt.expose_to_provider().as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests;
