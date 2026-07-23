use nimbus_core::{Result, SequenceNumber, TriggerInvocationRecord};

use crate::{
    CommitterLeaseError, CommitterLeaseResult, LibsqlReplicaTenantStore, MemoryTenantStore,
    MySqlTenantStore, PostgresTenantStore, SqliteTenantStore, TenantStore,
};

/// Durable lifecycle-transition seam for one trigger invocation.
///
/// The operation is an idempotent replacement of the complete invocation
/// record. Embedded adapters execute it under process-local tenant authority.
/// External-provider adapters must validate `(owner_id, epoch,
/// expected_durable_sequence)` in the same transaction as the replacement,
/// without advancing the mutation journal head. A rejected lease is returned
/// as [`CommitterLeaseError::Fenced`]; transport or serialization failures
/// retain their storage classification so the engine can retry the identical
/// record without re-running the handler.
pub trait TriggerInvocationTransitionStore {
    fn persist_trigger_invocation_transition(&self, record: &TriggerInvocationRecord)
    -> Result<()>;

    fn persist_fenced_trigger_invocation_transition(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_durable_sequence: SequenceNumber,
        record: &TriggerInvocationRecord,
    ) -> CommitterLeaseResult<()> {
        let _ = (owner_id, epoch, expected_durable_sequence, record);
        Err(CommitterLeaseError::Unsupported)
    }
}

macro_rules! impl_embedded_trigger_invocation_transition_store {
    ($store:ty) => {
        impl TriggerInvocationTransitionStore for $store {
            fn persist_trigger_invocation_transition(
                &self,
                record: &TriggerInvocationRecord,
            ) -> Result<()> {
                <$store>::save_trigger_invocation(self, record)
            }
        }
    };
}

macro_rules! impl_provider_trigger_invocation_transition_store {
    ($store:ty) => {
        impl TriggerInvocationTransitionStore for $store {
            fn persist_trigger_invocation_transition(
                &self,
                record: &TriggerInvocationRecord,
            ) -> Result<()> {
                <$store>::save_trigger_invocation(self, record)
            }

            fn persist_fenced_trigger_invocation_transition(
                &self,
                owner_id: &str,
                epoch: u64,
                expected_durable_sequence: SequenceNumber,
                record: &TriggerInvocationRecord,
            ) -> CommitterLeaseResult<()> {
                <$store>::fenced_save_trigger_invocation(
                    self,
                    owner_id,
                    epoch,
                    expected_durable_sequence,
                    record,
                )
            }
        }
    };
}

impl_embedded_trigger_invocation_transition_store!(TenantStore);
impl_embedded_trigger_invocation_transition_store!(SqliteTenantStore);
impl_embedded_trigger_invocation_transition_store!(MemoryTenantStore);
impl_provider_trigger_invocation_transition_store!(PostgresTenantStore);
impl_provider_trigger_invocation_transition_store!(MySqlTenantStore);
impl_provider_trigger_invocation_transition_store!(LibsqlReplicaTenantStore);
