use std::num::NonZeroU16;

use tempfile::TempDir;

use super::*;
use crate::{
    ListenerId, NetworkLeaseEpoch, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration, NetworkResourceId, PortBindAttempt, PortBindFailure,
    PortBindFailureKind, PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec,
    PortBoundEndpoint, PortExposure, PortLeaseAccounting, PortLeaseBinding, PortLeaseFence,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};

const PORT: u16 = 43_081;

#[test]
fn live_owner_recovery_reports_liveness_without_mutation() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);

    let attempt = fixture
        .authority
        .recover_dead_lifetime(&fixture.request)
        .expect("live-owner inspection should succeed");
    let PortLeaseRecoveryAttempt::LiveOwner(observed) = attempt else {
        panic!("the held lifetime lock must report a live owner");
    };
    assert_eq!(observed.phase(), PortLeasePhase::Active);
    assert_eq!(observed.active_lifetime(), Some(fixture.guard.lifetime()));
    assert_eq!(
        fixture
            .authority
            .inspect(fixture.request.lease_id())
            .expect("authority should remain readable")
            .expect("active record should remain"),
        observed,
        "liveness inspection must not mutate durable authority"
    );
}

#[test]
fn lifetime_free_rebind_cannot_clear_a_live_owner() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    let before = fixture
        .authority
        .inspect(fixture.request.lease_id())
        .expect("active request should inspect")
        .expect("active request should remain");
    let expected_binding = before
        .binding()
        .cloned()
        .expect("active fixture should carry an exact binding");

    assert!(matches!(
        fixture
            .authority
            .prepare_rebind_after_confirmed_stop(&fixture.request, &expected_binding),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        fixture
            .authority
            .inspect(fixture.request.lease_id())
            .expect("rejected request should inspect"),
        Some(before),
        "a lifetime-free transition must not clear or rewrite live-owner evidence"
    );
}

#[test]
fn lifetime_free_release_cannot_clear_a_live_owner() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    fixture
        .authority
        .withdraw(&fixture.request)
        .expect("the exact request should fence new use");
    let before = fixture
        .authority
        .inspect(fixture.request.lease_id())
        .expect("withdrawing request should inspect")
        .expect("withdrawing request should remain");

    assert!(matches!(
        fixture.authority.release(&fixture.request),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        fixture
            .authority
            .inspect(fixture.request.lease_id())
            .expect("rejected release should inspect"),
        Some(before),
        "portable identity alone must not clear a live process generation"
    );
}

#[test]
fn live_owner_release_rejects_foreign_lifetime_and_accepts_exact_guard() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    let foreign = active_fixture_with_role("beta", PortLeaseEffectScope::ProcessBound, PORT + 1);
    fixture
        .authority
        .withdraw(&fixture.request)
        .expect("the exact request should fence new use");
    let before = fixture
        .authority
        .inspect(fixture.request.lease_id())
        .expect("withdrawing request should inspect")
        .expect("withdrawing request should remain");

    assert!(matches!(
        fixture
            .authority
            .release_with_lifetime(&fixture.request, &foreign.guard),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        fixture
            .authority
            .inspect(fixture.request.lease_id())
            .expect("foreign release should inspect"),
        Some(before),
        "a foreign guard must not mutate the withdrawing owner"
    );

    let released = fixture
        .authority
        .release_with_lifetime(&fixture.request, &fixture.guard)
        .expect("the exact live owner should release after confirmed close");
    assert_eq!(released.phase(), PortLeasePhase::Released);
    assert!(released.active_lifetime().is_none());
    assert_eq!(
        fixture
            .authority
            .release_with_lifetime(&fixture.request, &fixture.guard)
            .expect("the exact terminal release should replay idempotently"),
        released
    );
}

#[test]
fn direct_listener_reservation_and_lifetime_are_one_recoverable_commit() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request_for("alpha", PORT);
    let claim = bind_claim("alpha");

    let reservation = authority
        .reserve_and_claim_bind_with_lifetime(
            request.clone(),
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("direct listener should reserve and claim one lifetime atomically");
    assert_eq!(reservation.record().phase(), PortLeasePhase::Reserved);
    assert_eq!(reservation.record().bind_claim(), Some(&claim));
    assert_eq!(
        reservation.record().active_lifetime(),
        Some(reservation.lifetime.lifetime())
    );
    assert!(matches!(
        authority
            .recover_dead_lifetime(&request)
            .expect("live direct-listener owner should inspect"),
        PortLeaseRecoveryAttempt::LiveOwner(_)
    ));

    let (_, lifetime) = reservation.into_parts();
    drop(lifetime);
    assert!(matches!(
        authority
            .recover_dead_lifetime(&request)
            .expect("crashed direct-listener owner should be recoverable"),
        PortLeaseRecoveryAttempt::Acquired(_)
    ));
}

