//! Executable request-phase model for the egress PEP.
//!
//! [`REQUEST_PHASE_ORDER`] names the security-critical phases that the worker
//! records while handling a request. Tests observe these phases through the
//! internal recorder so the important ordering constraints fail closed when code
//! moves: authorize before DNS, authorize the resolved peer before any upstream
//! contact, build pool/peer identity before forwarding, mutate credentials only
//! after authorization, acquire bounded DLP input before forwarding, and emit
//! exactly one terminal log.

use std::sync::Arc;

/// One phase of egress-proxy request handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressProxyRequestPhase {
    CanonicalizeAuthority,
    RejectMalformedOrCallerCredentials,
    PreDnsAuthorize,
    ResolveDns,
    AuthorizeResolvedIp,
    SelectPoolKey,
    BuildUpstreamPeer,
    CredentialHeaderMutation,
    BoundedDlpInspection,
    Forward,
    ResponseFilters,
    TerminalLog,
}

pub const REQUEST_PHASE_ORDER: [EgressProxyRequestPhase; 12] = [
    EgressProxyRequestPhase::CanonicalizeAuthority,
    EgressProxyRequestPhase::RejectMalformedOrCallerCredentials,
    EgressProxyRequestPhase::PreDnsAuthorize,
    EgressProxyRequestPhase::ResolveDns,
    EgressProxyRequestPhase::AuthorizeResolvedIp,
    EgressProxyRequestPhase::SelectPoolKey,
    EgressProxyRequestPhase::BuildUpstreamPeer,
    EgressProxyRequestPhase::CredentialHeaderMutation,
    EgressProxyRequestPhase::BoundedDlpInspection,
    EgressProxyRequestPhase::Forward,
    EgressProxyRequestPhase::ResponseFilters,
    EgressProxyRequestPhase::TerminalLog,
];

pub(crate) type PhaseObserver = Arc<dyn Fn(EgressProxyRequestPhase) + Send + Sync + 'static>;

pub(crate) fn noop_phase_observer() -> PhaseObserver {
    Arc::new(|_| {})
}

#[derive(Clone)]
pub(crate) struct RequestPhaseRecorder {
    observer: PhaseObserver,
    // Shared across clones so any terminal emission (worker paths, the Pingora
    // adapter's logging callback, the intercept relay) defuses the request's
    // abort guard.
    terminal_seen: Arc<std::sync::atomic::AtomicBool>,
}

impl RequestPhaseRecorder {
    pub(crate) fn new(observer: PhaseObserver) -> Self {
        Self {
            observer,
            terminal_seen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub(crate) fn record(&self, phase: EgressProxyRequestPhase) {
        if matches!(phase, EgressProxyRequestPhase::TerminalLog) {
            self.terminal_seen
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        (self.observer)(phase);
    }

    /// True once any terminal decision event has been recorded for this
    /// request, across every clone of the recorder.
    pub(crate) fn terminal_recorded(&self) -> bool {
        self.terminal_seen.load(std::sync::atomic::Ordering::SeqCst)
    }
}
