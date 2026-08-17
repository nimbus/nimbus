use super::*;

use std::collections::BTreeSet;
use std::fs;
use std::net::TcpListener;
use std::sync::Mutex;

use crate::backend::SandboxBackendKind;
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::backends::container::ContainerSandboxBackendConfig;
use crate::backends::oci::network::{
    AttachmentBackendKind, CurrentMachinePortForwardingObservation, MachinePortMutationDiagnostic,
    default_network_attachment_id, oci_attachment_plan,
};
use crate::spec::{
    SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
};

mod fault_matrix;
mod store_faults;

struct PublicationFixture {
    _temp: tempfile::TempDir,
    backend: ContainerSandboxBackend,
    manifest: ContainerSandboxManifest,
    expectation: MachinePortPublicationExpectation,
}

impl PublicationFixture {
    fn new(bindings: Vec<SandboxPortBinding>) -> Self {
        let temp = tempfile::tempdir().expect("temporary root should exist");
        let listener = TcpListener::bind("127.0.0.1:0").expect("provider fixture should bind");
        let port = listener
            .local_addr()
            .expect("provider fixture address should resolve")
            .port();
        drop(listener);
        let forwarder = OciMachinePortForwarderConfig::for_provider_instance(
            "127.0.0.1",
            port,
            "/services/forwarder",
            "machine-publication-tests",
            NetworkResourceGeneration::new(17),
        )
        .expect("provider fixture should validate");
        let mut config = ContainerSandboxBackendConfig::under_root(temp.path());
        config.machine_port_forwarder = Some(forwarder);
        let backend = ContainerSandboxBackend::new(config);
        let tenant_id =
            TenantId::new("tenant-machine-publication").expect("tenant should validate");
        let spec = SandboxSpec::new(
            tenant_id,
            SandboxOwnerSpec::service("machine-publication"),
            SandboxBackendKind::Container,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/tmp/rootfs")),
            SandboxProcessSpec::new(["/bin/true"]),
        )
        .with_port_bindings(bindings);
        let manifest = backend
            .plan_start_with_id(
                &spec,
                &SandboxId::new("machine-publication-state"),
                None,
                None,
            )
            .expect("publication fixture should plan")
            .manifest;
        let network_config = manifest
            .network_config
            .as_ref()
            .expect("publication fixture should carry placed network authority");
        let attachment_id = default_network_attachment_id(&manifest.handle.id);
        let reservation = backend
            .segment_allocator
            .inspect_attachment_reservation(
                &manifest.spec.tenant_id,
                &attachment_id,
                &network_config.reservation_claim,
            )
            .expect("fixture reservation should inspect");
        let association = reservation
            .association()
            .expect("fixture reservation should carry its exact association")
            .clone();
        let attachment_plan = oci_attachment_plan(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            AttachmentBackendKind::Container,
        );
        backend
            .attachment_authority
            .as_ref()
            .expect("fixture attachment authority should initialize")
            .reserve(
                &manifest.spec.tenant_id,
                host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container),
                &attachment_plan,
                attachment_id,
                association,
            )
            .expect("fixture attachment authority should reserve");
        let ports = backend
            .port_lease_coordinator_for_manifest(&manifest)
            .expect("fixture port authority should open");
        let claims = ports
            .claim_machine_bindings(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
            )
            .expect("fixture listener claims should prepare");
        ports
            .activate_machine_bindings(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                &claims,
            )
            .expect("fixture listener bindings should activate");
        let provider = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("fixture should retain provider");
        let expectation = MachinePortPublicationExpectation::from_manifest_for_record_test(
            &backend, &manifest, provider,
        )
        .expect("fixture expectation should authenticate");
        Self {
            _temp: temp,
            backend,
            manifest,
            expectation,
        }
    }

    fn exposed_receipts(&self) -> Vec<MachinePortForwardReceipt> {
        self.expectation
            .bindings
            .iter()
            .map(|binding| MachinePortForwardReceipt {
                outcome: MachinePortForwardOutcome::Exposed,
                tenant_id: self.expectation.tenant_id.clone(),
                sandbox_id: self.expectation.sandbox_id.clone(),
                binding: binding.clone(),
                provider_instance: self.expectation.provider_instance.clone(),
                provider_generation: self.expectation.provider_generation,
            })
            .collect()
    }

    fn absent_receipts(&self) -> Vec<MachinePortForwardReceipt> {
        self.expectation
            .bindings
            .iter()
            .map(|binding| MachinePortForwardReceipt {
                outcome: MachinePortForwardOutcome::ExactAlreadyAbsent,
                tenant_id: self.expectation.tenant_id.clone(),
                sandbox_id: self.expectation.sandbox_id.clone(),
                binding: binding.clone(),
                provider_instance: self.expectation.provider_instance.clone(),
                provider_generation: self.expectation.provider_generation,
            })
            .collect()
    }

    fn record(
        &self,
        phase: MachinePortPublicationPhase,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> MachinePortPublicationRecord {
        let slots = receipts
            .into_iter()
            .map(|receipt| match phase {
                MachinePortPublicationPhase::Exposed => {
                    MachinePortPublicationSlot::ObservedExposed(receipt)
                }
                MachinePortPublicationPhase::Absent => {
                    MachinePortPublicationSlot::ObservedAbsent(receipt)
                }
                MachinePortPublicationPhase::Exposing
                | MachinePortPublicationPhase::Withdrawing => unreachable!(),
            })
            .collect();
        MachinePortPublicationRecord::new(self.expectation.clone(), phase, 1, slots)
    }
}