#[test]
fn dead_process_bound_owner_quarantines_then_releases_exactly_once() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    let ActiveFixture {
        _root,
        authority,
        request,
        guard,
    } = fixture;
    drop(guard);

    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("dead-owner inspection should succeed")
    else {
        panic!("released lifetime lock must yield recovery authority");
    };
    let cleanup = authority
        .mark_cleanup_pending_after_owner_death(&request, &recovery)
        .expect("dead owner should enter cleanup pending");
    assert_eq!(cleanup.phase(), PortLeasePhase::CleanupPending);
    assert!(cleanup.binding().is_some(), "binding evidence must remain");

    let replacement = request_for("beta", PORT);
    assert!(matches!(
        authority.reserve(replacement.clone()),
        Err(PortLeaseError::PortConflict {
            existing_phase: PortLeasePhase::CleanupPending,
            ..
        })
    ));

    let released = authority
        .release_process_bound_after_owner_death(&request, &recovery)
        .expect("process death proves a process-bound effect absent");
    assert_eq!(released.phase(), PortLeasePhase::Released);
    assert!(released.active_lifetime().is_none());
    assert!(released.binding().is_none());
    assert_eq!(
        authority
            .release_process_bound_after_owner_death(&request, &recovery)
            .expect("exact release replay should be idempotent"),
        released
    );
    assert_eq!(
        authority
            .reserve(replacement)
            .expect("replacement should reuse the released slot")
            .reserved_port()
            .map(NonZeroU16::get),
        Some(PORT)
    );
}

#[test]
fn dead_process_bound_owner_retains_exact_slot_for_fenced_rebind() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    let ActiveFixture {
        _root,
        authority,
        request,
        guard,
    } = fixture;
    let expected_binding = authority
        .inspect(request.lease_id())
        .expect("active request should inspect")
        .expect("active request should remain")
        .binding()
        .cloned()
        .expect("active request should carry exact binding");
    let first_lifetime = guard.lifetime();
    drop(guard);

    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("dead-owner inspection should succeed")
    else {
        panic!("released lifetime lock must yield recovery authority");
    };
    authority
        .mark_cleanup_pending_after_owner_death(&request, &recovery)
        .expect("dead owner should enter cleanup pending");
    let retained = authority
        .prepare_rebind_process_bound_after_owner_death(&request, &recovery)
        .expect("dead process-bound effect should retain its exact slot");
    assert_eq!(retained.phase(), PortLeasePhase::Reserved);
    assert_eq!(
        retained.confirmed_stopped_binding(),
        Some(&expected_binding)
    );
    assert!(retained.binding().is_none());
    assert!(retained.active_lifetime().is_none());
    assert!(matches!(
        authority.reserve(request_for("beta", PORT)),
        Err(PortLeaseError::PortConflict {
            existing_phase: PortLeasePhase::Reserved,
            ..
        })
    ));
    assert_eq!(
        authority
            .prepare_rebind_process_bound_after_owner_death(&request, &recovery)
            .expect("exact retained rebind replay should be idempotent"),
        retained
    );
    drop(recovery);

    let claim = bind_claim("beta");
    let next = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("retained request should begin one new process lifetime");
    assert!(
        next.lifetime().generation() > first_lifetime.generation(),
        "rebind must fence the dead process with a higher lifetime generation"
    );
    let active = authority
        .adopt_claimed_and_activate_with_lifetime(&request, None, &claim, expected_binding, &next)
        .expect("new lifetime should activate the exact retained binding");
    assert_eq!(active.phase(), PortLeasePhase::Active);
}

