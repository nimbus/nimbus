//! Documented request-phase model for the egress PEP.
//!
//! [`REQUEST_PHASE_ORDER`] names the ordered phases that the PEP's planned
//! phase-driven dispatch will follow; the NEG plan treats this phase ordering as
//! a testable PEP invariant. The worker does NOT iterate this constant today —
//! `worker.rs::handle_client` encodes the same ordering inline. The model is
//! kept here so the security-critical orderings (resolve DNS before authorizing
//! the resolved peer, authorize before dialing, select the pool key before
//! dialing) have one named, test-guarded source of truth ahead of the dispatch
//! refactor.

/// One phase of egress-proxy request handling. See the module docs: this is the
/// documented model for planned phase-driven dispatch, not the worker's current
/// control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressProxyRequestPhase {
    CanonicalizeAuthority,
    ResolveDns,
    AuthorizeResolvedPeer,
    SelectPoolKey,
    Dial,
    Relay,
}

pub const REQUEST_PHASE_ORDER: [EgressProxyRequestPhase; 6] = [
    EgressProxyRequestPhase::CanonicalizeAuthority,
    EgressProxyRequestPhase::ResolveDns,
    EgressProxyRequestPhase::AuthorizeResolvedPeer,
    EgressProxyRequestPhase::SelectPoolKey,
    EgressProxyRequestPhase::Dial,
    EgressProxyRequestPhase::Relay,
];