fn bindings() -> Vec<SandboxPortBinding> {
    vec![
        SandboxPortBinding::tcp("http", 18_080, 8_080),
        SandboxPortBinding::tcp("metrics", 19_090, 9_090),
    ]
}

struct StatefulProvider {
    config: OciMachinePortForwarderConfig,
    bindings: Vec<SandboxPortBinding>,
    state: Mutex<StatefulProviderState>,
}

struct StatefulProviderState {
    exposed: Vec<bool>,
    inspections: usize,
    expose_mutations: Vec<usize>,
    withdraw_mutations: Vec<usize>,
    failed_inspections: BTreeSet<usize>,
    fail_expose_before_effect: BTreeSet<usize>,
    fail_withdraw_before_effect: BTreeSet<usize>,
    lose_expose_response: BTreeSet<usize>,
    lose_withdraw_response: BTreeSet<usize>,
    conflicting_slots: BTreeSet<usize>,
}

impl StatefulProvider {
    fn new(
        config: &OciMachinePortForwarderConfig,
        bindings: &[SandboxPortBinding],
        exposed: bool,
    ) -> Self {
        Self {
            config: config.clone(),
            bindings: bindings.to_vec(),
            state: Mutex::new(StatefulProviderState {
                exposed: vec![exposed; bindings.len()],
                inspections: 0,
                expose_mutations: Vec::new(),
                withdraw_mutations: Vec::new(),
                failed_inspections: BTreeSet::new(),
                fail_expose_before_effect: BTreeSet::new(),
                fail_withdraw_before_effect: BTreeSet::new(),
                lose_expose_response: BTreeSet::new(),
                lose_withdraw_response: BTreeSet::new(),
                conflicting_slots: BTreeSet::new(),
            }),
        }
    }

    fn snapshot(&self) -> (usize, Vec<usize>, Vec<usize>, Vec<bool>) {
        let state = self.state.lock().expect("provider state should lock");
        (
            state.inspections,
            state.expose_mutations.clone(),
            state.withdraw_mutations.clone(),
            state.exposed.clone(),
        )
    }

    fn fail_next_inspections(&self, ordinals: impl IntoIterator<Item = usize>) {
        self.state
            .lock()
            .expect("provider state should lock")
            .failed_inspections
            .extend(ordinals);
    }

    fn fail_expose_before_effect(&self, index: usize) {
        self.state
            .lock()
            .expect("provider state should lock")
            .fail_expose_before_effect
            .insert(index);
    }

    fn fail_withdraw_before_effect(&self, index: usize) {
        self.state
            .lock()
            .expect("provider state should lock")
            .fail_withdraw_before_effect
            .insert(index);
    }

    fn lose_expose_response(&self, index: usize) {
        self.state
            .lock()
            .expect("provider state should lock")
            .lose_expose_response
            .insert(index);
    }

    fn lose_withdraw_response(&self, index: usize) {
        self.state
            .lock()
            .expect("provider state should lock")
            .lose_withdraw_response
            .insert(index);
    }

    fn conflict_slot(&self, index: usize) {
        self.state
            .lock()
            .expect("provider state should lock")
            .conflicting_slots
            .insert(index);
    }
}

impl MachinePortForwardingProvider for StatefulProvider {
    fn provider_instance(&self) -> &NetworkProviderHandle {
        self.config.provider_instance()
    }

    fn provider_generation(&self) -> NetworkResourceGeneration {
        self.config.provider_generation()
    }

    fn inspect(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
    ) -> Result<CurrentMachinePortForwardingObservation> {
        assert_eq!(bindings, self.bindings);
        let mut state = self.state.lock().expect("provider state should lock");
        let inspection = state.inspections;
        state.inspections += 1;
        if state.failed_inspections.remove(&inspection) {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted inspection {inspection} failed"),
            });
        }
        let slots = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| {
                if state.conflicting_slots.contains(&index) {
                    return MachinePortForwardingSlotObservation::Conflicting {
                        binding: binding.clone(),
                        detail: format!("scripted conflicting slot {index}"),
                    };
                }
                let exposed = state.exposed[index];
                let receipt = MachinePortForwardReceipt {
                    outcome: if exposed {
                        MachinePortForwardOutcome::Exposed
                    } else {
                        MachinePortForwardOutcome::ExactAlreadyAbsent
                    },
                    tenant_id: tenant_id.clone(),
                    sandbox_id: sandbox_id.clone(),
                    binding: binding.clone(),
                    provider_instance: self.provider_instance().clone(),
                    provider_generation: self.provider_generation(),
                };
                if exposed {
                    MachinePortForwardingSlotObservation::Exposed(receipt)
                } else {
                    MachinePortForwardingSlotObservation::Absent(receipt)
                }
            })
            .collect();
        Ok(CurrentMachinePortForwardingObservation::authenticated(
            self.provider_instance(),
            self.provider_generation(),
            slots,
        ))
    }

    fn expose_one(&self, binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic> {
        let index = self
            .bindings
            .iter()
            .position(|candidate| candidate == binding)
            .expect("exposed binding should be canonical");
        let mut state = self.state.lock().expect("provider state should lock");
        if state.fail_expose_before_effect.remove(&index) {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted expose {index} failed before effect"),
            });
        }
        state.expose_mutations.push(index);
        state.exposed[index] = true;
        if state.lose_expose_response.remove(&index) {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted expose {index} response lost after effect"),
            });
        }
        Ok(MachinePortMutationDiagnostic::accepted())
    }

    fn withdraw_one(&self, binding: &SandboxPortBinding) -> Result<MachinePortMutationDiagnostic> {
        let index = self
            .bindings
            .iter()
            .position(|candidate| candidate == binding)
            .expect("withdrawn binding should be canonical");
        let mut state = self.state.lock().expect("provider state should lock");
        if state.fail_withdraw_before_effect.remove(&index) {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted withdrawal {index} failed before effect"),
            });
        }
        state.withdraw_mutations.push(index);
        state.exposed[index] = false;
        if state.lose_withdraw_response.remove(&index) {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted withdrawal {index} response lost after effect"),
            });
        }
        Ok(MachinePortMutationDiagnostic::accepted())
    }
}