#[test]
fn process_bound_rebind_crash_reconciles_the_new_lifetime() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    let ActiveFixture {
        _root,
        authority,
        request,
        guard,
    } = fixture;
    let old_binding = authority
        .inspect(request.lease_id())
        .expect("active request should inspect")
        .expect("active request should remain")
        .binding()
        .cloned()
        .expect("active request should carry its binding");
    drop(guard);
    let PortLeaseRecoveryAttempt::Acquired(first_recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("first dead owner should recover")
    else {
        panic!("first owner death must yield recovery");
    };
    authority
        .mark_cleanup_pending_after_owner_death(&request, &first_recovery)
        .expect("first dead owner should quarantine");
    authority
        .prepare_rebind_process_bound_after_owner_death(&request, &first_recovery)
        .expect("the stopped slot should be retained");
    drop(first_recovery);

    let next_claim = bind_claim("beta");
    let next_lifetime = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            next_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("the retained slot should start a new process-bound attempt");
    let next_generation = next_lifetime.lifetime();
    drop(next_lifetime);

    let PortLeaseRecoveryAttempt::Acquired(second_recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("the crashed rebind should recover")
    else {
        panic!("the crashed rebind owner must yield recovery");
    };
    let cleanup = authority
        .mark_cleanup_pending_after_owner_death(&request, &second_recovery)
        .expect("the crashed rebind must enter cleanup pending");
    assert_eq!(cleanup.phase(), PortLeasePhase::CleanupPending);
    assert_eq!(cleanup.bind_claim(), Some(&next_claim));
    assert_eq!(cleanup.active_lifetime(), Some(next_generation));
    assert_eq!(cleanup.confirmed_stopped_binding(), Some(&old_binding));

    authority
        .release_process_bound_after_owner_death(&request, &second_recovery)
        .expect("process death proves the second process-bound attempt absent");
    assert_eq!(
        authority
            .reserve(request_for("gamma", PORT))
            .expect("the released slot should become reusable")
            .reserved_port()
            .map(NonZeroU16::get),
        Some(PORT)
    );
}

#[test]
fn dead_provider_claim_adopts_exact_surviving_binding() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request_for("alpha", PORT);
    let claim = bind_claim("alpha");
    let reservation = authority
        .reserve_and_claim_bind_with_lifetime(
            request.clone(),
            claim.clone(),
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("external provider claim should reserve atomically");
    let first_generation = reservation.lifetime.lifetime();
    let (_, lifetime) = reservation.into_parts();
    drop(lifetime);

    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("dead external coordinator should recover")
    else {
        panic!("dead external coordinator must yield recovery");
    };
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_wildcard(),
        nonzero_port(PORT),
    )
    .expect("external endpoint should validate");
    let before = authority
        .inspect(request.lease_id())
        .expect("claimed request should inspect")
        .expect("claimed request should remain");
    let foreign_binding = PortLeaseBinding::new(
        endpoint.clone(),
        PortBindingProvenance::ExternallyOwned,
        foreign_provider_handle("binding-alpha"),
    );
    assert!(matches!(
        authority.reclaim_provider_managed_binding_after_owner_death(
            &request,
            &foreign_binding,
            recovery
        ),
        Err(PortLeaseError::InvalidTransition {
            operation: PortLeaseOperation::ReclaimProviderManagedBinding,
            ..
        })
    ));
    assert_eq!(
        authority
            .inspect(request.lease_id())
            .expect("foreign provider rejection should inspect"),
        Some(before),
        "a foreign provider registration must not mutate claimed authority"
    );
    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("rejected foreign registration should leave recovery available")
    else {
        panic!("rejected foreign registration must not consume durable recovery");
    };
    let binding = PortLeaseBinding::new(
        endpoint,
        PortBindingProvenance::ExternallyOwned,
        provider_handle("binding-alpha".to_owned()),
    );
    let replacement = authority
        .reclaim_provider_managed_binding_after_owner_death(&request, &binding, recovery)
        .expect("the exact surviving provider binding should adopt");
    let active = authority
        .inspect(request.lease_id())
        .expect("reclaimed request should inspect")
        .expect("reclaimed request should remain");
    assert_eq!(active.phase(), PortLeasePhase::Active);
    assert_eq!(active.binding(), Some(&binding));
    assert_eq!(active.adoption_claim(), Some(&claim));
    assert!(active.bind_claim().is_none());
    assert!(
        replacement.lifetime().generation() > first_generation.generation(),
        "reclaim must fence the dead coordinator generation"
    );
}

#[test]
fn dead_provider_rebind_adopts_new_binding_and_clears_stopped_evidence() {
    let ActiveFixture {
        _root,
        authority,
        request,
        guard,
    } = active_fixture(PortLeaseEffectScope::ProviderManaged);
    let old_binding = authority
        .inspect(request.lease_id())
        .expect("active provider request should inspect")
        .expect("active provider request should remain")
        .binding()
        .cloned()
        .expect("active provider request should carry its exact binding");
    drop(guard);

    let PortLeaseRecoveryAttempt::Acquired(first_recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("the first dead provider owner should recover")
    else {
        panic!("the first dead provider owner must yield recovery");
    };
    authority
        .mark_cleanup_pending_after_owner_death(&request, &first_recovery)
        .expect("the stopped provider binding should quarantine");
    authority
        .prepare_rebind_provider_managed_batch_after_confirmed_stop(
            &[(request.clone(), old_binding.clone())],
            std::slice::from_ref(&first_recovery),
        )
        .expect("exact provider absence should retain the stopped slot");
    drop(first_recovery);

    let next_claim = bind_claim("beta");
    let next_lifetime = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            next_claim.clone(),
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("the retained slot should claim one new provider attempt");
    drop(next_lifetime);

    let PortLeaseRecoveryAttempt::Acquired(second_recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("the crashed replacement provider should recover")
    else {
        panic!("the crashed replacement provider must yield recovery");
    };
    let new_binding = PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            nonzero_port(PORT),
        )
        .expect("replacement endpoint should validate"),
        PortBindingProvenance::NimbusOwned,
        next_claim.provider_attempt().clone(),
    );
    let replacement = authority
        .reclaim_provider_managed_binding_after_owner_death(&request, &new_binding, second_recovery)
        .expect("the exact new provider binding should replace stopped evidence");
    let active = authority
        .inspect(request.lease_id())
        .expect("replacement request should inspect")
        .expect("replacement request should remain");
    assert_eq!(active.phase(), PortLeasePhase::Active);
    assert_eq!(active.binding(), Some(&new_binding));
    assert_eq!(active.adoption_claim(), Some(&next_claim));
    assert!(active.confirmed_stopped_binding().is_none());
    assert_eq!(active.active_lifetime(), Some(replacement.lifetime()));
}

