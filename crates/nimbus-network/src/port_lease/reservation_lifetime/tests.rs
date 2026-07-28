use std::num::NonZeroU16;

use super::*;
use crate::{
    ListenerId, NetworkLeaseEpoch, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration, PortBindClaim, PortBindRealm, PortBindTarget, PortBindingSpec,
    PortExposure, PortLeaseAccounting, PortLeaseFence, PortLeasePhase, PortLeaseRequest,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};

#[test]
fn live_reservation_lifetime_fences_claim_only_release() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claim = reservation_claim("live-owner");
    let request = request(43_091);
    let lifetime = match authority
        .try_acquire_reservation_lifetime(&claim)
        .expect("reservation lifetime should inspect")
    {
        NetworkReservationLifetimeAttempt::Acquired(lifetime) => lifetime,
        NetworkReservationLifetimeAttempt::LiveOwner => {
            panic!("first coordinator should own its lifetime")
        }
    };
    authority
        .reserve_batch_for_coordinator(vec![request.clone()], &claim)
        .expect("claim-owned reservation should commit");

    assert!(matches!(
        authority.release_reserved_batch_without_effect(&[request], &claim),
        Err(PortLeaseError::ReservationLifetimeOwnerLive { .. })
    ));
    drop(lifetime);
}

#[test]
fn exact_claim_reacquires_only_after_owner_lifetime_ends() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claim = reservation_claim("owner-death");
    let lifetime = match authority
        .try_acquire_reservation_lifetime(&claim)
        .expect("reservation lifetime should inspect")
    {
        NetworkReservationLifetimeAttempt::Acquired(lifetime) => lifetime,
        NetworkReservationLifetimeAttempt::LiveOwner => {
            panic!("first coordinator should own its lifetime")
        }
    };
    assert!(matches!(
        authority
            .try_acquire_reservation_lifetime(&claim)
            .expect("contended lifetime should inspect"),
        NetworkReservationLifetimeAttempt::LiveOwner
    ));
    drop(lifetime);
    assert!(matches!(
        authority
            .try_acquire_reservation_lifetime(&claim)
            .expect("dead lifetime should be recoverable"),
        NetworkReservationLifetimeAttempt::Acquired(_)
    ));
}

#[test]
fn live_exact_lifetime_can_compensate_once_and_replay_idempotently() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let claim = reservation_claim("exact-compensation");
    let request = request(43_092);
    let lifetime = acquire(&authority, &claim);
    authority
        .reserve_batch_for_coordinator(vec![request.clone()], &claim)
        .expect("claim-owned reservation should commit");

    let released = authority
        .release_reserved_batch_without_effect_with_lifetime(
            std::slice::from_ref(&request),
            &lifetime,
        )
        .expect("the live exact coordinator may compensate before publication");
    assert_eq!(released[0].phase(), PortLeasePhase::Released);
    assert_eq!(
        authority
            .release_reserved_batch_without_effect_with_lifetime(&[request], &lifetime)
            .expect("exact compensation replay should be idempotent"),
        released
    );
}

#[test]
fn substituted_lifetime_and_provider_ambiguity_stay_fenced() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let owner = reservation_claim("owner");
    let substitute = reservation_claim("substitute");
    let request = request(43_093);
    let owner_lifetime = acquire(&authority, &owner);
    let substitute_lifetime = acquire(&authority, &substitute);
    authority
        .reserve_batch_for_coordinator(vec![request.clone()], &owner)
        .expect("owner reservation should commit");

    assert!(matches!(
        authority.release_reserved_batch_without_effect_with_lifetime(
            std::slice::from_ref(&request),
            &substitute_lifetime,
        ),
        Err(PortLeaseError::ReservationClaimConflict { .. })
    ));

    let bind_claim = PortBindClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("test.ambiguous-provider"),
            "attempt:ambiguous",
        )
        .expect("provider attempt should validate"),
    );
    authority
        .claim_bind(&request, Some(&owner), bind_claim)
        .expect("provider attempt should claim before its ambiguous effect");
    assert!(matches!(
        authority.release_reserved_batch_without_effect_with_lifetime(&[request], &owner_lifetime),
        Err(PortLeaseError::BindClaimConflict { .. })
    ));
}

fn acquire(
    authority: &LocalPortLeaseAuthority,
    claim: &NetworkReservationClaim,
) -> NetworkReservationLifetimeGuard {
    match authority
        .try_acquire_reservation_lifetime(claim)
        .expect("reservation lifetime should inspect")
    {
        NetworkReservationLifetimeAttempt::Acquired(lifetime) => lifetime,
        NetworkReservationLifetimeAttempt::LiveOwner => {
            panic!("fixture coordinator should own its lifetime")
        }
    }
}

fn reservation_claim(name: &str) -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("test.launch-reservation"),
            name,
        )
        .expect("fixture claim should validate"),
    )
}

fn request(port: u16) -> PortLeaseRequest {
    let listener = ListenerId::for_workload_listener("reservation-lifetime-test", "http");
    PortLeaseRequest::new(
        crate::PortLeaseId::for_listener(&listener),
        listener.into(),
        None,
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Unknown,
            PortRequestMode::Exact(NonZeroU16::new(port).expect("fixture port should be nonzero")),
        ),
    )
}
