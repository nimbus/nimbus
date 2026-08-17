//! Exact adopted-attempt replay fencing for atomic listener batches.

use super::*;

#[test]
fn activation_requires_the_exact_adopted_attempt_before_and_after_commit() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAS", 1, 1, PORT);
    let claim = bind_claim("activation-attempt");
    let binding = binding(PORT, "activation-resource");
    authority
        .reserve(request.clone())
        .expect("request should reserve");
    authority
        .claim_bind(&request, None, claim.clone())
        .expect("provider attempt should claim");
    let adopted = authority
        .adopt_claimed(&request, None, &claim, binding)
        .expect("exact attempt should adopt");
    let authority_path = authority.store.authority_path().to_path_buf();
    let adopted_bytes =
        std::fs::read(&authority_path).expect("adopted authority bytes should be readable");
    let foreign_same_provider_claim = bind_claim("foreign-activation-attempt");

    assert!(matches!(
        authority.activate_claimed(&request, &foreign_same_provider_claim),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    assert_eq!(
        std::fs::read(&authority_path).expect("rejected activation should leave readable state"),
        adopted_bytes,
        "foreign activation must not rewrite the adopted binding"
    );
    assert_eq!(
        authority
            .inspect(request.lease_id())
            .expect("adopted lease should inspect"),
        Some(adopted)
    );

    let active = authority
        .activate_claimed(&request, &claim)
        .expect("the exact adopted attempt should activate");
    let active_bytes =
        std::fs::read(&authority_path).expect("active authority bytes should be readable");
    assert!(matches!(
        authority.activate_claimed(&request, &foreign_same_provider_claim),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    assert_eq!(
        std::fs::read(&authority_path).expect("rejected replay should leave readable state"),
        active_bytes,
        "foreign active replay must not rewrite durable authority"
    );
    assert_eq!(
        authority
            .activate_claimed(&request, &claim)
            .expect("exact active replay should remain idempotent"),
        active
    );
}

#[test]
fn active_batch_replay_rejects_foreign_same_provider_attempt_atomically() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = provider_assigned_request("01ARZ3NDEKTSV4RRFFQ69G5FAQ");
    let second = provider_assigned_request("01ARZ3NDEKTSV4RRFFQ69G5FAR");
    let reservation_claim = reservation_claim("exact-adopted-batch");
    let first_claim = bind_claim("first-attempt");
    let second_claim = bind_claim("second-attempt");
    let first_binding = provider_assigned_binding(PORT, "first-resource");
    let second_binding = provider_assigned_binding(PORT + 1, "second-resource");
    let exact_batch = vec![
        (first.clone(), first_claim.clone(), first_binding.clone()),
        (second.clone(), second_claim.clone(), second_binding.clone()),
    ];

    authority
        .reserve_batch_for_coordinator(vec![first.clone(), second.clone()], &reservation_claim)
        .expect("batch should reserve");
    authority
        .claim_bind_batch(
            &[
                (first.clone(), first_claim.clone()),
                (second.clone(), second_claim.clone()),
            ],
            Some(&reservation_claim),
        )
        .expect("batch should claim");
    let active = authority
        .adopt_claimed_and_activate_batch(&exact_batch, Some(&reservation_claim))
        .expect("exact claimed batch should activate");
    assert_eq!(active[0].adoption_claim(), Some(&first_claim));
    assert_eq!(active[1].adoption_claim(), Some(&second_claim));

    let authority_path = authority.store.authority_path().to_path_buf();
    let before_bytes =
        std::fs::read(&authority_path).expect("durable authority bytes should be readable");
    let before_records = authority.list().expect("active batch should inspect");
    let foreign_same_provider_claim = bind_claim("foreign-second-attempt");
    let replay = vec![
        (first.clone(), first_claim.clone(), first_binding.clone()),
        (
            second.clone(),
            foreign_same_provider_claim,
            second_binding.clone(),
        ),
    ];

    assert!(matches!(
        authority.adopt_claimed_and_activate_batch(&replay, Some(&reservation_claim)),
        Err(PortLeaseError::BindClaimConflict { lease_id })
            if lease_id == *second.lease_id()
    ));
    assert_eq!(
        std::fs::read(&authority_path).expect("rejected replay must leave readable state"),
        before_bytes,
        "a foreign same-provider attempt must not rewrite durable authority"
    );
    assert_eq!(
        authority.list().expect("rejected replay should re-inspect"),
        before_records,
        "atomic prevalidation must preserve every active sibling exactly"
    );

    drop(authority);
    let reopened = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen after activation");
    assert_eq!(
        reopened
            .adopt_claimed_and_activate_batch(&exact_batch, Some(&reservation_claim))
            .expect("the exact adopted-attempt replay should remain idempotent after restart"),
        active
    );
}
