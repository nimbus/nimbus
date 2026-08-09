use super::{FinalIngressPhase, PublishedIngressAuthority};

#[test]
fn direct_fixture_never_authenticates_a_portable_publication() {
    let authority = PublishedIngressAuthority::direct_fixture();
    assert!(authority.reference.is_none());
    assert!(authority.provider_source_digest.is_none());
    assert!(authority.workload_source_digest.is_none());
    assert_eq!(FinalIngressPhase::Published, FinalIngressPhase::Published);
}
