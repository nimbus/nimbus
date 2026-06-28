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
