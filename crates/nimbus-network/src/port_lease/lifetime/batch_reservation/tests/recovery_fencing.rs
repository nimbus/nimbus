use super::*;

#[test]
fn dead_plan_subset_recovery_rejects_invalid_witnesses_and_members_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    let launch_claim = reservation_claim("invalid-recovery-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete invalid-input fixture should reserve");
    let claim = bind_claim("alpha");
    let lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            claim.clone(),
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("active fixture member should claim");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            &claim,
            binding("alpha", PORT),
            &lifetime,
        )
        .expect("active fixture member should adopt");
    drop(lifetime);
    let bytes_before = authority_bytes(root.path());

    assert!(matches!(
        authority
            .recover_dead_plan_members(&members, &[])
            .expect_err("empty recovery subset must fail"),
        PortLeaseError::CorruptAuthority { .. }
    ));
    let duplicate = [members[0].clone(), members[0].clone()];
    assert!(matches!(
        authority
            .recover_dead_plan_members(&members, &duplicate)
            .expect_err("duplicate recovery member must fail"),
        PortLeaseError::IdentityConflict { .. }
    ));
    assert!(matches!(
        authority
            .recover_dead_plan_members(&members[..1], std::slice::from_ref(&members[0]))
            .expect_err("incomplete witness must fail"),
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    let crossed_witness = [members[0].clone(), planned_request_for("gamma", PORT + 2)];
    assert!(matches!(
        authority
            .recover_dead_plan_members(&crossed_witness, std::slice::from_ref(&members[0]))
            .expect_err("crossed witness must fail"),
        PortLeaseError::PlanMembershipConflict { .. }
    ));
    assert!(matches!(
        authority
            .recover_dead_plan_members(&members, std::slice::from_ref(&members[1]))
            .expect_err("reserved sibling is not dead active recovery authority"),
        PortLeaseError::LifetimeMismatch { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
}

#[test]
fn process_bound_plan_member_rebind_survives_the_cleanup_checkpoint_crash() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
        planned_request_for("gamma", PORT + 2),
    ];
    let launch_claim = reservation_claim("checkpoint-recovery-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete checkpoint plan should reserve");

    let alpha_claim = bind_claim("alpha");
    let alpha_lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            alpha_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("process-bound member should claim");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            &alpha_claim,
            binding("alpha", PORT),
            &alpha_lifetime,
        )
        .expect("process-bound member should activate");
    let alpha_generation = alpha_lifetime.lifetime().generation();

    let beta_claim = bind_claim("beta");
    let beta_lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[1],
            &launch_claim,
            beta_claim.clone(),
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("independent live sibling should claim");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[1],
            &launch_claim,
            &beta_claim,
            binding("beta", PORT + 1),
            &beta_lifetime,
        )
        .expect("independent live sibling should activate");
    let beta_before = authority
        .inspect(members[1].lease_id())
        .expect("live sibling should inspect")
        .expect("live sibling should remain durable");
    let gamma_before = authority
        .inspect(members[2].lease_id())
        .expect("reserved sibling should inspect")
        .expect("reserved sibling should remain durable");

    drop(alpha_lifetime);
    let requests = std::slice::from_ref(&members[0]);
    let recoveries = authority
        .recover_dead_plan_members(&members, requests)
        .expect("dead process-bound member should recover");
    let cleanup = authority
        .mark_cleanup_pending_plan_members_after_owner_death(&members, requests, &recoveries)
        .expect("cleanup-pending checkpoint should commit");
    assert_eq!(cleanup[0].phase(), PortLeasePhase::CleanupPending);
    let cleanup_bytes = authority_bytes(root.path());
    drop(recoveries);

    let resumed = authority
        .recover_dead_plan_members(&members, requests)
        .expect("fresh process should recover the cleanup-pending generation");
    authority
        .mark_cleanup_pending_plan_members_after_owner_death(&members, requests, &resumed)
        .expect("cleanup checkpoint should replay without mutation");
    assert_eq!(authority_bytes(root.path()), cleanup_bytes);
    let retained = authority
        .prepare_rebind_process_bound_plan_members_after_owner_death(&members, requests, &resumed)
        .expect("cleanup-pending member should become retained for rebind");
    assert_eq!(retained[0].phase(), PortLeasePhase::Reserved);
    assert!(retained[0].active_lifetime().is_none());
    assert!(retained[0].binding().is_none());
    assert!(retained[0].confirmed_stopped_binding().is_some());
    assert_eq!(
        retained[0].last_lifetime_generation(),
        Some(alpha_generation)
    );
    let retained_bytes = authority_bytes(root.path());
    authority
        .prepare_rebind_process_bound_plan_members_after_owner_death(&members, requests, &resumed)
        .expect("retained rebind transition should replay exactly");
    assert_eq!(authority_bytes(root.path()), retained_bytes);
    let confirmed_stopped = retained[0]
        .confirmed_stopped_binding()
        .expect("retained member carries its exact stop receipt")
        .clone();
    drop(resumed);

    let rebind_claim = bind_claim("alpha");
    let first_rebind = authority
        .claim_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            rebind_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("retained member should durably claim before an external bind");
    assert_eq!(
        first_rebind.lifetime().generation().as_u64(),
        alpha_generation.as_u64() + 1
    );
    let claim_only = authority
        .inspect(members[0].lease_id())
        .expect("claim-only member should inspect")
        .expect("claim-only member should remain durable");
    assert_eq!(claim_only.phase(), PortLeasePhase::Reserved);
    assert_eq!(claim_only.bind_claim(), Some(&rebind_claim));
    assert!(claim_only.binding().is_none());
    assert_eq!(
        claim_only.confirmed_stopped_binding(),
        Some(&confirmed_stopped)
    );
    drop(first_rebind);

    let second_rebind = authority
        .claim_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            rebind_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("claim-only crash retry should fence with the next generation");
    assert_eq!(
        second_rebind.lifetime().generation().as_u64(),
        alpha_generation.as_u64() + 2
    );
    let effect_before_adopt = authority
        .inspect(members[0].lease_id())
        .expect("pre-adoption member should inspect")
        .expect("pre-adoption member should remain durable");
    assert_eq!(effect_before_adopt.phase(), PortLeasePhase::Reserved);
    assert_eq!(effect_before_adopt.bind_claim(), Some(&rebind_claim));
    assert_eq!(
        effect_before_adopt.confirmed_stopped_binding(),
        Some(&confirmed_stopped)
    );
    let adopted = authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            &rebind_claim,
            confirmed_stopped.clone(),
            &second_rebind,
        )
        .expect("effect-before-adopt should commit the exact retained binding");
    assert_eq!(adopted.phase(), PortLeasePhase::Active);
    assert_eq!(adopted.binding(), Some(&confirmed_stopped));
    let adopted_bytes = authority_bytes(root.path());
    authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            &rebind_claim,
            confirmed_stopped.clone(),
            &second_rebind,
        )
        .expect("lost adoption acknowledgement should replay exactly");
    assert_eq!(authority_bytes(root.path()), adopted_bytes);
    assert_eq!(
        authority
            .inspect(members[1].lease_id())
            .expect("live sibling should inspect"),
        Some(beta_before)
    );
    assert_eq!(
        authority
            .inspect(members[2].lease_id())
            .expect("reserved sibling should inspect"),
        Some(gamma_before)
    );
    drop(beta_lifetime);
}

