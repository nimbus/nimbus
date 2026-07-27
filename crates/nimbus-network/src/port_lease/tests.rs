use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::sync::{Arc, Barrier};
use std::thread;

use crate::{ListenerId, NetworkProviderHandle, NetworkProviderId};

use super::*;

#[path = "tests/adopted_replay.rs"]
mod adopted_replay;
#[path = "tests/claim_required.rs"]
mod claim_required;
#[path = "tests/tenant_quota.rs"]
mod tenant_quota;

const PORT: u16 = 41_473;

#[test]
fn lifecycle_is_idempotent_fenced_and_restart_durable() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 7, 11, PORT);
    let binding = binding(PORT, "provider-binding-a");

    let reserved = authority
        .reserve(request.clone())
        .expect("reservation should commit");
    assert_eq!(reserved.phase(), PortLeasePhase::Reserved);
    assert_eq!(
        authority
            .reserve(request.clone())
            .expect("reservation replay should be idempotent"),
        reserved
    );

    let activation_error = authority
        .activate_claimed(&request, &bind_claim("not-adopted"))
        .expect_err("activation before adoption must fail");
    assert!(matches!(
        activation_error,
        PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Reserved,
            operation: PortLeaseOperation::Activate,
            ..
        }
    ));

    let claim = PortBindClaim::new(binding.provider_handle().clone());
    authority
        .claim_bind(&request, None, claim.clone())
        .expect("bind attempt should claim the reservation");
    let adopted = authority
        .adopt_claimed(&request, None, &claim, binding.clone())
        .expect("binding should be adopted");
    assert_eq!(adopted.phase(), PortLeasePhase::Binding);
    assert_eq!(adopted.binding(), Some(&binding));
    assert_eq!(
        authority
            .adopt_claimed(&request, None, &claim, binding.clone())
            .expect("adoption replay should be idempotent"),
        adopted
    );

    let active = authority
        .activate_claimed(&request, &claim)
        .expect("adopted binding should activate");
    assert_eq!(active.phase(), PortLeasePhase::Active);
    assert_eq!(
        authority
            .activate_claimed(&request, &claim)
            .expect("activation replay should be idempotent"),
        active
    );

    let withdrawing = authority
        .withdraw(&request)
        .expect("active lease should withdraw");
    assert_eq!(withdrawing.phase(), PortLeasePhase::Withdrawing);
    assert_eq!(
        authority
            .withdraw(&request)
            .expect("withdraw replay should be idempotent"),
        withdrawing
    );

    let released = authority
        .release(&request)
        .expect("withdrawn lease should release");
    assert_eq!(released.phase(), PortLeasePhase::Released);
    assert_eq!(
        authority
            .release(&request)
            .expect("release replay should be idempotent"),
        released
    );

    drop(authority);
    let restarted = LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
    assert_eq!(
        restarted
            .inspect(request.lease_id())
            .expect("lease should inspect"),
        Some(released)
    );
}

#[test]
fn withdraw_rejects_durable_reserved_claim_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FBC", 7, 11, PORT);
    let claim = bind_claim("withdraw-in-flight");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    authority
        .reserve(request.clone())
        .expect("request should reserve");
    let claimed = authority
        .claim_bind(&request, None, claim.clone())
        .expect("exact bind attempt should claim");
    drop(authority);

    let restarted =
        LocalPortLeaseAuthority::open(root.path()).expect("authority should reopen after claim");
    let error = restarted
        .withdraw(&request)
        .expect_err("generic withdrawal must not erase an in-flight durable bind claim");
    assert!(matches!(
        error,
        PortLeaseError::BindClaimConflict { ref lease_id }
            if lease_id == request.lease_id()
    ));
    assert_eq!(
        restarted
            .inspect(request.lease_id())
            .expect("claimed request should inspect"),
        Some(claimed),
        "rejected withdrawal must preserve the exact durable attempt byte-for-byte"
    );
    restarted
        .abandon_bind_claims_without_effect(&[(request.clone(), claim)], None)
        .expect("the exact claimant may abandon after proving no provider effect");
    restarted
        .withdraw(&request)
        .expect("clean reservation may then withdraw");
    restarted
        .release(&request)
        .expect("confirmed no-effect withdrawal may release");
}

#[test]
fn confirmed_stop_rebind_transition_is_exact_fenced_and_idempotent() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let lease_request = request("01ARZ3NDEKTSV4RRFFQ69G5FBG", 7, 11, PORT);
    let expected_binding = binding(PORT, "rebind-exact");
    let expected_claim = PortBindClaim::new(expected_binding.provider_handle().clone());
    authority
        .reserve(lease_request.clone())
        .expect("request should reserve");
    claim_and_adopt(&authority, &lease_request, None, expected_binding.clone())
        .expect("binding should adopt");
    authority
        .activate_claimed(&lease_request, &expected_claim)
        .expect("binding should activate");

    let wrong_binding = binding(PORT, "rebind-foreign");
    assert!(matches!(
        authority.prepare_rebind_after_confirmed_stop(&lease_request, &wrong_binding),
        Err(PortLeaseError::BindingConflict { .. })
    ));
    let reserved = authority
        .prepare_rebind_after_confirmed_stop(&lease_request, &expected_binding)
        .expect("exact confirmed stop may retain the port for rebind");
    assert_eq!(reserved.phase(), PortLeasePhase::Reserved);
    assert_eq!(reserved.reserved_port(), NonZeroU16::new(PORT));
    assert!(reserved.binding().is_none());
    assert_eq!(
        reserved.confirmed_stopped_binding(),
        Some(&expected_binding),
        "the exact stopped binding must remain as durable absence evidence"
    );
    assert_eq!(
        authority
            .prepare_rebind_after_confirmed_stop(&lease_request, &expected_binding)
            .expect("exact transition replay should be idempotent"),
        reserved
    );

    let stale = request("01ARZ3NDEKTSV4RRFFQ69G5FBG", 8, 11, PORT);
    assert!(matches!(
        authority.prepare_rebind_after_confirmed_stop(&stale, &expected_binding),
        Err(PortLeaseError::StaleFence(_))
    ));
    authority
        .withdraw(&lease_request)
        .expect("reserved request should withdraw");
    assert!(matches!(
        authority.prepare_rebind_after_confirmed_stop(&lease_request, &expected_binding),
        Err(PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Withdrawing,
            operation: PortLeaseOperation::PrepareRebindAfterConfirmedStop,
            ..
        })
    ));
}

