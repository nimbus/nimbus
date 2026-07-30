use std::fs;
use std::sync::mpsc;
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::capability::test_requirements;
use crate::{
    LocalNetworkStateStoreOptions, NetworkPlanContentDigest, NetworkPlanId,
    NetworkResourceGeneration, NetworkResourcePhase, NetworkSegmentId, NetworkTransitionEvidence,
};

fn tenant(label: &str) -> TenantId {
    TenantId::new(label).expect("tenant fixture should validate")
}

fn attachment(label: &str) -> NetworkAttachmentId {
    NetworkAttachmentId::for_workload_attachment(label, "default")
}

fn provider(label: &str) -> NetworkProviderId {
    NetworkProviderId::for_registration_key(label)
}

fn plan(tenant_id: &TenantId, workload: &str, generation: u64, content: &[u8]) -> NetworkPlan {
    NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(tenant_id, workload),
        NetworkResourceGeneration::new(generation),
        NetworkPlanContentDigest::sha256(content),
        test_requirements(),
    )
}

fn reserve_fixture(
    authority: &LocalNetworkAttachmentAuthority,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
) -> DurableNetworkAttachmentState {
    authority
        .reserve(
            tenant_id,
            provider("nimbus.test.attachment"),
            &plan(tenant_id, "workload-a", 7, b"desired-a"),
            attachment_id.clone(),
            NetworkLeaseEpoch::new(11),
        )
        .expect("attachment should reserve")
}

fn transition(
    record: &DurableNetworkAttachmentState,
    target: NetworkResourcePhase,
    evidence: NetworkTransitionEvidence,
) -> NetworkStateTransition {
    NetworkStateTransition::new(record.resource().version().clone(), target, evidence)
}

fn authority_bytes(authority: &LocalNetworkAttachmentAuthority) -> Vec<u8> {
    fs::read(authority.authority_path()).expect("authority bytes should read")
}

fn assert_authority_bytes(
    authority: &LocalNetworkAttachmentAuthority,
    expected: &[u8],
    substitution: &str,
) {
    assert_eq!(
        authority_bytes(authority),
        expected,
        "{substitution} must not change authority bytes"
    );
}

#[test]
fn real_store_reopen_preserves_tenant_version_phase_provider_and_redacted_handle() {
    let root = TempDir::new().expect("temporary state root should exist");
    let tenant_id = tenant("tenant-a");
    let attachment_id = attachment("workload-a");
    let authority =
        LocalNetworkAttachmentAuthority::open(root.path()).expect("authority should open");
    let reserved = reserve_fixture(&authority, &tenant_id, &attachment_id);
    let (_, provisioning) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &reserved,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("attachment should enter provisioning");
    let handle = NetworkProviderHandle::new(
        provider("nimbus.test.attachment"),
        "opaque-secret-realization",
    )
    .expect("handle should validate");
    let (_, ready_handle) = authority
        .record_provider_handle(
            &tenant_id,
            provisioning.resource().version(),
            handle.clone(),
        )
        .expect("handle should persist");
    let (_, ready) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &ready_handle,
                NetworkResourcePhase::Ready,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("attachment should become ready");
    drop(authority);

    let reopened =
        LocalNetworkAttachmentAuthority::open(root.path()).expect("authority should reopen");
    let actual = reopened
        .get(&tenant_id, &attachment_id)
        .expect("inspection should succeed")
        .expect("attachment should remain");
    assert_eq!(actual, ready);
    assert_eq!(actual.tenant_id(), &tenant_id);
    assert_eq!(
        actual.selected_provider_id(),
        &provider("nimbus.test.attachment")
    );
    assert_eq!(actual.resource().phase(), NetworkResourcePhase::Ready);
    assert_eq!(actual.resource().provider_handle(), Some(&handle));
    assert!(!format!("{actual:?}").contains("opaque-secret-realization"));
}