#[test]
fn rebind_crossed_claims_fail_before_lifetime_advance_or_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [planned_request_for("alpha", PORT)];
    let launch_claim = reservation_claim("crossed-rebind-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("single-member plan should reserve");

    let initial_claim = bind_claim("initial-alpha");
    let initial_lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            initial_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("initial provider attempt should claim");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            &initial_claim,
            binding("initial-alpha", PORT),
            &initial_lifetime,
        )
        .expect("initial provider attempt should activate");
    drop(initial_lifetime);

    let requests = std::slice::from_ref(&members[0]);
    let recoveries = authority
        .recover_dead_plan_members(&members, requests)
        .expect("dead process-bound attempt should recover");
    authority
        .mark_cleanup_pending_plan_members_after_owner_death(&members, requests, &recoveries)
        .expect("dead attempt should checkpoint cleanup");
    let retained = authority
        .prepare_rebind_process_bound_plan_members_after_owner_death(
            &members,
            requests,
            &recoveries,
        )
        .expect("dead attempt should retain its confirmed stop receipt");
    let confirmed_stopped = retained[0]
        .confirmed_stopped_binding()
        .expect("retained attempt should carry its stop receipt")
        .clone();
    drop(recoveries);

    let retained_claim = bind_claim("retained-alpha");
    let retained_lifetime = authority
        .claim_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            retained_claim,
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("retained rebind attempt should claim");
    drop(retained_lifetime);
    let record_before = authority
        .inspect(members[0].lease_id())
        .expect("claim-only attempt should inspect")
        .expect("claim-only attempt should remain durable");
    let bytes_before = authority_bytes(root.path());

    let crossed_claim = bind_claim("crossed-alpha");
    let scalar_error = authority
        .claim_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            crossed_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect_err("a crossed scalar claim must fail before generation advance");
    assert!(matches!(
        scalar_error,
        PortLeaseError::BindClaimConflict { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
    assert_eq!(
        authority
            .inspect(members[0].lease_id())
            .expect("scalar rejection should inspect"),
        Some(record_before.clone())
    );

    let batch_error = authority
        .claim_rebind_plan_members_with_lifetimes(
            &members,
            &[(members[0].clone(), crossed_claim, confirmed_stopped)],
            PortLeaseEffectScope::ProcessBound,
        )
        .expect_err("a crossed batch claim must fail before generation advance");
    assert!(matches!(
        batch_error,
        PortLeaseError::BindClaimConflict { .. }
    ));
    assert_eq!(authority_bytes(root.path()), bytes_before);
    assert_eq!(
        authority
            .inspect(members[0].lease_id())
            .expect("batch rejection should inspect"),
        Some(record_before)
    );
}

#[test]
fn never_effected_plan_member_inspection_authenticates_all_effect_evidence() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    let launch_claim = reservation_claim("never-effected-inspection");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete plan should reserve");
    let bytes_before = authority_bytes(root.path());

    assert!(
        authority
            .inspect_plan_members_never_effected(
                &members,
                std::slice::from_ref(&members[0]),
                &launch_claim,
            )
            .expect("pristine exact member should inspect")
    );
    assert_eq!(authority_bytes(root.path()), bytes_before);

    let bind_claim = bind_claim("alpha-effect");
    let lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            bind_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("effect-evidence fixture should claim");
    assert!(
        !authority
            .inspect_plan_members_never_effected(
                &members,
                std::slice::from_ref(&members[0]),
                &launch_claim,
            )
            .expect("claimed exact member should inspect as effect-bearing")
    );
    assert!(
        authority
            .inspect_plan_members_never_effected(
                &members,
                std::slice::from_ref(&members[1]),
                &launch_claim,
            )
            .expect("untouched sibling should remain never effected")
    );

    let crossed = planned_request_for("alpha", PORT + 2);
    assert!(
        authority
            .inspect_plan_members_never_effected(&members, &[crossed], &launch_claim)
            .is_err(),
        "crossed immutable request identity must fail closed"
    );
    drop(lifetime);
}

#[test]
fn process_bound_plan_member_transition_rejects_crossed_witness_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
    ];
    let launch_claim = reservation_claim("crossed-transition-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete crossed-transition plan should reserve");
    let claim = bind_claim("alpha");
    let lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("transition fixture member should claim");
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            &members,
            &members[0],
            &launch_claim,
            &claim,
            binding("alpha", PORT),
            &lifetime,
        )
        .expect("transition fixture member should activate");
    drop(lifetime);
    let requests = std::slice::from_ref(&members[0]);
    let recoveries = authority
        .recover_dead_plan_members(&members, requests)
        .expect("transition fixture member should recover");
    let bytes_before = authority_bytes(root.path());

    for witness in [
        members[..1].to_vec(),
        vec![members[0].clone(), planned_request_for("gamma", PORT + 2)],
        vec![members[0].clone(), members[0].clone(), members[1].clone()],
    ] {
        authority
            .mark_cleanup_pending_plan_members_after_owner_death(&witness, requests, &recoveries)
            .expect_err("incomplete, crossed, or duplicate witness must fail");
        assert_eq!(authority_bytes(root.path()), bytes_before);
    }
    authority
        .mark_cleanup_pending_plan_members_after_owner_death(
            &members,
            std::slice::from_ref(&members[1]),
            &recoveries,
        )
        .expect_err("crossed requested member and recovery must fail");
    assert_eq!(authority_bytes(root.path()), bytes_before);

    authority
        .mark_cleanup_pending_plan_members_after_owner_death(&members, requests, &recoveries)
        .expect("valid transition fixture should checkpoint cleanup");
    let retained = authority
        .prepare_rebind_process_bound_plan_members_after_owner_death(
            &members,
            requests,
            &recoveries,
        )
        .expect("valid transition fixture should retain the stopped binding");
    let confirmed_stopped = retained[0]
        .confirmed_stopped_binding()
        .expect("retained transition carries a stop receipt")
        .clone();
    drop(recoveries);
    let retained_bytes = authority_bytes(root.path());
    authority
        .claim_rebind_plan_member_with_lifetime(
            &members[..1],
            &members[0],
            &confirmed_stopped,
            bind_claim("alpha"),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect_err("incomplete rebind witness must fail");
    authority
        .claim_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &binding("alpha", PORT + 1),
            bind_claim("alpha"),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect_err("crossed stop receipt must fail");
    assert_eq!(authority_bytes(root.path()), retained_bytes);

    let rebind_claim = bind_claim("alpha");
    let rebind_lifetime = authority
        .claim_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            rebind_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("valid rebind claim should persist");
    let foreign_claim = bind_claim("beta");
    let foreign_lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            &members,
            &members[1],
            &launch_claim,
            foreign_claim,
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("foreign generation fixture should claim its own member");
    let claimed_bytes = authority_bytes(root.path());
    authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &members[..1],
            &members[0],
            &confirmed_stopped,
            &rebind_claim,
            confirmed_stopped.clone(),
            &rebind_lifetime,
        )
        .expect_err("incomplete adoption witness must fail");
    authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &binding("alpha", PORT + 1),
            &rebind_claim,
            confirmed_stopped.clone(),
            &rebind_lifetime,
        )
        .expect_err("crossed retained receipt must fail");
    authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            &rebind_claim,
            binding("alpha", PORT + 1),
            &rebind_lifetime,
        )
        .expect_err("crossed actual binding must fail");
    authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            &members,
            &members[0],
            &confirmed_stopped,
            &rebind_claim,
            confirmed_stopped.clone(),
            &foreign_lifetime,
        )
        .expect_err("crossed member lifetime generation must fail");
    assert_eq!(authority_bytes(root.path()), claimed_bytes);
}

