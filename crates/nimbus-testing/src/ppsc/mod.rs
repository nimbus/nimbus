//! Deterministic scenario and legal-state contract for Nimbus's
//! parallel-prepare/serial-commit mutation path.
//!
//! This module owns bounded scenario data, backend capability validation,
//! canonical histories, shrinking, and the pure terminal-state auditor. It
//! deliberately performs no Engine or storage I/O. Production-interface
//! adapters consume this contract and are responsible for recording every
//! observed effect without normalization.

mod audit;
mod scenario;
mod shrink;

#[cfg(test)]
mod tests;

pub use audit::{
    PpscAuditError, PpscEffect, PpscFrontiers, PpscHistory, PpscJournalEntry, PpscObservedStep,
    PpscPublication, PpscSequenceClaim, PpscSequenceOwnership, PpscTenantState, PpscTerminalState,
    audit_ppsc_history,
};
pub use scenario::{
    PPSC_MAX_STEPS, PpscBackend, PpscBackendCapabilities, PpscCommitOrder, PpscExpectedOutcome,
    PpscInjectedFault, PpscOperation, PpscRoute, PpscScenario, PpscScenarioError, PpscStep,
    retained_ppsc_scenarios, retained_provider_authority_scenarios,
};
pub use shrink::shrink_failing_ppsc_scenario;