#[test]
fn confirmed_stop_rebind_batch_is_atomic_exact_and_idempotent() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FBD", 7, 11, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FBE", 7, 11, PORT + 1);
    let first_binding = binding(PORT, "batch-rebind-first");
    let second_binding = binding(PORT + 1, "batch-rebind-second");
    for (request, binding) in [(&first, &first_binding), (&second, &second_binding)] {
        let claim = PortBindClaim::new(binding.provider_handle().clone());
        authority
            .reserve(request.clone())
            .expect("request should reserve");
        claim_and_adopt(&authority, request, None, binding.clone()).expect("binding should adopt");
        authority
            .activate_claimed(request, &claim)
            .expect("binding should activate");
    }

    let wrong_second = binding(PORT + 1, "batch-rebind-foreign");
    assert!(matches!(
        authority.prepare_rebind_batch_after_confirmed_stop(&[
            (first.clone(), first_binding.clone()),
            (second.clone(), wrong_second),
        ]),
        Err(PortLeaseError::BindingConflict { .. })
    ));
    for request in [&first, &second] {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("lease should remain durable")
                .phase(),
            PortLeasePhase::Active,
            "one divergent sibling must leave the complete batch unchanged"
        );
    }

    let expected = [
        (first.clone(), first_binding),
        (second.clone(), second_binding),
    ];
    let rebound = authority
        .prepare_rebind_batch_after_confirmed_stop(&expected)
        .expect("exact stopped batch should retain every selected port");
    assert!(
        rebound.iter().all(|record| {
            record.phase() == PortLeasePhase::Reserved
                && record.binding().is_none()
                && record.bind_claim().is_none()
                && record.failure().is_none()
        }),
        "the complete exact batch must return to clean Reserved authority"
    );
    assert_eq!(
        rebound
            .iter()
            .map(PortLeaseRecord::confirmed_stopped_binding)
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|(_, binding)| Some(binding))
            .collect::<Vec<_>>(),
        "each restart-retained member must authenticate its own stopped provider binding"
    );
    assert_eq!(
        authority
            .prepare_rebind_batch_after_confirmed_stop(&expected)
            .expect("exact batch replay should be idempotent"),
        rebound
    );
}

#[test]
fn confirmed_stop_release_rejects_fresh_or_claimed_reservations_and_is_idempotent() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let fresh = request("01ARZ3NDEKTSV4RRFFQ69G5FBC", 7, 11, PORT);
    authority
        .reserve(fresh.clone())
        .expect("fresh request should reserve");
    assert!(matches!(
        authority.release_after_confirmed_stop(&fresh),
        Err(PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Reserved,
            operation: PortLeaseOperation::ReleaseAfterConfirmedStop,
            ..
        })
    ));

    let claim = bind_claim("fresh-reservation-claim");
    authority
        .claim_bind(&fresh, None, claim.clone())
        .expect("fresh reservation bind should claim");
    assert!(matches!(
        authority.release_after_confirmed_stop(&fresh),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    authority
        .abandon_bind_claims_without_effect(&[(fresh.clone(), claim)], None)
        .expect("exact no-effect claim should abandon");

    let active = request("01ARZ3NDEKTSV4RRFFQ69G5FBJ", 7, 11, PORT + 1);
    let expected_binding = binding(PORT + 1, "release-after-confirmed-stop");
    let expected_claim = PortBindClaim::new(expected_binding.provider_handle().clone());
    authority
        .reserve(active.clone())
        .expect("active request should reserve");
    claim_and_adopt(&authority, &active, None, expected_binding.clone())
        .expect("active request should adopt");
    authority
        .activate_claimed(&active, &expected_claim)
        .expect("active request should activate");
    authority
        .prepare_rebind_after_confirmed_stop(&active, &expected_binding)
        .expect("confirmed stop should retain exact durable evidence");
    let released = authority
        .release_after_confirmed_stop(&active)
        .expect("exact stopped reservation should release");
    assert_eq!(released.phase(), PortLeasePhase::Released);
    assert!(released.confirmed_stopped_binding().is_none());
    assert_eq!(
        authority
            .release_after_confirmed_stop(&active)
            .expect("terminal replay should be idempotent"),
        released
    );
}

#[test]
fn confirmed_stop_release_batch_rejects_mixed_phases_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FBK", 7, 11, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FBM", 7, 11, PORT + 1);
    let first_binding = binding(PORT, "partial-first");
    let second_binding = binding(PORT + 1, "partial-second");
    for (request, expected_binding) in [(&first, &first_binding), (&second, &second_binding)] {
        let claim = PortBindClaim::new(expected_binding.provider_handle().clone());
        authority
            .reserve(request.clone())
            .expect("request should reserve");
        claim_and_adopt(&authority, request, None, expected_binding.clone())
            .expect("binding should adopt");
        authority
            .activate_claimed(request, &claim)
            .expect("binding should activate");
        authority
            .prepare_rebind_after_confirmed_stop(request, expected_binding)
            .expect("confirmed stop should retain exact evidence");
    }
    authority
        .release_after_confirmed_stop(&first)
        .expect("the first exact member should release");
    let before = authority.list().expect("mixed batch should inspect");

    let error = authority
        .release_batch_after_confirmed_stop(&[first.clone(), second.clone()])
        .expect_err("mixed released and retained phases must fail closed");
    assert!(
        matches!(
            error,
            PortLeaseError::InvalidTransition {
                phase: PortLeasePhase::Released,
                ..
            }
        ),
        "mixed atomic-batch evidence must report the terminal member: {error}"
    );
    assert_eq!(
        authority.list().expect("mixed batch should re-inspect"),
        before,
        "mixed phase rejection must leave every sibling byte-for-byte unchanged"
    );

    authority
        .release_after_confirmed_stop(&second)
        .expect("the remaining exact member should release");
    let released = authority
        .release_batch_after_confirmed_stop(&[first.clone(), second.clone()])
        .expect("an all-released batch replay should be idempotent");
    assert!(
        released.iter().all(|record| {
            record.phase() == PortLeasePhase::Released
                && record.confirmed_stopped_binding().is_none()
        }),
        "all-released replay must preserve terminal evidence"
    );
    assert_eq!(
        authority
            .release_batch_after_confirmed_stop(&[first, second])
            .expect("a repeated all-released batch replay should remain idempotent"),
        released
    );
}

#[test]
fn confirmed_stop_release_batch_rejects_invalid_sibling_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FBN", 7, 11, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FBP", 7, 11, PORT + 1);
    for (request, expected_binding) in [
        (&first, binding(PORT, "atomic-first")),
        (&second, binding(PORT + 1, "atomic-second")),
    ] {
        let claim = PortBindClaim::new(expected_binding.provider_handle().clone());
        authority
            .reserve(request.clone())
            .expect("request should reserve");
        claim_and_adopt(&authority, request, None, expected_binding.clone())
            .expect("binding should adopt");
        authority
            .activate_claimed(request, &claim)
            .expect("binding should activate");
        authority
            .prepare_rebind_after_confirmed_stop(request, &expected_binding)
            .expect("confirmed stop should retain exact evidence");
    }
    authority
        .claim_bind(&second, None, bind_claim("invalid-sibling-attempt"))
        .expect("the sibling should carry an in-flight bind attempt");
    let before = authority.list().expect("authority should inspect");

    let error = authority
        .release_batch_after_confirmed_stop(&[first, second])
        .expect_err("one in-flight sibling must reject the complete release batch");
    assert!(
        matches!(error, PortLeaseError::BindClaimConflict { .. }),
        "the exact conflicting evidence must be reported: {error}"
    );
    assert_eq!(
        authority.list().expect("authority should re-inspect"),
        before,
        "transactional prevalidation must leave every valid and invalid sibling byte-for-byte unchanged"
    );
}