#[test]
fn no_effect_compensation_clears_exact_live_lifetime() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request_for("alpha", PORT);
    authority
        .reserve(request.clone())
        .expect("request should reserve");
    let claim = bind_claim("alpha");
    let guard = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("bind and lifetime should claim atomically");

    let abandoned = authority
        .abandon_bind_with_lifetime_without_effect(&request, None, &claim, &guard)
        .expect("exact no-effect compensation should succeed");
    assert_eq!(abandoned.phase(), PortLeasePhase::Reserved);
    assert!(abandoned.bind_claim().is_none());
    assert!(abandoned.active_lifetime().is_none());
    assert_eq!(
        authority
            .abandon_bind_with_lifetime_without_effect(&request, None, &claim, &guard)
            .expect("exact compensation replay should be idempotent"),
        abandoned
    );
    drop(guard);

    authority
        .claim_bind_with_lifetime(
            &request,
            None,
            bind_claim("beta"),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("cleared no-effect attempt must permit a higher lifetime");
}

#[test]
fn lifetime_free_no_effect_receipts_cannot_settle_a_live_attempt() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = request_for("alpha", PORT);
    let claim = bind_claim("alpha");
    authority
        .reserve(request.clone())
        .expect("request should reserve");
    let lifetime = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("attempt should carry a live lifetime");
    let before = authority
        .inspect(request.lease_id())
        .expect("attempt should inspect")
        .expect("attempt should remain");

    assert!(matches!(
        authority.abandon_bind_claims_without_effect(&[(request.clone(), claim.clone())], None),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert!(matches!(
        authority.record_claimed_bind_failure_without_effect(
            &request,
            None,
            &claim,
            bind_failure("alpha", PORT),
        ),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        authority
            .inspect(request.lease_id())
            .expect("rejected attempt should inspect"),
        Some(before),
        "lifetime-free no-effect evidence must not rewrite a live attempt"
    );
    drop(lifetime);
}

#[test]
fn no_effect_failure_requires_and_clears_the_exact_live_lifetime() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let alpha = request_for("alpha", PORT);
    let beta = request_for("beta", PORT + 1);
    authority
        .reserve(alpha.clone())
        .expect("alpha should reserve");
    authority
        .reserve(beta.clone())
        .expect("beta should reserve");
    let alpha_claim = bind_claim("alpha");
    let beta_claim = bind_claim("beta");
    let alpha_lifetime = authority
        .claim_bind_with_lifetime(
            &alpha,
            None,
            alpha_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("alpha should own one live attempt");
    let beta_lifetime = authority
        .claim_bind_with_lifetime(&beta, None, beta_claim, PortLeaseEffectScope::ProcessBound)
        .expect("beta should own a distinct live attempt");
    let failure = bind_failure("alpha", PORT);

    assert!(matches!(
        authority.record_claimed_bind_failure_with_lifetime_without_effect(
            &alpha,
            None,
            &alpha_claim,
            failure.clone(),
            &beta_lifetime,
        ),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        authority
            .inspect(alpha.lease_id())
            .expect("alpha should inspect")
            .expect("alpha should remain")
            .active_lifetime(),
        Some(alpha_lifetime.lifetime()),
        "a foreign guard must not clear the live attempt"
    );

    let failed = authority
        .record_claimed_bind_failure_with_lifetime_without_effect(
            &alpha,
            None,
            &alpha_claim,
            failure.clone(),
            &alpha_lifetime,
        )
        .expect("the exact live owner should record proven no-effect failure");
    assert_eq!(failed.phase(), PortLeasePhase::Failed);
    assert!(failed.active_lifetime().is_none());
    assert_eq!(
        authority
            .record_claimed_bind_failure_with_lifetime_without_effect(
                &alpha,
                None,
                &alpha_claim,
                failure,
                &alpha_lifetime,
            )
            .expect("exact terminal replay should be idempotent"),
        failed
    );
}

#[test]
fn batch_claim_activation_and_no_effect_compensation_are_lifetime_atomic() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let alpha = request_for("alpha", PORT);
    let beta = request_for("beta", PORT + 1);
    authority
        .reserve(alpha.clone())
        .expect("alpha should reserve");
    authority
        .reserve(beta.clone())
        .expect("beta should reserve");
    let claims = [
        (alpha.clone(), bind_claim("alpha")),
        (beta.clone(), bind_claim("beta")),
    ];
    let lifetimes = authority
        .claim_bind_batch_with_lifetimes(&claims, None, PortLeaseEffectScope::ProviderManaged)
        .expect("the complete batch should claim atomically");
    assert_eq!(lifetimes.len(), 2);
    for ((request, claim), lifetime) in claims.iter().zip(&lifetimes) {
        let record = authority
            .inspect(request.lease_id())
            .expect("claimed request should inspect")
            .expect("claimed request should remain");
        assert_eq!(record.bind_claim(), Some(claim));
        assert_eq!(record.active_lifetime(), Some(lifetime.lifetime()));
        assert_eq!(
            lifetime.lifetime().effect_scope(),
            PortLeaseEffectScope::ProviderManaged
        );
    }

    let bindings = [
        (alpha.clone(), claims[0].1.clone(), binding("alpha", PORT)),
        (beta.clone(), claims[1].1.clone(), binding("beta", PORT + 1)),
    ];
    let active = authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &lifetimes)
        .expect("the complete exact batch should activate atomically");
    assert!(
        active
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Active)
    );

    drop(lifetimes);
    for request in [&alpha, &beta] {
        let PortLeaseRecoveryAttempt::Acquired(_recovery) = authority
            .recover_dead_lifetime(request)
            .expect("each dead batch owner should expose exact recovery")
        else {
            panic!("released batch locks must yield recovery authority");
        };
    }
}