struct FailAtCheckpoint {
    target: MachinePortPublicationCheckpoint,
}

impl MachinePortPublicationObserver for FailAtCheckpoint {
    fn checkpoint(&mut self, checkpoint: MachinePortPublicationCheckpoint) -> Result<()> {
        if checkpoint == self.target {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted acknowledgement loss at {checkpoint:?}"),
            });
        }
        Ok(())
    }
}

fn checkpoint_for(
    action: MachinePortPublicationAction,
    generation: u64,
    index: Option<usize>,
    boundary: &str,
) -> MachinePortPublicationCheckpoint {
    match (boundary, index) {
        ("batch-prepared", None) => {
            MachinePortPublicationCheckpoint::BatchPrepared { action, generation }
        }
        ("slot-effect-prepared", Some(index)) => {
            MachinePortPublicationCheckpoint::SlotEffectPrepared {
                action,
                generation,
                index,
            }
        }
        ("slot-effect-returned", Some(index)) => {
            MachinePortPublicationCheckpoint::SlotEffectReturned {
                action,
                generation,
                index,
            }
        }
        ("slot-observed", Some(index)) => MachinePortPublicationCheckpoint::SlotObserved {
            action,
            generation,
            index,
        },
        ("batch-terminal", None) => {
            MachinePortPublicationCheckpoint::BatchTerminal { action, generation }
        }
        _ => panic!("invalid publication checkpoint boundary {boundary:?} index {index:?}"),
    }
}

fn assert_checkpoint_record(
    record: &MachinePortPublicationRecord,
    action: MachinePortPublicationAction,
    boundary: &str,
    index: Option<usize>,
) {
    let observed = |slot: &MachinePortPublicationSlot| match action {
        MachinePortPublicationAction::Expose => {
            matches!(slot, MachinePortPublicationSlot::ObservedExposed(_))
        }
        MachinePortPublicationAction::Withdraw => {
            matches!(slot, MachinePortPublicationSlot::ObservedAbsent(_))
        }
    };
    match (boundary, index) {
        ("batch-prepared", None) => {
            assert_eq!(record.phase, action.in_progress_phase());
            assert!(
                record
                    .slots
                    .iter()
                    .all(|slot| *slot == MachinePortPublicationSlot::Pending)
            );
        }
        ("slot-effect-prepared" | "slot-effect-returned", Some(index)) => {
            assert_eq!(record.phase, action.in_progress_phase());
            assert!(record.slots[..index].iter().all(observed));
            assert_eq!(
                record.slots[index],
                MachinePortPublicationSlot::EffectMayExist
            );
            assert!(
                record.slots[index + 1..]
                    .iter()
                    .all(|slot| *slot == MachinePortPublicationSlot::Pending)
            );
        }
        ("slot-observed", Some(index)) => {
            assert_eq!(record.phase, action.in_progress_phase());
            assert!(record.slots[..=index].iter().all(observed));
            assert!(
                record.slots[index + 1..]
                    .iter()
                    .all(|slot| *slot == MachinePortPublicationSlot::Pending)
            );
        }
        ("batch-terminal", None) => {
            assert_eq!(record.phase, action.terminal_phase());
            assert!(record.slots.iter().all(observed));
        }
        _ => panic!("invalid record assertion boundary {boundary:?} index {index:?}"),
    }
}