#[test]
fn publication_intent_is_part_of_exact_request_fence() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let original = published_request("01ARZ3NDEKTSV4RRFFQ69G5FBF", PORT);
    assert_eq!(
        original.publication().host_address(),
        Some(Ipv4Addr::LOCALHOST.into())
    );
    authority
        .reserve(original.clone())
        .expect("original publication should reserve");

    let mut substituted = original.clone();
    substituted.publication = PortPublicationIntent::host(Ipv4Addr::new(203, 0, 113, 7).into());
    assert!(matches!(
        authority.reserve(substituted.clone()),
        Err(PortLeaseError::IdentityConflict { .. })
    ));
    assert!(matches!(
        authority.withdraw(&substituted),
        Err(PortLeaseError::StaleFence(_))
    ));
    assert_eq!(
        authority
            .inspect(original.lease_id())
            .expect("original should inspect")
            .expect("original should remain durable")
            .request(),
        &original
    );

    let mapped = PortPublicationIntent::host(
        "::ffff:127.0.0.1"
            .parse()
            .expect("mapped IPv6 fixture should parse"),
    );
    assert_eq!(mapped, original.publication().clone());
    let directly_constructed = PortLeaseRequest::new(
        original.lease_id().clone(),
        original.owner_id().clone(),
        original.tenant_id().cloned(),
        PortLeaseFence::new(original.generation(), original.lease_epoch()),
        original.accounting(),
        PortPublicationIntent::Host {
            address: "::ffff:127.0.0.1"
                .parse()
                .expect("mapped IPv6 fixture should parse"),
        },
        original.binding().clone(),
    );
    assert_eq!(
        directly_constructed.publication(),
        original.publication(),
        "the durable request constructor must canonicalize even direct enum construction"
    );
    let encoded = serde_json::to_value(&original).expect("request should serialize");
    let mut missing_publication = encoded.clone();
    missing_publication
        .as_object_mut()
        .expect("request should encode as an object")
        .remove("publication");
    assert!(
        serde_json::from_value::<PortLeaseRequest>(missing_publication).is_err(),
        "the pre-launch wire break must reject requests missing publication intent"
    );
    assert_eq!(
        serde_json::from_value::<PortLeaseRequest>(encoded).expect("request should round trip"),
        original
    );
}