#[test]
fn contended_batch_lifetime_claim_leaves_every_sibling_unmodified() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let alpha = request_for("alpha", PORT);
    let beta = request_for("beta", PORT + 1);
    authority
        .reserve(alpha.clone())
        .expect("alpha should reserve");
    authority
        .reserve(beta.clone())
        .expect("beta should reserve");
    let alpha_claim = bind_claim("alpha");
    let alpha_lifetime = authority
        .claim_bind_with_lifetime(
            &alpha,
            None,
            alpha_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("alpha should own the contended lifetime");
    let beta_before = authority
        .inspect(beta.lease_id())
        .expect("beta should inspect")
        .expect("beta should remain");

    let error = authority
        .claim_bind_batch_with_lifetimes(
            &[
                (alpha.clone(), bind_claim("beta")),
                (beta.clone(), bind_claim("gamma")),
            ],
            None,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect_err("one live sibling must reject the complete batch");
    assert!(matches!(error, PortLeaseError::LifetimeOwnerLive { .. }));
    assert_eq!(
        authority
            .inspect(beta.lease_id())
            .expect("beta should inspect")
            .expect("beta should remain"),
        beta_before,
        "batch contention must not partially claim an uncontended sibling"
    );
    drop(alpha_lifetime);
}

#[test]
fn batch_no_effect_compensation_requires_every_exact_lifetime() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let requests = [request_for("alpha", PORT), request_for("beta", PORT + 1)];
    for request in &requests {
        authority
            .reserve(request.clone())
            .expect("request should reserve");
    }
    let claims = [
        (requests[0].clone(), bind_claim("alpha")),
        (requests[1].clone(), bind_claim("beta")),
    ];
    let lifetimes = authority
        .claim_bind_batch_with_lifetimes(&claims, None, PortLeaseEffectScope::ProcessBound)
        .expect("batch should claim");

    let error = authority
        .abandon_bind_batch_with_lifetimes_without_effect(&claims, None, &lifetimes[..1])
        .expect_err("a partial lifetime set must not compensate any sibling");
    assert!(matches!(error, PortLeaseError::LifetimeMismatch { .. }));
    assert!(claims.iter().all(|(request, claim)| {
        authority
            .inspect(request.lease_id())
            .expect("request should inspect")
            .expect("request should remain")
            .bind_claim()
            == Some(claim)
    }));

    let abandoned = authority
        .abandon_bind_batch_with_lifetimes_without_effect(&claims, None, &lifetimes)
        .expect("the exact complete batch should compensate");
    assert!(abandoned.iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.bind_claim().is_none()
            && record.active_lifetime().is_none()
    }));
}

#[test]
fn provider_managed_owner_death_stays_fenced_without_provider_absence() {
    let fixture = active_fixture(PortLeaseEffectScope::ProviderManaged);
    let ActiveFixture {
        _root,
        authority,
        request,
        guard,
    } = fixture;
    drop(guard);

    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("dead coordinator should yield inspection authority")
    else {
        panic!("released lifetime lock must yield recovery authority");
    };
    authority
        .mark_cleanup_pending_after_owner_death(&request, &recovery)
        .expect("provider-managed effect should be quarantined");
    assert!(matches!(
        authority.release_process_bound_after_owner_death(&request, &recovery),
        Err(PortLeaseError::LifetimeScopeMismatch { .. })
    ));
    assert_eq!(
        authority
            .inspect(request.lease_id())
            .expect("authority should remain readable")
            .expect("cleanup record should remain")
            .phase(),
        PortLeasePhase::CleanupPending
    );
    assert!(matches!(
        authority.reserve(request_for("beta", PORT)),
        Err(PortLeaseError::PortConflict {
            existing_phase: PortLeasePhase::CleanupPending,
            ..
        })
    ));
}