#[test]
fn batch_prepared_ack_loss_is_durable_and_precedes_provider_io() {
    let fixture = PublicationFixture::new(bindings());
    let provider_config = fixture
        .manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("fixture provider should exist");
    let provider = StatefulProvider::new(provider_config, &fixture.expectation.bindings, false);
    let mut observer = FailAtCheckpoint {
        target: MachinePortPublicationCheckpoint::BatchPrepared {
            action: MachinePortPublicationAction::Expose,
            generation: 1,
        },
    };

    let error = fixture
        .backend
        .converge_machine_port_publication_for_test_with_observer(
            &fixture.manifest,
            &provider,
            MachinePortPublicationAction::Expose,
            &mut observer,
        )
        .expect_err("lost batch-prepared acknowledgement should surface");
    assert!(
        error.to_string().contains("BatchPrepared"),
        "the injected boundary should surface exactly: {error}"
    );
    assert_eq!(
        provider.snapshot(),
        (0, Vec::new(), Vec::new(), vec![false, false]),
        "the exact Exposing batch must be durable before any provider I/O"
    );
    let durable = read_record(&fixture.manifest.conmon_layout.container_state_dir)
        .expect("prepared batch should reopen");
    assert_eq!(durable.phase, MachinePortPublicationPhase::Exposing);
    assert_eq!(durable.batch_generation, 1);
    assert!(
        durable
            .slots
            .iter()
            .all(|slot| *slot == MachinePortPublicationSlot::Pending)
    );

    fixture
        .backend
        .converge_machine_port_publication(
            &fixture.manifest,
            &provider,
            MachinePortPublicationAction::Expose,
        )
        .expect("fresh retry should converge the prepared batch");
    let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
    assert_eq!(expose_mutations, vec![0, 1]);
    assert!(withdraw_mutations.is_empty());
    assert_eq!(exposed, vec![true, true]);
}

#[test]
fn withdrawal_preparation_is_durable_before_listener_or_provider_effects() {
    let fixture = PublicationFixture::new(bindings());
    publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        &fixture.manifest.conmon_layout.container_state_dir,
        fixture.record(
            MachinePortPublicationPhase::Exposed,
            fixture.exposed_receipts(),
        ),
    )
    .expect("exact Exposed evidence should seed teardown");
    let mut observer = FailAtCheckpoint {
        target: MachinePortPublicationCheckpoint::BatchPrepared {
            action: MachinePortPublicationAction::Withdraw,
            generation: 2,
        },
    };

    let error = fixture
        .backend
        .prepare_machine_port_publication_withdrawal_for_test_with_observer(
            &fixture.manifest,
            &mut observer,
        )
        .expect_err("lost withdrawal-prepared acknowledgement should surface");
    assert!(
        error.to_string().contains("BatchPrepared"),
        "the exact pre-effect withdrawal boundary must surface: {error}"
    );
    let durable = read_record(&fixture.manifest.conmon_layout.container_state_dir)
        .expect("Withdrawing record should reopen");
    assert_eq!(durable.phase, MachinePortPublicationPhase::Withdrawing);
    assert_eq!(durable.batch_generation, 2);
    assert!(
        durable
            .slots
            .iter()
            .all(|slot| *slot == MachinePortPublicationSlot::Pending)
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&fixture.backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in &fixture.manifest.port_leases {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("listener lease should inspect")
                .expect("listener lease should remain durable")
                .phase(),
            nimbus_network::PortLeasePhase::Active,
            "Withdrawing must be durable before the listener generation changes"
        );
    }
}

#[test]
fn every_durable_effect_boundary_recovers_without_duplicate_provider_mutation() {
    for action in [
        MachinePortPublicationAction::Expose,
        MachinePortPublicationAction::Withdraw,
    ] {
        let generation = match action {
            MachinePortPublicationAction::Expose => 1,
            MachinePortPublicationAction::Withdraw => 2,
        };
        let mut cuts = vec![("batch-prepared", None), ("batch-terminal", None)];
        for index in 0..bindings().len() {
            cuts.extend([
                ("slot-effect-prepared", Some(index)),
                ("slot-effect-returned", Some(index)),
                ("slot-observed", Some(index)),
            ]);
        }

        for (boundary, index) in cuts {
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
            let target = checkpoint_for(action, generation, index, boundary);
            let mut observer = FailAtCheckpoint { target };

            let error = fixture
                .backend
                .converge_machine_port_publication_for_test_with_observer(
                    &fixture.manifest,
                    &provider,
                    action,
                    &mut observer,
                )
                .expect_err("scripted acknowledgement loss must surface");
            assert!(
                error.to_string().contains(&format!("{target:?}")),
                "{action:?} {boundary} {index:?} must surface the exact boundary: {error}"
            );
            let durable = read_record(&fixture.manifest.conmon_layout.container_state_dir)
                .expect("acknowledged durable boundary should reopen");
            assert_eq!(durable.batch_generation, generation);
            assert_checkpoint_record(&durable, action, boundary, index);

            fixture
                .backend
                .converge_machine_port_publication(&fixture.manifest, &provider, action)
                .unwrap_or_else(|error| {
                    panic!("{action:?} {boundary} {index:?} must converge on retry: {error}")
                });
            let (_, expose_mutations, withdraw_mutations, exposed) = provider.snapshot();
            let expected = vec![0, 1];
            match action {
                MachinePortPublicationAction::Expose => {
                    assert_eq!(
                        expose_mutations, expected,
                        "{boundary} {index:?} must expose each binding exactly once"
                    );
                    assert!(withdraw_mutations.is_empty());
                    assert_eq!(exposed, vec![true, true]);
                }
                MachinePortPublicationAction::Withdraw => {
                    assert!(expose_mutations.is_empty());
                    assert_eq!(
                        withdraw_mutations, expected,
                        "{boundary} {index:?} must withdraw each binding exactly once"
                    );
                    assert_eq!(exposed, vec![false, false]);
                }
            }
        }
    }
}

