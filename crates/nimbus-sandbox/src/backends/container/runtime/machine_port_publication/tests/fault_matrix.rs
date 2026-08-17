//! Deterministic fail-Nth and ambiguity matrix for machine publication batches.

use super::*;

fn fixture_for(action: MachinePortPublicationAction) -> (PublicationFixture, StatefulProvider) {
    let fixture = PublicationFixture::new(bindings());
    let provider_config = fixture
        .manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("fixture provider should exist");
    let initially_exposed = action == MachinePortPublicationAction::Withdraw;
    let provider = StatefulProvider::new(
        provider_config,
        &fixture.expectation.bindings,
        initially_exposed,
    );
    if initially_exposed {
        publish_record(
            &fixture.manifest.runner_config.workload_state_root,
            &fixture.manifest.conmon_layout.container_state_dir,
            fixture.record(
                MachinePortPublicationPhase::Exposed,
                fixture.exposed_receipts(),
            ),
        )
        .expect("withdrawal fixture should start with exact Exposed evidence");
    }
    (fixture, provider)
}

fn assert_terminal(
    fixture: &PublicationFixture,
    action: MachinePortPublicationAction,
    generation: u64,
) {
    let record = read_record(&fixture.manifest.conmon_layout.container_state_dir)
        .expect("terminal publication record should reopen");
    assert_eq!(record.phase, action.terminal_phase());
    assert_eq!(record.batch_generation, generation);
    assert_eq!(record.slots.len(), fixture.expectation.bindings.len());
    assert!(record.slots.iter().all(|slot| match action {
        MachinePortPublicationAction::Expose =>
            matches!(slot, MachinePortPublicationSlot::ObservedExposed(_)),
        MachinePortPublicationAction::Withdraw =>
            matches!(slot, MachinePortPublicationSlot::ObservedAbsent(_)),
    }));
}

fn sorted(mut values: Vec<usize>) -> Vec<usize> {
    values.sort_unstable();
    values
}

#[test]
fn nnc5_4a_fail_nth_mutation_preserves_siblings_and_publishes_only_complete_batches() {
    for action in [
        MachinePortPublicationAction::Expose,
        MachinePortPublicationAction::Withdraw,
    ] {
        for failed_index in 0..bindings().len() {
            let (fixture, provider) = fixture_for(action);
            match action {
                MachinePortPublicationAction::Expose => {
                    provider.fail_expose_before_effect(failed_index);
                }
                MachinePortPublicationAction::Withdraw => {
                    provider.fail_withdraw_before_effect(failed_index);
                }
            }

            let error = fixture
                .backend
                .converge_machine_port_publication(&fixture.manifest, &provider, action)
                .expect_err("fail-Nth mutation should retain an in-progress batch");
            assert!(
                error
                    .to_string()
                    .contains(&format!("{} batch retained", action.label())),
                "{action:?} slot {failed_index} must expose the batch diagnostic: {error}"
            );
            let in_progress = read_record(&fixture.manifest.conmon_layout.container_state_dir)
                .expect("failed batch should remain durable");
            assert_eq!(in_progress.phase, action.in_progress_phase());
            assert_eq!(
                in_progress.slots[failed_index],
                MachinePortPublicationSlot::EffectMayExist
            );
            for (index, slot) in in_progress.slots.iter().enumerate() {
                if index == failed_index {
                    continue;
                }
                assert!(
                    match action {
                        MachinePortPublicationAction::Expose =>
                            matches!(slot, MachinePortPublicationSlot::ObservedExposed(_)),
                        MachinePortPublicationAction::Withdraw =>
                            matches!(slot, MachinePortPublicationSlot::ObservedAbsent(_)),
                    },
                    "{action:?} slot {index} must remain an independently satisfied sibling"
                );
            }

            fixture
                .backend
                .converge_machine_port_publication(&fixture.manifest, &provider, action)
                .expect("retry should converge only the failed slot");
            let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
            match action {
                MachinePortPublicationAction::Expose => {
                    assert_eq!(sorted(expose_mutations), vec![0, 1]);
                    assert!(withdraw_mutations.is_empty());
                    assert_eq!(exposed, vec![true, true]);
                    assert_terminal(&fixture, action, 1);
                }
                MachinePortPublicationAction::Withdraw => {
                    assert!(expose_mutations.is_empty());
                    assert_eq!(sorted(withdraw_mutations), vec![0, 1]);
                    assert_eq!(exposed, vec![false, false]);
                    assert_terminal(&fixture, action, 2);
                }
            }
        }
    }
}