#[test]
fn dead_provider_claim_batch_becomes_rebindable_only_after_confirmed_absence() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let requests = [request_for("alpha", PORT), request_for("beta", PORT + 1)];
    for request in &requests {
        authority
            .reserve(request.clone())
            .expect("request should reserve");
    }
    let claims = [
        (requests[0].clone(), bind_claim("alpha")),
        (requests[1].clone(), bind_claim("beta")),
    ];
    let lifetimes = authority
        .claim_bind_batch_with_lifetimes(&claims, None, PortLeaseEffectScope::ProviderManaged)
        .expect("provider batch should claim");
    drop(lifetimes);

    let mut recoveries = Vec::new();
    for request in &requests {
        let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
            .recover_dead_lifetime(request)
            .expect("dead provider claim should recover")
        else {
            panic!("dead provider claim must yield recovery");
        };
        recoveries.push(recovery);
    }
    authority
        .mark_cleanup_pending_batch_after_owner_death(&requests, &recoveries)
        .expect("dead provider claims should remain quarantined before inspection");
    assert!(matches!(
        authority.reserve(request_for("gamma", PORT)),
        Err(PortLeaseError::PortConflict {
            existing_phase: PortLeasePhase::CleanupPending,
            ..
        })
    ));

    let retained = authority
        .prepare_rebind_provider_managed_claim_batch_after_confirmed_stop(&requests, &recoveries)
        .expect("confirmed provider absence should retire only the dead claims");
    assert!(retained.iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.bind_claim().is_none()
            && record.binding().is_none()
            && record.active_lifetime().is_none()
    }));
    assert_eq!(
        authority
            .prepare_rebind_provider_managed_claim_batch_after_confirmed_stop(
                &requests,
                &recoveries,
            )
            .expect("exact retained replay should be idempotent"),
        retained
    );
    drop(recoveries);

    authority
        .claim_bind_batch_with_lifetimes(
            &[
                (requests[0].clone(), bind_claim("beta")),
                (requests[1].clone(), bind_claim("gamma")),
            ],
            None,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("the same desired batch should begin one higher lifetime");
}

#[test]
fn surviving_provider_binding_can_transfer_only_under_exact_dead_owner_recovery() {
    let fixture = active_fixture(PortLeaseEffectScope::ProviderManaged);
    let ActiveFixture {
        _root,
        authority,
        request,
        guard,
    } = fixture;
    let before = authority
        .inspect(request.lease_id())
        .expect("active provider request should inspect")
        .expect("active provider request should remain");
    let binding = before
        .binding()
        .cloned()
        .expect("active provider request should carry exact binding");
    let first_lifetime = guard.lifetime();
    drop(guard);

    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("dead coordinator should yield recovery authority")
    else {
        panic!("released provider lifetime must be recoverable");
    };
    let replacement = authority
        .reclaim_provider_managed_binding_after_owner_death(&request, &binding, recovery)
        .expect("adapter-authenticated surviving binding should transfer");
    let active = authority
        .inspect(request.lease_id())
        .expect("transferred request should inspect")
        .expect("transferred request should remain");
    assert_eq!(active.phase(), PortLeasePhase::Active);
    assert_eq!(active.binding(), Some(&binding));
    assert_eq!(active.active_lifetime(), Some(replacement.lifetime()));
    assert!(
        replacement.lifetime().generation() > first_lifetime.generation(),
        "replacement owner must fence the dead coordinator with a higher generation"
    );
}

#[test]
fn provider_managed_batch_recovery_is_exact_and_atomic_after_confirmed_absence() {
    for release in [false, true] {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let requests = [request_for("alpha", PORT), request_for("beta", PORT + 1)];
        for request in &requests {
            authority
                .reserve(request.clone())
                .expect("request should reserve");
        }
        let claims = [
            (requests[0].clone(), bind_claim("alpha")),
            (requests[1].clone(), bind_claim("beta")),
        ];
        let lifetimes = authority
            .claim_bind_batch_with_lifetimes(&claims, None, PortLeaseEffectScope::ProviderManaged)
            .expect("provider batch should claim");
        let bindings = [
            (
                requests[0].clone(),
                claims[0].1.clone(),
                binding("alpha", PORT),
            ),
            (
                requests[1].clone(),
                claims[1].1.clone(),
                binding("beta", PORT + 1),
            ),
        ];
        authority
            .adopt_claimed_and_activate_batch_with_lifetimes(&bindings, None, &lifetimes)
            .expect("provider batch should activate");
        drop(lifetimes);

        let recoveries = requests
            .iter()
            .map(|request| {
                let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
                    .recover_dead_lifetime(request)
                    .expect("dead provider coordinator should be recoverable")
                else {
                    panic!("released provider lifetimes must yield recovery guards");
                };
                recovery
            })
            .collect::<Vec<_>>();
        let before = requests
            .iter()
            .map(|request| {
                authority
                    .inspect(request.lease_id())
                    .expect("request should inspect")
                    .expect("request should remain")
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            authority.mark_cleanup_pending_batch_after_owner_death(&requests, &recoveries[..1]),
            Err(PortLeaseError::LifetimeMismatch { .. })
        ));
        assert_eq!(
            requests
                .iter()
                .map(|request| {
                    authority
                        .inspect(request.lease_id())
                        .expect("request should inspect")
                        .expect("request should remain")
                })
                .collect::<Vec<_>>(),
            before,
            "partial recovery authority must not quarantine any sibling"
        );

        let cleanup = authority
            .mark_cleanup_pending_batch_after_owner_death(&requests, &recoveries)
            .expect("the exact dead-owner batch should quarantine atomically");
        assert!(
            cleanup
                .iter()
                .all(|record| record.phase() == PortLeasePhase::CleanupPending)
        );
        assert!(matches!(
            authority.reserve(request_for("gamma", PORT)),
            Err(PortLeaseError::PortConflict {
                existing_phase: PortLeasePhase::CleanupPending,
                ..
            })
        ));

        if release {
            let released = authority
                .release_provider_managed_batch_after_confirmed_stop(&requests, &recoveries)
                .expect("exact provider absence should release the complete batch");
            assert!(
                released
                    .iter()
                    .all(|record| record.phase() == PortLeasePhase::Released)
            );
        } else {
            let expected = bindings
                .iter()
                .map(|(request, _, binding)| (request.clone(), binding.clone()))
                .collect::<Vec<_>>();
            let retained = authority
                .prepare_rebind_provider_managed_batch_after_confirmed_stop(&expected, &recoveries)
                .expect("exact provider absence should retain the complete batch");
            assert!(retained.iter().all(|record| {
                record.phase() == PortLeasePhase::Reserved
                    && record.active_lifetime().is_none()
                    && record.confirmed_stopped_binding().is_some()
            }));
        }
    }
}