#[test]
fn terminal_publication_round_trips_complete_canonical_batches() {
    let fixture = PublicationFixture::new(bindings());
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    let root = &fixture.manifest.runner_config.workload_state_root;
    let exposed = fixture.record(
        MachinePortPublicationPhase::Exposed,
        fixture.exposed_receipts(),
    );

    publish_record(root, state_dir, exposed.clone()).expect("exposed batch should publish");
    let reloaded = read_record(state_dir).expect("exposed batch should reload");
    assert_eq!(reloaded, exposed);
    assert_eq!(
        terminal_receipts(&reloaded, MachinePortPublicationPhase::Exposed)
            .expect("terminal receipts should authenticate"),
        fixture.exposed_receipts()
    );

    let absent = MachinePortPublicationRecord::new(
        fixture.expectation.clone(),
        MachinePortPublicationPhase::Absent,
        2,
        fixture
            .absent_receipts()
            .into_iter()
            .map(MachinePortPublicationSlot::ObservedAbsent)
            .collect(),
    );
    publish_record(root, state_dir, absent.clone()).expect("absent batch should publish");
    assert_eq!(
        read_record(state_dir).expect("absent batch should reload"),
        absent
    );
}

#[test]
fn empty_batch_converges_and_terminal_replay_is_byte_stable() {
    let fixture = PublicationFixture::new(Vec::new());
    let provider = fixture
        .manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("fixture provider should exist");
    let exposed_provider = DeterministicMachinePortForwardingProvider::exposed(provider);
    let absent_provider = DeterministicMachinePortForwardingProvider::absent(provider);
    let evidence_path = fixture
        .manifest
        .conmon_layout
        .container_state_dir
        .join(MACHINE_PORT_EVIDENCE_FILE);

    assert!(
        fixture
            .backend
            .converge_machine_port_publication(
                &fixture.manifest,
                &exposed_provider,
                MachinePortPublicationAction::Expose,
            )
            .expect("empty exposure should converge")
            .is_empty()
    );
    let exposed_bytes = fs::read(&evidence_path).expect("empty exposed record should persist");
    fixture
        .backend
        .converge_machine_port_publication(
            &fixture.manifest,
            &exposed_provider,
            MachinePortPublicationAction::Expose,
        )
        .expect("empty exposed replay should be effect-free");
    assert_eq!(
        fs::read(&evidence_path).expect("empty exposed replay should remain readable"),
        exposed_bytes,
        "terminal exposed replay must preserve canonical bytes"
    );

    assert!(
        fixture
            .backend
            .converge_machine_port_publication(
                &fixture.manifest,
                &absent_provider,
                MachinePortPublicationAction::Withdraw,
            )
            .expect("empty withdrawal should converge")
            .is_empty()
    );
    let absent_bytes = fs::read(&evidence_path).expect("empty absent record should persist");
    fixture
        .backend
        .converge_machine_port_publication(
            &fixture.manifest,
            &absent_provider,
            MachinePortPublicationAction::Withdraw,
        )
        .expect("empty absent replay should be effect-free");
    assert_eq!(
        fs::read(&evidence_path).expect("empty absent replay should remain readable"),
        absent_bytes,
        "terminal absent replay must preserve canonical bytes"
    );
}

#[test]
fn action_preparation_is_exhaustive_and_generation_fenced() {
    let fixture = PublicationFixture::new(bindings());
    let record_for = |phase, generation| match phase {
        MachinePortPublicationPhase::Exposed => MachinePortPublicationRecord::new(
            fixture.expectation.clone(),
            phase,
            generation,
            fixture
                .exposed_receipts()
                .into_iter()
                .map(MachinePortPublicationSlot::ObservedExposed)
                .collect(),
        ),
        MachinePortPublicationPhase::Absent => MachinePortPublicationRecord::new(
            fixture.expectation.clone(),
            phase,
            generation,
            fixture
                .absent_receipts()
                .into_iter()
                .map(MachinePortPublicationSlot::ObservedAbsent)
                .collect(),
        ),
        MachinePortPublicationPhase::Exposing | MachinePortPublicationPhase::Withdrawing => {
            MachinePortPublicationRecord::new(
                fixture.expectation.clone(),
                phase,
                generation,
                vec![MachinePortPublicationSlot::Pending; fixture.expectation.bindings.len()],
            )
        }
    };

    for (existing, action, expected_phase, expected_generation) in [
        (
            None,
            MachinePortPublicationAction::Expose,
            MachinePortPublicationPhase::Exposing,
            1,
        ),
        (
            None,
            MachinePortPublicationAction::Withdraw,
            MachinePortPublicationPhase::Withdrawing,
            1,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Absent, 2)),
            MachinePortPublicationAction::Expose,
            MachinePortPublicationPhase::Exposing,
            3,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Absent, 2)),
            MachinePortPublicationAction::Withdraw,
            MachinePortPublicationPhase::Absent,
            2,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Exposing, 3)),
            MachinePortPublicationAction::Expose,
            MachinePortPublicationPhase::Exposing,
            3,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Exposing, 3)),
            MachinePortPublicationAction::Withdraw,
            MachinePortPublicationPhase::Withdrawing,
            4,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Exposed, 3)),
            MachinePortPublicationAction::Expose,
            MachinePortPublicationPhase::Exposed,
            3,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Exposed, 3)),
            MachinePortPublicationAction::Withdraw,
            MachinePortPublicationPhase::Withdrawing,
            4,
        ),
        (
            Some(record_for(MachinePortPublicationPhase::Withdrawing, 4)),
            MachinePortPublicationAction::Withdraw,
            MachinePortPublicationPhase::Withdrawing,
            4,
        ),
    ] {
        let prepared =
            MachinePortPublicationRecord::prepare(existing, &fixture.expectation, action)
                .expect("enumerated transition should be legal");
        assert_eq!(prepared.phase, expected_phase);
        assert_eq!(prepared.batch_generation, expected_generation);
        prepared
            .validate_self()
            .expect("every prepared state must remain internally valid");
    }

    assert!(
        MachinePortPublicationRecord::prepare(
            Some(record_for(MachinePortPublicationPhase::Withdrawing, 4)),
            &fixture.expectation,
            MachinePortPublicationAction::Expose,
        )
        .expect_err("exposure cannot cross an in-flight withdrawal")
        .to_string()
        .contains("still withdrawing")
    );
}