#[test]
fn bind_claim_batch_is_atomic_exclusive_durable_and_exactly_abandonable() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let reservation_claim = reservation_claim("bind-batch-coordinator");
    authority
        .reserve_batch_for_coordinator(vec![first.clone(), second.clone()], &reservation_claim)
        .expect("batch should reserve");
    let winner = vec![
        (first.clone(), bind_claim("winner-first")),
        (second.clone(), bind_claim("winner-second")),
    ];
    let claimed = authority
        .claim_bind_batch(&winner, Some(&reservation_claim))
        .expect("one complete attempt should claim the batch");
    assert_eq!(claimed[0].bind_claim(), Some(&winner[0].1));
    assert_eq!(claimed[1].bind_claim(), Some(&winner[1].1));

    let contender = vec![
        (first.clone(), bind_claim("contender-first")),
        (second.clone(), bind_claim("contender-second")),
    ];
    assert!(matches!(
        authority.claim_bind_batch(&contender, Some(&reservation_claim)),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    assert!(matches!(
        authority.release_reserved_batch_without_effect(
            &[first.clone(), second.clone()],
            &reservation_claim
        ),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));

    drop(authority);
    let restarted = LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
    assert_eq!(
        restarted
            .inspect(first.lease_id())
            .expect("claim should inspect")
            .expect("claim should remain durable")
            .bind_claim(),
        Some(&winner[0].1)
    );
    restarted
        .abandon_bind_claims_without_effect(&winner, Some(&reservation_claim))
        .expect("the exact claimant may prove no effect and abandon");
    let released = restarted
        .release_reserved_batch_without_effect(&[first, second], &reservation_claim)
        .expect("unclaimed reservations may then release");
    assert!(
        released
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
}

#[test]
fn claimed_adoption_and_failure_reject_unclaimed_or_foreign_attempts() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let adopted_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let failed_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let coordinator_claim = reservation_claim("launch-coordinator");
    let foreign_reservation_claim = reservation_claim("foreign-launch-coordinator");
    authority
        .reserve_batch_for_coordinator(
            vec![adopted_request.clone(), failed_request.clone()],
            &coordinator_claim,
        )
        .expect("batch should reserve");
    let adopted_claim = bind_claim("adopted-attempt");
    let failed_claim = bind_claim("failed-attempt");
    authority
        .claim_bind(
            &adopted_request,
            Some(&coordinator_claim),
            adopted_claim.clone(),
        )
        .expect("adoption attempt should claim");
    authority
        .claim_bind(
            &failed_request,
            Some(&coordinator_claim),
            failed_claim.clone(),
        )
        .expect("failure attempt should claim");

    let adopted_binding = binding(PORT, "adopted-provider");
    assert!(matches!(
        authority.adopt_claimed(
            &adopted_request,
            None,
            &adopted_claim,
            adopted_binding.clone()
        ),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.adopt_claimed(
            &adopted_request,
            Some(&foreign_reservation_claim),
            &adopted_claim,
            adopted_binding.clone()
        ),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.adopt_claimed(
            &adopted_request,
            Some(&coordinator_claim),
            &bind_claim("foreign-attempt"),
            adopted_binding.clone()
        ),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    let adopted = authority
        .adopt_claimed(
            &adopted_request,
            Some(&coordinator_claim),
            &adopted_claim,
            adopted_binding.clone(),
        )
        .expect("exact claimant should adopt");
    assert_eq!(adopted.bind_claim(), Some(&adopted_claim));
    assert_eq!(
        authority
            .adopt_claimed(
                &adopted_request,
                Some(&coordinator_claim),
                &adopted_claim,
                adopted_binding
            )
            .expect("exact adoption replay should be idempotent"),
        adopted
    );
    let active = authority
        .activate_claimed(&adopted_request, &adopted_claim)
        .expect("adopted binding should activate");
    assert_eq!(active.bind_claim(), None);

    let failure = bind_failure(PORT + 1, "failed-attempt");
    assert!(matches!(
        authority.record_claimed_bind_failure_without_effect(
            &failed_request,
            None,
            &failed_claim,
            failure.clone()
        ),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.record_claimed_bind_failure_without_effect(
            &failed_request,
            Some(&foreign_reservation_claim),
            &failed_claim,
            failure.clone()
        ),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.record_claimed_bind_failure_without_effect(
            &failed_request,
            Some(&coordinator_claim),
            &bind_claim("foreign-attempt"),
            failure.clone()
        ),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
    let failed = authority
        .record_claimed_bind_failure_without_effect(
            &failed_request,
            Some(&coordinator_claim),
            &failed_claim,
            failure.clone(),
        )
        .expect("exact claimant should record confirmed no-effect failure");
    assert_eq!(failed.phase(), PortLeasePhase::Failed);
    assert_eq!(failed.bind_claim(), None);
    assert_eq!(failed.failure(), Some(&failure));
}

#[test]
fn claimed_adoption_rejects_foreign_provider_registration_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let reservation_claim = reservation_claim("provider-fence-coordinator");
    let bind_claim = bind_claim("provider-a-attempt");
    authority
        .reserve_batch_for_coordinator(vec![request.clone()], &reservation_claim)
        .expect("request should reserve");
    let reserved = authority
        .claim_bind(&request, Some(&reservation_claim), bind_claim.clone())
        .expect("provider A should claim the bind attempt");

    let error = authority
        .adopt_claimed(
            &request,
            Some(&reservation_claim),
            &bind_claim,
            binding_for_provider(
                PORT,
                PortBindingProvenance::NimbusOwned,
                alternate_provider_id(),
                "provider-b-resource",
            ),
        )
        .expect_err("provider B cleanup evidence must not satisfy provider A's claim");
    assert!(matches!(error, PortLeaseError::BindClaimConflict { .. }));
    assert_eq!(
        authority
            .inspect(request.lease_id())
            .expect("rejected adoption should inspect"),
        Some(reserved),
        "foreign provider evidence must not mutate the claimed reservation"
    );

    let adopted = authority
        .adopt_claimed(
            &request,
            Some(&reservation_claim),
            &bind_claim,
            binding(PORT, "provider-a-resource"),
        )
        .expect("the claiming provider may use a distinct resource handle");
    assert_eq!(adopted.phase(), PortLeasePhase::Binding);
}

#[test]
fn provider_assigned_batch_rejects_internal_port_conflict_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = provider_assigned_request("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let second = provider_assigned_request("01ARZ3NDEKTSV4RRFFQ69G5FAW");
    let reservation_claim = reservation_claim("provider-batch-coordinator");
    authority
        .reserve_batch_for_coordinator(vec![first.clone(), second.clone()], &reservation_claim)
        .expect("provider-assigned batch should reserve");
    let first_claim = bind_claim("provider-batch-first");
    let second_claim = bind_claim("provider-batch-second");
    authority
        .claim_bind_batch(
            &[
                (first.clone(), first_claim.clone()),
                (second.clone(), second_claim.clone()),
            ],
            Some(&reservation_claim),
        )
        .expect("complete batch should claim");
    let before = authority.list().expect("claimed batch should list");

    let error = authority
        .adopt_claimed_and_activate_batch(
            &[
                (
                    first.clone(),
                    first_claim,
                    provider_assigned_binding(PORT, "provider-batch-first-resource"),
                ),
                (
                    second.clone(),
                    second_claim,
                    provider_assigned_binding(PORT, "provider-batch-second-resource"),
                ),
            ],
            Some(&reservation_claim),
        )
        .expect_err("overlapping provider-assigned siblings must conflict before mutation");
    assert!(matches!(error, PortLeaseError::PortConflict { .. }));
    assert_eq!(
        authority.list().expect("rejected batch should list"),
        before,
        "an internal provider-assigned collision must leave the complete batch unchanged"
    );
}

#[test]
fn provider_assigned_batch_authenticates_every_provider_then_activates_atomically() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = provider_assigned_request("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let second = provider_assigned_request("01ARZ3NDEKTSV4RRFFQ69G5FAW");
    let reservation_claim = reservation_claim("provider-batch-auth-coordinator");
    authority
        .reserve_batch_for_coordinator(vec![first.clone(), second.clone()], &reservation_claim)
        .expect("provider-assigned batch should reserve");
    let first_claim = bind_claim("provider-batch-auth-first");
    let second_claim = bind_claim("provider-batch-auth-second");
    authority
        .claim_bind_batch(
            &[
                (first.clone(), first_claim.clone()),
                (second.clone(), second_claim.clone()),
            ],
            Some(&reservation_claim),
        )
        .expect("complete batch should claim");
    let before = authority.list().expect("claimed batch should list");

    let foreign_error = authority
        .adopt_claimed_and_activate_batch(
            &[
                (
                    first.clone(),
                    first_claim.clone(),
                    provider_assigned_binding(PORT, "provider-batch-auth-first-resource"),
                ),
                (
                    second.clone(),
                    second_claim.clone(),
                    binding_for_provider(
                        PORT + 1,
                        PortBindingProvenance::ProviderAssigned,
                        alternate_provider_id(),
                        "foreign-provider-resource",
                    ),
                ),
            ],
            Some(&reservation_claim),
        )
        .expect_err("one foreign provider must reject the complete batch");
    assert!(matches!(
        foreign_error,
        PortLeaseError::BindClaimConflict { .. }
    ));
    assert_eq!(
        authority.list().expect("rejected batch should list"),
        before,
        "provider authentication must precede every batch mutation"
    );

    let activated = authority
        .adopt_claimed_and_activate_batch(
            &[
                (
                    first,
                    first_claim,
                    provider_assigned_binding(PORT, "provider-batch-auth-first-resource"),
                ),
                (
                    second,
                    second_claim,
                    provider_assigned_binding(PORT + 1, "provider-batch-auth-second-resource"),
                ),
            ],
            Some(&reservation_claim),
        )
        .expect("same-provider disjoint bindings should activate atomically");
    assert!(
        activated.iter().all(|record| {
            record.phase() == PortLeasePhase::Active
                && record.reservation_claim().is_none()
                && record.bind_claim().is_none()
        }),
        "activation must clear every launch and bind claim: {activated:?}"
    );
    assert_eq!(
        activated
            .iter()
            .map(|record| record.reserved_port().expect("active port").get())
            .collect::<Vec<_>>(),
        [PORT, PORT + 1]
    );
}

#[test]
fn reservation_batch_is_all_or_nothing_and_replays_in_order() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let conflicting = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT);

    let error = authority
        .reserve_batch(vec![first.clone(), conflicting.clone()])
        .expect_err("one conflict must abort the complete batch");
    assert!(matches!(error, PortLeaseError::PortConflict { .. }));
    assert!(
        authority
            .inspect(first.lease_id())
            .expect("authority should remain readable")
            .is_none()
            && authority
                .inspect(conflicting.lease_id())
                .expect("authority should remain readable")
                .is_none(),
        "a failed batch must not leak an earlier reservation"
    );

    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAX", 1, 1, PORT + 1);
    let committed = authority
        .reserve_batch(vec![first.clone(), second.clone()])
        .expect("disjoint batch should commit");
    assert_eq!(
        committed
            .iter()
            .map(PortLeaseRecord::request)
            .collect::<Vec<_>>(),
        vec![&first, &second],
        "batch results must preserve caller order"
    );
    assert_eq!(
        authority
            .reserve_batch(vec![first, second])
            .expect("identical replay should be idempotent"),
        committed
    );
}

#[test]
fn reserved_batch_compensation_is_atomic_idempotent_and_reusable() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let requests = vec![first.clone(), second.clone()];
    let reservation_claim = reservation_claim("compensation-coordinator");
    authority
        .reserve_batch_for_coordinator(requests.clone(), &reservation_claim)
        .expect("batch should reserve");

    let released = authority
        .release_reserved_batch_without_effect(&requests, &reservation_claim)
        .expect("never-bound batch should compensate");
    assert!(
        released
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
    assert_eq!(
        authority
            .release_reserved_batch_without_effect(&requests, &reservation_claim)
            .expect("compensation replay should be idempotent"),
        released
    );

    let replacement = request("01ARZ3NDEKTSV4RRFFQ69G5FAX", 1, 1, PORT);
    assert_eq!(
        authority
            .reserve(replacement)
            .expect("released capacity should be reusable")
            .reserved_port()
            .map(NonZeroU16::get),
        Some(PORT)
    );
}

#[test]
fn concurrent_replay_cannot_release_another_coordinator_reservation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let winner = reservation_claim("winning-coordinator");
    let losing_replay = reservation_claim("losing-coordinator");

    authority
        .reserve_batch_for_coordinator(vec![request.clone()], &winner)
        .expect("first coordinator should reserve");
    assert!(matches!(
        authority.reserve_batch_for_coordinator(vec![request.clone()], &losing_replay),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority
            .release_reserved_batch_without_effect(std::slice::from_ref(&request), &losing_replay),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.claim_bind(&request, None, bind_claim("unclaimed-replay")),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.claim_bind(
            &request,
            Some(&losing_replay),
            bind_claim("foreign-coordinator")
        ),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));

    authority
        .claim_bind(&request, Some(&winner), bind_claim("winning-coordinator"))
        .expect(
            "one coordinator's failed replay must not release another coordinator's reservation",
        );
}

#[test]
fn reservation_replay_requires_the_exact_optional_coordinator_claim() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claimed = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let unclaimed = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let coordinator = reservation_claim("exact-optional-claim");

    authority
        .reserve_for_coordinator(claimed.clone(), &coordinator)
        .expect("coordinator should reserve");
    assert!(matches!(
        authority.reserve(claimed.clone()),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.reserve_batch(vec![claimed.clone()]),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    authority
        .reserve_for_coordinator(claimed, &coordinator)
        .expect("the exact coordinator replay should remain idempotent");

    authority
        .reserve(unclaimed.clone())
        .expect("generic caller should reserve");
    assert!(matches!(
        authority.reserve_for_coordinator(unclaimed.clone(), &coordinator),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        authority.reserve_batch_for_coordinator(vec![unclaimed], &coordinator),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
}

#[test]
fn generic_withdraw_cannot_bypass_reservation_compensation_claim() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let coordinator = reservation_claim("exclusive-compensation-coordinator");
    authority
        .reserve_for_coordinator(request.clone(), &coordinator)
        .expect("coordinator should reserve");

    assert!(matches!(
        authority.withdraw(&request),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    let retained = authority
        .inspect(request.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(retained.phase(), PortLeasePhase::Reserved);
    assert_eq!(retained.reservation_claim(), Some(&coordinator));

    authority
        .release_reserved_batch_without_effect(&[request], &coordinator)
        .expect("only the exact coordinator may compensate the never-bound reservation");
}

#[test]
fn reservation_compensation_claim_survives_reopen_and_replays_exactly() {
    let root = tempfile::tempdir().expect("state root should exist");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let requests = vec![first, second];
    let winner = reservation_claim("crash-reopen-winner");
    let loser = reservation_claim("crash-reopen-loser");
    let reserved = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should open")
        .reserve_batch_for_coordinator(requests.clone(), &winner)
        .expect("winner should reserve atomically");
    drop(reserved);

    let reopened = LocalPortLeaseAuthority::open(root.path()).expect("authority should reopen");
    for request in &requests {
        assert_eq!(
            reopened
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("lease should survive reopen")
                .reservation_claim(),
            Some(&winner),
            "the exact compensation capability must survive a coordinator crash"
        );
    }
    assert!(matches!(
        reopened.reserve_batch_for_coordinator(requests.clone(), &loser),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(matches!(
        reopened.release_reserved_batch_without_effect(&requests, &loser),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    let released = reopened
        .release_reserved_batch_without_effect(&requests, &winner)
        .expect("the exact durable coordinator may compensate after reopen");
    assert!(
        released
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
    drop(reopened);

    let replayed = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen again")
        .release_reserved_batch_without_effect(&requests, &winner)
        .expect("exact compensation replay should be idempotent after another reopen");
    assert_eq!(replayed, released);
}

#[test]
fn foreign_claim_conflict_rolls_back_new_batch_siblings() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let existing = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let new_sibling = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let winner = reservation_claim("atomic-batch-winner");
    let loser = reservation_claim("atomic-batch-loser");
    authority
        .reserve_for_coordinator(existing.clone(), &winner)
        .expect("winner should reserve");

    assert!(matches!(
        authority
            .reserve_batch_for_coordinator(vec![new_sibling.clone(), existing.clone()], &loser),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));
    assert!(
        authority
            .inspect(new_sibling.lease_id())
            .expect("new sibling should inspect")
            .is_none(),
        "a later foreign-claim conflict must roll back every earlier batch insertion"
    );
    assert_eq!(
        authority
            .inspect(existing.lease_id())
            .expect("existing lease should inspect")
            .expect("winner should remain")
            .reservation_claim(),
        Some(&winner)
    );
}

#[test]
fn reserved_batch_compensation_rejects_adopted_member_without_partial_release() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let requests = vec![first.clone(), second.clone()];
    let reservation_claim = reservation_claim("adopted-member-coordinator");
    authority
        .reserve_batch_for_coordinator(requests.clone(), &reservation_claim)
        .expect("batch should reserve");
    claim_and_adopt(
        &authority,
        &second,
        Some(&reservation_claim),
        binding(PORT + 1, "provider-binding"),
    )
    .expect("second member should adopt");

    let error = authority
        .release_reserved_batch_without_effect(&requests, &reservation_claim)
        .expect_err("an adopted member must veto complete compensation");
    assert!(matches!(
        error,
        PortLeaseError::ReservationClaimConflict { .. }
    ));
    assert_eq!(
        authority
            .inspect(first.lease_id())
            .expect("first should inspect")
            .expect("first should remain")
            .phase(),
        PortLeasePhase::Reserved,
        "prevalidation must prevent partial release"
    );
}

#[test]
fn failed_no_effect_member_allows_reserved_siblings_to_compensate() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT + 1);
    let requests = vec![first.clone(), second.clone()];
    let reservation_claim = reservation_claim("failed-member-coordinator");
    authority
        .reserve_batch_for_coordinator(requests.clone(), &reservation_claim)
        .expect("batch should reserve");
    let failure = bind_failure(PORT + 1, "confirmed-no-effect");
    claim_and_record_bind_failure(
        &authority,
        &second,
        Some(&reservation_claim),
        failure.clone(),
    )
    .expect("confirmed no-effect failure should commit");

    let compensated = authority
        .release_reserved_batch_without_effect(&requests, &reservation_claim)
        .expect("terminal no-effect member must not strand reserved siblings");
    assert_eq!(compensated[0].phase(), PortLeasePhase::Released);
    assert_eq!(compensated[1].phase(), PortLeasePhase::Failed);
    assert_eq!(compensated[1].failure(), Some(&failure));

    for (lease_id, port) in [
        ("01ARZ3NDEKTSV4RRFFQ69G5FAX", PORT),
        ("01ARZ3NDEKTSV4RRFFQ69G5FAY", PORT + 1),
    ] {
        authority
            .reserve(request(lease_id, 1, 1, port))
            .expect("both terminal slots should be reusable");
    }
}

#[test]
fn divergent_identity_and_stale_fence_fail_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let original = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 7, 11, PORT);
    let divergent_owner = request_with_owner(
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        7,
        11,
        PORT,
    );
    let stale_epoch = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 7, 10, PORT);

    let reserved = authority
        .reserve(original.clone())
        .expect("original reservation should commit");
    assert!(matches!(
        authority.reserve(divergent_owner.clone()),
        Err(PortLeaseError::IdentityConflict { .. })
    ));
    assert!(matches!(
        authority.withdraw(&divergent_owner),
        Err(PortLeaseError::StaleFence(mismatch))
            if mismatch.expected().owner_id() != mismatch.candidate().owner_id()
    ));
    assert!(matches!(
        authority.withdraw(&stale_epoch),
        Err(PortLeaseError::StaleFence(mismatch))
            if mismatch.expected().lease_epoch() == NetworkLeaseEpoch::new(11)
                && mismatch.candidate().lease_epoch() == NetworkLeaseEpoch::new(10)
    ));
    assert_eq!(
        authority
            .inspect(original.lease_id())
            .expect("original should inspect"),
        Some(reserved)
    );
}

#[test]
fn non_terminal_records_conflict_and_release_permits_new_identity() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let first = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let second = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT);

    authority
        .reserve(first.clone())
        .expect("first reservation should commit");
    let conflict = authority
        .reserve(second.clone())
        .expect_err("second reservation must conflict");
    assert!(matches!(
        conflict,
        PortLeaseError::PortConflict {
            conflicting_port,
            existing_phase: PortLeasePhase::Reserved,
            ..
        } if conflicting_port.get() == PORT
    ));

    authority
        .withdraw(&first)
        .expect("reserved lease should withdraw");
    assert!(
        matches!(
            authority.reserve(second.clone()),
            Err(PortLeaseError::PortConflict {
                existing_phase: PortLeasePhase::Withdrawing,
                ..
            })
        ),
        "withdrawal must retain the fence until terminal release"
    );
    authority
        .release(&first)
        .expect("withdrawn lease should release");
    let replacement = authority
        .reserve(second)
        .expect("new stable identity may reserve after release");
    assert_eq!(replacement.phase(), PortLeasePhase::Reserved);
    assert_eq!(authority.list().expect("leases should list").len(), 2);
}

#[test]
fn separately_opened_thread_contenders_choose_exactly_one_winner() {
    let root = Arc::new(tempfile::tempdir().expect("state root should exist"));
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();

    for (payload, owner_payload) in [
        ("01ARZ3NDEKTSV4RRFFQ69G5FAV", "01ARZ3NDEKTSV4RRFFQ69G5FAV"),
        ("01ARZ3NDEKTSV4RRFFQ69G5FAW", "01ARZ3NDEKTSV4RRFFQ69G5FAW"),
    ] {
        let root = Arc::clone(&root);
        let barrier = Arc::clone(&barrier);
        threads.push(thread::spawn(move || {
            let authority =
                LocalPortLeaseAuthority::open(root.path()).expect("thread authority should open");
            let request = request_with_owner(payload, owner_payload, 3, 5, PORT);
            barrier.wait();
            let reserved = authority.reserve(request.clone())?;
            assert_eq!(reserved.phase(), PortLeasePhase::Reserved);
            let binding = binding(PORT, payload);
            let claim = PortBindClaim::new(binding.provider_handle().clone());
            claim_and_adopt(&authority, &request, None, binding)?;
            authority.activate_claimed(&request, &claim)
        }));
    }

    barrier.wait();
    let outcomes = threads
        .into_iter()
        .map(|thread| thread.join().expect("contender should not panic"))
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(PortLeaseError::PortConflict { .. })))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter_map(|result| result.as_ref().ok())
            .filter(|record| record.phase() == PortLeasePhase::Active)
            .count(),
        1
    );
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should reopen");
    let records = authority.list().expect("leases should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Active);
}