#[test]
fn live_provider_batch_release_is_lifetime_authenticated_atomic_and_auditable() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let requests = [request_for("alpha", PORT), request_for("beta", PORT + 1)];
    for request in &requests {
        authority
            .reserve(request.clone())
            .expect("request should reserve");
    }
    let claims = [
        (requests[0].clone(), bind_claim("alpha")),
        (requests[1].clone(), bind_claim("beta")),
    ];
    let lifetimes = authority
        .claim_bind_batch_with_lifetimes(&claims, None, PortLeaseEffectScope::ProviderManaged)
        .expect("provider batch should claim");
    let adopted = [
        (
            requests[0].clone(),
            claims[0].1.clone(),
            binding("alpha", PORT),
        ),
        (
            requests[1].clone(),
            claims[1].1.clone(),
            binding("beta", PORT + 1),
        ),
    ];
    authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&adopted, None, &lifetimes)
        .expect("provider batch should activate");
    let expected = adopted
        .iter()
        .map(|(request, _, binding)| (request.clone(), binding.clone()))
        .collect::<Vec<_>>();
    let before = requests
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("request should inspect")
                .expect("request should remain")
        })
        .collect::<Vec<_>>();

    assert!(matches!(
        authority.release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
            &expected,
            &lifetimes[..1],
        ),
        Err(PortLeaseError::LifetimeMismatch { .. })
    ));
    assert_eq!(
        requests
            .iter()
            .map(|request| {
                authority
                    .inspect(request.lease_id())
                    .expect("request should inspect")
                    .expect("request should remain")
            })
            .collect::<Vec<_>>(),
        before,
        "partial lifetime authority must not release any sibling"
    );

    let released = authority
        .release_provider_managed_batch_after_confirmed_stop_with_lifetimes(&expected, &lifetimes)
        .expect("exact live provider batch should release atomically");
    assert!(released.iter().all(|record| {
        record.phase() == PortLeasePhase::Released
            && record.active_lifetime().is_none()
            && record.binding().is_some()
            && record.adoption_claim().is_some()
    }));
    assert_eq!(
        authority
            .release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
                &expected, &lifetimes,
            )
            .expect("exact terminal release replay should be idempotent"),
        released
    );
}

#[test]
fn stale_request_cannot_recover_another_desired_generation() {
    let fixture = active_fixture(PortLeaseEffectScope::ProcessBound);
    let stale = PortLeaseRequest::new(
        fixture.request.lease_id().clone(),
        fixture.request.owner_id().clone(),
        fixture.request.tenant_id().cloned(),
        PortLeaseFence::new(
            fixture
                .request
                .generation()
                .checked_next()
                .expect("fixture generation should advance"),
            fixture.request.lease_epoch(),
        ),
        fixture.request.accounting(),
        fixture.request.publication().clone(),
        fixture.request.binding().clone(),
    );

    assert!(matches!(
        fixture.authority.recover_dead_lifetime(&stale),
        Err(PortLeaseError::StaleFence(_))
    ));
}