fn replacement_attachment_version(
    fixture: &PublicationFixture,
    attachment_id: NetworkAttachmentId,
    plan_id: nimbus_network::NetworkPlanId,
    generation: NetworkResourceGeneration,
    content_digest: nimbus_network::NetworkPlanContentDigest,
    lease_epoch: nimbus_network::NetworkLeaseEpoch,
) -> NetworkResourceVersion {
    let canonical = oci_attachment_plan(
        &fixture.expectation.tenant_id,
        &fixture.expectation.sandbox_id,
        AttachmentBackendKind::Container,
    );
    let plan = nimbus_network::NetworkPlan::new(
        plan_id,
        generation,
        content_digest,
        canonical.requirements().clone(),
    )
    .with_readiness_requirements(canonical.readiness_requirements().iter().cloned())
    .expect("substituted plan should preserve canonical readiness");
    NetworkResourceVersion::for_plan(
        &plan,
        NetworkResourceId::Attachment(attachment_id),
        lease_epoch,
    )
}

fn replacement_port_lease(
    request: &PortLeaseRequest,
    lease_id: nimbus_network::PortLeaseId,
    generation: NetworkResourceGeneration,
    lease_epoch: nimbus_network::NetworkLeaseEpoch,
) -> PortLeaseRequest {
    let replacement = PortLeaseRequest::new(
        lease_id,
        request.owner_id().clone(),
        request.tenant_id().cloned(),
        nimbus_network::PortLeaseFence::new(generation, lease_epoch),
        request.accounting(),
        request.publication().clone(),
        request.binding().clone(),
    );
    match request.plan_id() {
        Some(plan_id) => replacement.with_plan_id(plan_id.clone()),
        None => replacement,
    }
}

fn assert_semantic_substitution_fenced(
    fixture: &PublicationFixture,
    label: &str,
    candidate: MachinePortPublicationRecord,
) {
    candidate
        .validate_self()
        .unwrap_or_else(|error| panic!("{label} must be internally coherent: {error}"));
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        state_dir,
        candidate,
    )
    .unwrap_or_else(|error| panic!("{label} should publish through the strict envelope: {error}"));
    let before = fs::read(state_dir.join(MACHINE_PORT_EVIDENCE_FILE))
        .expect("substituted record bytes should read");
    let provider_config = fixture
        .manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("fixture provider should exist");
    let provider = StatefulProvider::new(provider_config, &fixture.expectation.bindings, true);

    let error = fixture
        .backend
        .converge_machine_port_publication(
            &fixture.manifest,
            &provider,
            MachinePortPublicationAction::Expose,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("crossed or stale"),
        "{label} must fail at exact durable identity authentication: {error}"
    );
    assert_eq!(
        provider.snapshot(),
        (0, Vec::new(), Vec::new(), vec![true, true]),
        "{label} must fail before provider inspection or mutation"
    );
    assert_eq!(
        fs::read(state_dir.join(MACHINE_PORT_EVIDENCE_FILE))
            .expect("rejected record bytes should remain readable"),
        before,
        "{label} rejection must preserve the exact durable bytes"
    );
}