#[test]
fn planned_subset_claim_and_abandon_reject_crossed_authority_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let members = [
        planned_request_for("alpha", PORT),
        planned_request_for("beta", PORT + 1),
        planned_request_for("gamma", PORT + 2),
    ];
    let launch_claim = reservation_claim("planned-subset-launch");
    authority
        .reserve_batch_for_coordinator(members.to_vec(), &launch_claim)
        .expect("complete planned launch should reserve");
    let subset_claims = [
        (members[0].clone(), bind_claim("alpha")),
        (members[1].clone(), bind_claim("beta")),
    ];
    let before = authority_bytes(root.path());

    let omitted = members[..2].to_vec();
    let duplicate = [members[0].clone(), members[1].clone(), members[1].clone()];
    for witness in [&omitted[..], &duplicate[..]] {
        authority
            .claim_bind_plan_members_with_lifetimes(
                witness,
                &subset_claims,
                &launch_claim,
                PortLeaseEffectScope::ProviderManaged,
            )
            .expect_err("crossed complete witness must fail before subset mutation");
        assert_eq!(authority_bytes(root.path()), before);
    }
    let crossed_claim = reservation_claim("crossed-launch");
    authority
        .claim_bind_plan_members_with_lifetimes(
            &members,
            &subset_claims,
            &crossed_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect_err("crossed reservation claim must fail before subset mutation");
    assert_eq!(authority_bytes(root.path()), before);

    let lifetimes = authority
        .claim_bind_plan_members_with_lifetimes(
            &members,
            &subset_claims,
            &launch_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("exact subset should claim");
    let claimed = authority_bytes(root.path());
    authority
        .abandon_bind_plan_members_with_lifetimes_without_effect(
            &omitted,
            &subset_claims,
            &launch_claim,
            &lifetimes,
        )
        .expect_err("crossed witness must fail before subset abandonment");
    assert_eq!(authority_bytes(root.path()), claimed);
    authority
        .abandon_bind_plan_members_with_lifetimes_without_effect(
            &members,
            &subset_claims,
            &launch_claim,
            &lifetimes,
        )
        .expect("exact no-effect subset should abandon atomically");
    for member in &members[..2] {
        let record = authority
            .inspect(member.lease_id())
            .expect("published member should inspect")
            .expect("published member should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(record.bind_claim().is_none());
        assert!(record.active_lifetime().is_none());
    }
    let sibling = authority
        .inspect(members[2].lease_id())
        .expect("sibling should inspect")
        .expect("sibling should remain durable");
    assert_eq!(sibling.phase(), PortLeasePhase::Reserved);
    assert!(sibling.bind_claim().is_none());
}