#[test]
fn substitutions_fail_without_changing_authority_bytes() {
    let root = TempDir::new().expect("temporary state root should exist");
    let tenant_id = tenant("tenant-a");
    let attachment_id = attachment("workload-a");
    let authority =
        LocalNetworkAttachmentAuthority::open(root.path()).expect("authority should open");
    let record = reserve_fixture(&authority, &tenant_id, &attachment_id);
    let reserved_bytes = authority_bytes(&authority);

    let wrong_provider = authority.reserve(
        &tenant_id,
        provider("nimbus.test.other"),
        &plan(&tenant_id, "workload-a", 7, b"desired-a"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(11),
    );
    assert!(matches!(
        wrong_provider,
        Err(NetworkAttachmentStateError::SelectedProviderConflict { .. })
    ));
    assert_authority_bytes(
        &authority,
        &reserved_bytes,
        "selected provider substitution",
    );

    let wrong_plan = authority.reserve(
        &tenant_id,
        provider("nimbus.test.attachment"),
        &plan(&tenant_id, "workload-b", 7, b"desired-a"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(11),
    );
    assert!(matches!(
        wrong_plan,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::PlanIdentityMismatch
        ))
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "plan identity substitution");

    let stale_generation = authority.reserve(
        &tenant_id,
        provider("nimbus.test.attachment"),
        &plan(&tenant_id, "workload-a", 6, b"desired-a"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(11),
    );
    assert!(matches!(
        stale_generation,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::StaleGeneration { .. }
        ))
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "stale generation");

    let wrong_generation = authority.reserve(
        &tenant_id,
        provider("nimbus.test.attachment"),
        &plan(&tenant_id, "workload-a", 8, b"desired-a"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(11),
    );
    assert!(matches!(
        wrong_generation,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::FutureGeneration { .. }
        ))
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "future generation");

    let wrong_digest = authority.reserve(
        &tenant_id,
        provider("nimbus.test.attachment"),
        &plan(&tenant_id, "workload-a", 7, b"desired-b"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(11),
    );
    assert!(matches!(
        wrong_digest,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::PlanDigestConflict { .. }
        ))
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "plan digest substitution");

    let stale_epoch = authority.reserve(
        &tenant_id,
        provider("nimbus.test.attachment"),
        &plan(&tenant_id, "workload-a", 7, b"desired-a"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(10),
    );
    assert!(matches!(
        stale_epoch,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::StaleLeaseEpoch { .. }
        ))
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "stale lease epoch");

    let wrong_epoch = authority.reserve(
        &tenant_id,
        provider("nimbus.test.attachment"),
        &plan(&tenant_id, "workload-a", 7, b"desired-a"),
        attachment_id.clone(),
        NetworkLeaseEpoch::new(12),
    );
    assert!(matches!(
        wrong_epoch,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::FutureLeaseEpoch { .. }
        ))
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "future lease epoch");

    let wrong_tenant = tenant("tenant-b");
    let wrong_tenant_result = authority.apply_transition(
        &wrong_tenant,
        &transition(
            &record,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::Progress,
        ),
    );
    assert!(matches!(
        wrong_tenant_result,
        Err(NetworkAttachmentStateError::NotFound { .. })
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "tenant substitution");

    let wrong_attachment = attachment("workload-b");
    let wrong_attachment_version = NetworkResourceVersion::for_plan(
        &plan(&tenant_id, "workload-a", 7, b"desired-a"),
        NetworkResourceId::Attachment(wrong_attachment),
        NetworkLeaseEpoch::new(11),
    );
    let wrong_attachment_result = authority.apply_transition(
        &tenant_id,
        &NetworkStateTransition::new(
            wrong_attachment_version,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::Progress,
        ),
    );
    assert!(matches!(
        wrong_attachment_result,
        Err(NetworkAttachmentStateError::NotFound { .. })
    ));
    assert_authority_bytes(
        &authority,
        &reserved_bytes,
        "attachment identity substitution",
    );

    let segment_id: NetworkSegmentId = "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("segment fixture should parse");
    let wrong_resource_kind = NetworkResourceVersion::for_plan(
        &plan(&tenant_id, "workload-a", 7, b"desired-a"),
        NetworkResourceId::Segment(segment_id),
        NetworkLeaseEpoch::new(11),
    );
    let wrong_resource_result = authority.apply_transition(
        &tenant_id,
        &NetworkStateTransition::new(
            wrong_resource_kind,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::Progress,
        ),
    );
    assert!(matches!(
        wrong_resource_result,
        Err(NetworkAttachmentStateError::ResourceKindConflict { .. })
    ));
    assert_authority_bytes(&authority, &reserved_bytes, "resource kind substitution");

    let (_, provisioning) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &record,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("attachment should enter provisioning");
    let after_transition = authority_bytes(&authority);
    let wrong_handle =
        NetworkProviderHandle::new(provider("nimbus.test.other"), "opaque-other-provider")
            .expect("handle should validate");
    let handle_result = authority.record_provider_handle(
        &tenant_id,
        provisioning.resource().version(),
        wrong_handle,
    );
    assert!(matches!(
        handle_result,
        Err(NetworkAttachmentStateError::HandleProviderConflict { .. })
    ));
    assert_authority_bytes(
        &authority,
        &after_transition,
        "handle provider substitution",
    );

    let stable_handle = NetworkProviderHandle::new(
        provider("nimbus.test.attachment"),
        "attachment:stable-workload-incarnation",
    )
    .expect("stable handle should validate");
    let (_, with_handle) = authority
        .record_provider_handle(&tenant_id, provisioning.resource().version(), stable_handle)
        .expect("stable handle should persist");
    let stable_bytes = authority_bytes(&authority);
    let transient_attempt = NetworkProviderHandle::new(
        provider("nimbus.test.attachment"),
        "attempt:transient-netavark-operation",
    )
    .expect("transient attempt fixture should validate");
    let transient_result = authority.record_provider_handle(
        &tenant_id,
        with_handle.resource().version(),
        transient_attempt,
    );
    assert!(matches!(
        transient_result,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::ProviderHandleConflict
        ))
    ));
    assert_authority_bytes(
        &authority,
        &stable_bytes,
        "transient provider attempt replacing stable handle",
    );
    assert_ne!(reserved_bytes, stable_bytes);
}