#[test]
fn every_semantic_identity_substitution_fails_byte_stable_before_provider_use() {
    let fixture = PublicationFixture::new(bindings());
    let canonical = fixture.record(
        MachinePortPublicationPhase::Exposed,
        fixture.exposed_receipts(),
    );
    let canonical_plan = oci_attachment_plan(
        &fixture.expectation.tenant_id,
        &fixture.expectation.sandbox_id,
        AttachmentBackendKind::Container,
    );
    let mut candidates = Vec::new();

    let mut tenant = canonical.clone();
    tenant.tenant_id = TenantId::new("tenant-substituted").expect("tenant should validate");
    for slot in &mut tenant.slots {
        let MachinePortPublicationSlot::ObservedExposed(receipt) = slot else {
            unreachable!()
        };
        receipt.tenant_id = tenant.tenant_id.clone();
    }
    candidates.push(("tenant", tenant));

    let mut sandbox = canonical.clone();
    sandbox.sandbox_id = SandboxId::new("substituted-sandbox");
    for slot in &mut sandbox.slots {
        let MachinePortPublicationSlot::ObservedExposed(receipt) = slot else {
            unreachable!()
        };
        receipt.sandbox_id = sandbox.sandbox_id.clone();
    }
    candidates.push(("sandbox", sandbox));

    let mut attachment_id = canonical.clone();
    attachment_id.attachment_id =
        NetworkAttachmentId::for_workload_attachment("substituted-workload", "default");
    attachment_id.attachment_version = replacement_attachment_version(
        &fixture,
        attachment_id.attachment_id.clone(),
        canonical_plan.plan_id().clone(),
        canonical_plan.generation(),
        canonical_plan.content_digest(),
        fixture.expectation.attachment_version.lease_epoch(),
    );
    candidates.push(("attachment-id", attachment_id));

    let mut plan_id = canonical.clone();
    plan_id.attachment_version = replacement_attachment_version(
        &fixture,
        fixture.expectation.attachment_id.clone(),
        nimbus_network::NetworkPlanId::for_tenant_workload_plan(
            &fixture.expectation.tenant_id,
            "substituted-plan",
        ),
        canonical_plan.generation(),
        canonical_plan.content_digest(),
        fixture.expectation.attachment_version.lease_epoch(),
    );
    candidates.push(("plan-id", plan_id));

    let mut resource_generation = canonical.clone();
    resource_generation.attachment_version = replacement_attachment_version(
        &fixture,
        fixture.expectation.attachment_id.clone(),
        canonical_plan.plan_id().clone(),
        NetworkResourceGeneration::new(canonical_plan.generation().as_u64() + 1),
        canonical_plan.content_digest(),
        fixture.expectation.attachment_version.lease_epoch(),
    );
    candidates.push(("resource-generation", resource_generation));

    let mut plan_digest = canonical.clone();
    plan_digest.attachment_version = replacement_attachment_version(
        &fixture,
        fixture.expectation.attachment_id.clone(),
        canonical_plan.plan_id().clone(),
        canonical_plan.generation(),
        nimbus_network::NetworkPlanContentDigest::sha256(b"substituted-plan-content"),
        fixture.expectation.attachment_version.lease_epoch(),
    );
    candidates.push(("plan-digest", plan_digest));

    let mut attachment_epoch = canonical.clone();
    attachment_epoch.attachment_version = replacement_attachment_version(
        &fixture,
        fixture.expectation.attachment_id.clone(),
        canonical_plan.plan_id().clone(),
        canonical_plan.generation(),
        canonical_plan.content_digest(),
        nimbus_network::NetworkLeaseEpoch::new(
            fixture
                .expectation
                .attachment_version
                .lease_epoch()
                .as_u64()
                + 1,
        ),
    );
    candidates.push(("attachment-lease-epoch", attachment_epoch));

    let mut provider_handle = canonical.clone();
    provider_handle.provider_instance = nimbus_network::NetworkProviderHandle::new(
        nimbus_network::NetworkProviderId::for_registration_key("machine-publication-substitution"),
        "substituted-provider-handle",
    )
    .expect("substituted provider handle should validate");
    for slot in &mut provider_handle.slots {
        let MachinePortPublicationSlot::ObservedExposed(receipt) = slot else {
            unreachable!()
        };
        receipt.provider_instance = provider_handle.provider_instance.clone();
    }
    candidates.push(("provider-handle", provider_handle));

    let mut provider_generation = canonical.clone();
    provider_generation.provider_generation = NetworkResourceGeneration::new(18);
    for slot in &mut provider_generation.slots {
        let MachinePortPublicationSlot::ObservedExposed(receipt) = slot else {
            unreachable!()
        };
        receipt.provider_generation = provider_generation.provider_generation;
    }
    candidates.push(("provider-generation", provider_generation));

    let mut binding_order = canonical.clone();
    binding_order.bindings.swap(0, 1);
    binding_order.port_leases.swap(0, 1);
    binding_order.slots.swap(0, 1);
    candidates.push(("binding-order", binding_order));

    let mut binding_member = canonical.clone();
    binding_member.bindings[0] = SandboxPortBinding::tcp("substituted", 28_080, 8_080);
    let MachinePortPublicationSlot::ObservedExposed(receipt) = &mut binding_member.slots[0] else {
        unreachable!()
    };
    receipt.binding = binding_member.bindings[0].clone();
    candidates.push(("binding-member", binding_member));

    let mut lease_order = canonical.clone();
    lease_order.port_leases.swap(0, 1);
    candidates.push(("lease-order", lease_order));

    for (label, lease_id, generation, lease_epoch) in [
        (
            "lease-id",
            nimbus_network::PortLeaseId::for_listener(
                &nimbus_network::ListenerId::for_workload_listener(
                    "machine-publication-substitution",
                    "http",
                ),
            ),
            canonical.port_leases[0].generation(),
            canonical.port_leases[0].lease_epoch(),
        ),
        (
            "lease-generation",
            canonical.port_leases[0].lease_id().clone(),
            NetworkResourceGeneration::new(canonical.port_leases[0].generation().as_u64() + 1),
            canonical.port_leases[0].lease_epoch(),
        ),
        (
            "lease-epoch",
            canonical.port_leases[0].lease_id().clone(),
            canonical.port_leases[0].generation(),
            nimbus_network::NetworkLeaseEpoch::new(
                canonical.port_leases[0].lease_epoch().as_u64() + 1,
            ),
        ),
    ] {
        let mut lease = canonical.clone();
        lease.port_leases[0] =
            replacement_port_lease(&lease.port_leases[0], lease_id, generation, lease_epoch);
        candidates.push((label, lease));
    }

    for (label, candidate) in candidates {
        assert_semantic_substitution_fenced(&fixture, label, candidate);
    }
}