#[test]
fn explicit_reconciliation_releases_only_dead_process_bound_owners() {
    let _root = tempfile::tempdir().expect("state root should exist");
    let authority =
        LocalPortLeaseAuthority::open(_root.path()).expect("shared authority should open");
    let (live_request, live_guard) = activate_in_authority(
        &authority,
        "alpha",
        PortLeaseEffectScope::ProcessBound,
        PORT,
    );
    let (dead_request, dead_guard) = activate_in_authority(
        &authority,
        "beta",
        PortLeaseEffectScope::ProcessBound,
        PORT + 1,
    );
    let (provider_request, provider_guard) = activate_in_authority(
        &authority,
        "gamma",
        PortLeaseEffectScope::ProviderManaged,
        PORT + 2,
    );
    drop(dead_guard);
    drop(provider_guard);

    let report = authority
        .reconcile_dead_process_bound_leases()
        .expect("process-bound reconciliation should succeed");
    assert_eq!(report.released(), &[dead_request.lease_id().clone()]);
    assert_eq!(report.live(), &[live_request.lease_id().clone()]);
    assert_eq!(
        report.provider_managed(),
        &[provider_request.lease_id().clone()]
    );
    assert!(report.missing_lifetime().is_empty());
    assert_eq!(
        authority
            .inspect(dead_request.lease_id())
            .expect("released record should remain readable")
            .expect("released record should remain")
            .phase(),
        PortLeasePhase::Released
    );
    assert_eq!(
        authority
            .inspect(provider_request.lease_id())
            .expect("provider record should remain readable")
            .expect("provider record should remain")
            .phase(),
        PortLeasePhase::Active
    );
    drop(live_guard);
}

struct ActiveFixture {
    _root: TempDir,
    authority: LocalPortLeaseAuthority,
    request: PortLeaseRequest,
    guard: PortLeaseLifetimeGuard,
}

fn active_fixture(effect_scope: PortLeaseEffectScope) -> ActiveFixture {
    active_fixture_with_role("alpha", effect_scope, PORT)
}

fn active_fixture_with_role(
    role: &str,
    effect_scope: PortLeaseEffectScope,
    port: u16,
) -> ActiveFixture {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let (request, guard) = activate_in_authority(&authority, role, effect_scope, port);
    ActiveFixture {
        _root: root,
        authority,
        request,
        guard,
    }
}

fn activate_in_authority(
    authority: &LocalPortLeaseAuthority,
    role: &str,
    effect_scope: PortLeaseEffectScope,
    port: u16,
) -> (PortLeaseRequest, PortLeaseLifetimeGuard) {
    let request = request_for(role, port);
    authority
        .reserve(request.clone())
        .expect("fixture request should reserve");
    let claim = bind_claim(role);
    let guard = authority
        .claim_bind_with_lifetime(&request, None, claim.clone(), effect_scope)
        .expect("fixture bind and lifetime should be claimed atomically");
    authority
        .adopt_claimed_and_activate_with_lifetime(
            &request,
            None,
            &claim,
            binding(role, port),
            &guard,
        )
        .expect("fixture binding should adopt and activate under its exact lifetime");
    (request, guard)
}

fn request_for(role: &str, port: u16) -> PortLeaseRequest {
    PortLeaseRequest::new(
        lease_id(role),
        owner_id(role),
        None,
        PortLeaseFence::new(
            NetworkResourceGeneration::new(7),
            NetworkLeaseEpoch::new(11),
        ),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::Exact(nonzero_port(port)),
        ),
    )
}

fn lease_id(role: &str) -> PortLeaseId {
    match role {
        "alpha" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "beta" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "gamma" => "netportlease_01ARZ3NDEKTSV4RRFFQ69G5FAX",
        other => panic!("unexpected fixture role {other:?}"),
    }
    .parse()
    .expect("fixture lease ID should parse")
}

fn owner_id(role: &str) -> NetworkResourceId {
    let listener: ListenerId = match role {
        "alpha" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "beta" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "gamma" => "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAX",
        other => panic!("unexpected fixture role {other:?}"),
    }
    .parse()
    .expect("fixture listener ID should parse");
    listener.into()
}

fn bind_claim(role: &str) -> PortBindClaim {
    PortBindClaim::new(provider_handle(format!("attempt-{role}")))
}

fn binding(role: &str, port: u16) -> PortLeaseBinding {
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            nonzero_port(port),
        )
        .expect("fixture endpoint should validate"),
        PortBindingProvenance::NimbusOwned,
        provider_handle(format!("binding-{role}")),
    )
}

fn bind_failure(role: &str, port: u16) -> PortBindFailure {
    PortBindFailure::new(
        PortBindFailureKind::AddrInUse,
        PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            port,
        )
        .expect("fixture bind attempt should validate"),
        provider_handle(format!("attempt-{role}")),
    )
}

fn provider_handle(resource: String) -> NetworkProviderHandle {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider ID should parse");
    NetworkProviderHandle::new(provider_id, resource)
        .expect("fixture provider handle should validate")
}

fn foreign_provider_handle(resource: &str) -> NetworkProviderHandle {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAW"
        .parse()
        .expect("fixture foreign provider ID should parse");
    NetworkProviderHandle::new(provider_id, resource)
        .expect("fixture foreign provider handle should validate")
}

fn nonzero_port(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).expect("fixture port should be non-zero")
}