#[test]
fn checksum_valid_key_record_mismatch_is_rejected_on_reopen() {
    let root = TempDir::new().expect("temporary state root should exist");
    let tenant_id = tenant("tenant-a");
    let attachment_id = attachment("workload-a");
    let authority =
        LocalNetworkAttachmentAuthority::open(root.path()).expect("authority should open");
    let record = reserve_fixture(&authority, &tenant_id, &attachment_id);

    authority
        .store
        .transaction(
            &NetworkStatePartition::AttachmentStates,
            |state: &mut NetworkAttachmentState| -> Result<(), ()> {
                state.records.clear();
                state.records.insert("wrong-key".to_owned(), record);
                Ok(())
            },
        )
        .expect("test should write checksum-valid invalid payload");
    drop(authority);

    assert!(matches!(
        LocalNetworkAttachmentAuthority::open(root.path()),
        Err(NetworkAttachmentStateError::CorruptAuthority { .. })
    ));
}

#[test]
fn checksum_valid_attachment_schema_extension_is_rejected_on_reopen() {
    let root = TempDir::new().expect("temporary state root should exist");
    let store = LocalNetworkStateStore::open(root.path()).expect("network store should open");
    store
        .transaction(
            &NetworkStatePartition::AttachmentStates,
            |payload: &mut serde_json::Value| -> Result<(), ()> {
                *payload = serde_json::json!({
                    "records": {},
                    "unrecognized_attachment_authority": true
                });
                Ok(())
            },
        )
        .expect("test should write checksum-valid schema-invalid payload");
    let before = fs::read(store.authority_path()).expect("schema-invalid bytes should read");

    assert!(matches!(
        LocalNetworkAttachmentAuthority::open(root.path()),
        Err(NetworkAttachmentStateError::Store(
            NetworkStateStoreError::Corrupt { .. }
        ))
    ));
    assert_eq!(
        fs::read(store.authority_path()).expect("rejected authority bytes should remain readable"),
        before,
        "schema rejection must not rewrite the checksum-valid invalid authority"
    );
}

