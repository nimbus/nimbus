use super::*;

pub(super) fn three_route_scenario() -> PpscScenario {
    PpscScenario::new(
        "three-production-routes",
        401,
        vec![
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-hot".to_string(),
                    route: PpscRoute::QueuedJournal,
                    key: "queued".to_string(),
                    value: 1,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-hot".to_string(),
                    route: PpscRoute::Direct,
                    key: "direct".to_string(),
                    value: 2,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-hot".to_string(),
                    route: PpscRoute::ExecutionUnit,
                    key: "execution-unit".to_string(),
                    value: 3,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-peer".to_string(),
                    route: PpscRoute::QueuedJournal,
                    key: "peer".to_string(),
                    value: 4,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("three-route scenario should build")
}

pub(super) fn internal_durable_jobs_scenario() -> PpscScenario {
    let tenant = "ppsc-internal".to_string();
    let restore_tenant = "ppsc-restore".to_string();
    PpscScenario::new(
        "internal-durable-jobs",
        409,
        vec![
            PpscStep::new(
                PpscOperation::SchemaSet {
                    tenant: tenant.clone(),
                    revision: 41,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Schedule {
                    tenant: tenant.clone(),
                    job: 42,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::TriggerCursorAdvance {
                    tenant: tenant.clone(),
                    through: 1,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::ProjectionUpdate {
                    tenant: tenant.clone(),
                    revision: 43,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::SchemaDelete { tenant },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::RestoreImport {
                    tenant: restore_tenant,
                    archive: 44,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("internal durable-jobs scenario should build")
}

pub(super) fn mutation_edge_scenario(order: PpscCommitOrder) -> PpscScenario {
    let tenant = "ppsc-mutation-edges".to_string();
    PpscScenario::new(
        format!("mutation-edges-{order:?}"),
        419,
        vec![
            PpscStep::new(
                PpscOperation::CommitPermutation {
                    tenant: tenant.clone(),
                    order,
                    value_base: 100,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::ZeroWriteExecutionUnit {
                    tenant: tenant.clone(),
                },
                PpscExpectedOutcome::Observed,
            ),
            PpscStep::new(
                PpscOperation::ConflictRetry {
                    tenant,
                    key: "shared".to_string(),
                    first: 201,
                    second: 202,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("mutation-edge scenario should build")
}

pub(super) fn storage_fault_scenario() -> PpscScenario {
    let hot = "ppsc-fault-hot".to_string();
    let peer = "ppsc-fault-peer".to_string();
    PpscScenario::new(
        "storage-fault-recovery",
        431,
        vec![
            PpscStep::new(
                PpscOperation::ArmFault {
                    tenant: hot.clone(),
                    fault: PpscInjectedFault::AcknowledgementLoss,
                },
                PpscExpectedOutcome::Observed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: hot.clone(),
                    route: PpscRoute::QueuedJournal,
                    key: "acknowledgement-loss".to_string(),
                    value: 301,
                },
                PpscExpectedOutcome::AmbiguousRecovered,
            ),
            PpscStep::new(
                PpscOperation::ArmFault {
                    tenant: hot.clone(),
                    fault: PpscInjectedFault::ProviderTransient,
                },
                PpscExpectedOutcome::Observed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: hot.clone(),
                    route: PpscRoute::QueuedJournal,
                    key: "provider-transient".to_string(),
                    value: 302,
                },
                PpscExpectedOutcome::ProviderError,
            ),
            PpscStep::new(
                PpscOperation::ReleaseFault {
                    tenant: hot.clone(),
                    fault: PpscInjectedFault::ProviderTransient,
                },
                PpscExpectedOutcome::Observed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: hot,
                    route: PpscRoute::QueuedJournal,
                    key: "after-release".to_string(),
                    value: 303,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: peer,
                    route: PpscRoute::ExecutionUnit,
                    key: "peer-progress".to_string(),
                    value: 304,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("storage-fault scenario should build")
}

pub(super) fn cancellation_overload_scenario() -> PpscScenario {
    let hot = "ppsc-pressure-hot".to_string();
    let peer = "ppsc-pressure-peer".to_string();
    PpscScenario::new(
        "cancellation-overload-isolation",
        433,
        vec![
            PpscStep::new(
                PpscOperation::CancelNext {
                    tenant: hot.clone(),
                    route: PpscRoute::ExecutionUnit,
                },
                PpscExpectedOutcome::Cancelled,
            ),
            PpscStep::new(
                PpscOperation::CancelNext {
                    tenant: hot.clone(),
                    route: PpscRoute::QueuedJournal,
                },
                PpscExpectedOutcome::Cancelled,
            ),
            PpscStep::new(
                PpscOperation::ForceOverload {
                    tenant: hot.clone(),
                },
                PpscExpectedOutcome::Overloaded,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: hot,
                    route: PpscRoute::QueuedJournal,
                    key: "after-pressure".to_string(),
                    value: 311,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: peer,
                    route: PpscRoute::ExecutionUnit,
                    key: "peer-progress".to_string(),
                    value: 312,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("cancellation/overload scenario should build")
}

pub(super) fn settled_restart_scenario() -> PpscScenario {
    let tenant = "ppsc-reopen".to_string();
    PpscScenario::new(
        "durable-settled-restart",
        439,
        vec![
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: tenant.clone(),
                    route: PpscRoute::QueuedJournal,
                    key: "before-restart".to_string(),
                    value: 321,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::SettledRestart, PpscExpectedOutcome::Observed),
            PpscStep::new(PpscOperation::Reopen, PpscExpectedOutcome::Observed),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant,
                    route: PpscRoute::ExecutionUnit,
                    key: "after-reopen".to_string(),
                    value: 322,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("settled-restart scenario should build")
}

pub(super) fn commit_phase_fault_scenario() -> PpscScenario {
    let hot = "ppsc-commit-phase-hot".to_string();
    let peer = "ppsc-commit-phase-peer".to_string();
    PpscScenario::new(
        "commit-phase-faults",
        443,
        vec![
            PpscStep::new(
                PpscOperation::PublicationPredecessorRace {
                    tenant: hot.clone(),
                    predecessor_route: PpscRoute::QueuedJournal,
                    successor_route: PpscRoute::Direct,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::CommitPhaseFault {
                    tenant: hot.clone(),
                    route: PpscRoute::ExecutionUnit,
                    fault: PpscInjectedFault::DurableBeforePublish,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::CommitPhaseFault {
                    tenant: hot.clone(),
                    route: PpscRoute::QueuedJournal,
                    fault: PpscInjectedFault::PanicAfterDurable,
                },
                PpscExpectedOutcome::AmbiguousRecovered,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: hot,
                    route: PpscRoute::Direct,
                    key: "after-recovery".to_string(),
                    value: 431,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: peer,
                    route: PpscRoute::ExecutionUnit,
                    key: "peer-progress".to_string(),
                    value: 432,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("commit-phase fault scenario should build")
}