#[test]
fn exact_adoption_rejects_a_different_actual_port_and_provider_rewrite() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    authority
        .reserve(request.clone())
        .expect("reservation should commit");
    let claim = bind_claim("provider-binding-a");
    authority
        .claim_bind(&request, None, claim.clone())
        .expect("provider attempt should claim the reservation");

    assert!(matches!(
        authority.adopt_claimed(&request, None, &claim, binding(PORT + 1, "wrong-port")),
        Err(PortLeaseError::BindingMismatch {
            mismatch: PortBindingMismatch::Port,
            ..
        })
    ));
    authority
        .adopt_claimed(&request, None, &claim, binding(PORT, "provider-binding-a"))
        .expect("matching port should adopt");
    assert!(matches!(
        authority.adopt_claimed(&request, None, &claim, binding(PORT, "provider-binding-b")),
        Err(PortLeaseError::BindingConflict { .. })
    ));
}

#[test]
fn bind_failure_is_idempotent_durable_and_cannot_activate() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
    let failure = bind_failure(PORT, "bind-attempt-a");
    authority
        .reserve(request.clone())
        .expect("reservation should commit");
    let claim = bind_claim("bind-attempt-a");
    authority
        .claim_bind(&request, None, claim.clone())
        .expect("failed provider attempt should claim the reservation");

    let failed = authority
        .record_claimed_bind_failure_without_effect(&request, None, &claim, failure.clone())
        .expect("bind failure should commit");
    assert_eq!(failed.phase(), PortLeasePhase::Failed);
    assert_eq!(failed.failure(), Some(&failure));
    assert_eq!(failed.binding(), None);
    assert_eq!(
        authority
            .record_claimed_bind_failure_without_effect(&request, None, &claim, failure.clone(),)
            .expect("same failed-bind evidence should be idempotent"),
        failed
    );
    assert!(matches!(
        authority.record_claimed_bind_failure_without_effect(
            &request,
            None,
            &claim,
            bind_failure(PORT, "different-bind-attempt")
        ),
        Err(PortLeaseError::BindFailureConflict { .. })
    ));
    assert!(matches!(
        authority.activate_claimed(&request, &claim),
        Err(PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Failed,
            operation: PortLeaseOperation::Activate,
            ..
        })
    ));
    assert!(matches!(
        authority.adopt_claimed(&request, None, &claim, binding(PORT, "late-binding")),
        Err(PortLeaseError::InvalidTransition {
            phase: PortLeasePhase::Failed,
            operation: PortLeaseOperation::Adopt,
            ..
        })
    ));

    drop(authority);
    let restarted = LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
    let durable = restarted
        .inspect(request.lease_id())
        .expect("failed lease should inspect")
        .expect("failed lease should remain durable");
    assert_eq!(durable.phase(), PortLeasePhase::Failed);
    assert_eq!(durable.failure(), Some(&failure));
}