#[test]
fn nnc5_4a_response_loss_at_every_slot_converges_from_current_observation() {
    for action in [
        MachinePortPublicationAction::Expose,
        MachinePortPublicationAction::Withdraw,
    ] {
        for lost_index in 0..bindings().len() {
            let (fixture, provider) = fixture_for(action);
            match action {
                MachinePortPublicationAction::Expose => {
                    provider.lose_expose_response(lost_index);
                }
                MachinePortPublicationAction::Withdraw => {
                    provider.lose_withdraw_response(lost_index);
                }
            }

            fixture
                .backend
                .converge_machine_port_publication(&fixture.manifest, &provider, action)
                .unwrap_or_else(|error| {
                    panic!(
                        "{action:?} response loss at slot {lost_index} must converge from exact \
                         current observation: {error}"
                    )
                });
            let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
            match action {
                MachinePortPublicationAction::Expose => {
                    assert_eq!(expose_mutations, vec![0, 1]);
                    assert!(withdraw_mutations.is_empty());
                    assert_eq!(exposed, vec![true, true]);
                    assert_terminal(&fixture, action, 1);
                }
                MachinePortPublicationAction::Withdraw => {
                    assert!(expose_mutations.is_empty());
                    assert_eq!(withdraw_mutations, vec![0, 1]);
                    assert_eq!(exposed, vec![false, false]);
                    assert_terminal(&fixture, action, 2);
                }
            }
        }
    }
}

#[test]
fn nnc5_4a_post_effect_inspection_loss_reopens_without_duplicate_mutation() {
    for action in [
        MachinePortPublicationAction::Expose,
        MachinePortPublicationAction::Withdraw,
    ] {
        for lost_index in 0..bindings().len() {
            let (fixture, provider) = fixture_for(action);
            let post_effect_inspection = if lost_index == 0 { 1 } else { 3 };
            provider.fail_next_inspections([post_effect_inspection]);

            let error = fixture
                .backend
                .converge_machine_port_publication(&fixture.manifest, &provider, action)
                .expect_err("lost post-effect inspection must retain ambiguity");
            assert!(
                error.to_string().contains(&format!(
                    "scripted inspection {post_effect_inspection} failed"
                )),
                "{action:?} slot {lost_index} must retain the exact inspection failure: {error}"
            );
            let ambiguous = read_record(&fixture.manifest.conmon_layout.container_state_dir)
                .expect("ambiguous slot should remain durable");
            assert_eq!(
                ambiguous.slots[lost_index],
                MachinePortPublicationSlot::EffectMayExist
            );

            fixture
                .backend
                .converge_machine_port_publication(&fixture.manifest, &provider, action)
                .expect("fresh inspection should observe the existing effect and converge");
            let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
            match action {
                MachinePortPublicationAction::Expose => {
                    assert_eq!(expose_mutations, vec![0, 1]);
                    assert!(withdraw_mutations.is_empty());
                    assert_eq!(exposed, vec![true, true]);
                    assert_terminal(&fixture, action, 1);
                }
                MachinePortPublicationAction::Withdraw => {
                    assert!(expose_mutations.is_empty());
                    assert_eq!(withdraw_mutations, vec![0, 1]);
                    assert_eq!(exposed, vec![false, false]);
                    assert_terminal(&fixture, action, 2);
                }
            }
        }
    }
}

#[test]
fn nnc5_4a_unknown_conflict_and_ambiguous_diagnostics_fail_closed() {
    for mode in ["unknown", "conflict"] {
        let (fixture, provider) = fixture_for(MachinePortPublicationAction::Expose);
        match mode {
            "unknown" => provider.fail_next_inspections([0]),
            "conflict" => provider.conflict_slot(0),
            _ => unreachable!(),
        }

        let error = fixture
            .backend
            .converge_machine_port_publication(
                &fixture.manifest,
                &provider,
                MachinePortPublicationAction::Expose,
            )
            .expect_err("unknown or conflicting evidence must fence");
        assert!(
            error.to_string().contains(if mode == "unknown" {
                "scripted inspection 0 failed"
            } else {
                "conflicting"
            }),
            "{mode} must retain its precise provider-evidence diagnostic: {error}"
        );
        let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
        assert!(expose_mutations.is_empty());
        assert!(withdraw_mutations.is_empty());
        assert_eq!(exposed, vec![false, false]);
        let durable = read_record(&fixture.manifest.conmon_layout.container_state_dir)
            .expect("fenced batch must remain durable");
        assert_eq!(durable.phase, MachinePortPublicationPhase::Exposing);
        assert!(
            durable
                .slots
                .iter()
                .all(|slot| *slot == MachinePortPublicationSlot::Pending)
        );
    }

    let (fixture, provider) = fixture_for(MachinePortPublicationAction::Expose);
    provider.lose_expose_response(0);
    provider.fail_next_inspections([1]);
    let error = fixture
        .backend
        .converge_machine_port_publication(
            &fixture.manifest,
            &provider,
            MachinePortPublicationAction::Expose,
        )
        .expect_err("lost mutation response plus lost inspection must remain ambiguous");
    let message = error.to_string();
    assert!(
        message.contains("scripted expose 0 response lost after effect")
            && message.contains("scripted inspection 1 failed"),
        "primary mutation and recovery-inspection diagnostics must both survive: {message}"
    );
    let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
    assert_eq!(expose_mutations, vec![0]);
    assert!(withdraw_mutations.is_empty());
    assert_eq!(exposed, vec![true, false]);
    let durable = read_record(&fixture.manifest.conmon_layout.container_state_dir)
        .expect("ambiguous response-loss record must remain durable");
    assert_eq!(durable.slots[0], MachinePortPublicationSlot::EffectMayExist);
}