#[test]
fn batch_generation_raw_substitution_fails_integrity_byte_stable_before_provider_use() {
    let fixture = PublicationFixture::new(bindings());
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        state_dir,
        fixture.record(
            MachinePortPublicationPhase::Exposed,
            fixture.exposed_receipts(),
        ),
    )
    .expect("canonical batch should publish");
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("canonical envelope should read"))
            .expect("canonical envelope should decode");
    envelope["record"]["batch_generation"] = serde_json::json!(99);
    let mut tampered =
        serde_json::to_vec_pretty(&envelope).expect("tampered envelope should encode");
    tampered.push(b'\n');
    fs::write(&path, &tampered).expect("test should substitute the raw batch generation");

    let provider_config = fixture
        .manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("fixture provider should exist");
    let provider = StatefulProvider::new(provider_config, &fixture.expectation.bindings, true);
    let error = fixture
        .backend
        .converge_machine_port_publication(
            &fixture.manifest,
            &provider,
            MachinePortPublicationAction::Expose,
        )
        .expect_err("raw batch-generation substitution must fail strict integrity");
    assert!(
        error.to_string().contains("SHA-256 integrity"),
        "the exact envelope-integrity diagnostic must surface: {error}"
    );
    assert_eq!(
        provider.snapshot(),
        (0, Vec::new(), Vec::new(), vec![true, true]),
        "batch-generation corruption must fail before provider I/O"
    );
    assert_eq!(
        fs::read(&path).expect("tampered bytes should remain readable"),
        tampered,
        "integrity rejection must not rewrite the corrupted authority"
    );
}

#[test]
fn illegal_partial_terminal_and_crossed_receipt_are_rejected() {
    let fixture = PublicationFixture::new(bindings());
    let mut partial = fixture.record(
        MachinePortPublicationPhase::Exposed,
        fixture.exposed_receipts(),
    );
    partial.slots[1] = MachinePortPublicationSlot::Pending;
    assert!(
        partial
            .validate_self()
            .expect_err("terminal batch cannot contain Pending")
            .to_string()
            .contains("illegal")
    );

    let mut crossed = fixture.record(
        MachinePortPublicationPhase::Exposed,
        fixture.exposed_receipts(),
    );
    let MachinePortPublicationSlot::ObservedExposed(receipt) = &mut crossed.slots[0] else {
        unreachable!()
    };
    receipt.provider_generation = NetworkResourceGeneration::new(99);
    assert!(
        crossed
            .validate_self()
            .expect_err("crossed receipt must fail")
            .to_string()
            .contains("crossed")
    );
}

#[test]
fn staged_and_lock_artifacts_fail_closed_when_not_regular() {
    let fixture = PublicationFixture::new(bindings());
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    fs::create_dir_all(state_dir).expect("state directory should exist");
    let stage = state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE);
    fs::create_dir(&stage).expect("non-regular stage should exist");
    let error = publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        state_dir,
        fixture.record(
            MachinePortPublicationPhase::Exposed,
            fixture.exposed_receipts(),
        ),
    )
    .expect_err("non-regular stage must fail closed");
    assert!(error.to_string().contains("not a regular file"));
}

#[test]
fn backend_terminal_helpers_publish_versioned_state_machine_records() {
    let fixture = PublicationFixture::new(bindings());
    fixture
        .backend
        .persist_exposed_machine_port_receipts(&fixture.manifest, fixture.exposed_receipts())
        .expect("test terminal helper should publish exposed state");
    assert_eq!(
        terminal_receipts(
            &read_record(&fixture.manifest.conmon_layout.container_state_dir)
                .expect("exposed helper record should read"),
            MachinePortPublicationPhase::Exposed,
        )
        .expect("exposed helper record should authenticate"),
        fixture.exposed_receipts()
    );
    let provider = fixture
        .manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("fixture provider should exist");
    fixture
        .backend
        .persist_absent_machine_port_receipts(
            &fixture.manifest.spec.tenant_id,
            &fixture.manifest.handle.id,
            &fixture.manifest.spec.port_bindings,
            provider,
            fixture.absent_receipts(),
        )
        .expect("test terminal helper should publish absent state");
    assert_eq!(
        terminal_receipts(
            &read_record(&fixture.manifest.conmon_layout.container_state_dir)
                .expect("absent helper record should read"),
            MachinePortPublicationPhase::Absent,
        )
        .expect("absent helper record should authenticate"),
        fixture.absent_receipts()
    );
}
