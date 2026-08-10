use super::*;

#[test]
fn forwarded_adapter_implements_all_five_exact_capabilities() {
    fn assert_capabilities<T>()
    where
        T: FinalIngressWithdrawalCapability
            + WorkloadExecutionDrainCapability
            + WorkloadExecutionStopCapability
            + NetworkDetachmentCapability
            + NetworkReleaseCapability,
    {
    }

    assert_capabilities::<ForwardedMachineTeardownAdapter>();
}

#[test]
fn parent_publication_must_be_withdrawn_before_every_guest_phase() {
    for phase in [
        ConfirmedMachinePublicationRetirementPhase::Active,
        ConfirmedMachinePublicationRetirementPhase::WithdrawalMayExist,
    ] {
        for step in [
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownStep::ReleaseNetwork,
        ] {
            assert!(
                validate_retirement_order(step, phase).is_err(),
                "{step:?} must reject parent phase {phase:?}"
            );
        }
    }

    for phase in [
        ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained,
        ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist,
        ConfirmedMachinePublicationRetirementPhase::Released,
    ] {
        for step in [
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownStep::ReleaseNetwork,
        ] {
            validate_retirement_order(step, phase)
                .unwrap_or_else(|_| panic!("{step:?} must accept parent phase {phase:?}"));
        }
    }
}

#[test]
fn parent_local_withdrawal_can_replay_every_exact_phase() {
    for phase in [
        ConfirmedMachinePublicationRetirementPhase::Active,
        ConfirmedMachinePublicationRetirementPhase::WithdrawalMayExist,
        ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained,
        ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist,
        ConfirmedMachinePublicationRetirementPhase::Released,
    ] {
        validate_retirement_order(WorkloadTeardownStep::WithdrawPublication, phase)
            .unwrap_or_else(|_| panic!("withdrawal replay must accept parent phase {phase:?}"));
    }
}