#[test]
fn checksum_valid_semantically_corrupt_authority_fails_closed() {
    for (corruption, expected_reason) in [
        (CorruptionFixture::MapKeyMismatch, "does not match"),
        (CorruptionFixture::ActiveWithoutBinding, "has no binding"),
        (
            CorruptionFixture::BindingWithoutBindClaim,
            "no durable provider bind claim",
        ),
        (CorruptionFixture::BindingPortMismatch, "reserves Some"),
        (
            CorruptionFixture::ExactWithoutReservedPort,
            "incompatible with request",
        ),
        (
            CorruptionFixture::ExactWrongReservedPort,
            "incompatible with request",
        ),
        (CorruptionFixture::FailedWithBinding, "terminal failed"),
        (
            CorruptionFixture::FailedWithoutFailure,
            "has no failure evidence",
        ),
        (
            CorruptionFixture::ReservedWithFailure,
            "has bind failure evidence",
        ),
        (
            CorruptionFixture::FailureEndpointMismatch,
            "bind failure incompatible",
        ),
        (
            CorruptionFixture::RangeFailureSelectedPortMismatch,
            "does not match its selected port",
        ),
        (
            CorruptionFixture::ProviderAssignedFailureWithReservedPort,
            "does not match its selected port",
        ),
        (
            CorruptionFixture::TenantPublishedWithoutTenant,
            "requires tenant attribution",
        ),
        (
            CorruptionFixture::HostInternalWithPublication,
            "publication intent does not match",
        ),
        (
            CorruptionFixture::ReleasedWithBindingAndReservationClaim,
            "alongside provider binding evidence",
        ),
        (
            CorruptionFixture::BindingClaimProviderMismatch,
            "different registration",
        ),
        (
            CorruptionFixture::ActiveWithConfirmedStoppedBinding,
            "retains confirmed stopped-binding evidence",
        ),
        (
            CorruptionFixture::ConfirmedStoppedBindingPortMismatch,
            "records confirmed stopped binding",
        ),
        (
            CorruptionFixture::ConfirmedStoppedBindingWithReservationClaim,
            "incompatible lifecycle authority",
        ),
        (CorruptionFixture::DuplicateLivePort, "both fence"),
    ] {
        let root = tempfile::tempdir().expect("state root should exist");
        write_corrupt_state(root.path(), corruption);

        let error = LocalPortLeaseAuthority::open(root.path())
            .expect_err("semantic corruption must fail closed during authority startup");
        assert!(
            matches!(
                &error,
                PortLeaseError::CorruptAuthority { reason }
                    if reason.contains(expected_reason)
            ),
            "{corruption:?} produced unexpected error: {error}"
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum CorruptionFixture {
    MapKeyMismatch,
    ActiveWithoutBinding,
    BindingWithoutBindClaim,
    BindingPortMismatch,
    ExactWithoutReservedPort,
    ExactWrongReservedPort,
    FailedWithBinding,
    FailedWithoutFailure,
    ReservedWithFailure,
    FailureEndpointMismatch,
    RangeFailureSelectedPortMismatch,
    ProviderAssignedFailureWithReservedPort,
    TenantPublishedWithoutTenant,
    HostInternalWithPublication,
    ReleasedWithBindingAndReservationClaim,
    BindingClaimProviderMismatch,
    ActiveWithConfirmedStoppedBinding,
    ConfirmedStoppedBindingPortMismatch,
    ConfirmedStoppedBindingWithReservationClaim,
    DuplicateLivePort,
}

fn write_corrupt_state(state_root: &Path, corruption: CorruptionFixture) {
    let store = LocalNetworkStateStore::open(state_root).expect("raw store should open");
    store
        .transaction(
            &NetworkStatePartition::PortLeases,
            |state: &mut PortLeaseState| -> Result<(), Infallible> {
                let first_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", 1, 1, PORT);
                let mut first = PortLeaseRecord {
                    request: first_request.clone(),
                    reserved_port: Some(
                        NonZeroU16::new(PORT).expect("fixture port should be non-zero"),
                    ),
                    phase: PortLeasePhase::Reserved,
                    reservation_claim: None,
                    bind_claim: None,
                    adoption_claim: None,
                    binding: None,
                    confirmed_stopped_binding: None,
                    failure: None,
                };

                match corruption {
                    CorruptionFixture::MapKeyMismatch => {
                        let wrong_key = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT)
                            .lease_id()
                            .clone();
                        state.leases.insert(wrong_key, first);
                    }
                    CorruptionFixture::ActiveWithoutBinding => {
                        first.phase = PortLeasePhase::Active;
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::BindingWithoutBindClaim => {
                        first.phase = PortLeasePhase::Binding;
                        first.adoption_claim = Some(bind_claim("claimless-binding"));
                        first.binding = Some(binding(PORT, "claimless-binding"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::BindingPortMismatch => {
                        first.phase = PortLeasePhase::Binding;
                        first.bind_claim = Some(bind_claim("wrong-port"));
                        first.adoption_claim = Some(bind_claim("wrong-port"));
                        first.binding = Some(binding(PORT + 1, "wrong-port"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ExactWithoutReservedPort => {
                        first.reserved_port = None;
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ExactWrongReservedPort => {
                        first.reserved_port = NonZeroU16::new(PORT + 1);
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::FailedWithBinding => {
                        first.phase = PortLeasePhase::Failed;
                        first.adoption_claim = Some(bind_claim("unexpected-provider-effect"));
                        first.binding = Some(binding(PORT, "unexpected-provider-effect"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::FailedWithoutFailure => {
                        first.phase = PortLeasePhase::Failed;
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ReservedWithFailure => {
                        first.failure = Some(bind_failure(PORT, "unexpected-failure"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::FailureEndpointMismatch => {
                        first.phase = PortLeasePhase::Failed;
                        first.failure = Some(bind_failure(PORT + 1, "wrong-endpoint"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::RangeFailureSelectedPortMismatch => {
                        first.request.binding = PortBindingSpec::new(
                            PortProtocol::Tcp,
                            PortBindRealm::Host,
                            PortBindTarget::ipv4_wildcard(),
                            PortExposure::Unknown,
                            PortRequestMode::Range(
                                PortRange::new(
                                    NonZeroU16::new(PORT).expect("fixture port should be non-zero"),
                                    NonZeroU16::new(PORT + 1)
                                        .expect("fixture port should be non-zero"),
                                )
                                .expect("fixture range should validate"),
                            ),
                        );
                        first.phase = PortLeasePhase::Failed;
                        first.failure = Some(bind_failure(PORT + 1, "different-selected-port"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ProviderAssignedFailureWithReservedPort => {
                        first.request.binding = PortBindingSpec::new(
                            PortProtocol::Tcp,
                            PortBindRealm::Host,
                            PortBindTarget::ipv4_wildcard(),
                            PortExposure::Unknown,
                            PortRequestMode::ProviderAssigned,
                        );
                        first.phase = PortLeasePhase::Failed;
                        first.failure =
                            Some(provider_assigned_bind_failure("provider-assigned-attempt"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::TenantPublishedWithoutTenant => {
                        first.request.tenant_id = None;
                        first.request.accounting = PortLeaseAccounting::TenantPublished;
                        first.request.publication =
                            PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into());
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::HostInternalWithPublication => {
                        first.request.publication =
                            PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into());
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ReleasedWithBindingAndReservationClaim => {
                        first.phase = PortLeasePhase::Released;
                        first.reservation_claim =
                            Some(reservation_claim("impossible-released-provider-effect"));
                        first.adoption_claim = Some(bind_claim("unexpected-provider-effect"));
                        first.binding = Some(binding(PORT, "unexpected-provider-effect"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::BindingClaimProviderMismatch => {
                        first.phase = PortLeasePhase::Binding;
                        let foreign_claim = PortBindClaim::new(
                            NetworkProviderHandle::new(
                                alternate_provider_id(),
                                "foreign-provider-attempt",
                            )
                            .expect("foreign bind claim should validate"),
                        );
                        first.bind_claim = Some(foreign_claim.clone());
                        first.adoption_claim = Some(foreign_claim);
                        first.binding = Some(binding(PORT, "local-provider-effect"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ActiveWithConfirmedStoppedBinding => {
                        first.phase = PortLeasePhase::Active;
                        first.adoption_claim = Some(bind_claim("active-provider-effect"));
                        first.binding = Some(binding(PORT, "active-provider-effect"));
                        first.confirmed_stopped_binding =
                            Some(binding(PORT, "impossible-stopped-provider"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ConfirmedStoppedBindingPortMismatch => {
                        first.confirmed_stopped_binding =
                            Some(binding(PORT + 1, "wrong-stopped-port"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::ConfirmedStoppedBindingWithReservationClaim => {
                        first.reservation_claim =
                            Some(reservation_claim("impossible-stopped-reservation"));
                        first.confirmed_stopped_binding =
                            Some(binding(PORT, "confirmed-stopped-provider"));
                        state.leases.insert(first_request.lease_id().clone(), first);
                    }
                    CorruptionFixture::DuplicateLivePort => {
                        let second_request = request("01ARZ3NDEKTSV4RRFFQ69G5FAW", 1, 1, PORT);
                        let second = PortLeaseRecord {
                            request: second_request.clone(),
                            reserved_port: Some(
                                NonZeroU16::new(PORT).expect("fixture port should be non-zero"),
                            ),
                            phase: PortLeasePhase::Reserved,
                            reservation_claim: None,
                            bind_claim: None,
                            adoption_claim: None,
                            binding: None,
                            confirmed_stopped_binding: None,
                            failure: None,
                        };
                        state.leases.insert(first_request.lease_id().clone(), first);
                        state
                            .leases
                            .insert(second_request.lease_id().clone(), second);
                    }
                }
                Ok(())
            },
        )
        .expect("checksum-valid corrupt state should be written");
}

fn request(payload: &str, generation: u64, epoch: u64, port: u16) -> PortLeaseRequest {
    request_with_owner(payload, payload, generation, epoch, port)
}

fn request_with_owner(
    lease_payload: &str,
    owner_payload: &str,
    generation: u64,
    epoch: u64,
    port: u16,
) -> PortLeaseRequest {
    request_with_accounting(
        lease_payload,
        owner_payload,
        Some(TenantId::new("tenant-a").expect("fixture tenant should parse")),
        generation,
        epoch,
        port,
        PortLeaseAccounting::HostInternal,
    )
}

fn published_request(payload: &str, port: u16) -> PortLeaseRequest {
    request_with_accounting(
        payload,
        payload,
        Some(TenantId::new("tenant-a").expect("fixture tenant should parse")),
        7,
        11,
        port,
        PortLeaseAccounting::TenantPublished,
    )
}

fn provider_assigned_request(payload: &str) -> PortLeaseRequest {
    let lease_id = format!("netportlease_{payload}")
        .parse()
        .expect("fixture lease id should parse");
    let owner_id: ListenerId = format!("netlistener_{payload}")
        .parse()
        .expect("fixture listener id should parse");
    PortLeaseRequest::new(
        lease_id,
        owner_id.into(),
        Some(TenantId::new("tenant-a").expect("fixture tenant should parse")),
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::ProviderAssigned,
        ),
    )
}

fn request_with_accounting(
    lease_payload: &str,
    owner_payload: &str,
    tenant_id: Option<TenantId>,
    generation: u64,
    epoch: u64,
    port: u16,
    accounting: PortLeaseAccounting,
) -> PortLeaseRequest {
    let lease_id = format!("netportlease_{lease_payload}")
        .parse()
        .expect("fixture lease id should parse");
    let owner_id: ListenerId = format!("netlistener_{owner_payload}")
        .parse()
        .expect("fixture listener id should parse");
    PortLeaseRequest::new(
        lease_id,
        owner_id.into(),
        tenant_id,
        PortLeaseFence::new(
            NetworkResourceGeneration::new(generation),
            NetworkLeaseEpoch::new(epoch),
        ),
        accounting,
        match accounting {
            PortLeaseAccounting::HostInternal => PortPublicationIntent::Unpublished,
            PortLeaseAccounting::TenantPublished => {
                PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into())
            }
        },
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::Exact(NonZeroU16::new(port).expect("fixture port is non-zero")),
        ),
    )
}

fn binding(port: u16, opaque: &str) -> PortLeaseBinding {
    binding_for_provider(
        port,
        PortBindingProvenance::NimbusOwned,
        provider_id(),
        opaque,
    )
}

fn provider_assigned_binding(port: u16, opaque: &str) -> PortLeaseBinding {
    binding_for_provider(
        port,
        PortBindingProvenance::ProviderAssigned,
        provider_id(),
        opaque,
    )
}

fn binding_for_provider(
    port: u16,
    provenance: PortBindingProvenance,
    provider_id: NetworkProviderId,
    opaque: &str,
) -> PortLeaseBinding {
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            NonZeroU16::new(port).expect("fixture port is non-zero"),
        )
        .expect("fixture endpoint should validate"),
        provenance,
        NetworkProviderHandle::new(provider_id, opaque)
            .expect("fixture provider handle should validate"),
    )
}

fn bind_claim(opaque: &str) -> PortBindClaim {
    PortBindClaim::new(
        NetworkProviderHandle::new(provider_id(), opaque)
            .expect("fixture bind claim should validate"),
    )
}

fn claim_and_adopt(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    reservation_claim: Option<&NetworkReservationClaim>,
    binding: PortLeaseBinding,
) -> Result<PortLeaseRecord, PortLeaseError> {
    let claim = PortBindClaim::new(binding.provider_handle().clone());
    authority.claim_bind(request, reservation_claim, claim.clone())?;
    authority.adopt_claimed(request, reservation_claim, &claim, binding)
}

fn claim_and_record_bind_failure(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    reservation_claim: Option<&NetworkReservationClaim>,
    failure: PortBindFailure,
) -> Result<PortLeaseRecord, PortLeaseError> {
    let claim = PortBindClaim::new(failure.provider_attempt().clone());
    authority.claim_bind(request, reservation_claim, claim.clone())?;
    authority.record_claimed_bind_failure_without_effect(
        request,
        reservation_claim,
        &claim,
        failure,
    )
}

fn provider_id() -> NetworkProviderId {
    "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse")
}

fn alternate_provider_id() -> NetworkProviderId {
    "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAW"
        .parse()
        .expect("alternate fixture provider id should parse")
}

fn reservation_claim(opaque: &str) -> NetworkReservationClaim {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(provider_id, opaque)
            .expect("fixture reservation claim should validate"),
    )
}

fn bind_failure(port: u16, opaque: &str) -> PortBindFailure {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    PortBindFailure::new(
        PortBindFailureKind::AddrInUse,
        PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            port,
        )
        .expect("fixture attempt should validate"),
        NetworkProviderHandle::new(provider_id, opaque)
            .expect("fixture provider attempt should validate"),
    )
}

fn provider_assigned_bind_failure(opaque: &str) -> PortBindFailure {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    PortBindFailure::new(
        PortBindFailureKind::ResourceExhausted,
        PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            0,
        )
        .expect("fixture attempt should validate"),
        NetworkProviderHandle::new(provider_id, opaque)
            .expect("fixture provider attempt should validate"),
    )
}