#[test]
fn only_explicit_confirmed_deletion_can_reprovision_without_terminal_resurrection() {
    let root = TempDir::new().expect("temporary state root should exist");
    let tenant_id = tenant("tenant-a");
    let attachment_id = attachment("workload-a");
    let authority =
        LocalNetworkAttachmentAuthority::open(root.path()).expect("authority should open");
    let reserved = reserve_fixture(&authority, &tenant_id, &attachment_id);
    let (_, provisioning) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &reserved,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("attachment should enter provisioning");
    let (_, withdrawing) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &provisioning,
                NetworkResourcePhase::Withdrawing,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("attachment should begin withdrawal");
    let (_, deleting) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &withdrawing,
                NetworkResourcePhase::Deleting,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("attachment should begin deletion");

    let illegal = authority.apply_transition(
        &tenant_id,
        &transition(
            &deleting,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::Progress,
        ),
    );
    assert!(matches!(
        illegal,
        Err(NetworkAttachmentStateError::State(
            NetworkStateError::IllegalTransition { .. }
        ))
    ));

    let (_, reprovisioning) = authority
        .apply_transition(
            &tenant_id,
            &transition(
                &deleting,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::DeletionConfirmedForReprovision,
            ),
        )
        .expect("confirmed deletion should permit same-generation reprovision");
    assert_eq!(
        reprovisioning.resource().phase(),
        NetworkResourcePhase::Provisioning
    );
    assert_eq!(
        reprovisioning.resource().version(),
        deleting.resource().version()
    );

    for terminal in [NetworkResourcePhase::Released, NetworkResourcePhase::Failed] {
        let isolated_attachment = attachment(&format!("terminal-{terminal:?}"));
        let terminal_reserved = reserve_fixture(&authority, &tenant_id, &isolated_attachment);
        let (_, terminal_record) = authority
            .apply_transition(
                &tenant_id,
                &transition(
                    &terminal_reserved,
                    terminal,
                    NetworkTransitionEvidence::ConfirmedNoEffect,
                ),
            )
            .expect("terminal fixture should persist");
        let before = authority_bytes(&authority);
        let resurrection = authority.apply_transition(
            &tenant_id,
            &transition(
                &terminal_record,
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::DeletionConfirmedForReprovision,
            ),
        );
        assert!(matches!(
            resurrection,
            Err(NetworkAttachmentStateError::State(
                NetworkStateError::IllegalTransition { .. }
            ))
        ));
        assert_authority_bytes(&authority, &before, "terminal resurrection");
    }
}

#[test]
fn contended_store_lock_fails_before_attachment_state_can_be_read() {
    use crate::state_store::test_support::{
        NetworkStateDurabilityEvent, transaction_with_durability_observer,
    };

    let root = TempDir::new().expect("temporary state root should exist");
    let options = LocalNetworkStateStoreOptions {
        lock_timeout: Duration::from_millis(50),
        lock_retry_interval: Duration::from_millis(2),
    };
    let holder = LocalNetworkStateStore::open_with_options(root.path(), options)
        .expect("holder should open");
    let contender =
        LocalNetworkStateStore::open_with_options(root.path(), options).expect("contender open");
    let authority =
        LocalNetworkAttachmentAuthority::from_store(contender).expect("authority should open");
    let tenant_id = tenant("tenant-lock");
    let attachment_id = attachment("workload-lock");
    reserve_fixture(&authority, &tenant_id, &attachment_id);

    let (held_tx, held_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    let holder_thread = std::thread::spawn(move || {
        transaction_with_durability_observer(
            &holder,
            &NetworkStatePartition::SegmentAllocations,
            |event| {
                if event == NetworkStateDurabilityEvent::StateFileSynced {
                    held_tx.send(()).expect("held signal should deliver");
                    release_rx.recv().expect("release signal should deliver");
                }
            },
            |state: &mut BTreeMap<String, String>| {
                state.insert("holder".to_owned(), "active".to_owned());
                Ok::<_, ()>(())
            },
        )
        .expect("holder transaction should finish after release");
    });
    held_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("holder must reach the synced stage");

    let error = authority
        .get(&tenant_id, &attachment_id)
        .expect_err("contended attachment read must fail closed");
    assert!(matches!(
        error,
        NetworkAttachmentStateError::Store(NetworkStateStoreError::LockTimeout { .. })
    ));

    release_tx.send(()).expect("holder release should deliver");
    holder_thread.join().expect("holder thread should join");
    assert!(
        authority
            .get(&tenant_id, &attachment_id)
            .expect("attachment read should recover")
            .is_some()
    );
}
