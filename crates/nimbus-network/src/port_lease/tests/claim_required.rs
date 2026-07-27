//! Exact durable bind-claim requirements for provider evidence.

use super::*;

#[test]
fn reserved_lease_rejects_adoption_before_exact_bind_claim() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAZ", 1, 1, PORT);
    let binding = binding(PORT, "claimed-only-adoption");
    let claim = PortBindClaim::new(binding.provider_handle().clone());
    authority
        .reserve(request.clone())
        .expect("reservation should commit");
    let authority_path = authority.store.authority_path().to_path_buf();
    let before =
        std::fs::read(&authority_path).expect("durable authority bytes should be readable");

    assert!(matches!(
        authority.adopt_claimed(&request, None, &claim, binding.clone()),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    assert_eq!(
        std::fs::read(&authority_path).expect("rejected adoption must leave readable state"),
        before,
        "rejected claimless adoption must not rewrite durable authority"
    );

    authority
        .claim_bind(&request, None, claim.clone())
        .expect("exact provider attempt should claim");
    drop(authority);
    let reopened =
        LocalPortLeaseAuthority::open(root.path()).expect("authority should reopen after claim");
    let adopted = reopened
        .adopt_claimed(&request, None, &claim, binding)
        .expect("durably claimed provider attempt should adopt");
    assert_eq!(adopted.phase(), PortLeasePhase::Binding);
    assert_eq!(adopted.bind_claim(), Some(&claim));
}
