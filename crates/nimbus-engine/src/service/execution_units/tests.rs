use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, Error, FieldTransform,
    FieldTransformOperation, IndexDefinition, NumericValue, OrderBy, OrderDirection,
    PrincipalContext, Query, QueryDirection, SpecialDouble, StoredValue, StructuredOrder,
    StructuredQuery, TenantId, Timestamp, TriggerInvocationKey, TriggerWriteOrigin,
    TypedScalarValue, WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_testing::{BlockingFaultInjector, ServiceFixture};
use serde_json::json;
use tempfile::tempdir;
use tokio::time::{Duration, timeout};

use crate::Service;
use crate::test_support::{
    messages_schema, messages_table, owner_read_write_policy, principal_with_subject,
};
use nimbus_storage::{FaultPoint, ManualClock};

#[test]
fn mutation_execution_unit_aborts_on_overlapping_document_conflict() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_doc");

    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document = execution_unit
        .get_document(&table, document_id.clone())
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(document.get_field("body"), Some(&json!("Initial")));
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should remain committed")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn mutation_execution_unit_commits_when_concurrent_write_is_disjoint() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_disjoint");

    let first_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("First")),
            ]),
        )
        .expect("first fixture insert should succeed");
    let second_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect("second fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let read_back = execution_unit
        .get_document(&table, first_id.clone())
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(read_back.get_field("body"), Some(&json!("First")));
    execution_unit
        .update_document(
            table.clone(),
            first_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            second_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("disjoint update should commit");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(
        service
            .get_document(&tenant_id, &table, first_id.clone())
            .expect("first document should exist")
            .get_field("body"),
        Some(&json!("Tx update"))
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, second_id.clone())
            .expect("second document should exist")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn mutation_execution_unit_insert_then_update_commits_as_single_insert() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_insert_update");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("staged insert should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Updated"))]),
        )
        .expect("staged update should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);
    assert!(commit.writes[0].previous.is_none());
    assert_eq!(
        commit.writes[0]
            .current
            .as_ref()
            .and_then(|document| document.get_field("body")),
        Some(&json!("Updated"))
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("inserted document should exist")
            .get_field("body"),
        Some(&json!("Updated"))
    );
}

#[test]
fn mutation_execution_unit_insert_then_delete_commits_as_noop() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_insert_delete");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Transient")),
            ]),
        )
        .expect("staged insert should succeed");
    execution_unit
        .delete_document(table.clone(), document_id.clone())
        .expect("staged delete should succeed");

    let commit = execution_unit.commit().expect("commit should succeed");
    assert!(
        commit.is_none(),
        "insert followed by delete should collapse to a no-op"
    );
    let error = service
        .get_document(&tenant_id, &table, document_id.clone())
        .expect_err("transient document should not exist");
    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn mutation_execution_unit_persists_trigger_write_origin_on_committed_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_trigger_origin");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::system())
        .expect("execution unit should start");
    let origin = TriggerWriteOrigin::new(
        TriggerInvocationKey::new("firebase:messagesWritten", "evt-root")
            .expect("invocation key should parse"),
        2,
    );
    execution_unit
        .set_trigger_write_origin(origin.clone())
        .expect("trigger write origin should stage");
    execution_unit
        .insert_document(
            table,
            serde_json::Map::from_iter([("body".to_string(), json!("from trigger"))]),
        )
        .expect("staged insert should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");

    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].trigger_write_origin, Some(origin));
}

#[test]
fn mutation_execution_unit_restage_after_revert_commits_once() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_restage");

    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("First"))]),
        )
        .expect("first staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Initial"))]),
        )
        .expect("revert staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Second"))]),
        )
        .expect("restaged update should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(
        commit.writes.len(),
        1,
        "restaging after a revert should only produce one final write"
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should exist")
            .get_field("body"),
        Some(&json!("Second"))
    );
}

#[tokio::test]
async fn mutation_execution_unit_conflicts_with_durable_unapplied_write() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(92_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_occ_apply_lag");

    let mut outside_update = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let table = table.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    table,
                    serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-456")),
                        ("body".to_string(), json!("Outside insert")),
                    ]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut outside_update)
            .await
            .is_err(),
        "outside update should remain pending while apply is blocked"
    );

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let visible = execution_unit
        .query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )
        .expect("query should succeed");
    assert!(
        visible.is_empty(),
        "execution unit should still see the applied snapshot while the outside write lags"
    );
    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Tx insert")),
            ]),
        )
        .expect("staged insert should succeed");

    let commit_handle = tokio::task::spawn_blocking({
        let execution_unit = execution_unit.clone();
        move || execution_unit.commit()
    });

    let commit_result = timeout(Duration::from_secs(1), commit_handle)
        .await
        .expect("commit should resolve promptly while the journal worker is still blocked")
        .expect("commit task should join successfully");
    faults.release();
    timeout(Duration::from_secs(1), outside_update)
        .await
        .expect("outside update should finish after apply resumes")
        .expect("outside update task should join successfully")
        .expect("outside update should succeed");

    let error = commit_result.expect_err(
        "commit should conflict with the durable journal write that was not part of the applied snapshot",
    );
    assert!(matches!(error, Error::Conflict(_)));
    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "body".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed after apply");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].get_field("body"),
        Some(&json!("Outside insert"))
    );
}

#[test]
fn mutation_execution_unit_structured_query_reads_staged_rows() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_structured_reads");

    let alpha_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("alpha"))]),
        )
        .expect("seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("bravo"))]),
        )
        .expect("second seed insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .update_document(
            table.clone(),
            alpha_id,
            serde_json::Map::from_iter([("body".to_string(), json!("zulu"))]),
        )
        .expect("staged update should succeed");
    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("beta"))]),
        )
        .expect("staged insert should succeed");

    let documents = execution_unit
        .query_documents_structured_cancellable(
            &table,
            &StructuredQuery {
                order_by: vec![StructuredOrder {
                    field: nimbus_core::FieldReference::new("body"),
                    direction: QueryDirection::Ascending,
                }],
                ..StructuredQuery::default()
            },
            &mut || Ok(()),
        )
        .expect("structured query should succeed");

    assert_eq!(
        documents
            .iter()
            .map(|document| document.get_field("body").cloned().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![json!("beta"), json!("bravo"), json!("zulu")]
    );
}

#[test]
fn mutation_execution_unit_conflicts_when_auth_filtered_visibility_changes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_auth");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_occ_auth",
                vec![IndexDefinition {
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(owner_read_write_policy()),
            ),
        )
        .expect("schema should save");
    let hidden_owner = principal_with_subject("user-456");

    let hidden_id = service
        .insert_document_with(
            &tenant_id,
            table.clone(),
            None,
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Hidden")),
            ]),
            crate::MutationActor::with_principal(&hidden_owner),
        )
        .expect("hidden document insert should succeed");

    let principal = principal_with_subject("user-123");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), principal.clone())
        .expect("execution unit should start");
    let visible = execution_unit
        .query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )
        .expect("authorized query should succeed");
    assert!(visible.is_empty(), "hidden row should not be visible yet");

    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Tx insert")),
            ]),
        )
        .expect("authorized staged insert should succeed");

    service
        .update_document_with(
            &tenant_id,
            table.clone(),
            hidden_id,
            serde_json::Map::from_iter([("owner".to_string(), json!("user-123"))]),
            crate::MutationActor::with_principal(&hidden_owner),
        )
        .expect("external update should make the hidden row visible");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the auth-filtered visibility change");
    assert!(matches!(error, Error::Conflict(_)));
}

#[test]
fn mutation_execution_unit_rejects_reuse_after_successful_commit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_finalize_success");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Committed")),
            ]),
        )
        .expect("staged insert should succeed");
    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);

    let read_error = execution_unit
        .get_document(&table, document_id.clone())
        .expect_err("finalized execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect_err("finalized execution unit should reject further writes");
    assert!(matches!(write_error, Error::InvalidInput(message) if message.contains("finalized")));

    let commit_error = execution_unit
        .commit()
        .expect_err("finalized execution unit should reject a second commit");
    assert!(matches!(commit_error, Error::InvalidInput(message) if message.contains("finalized")));
}

#[test]
fn mutation_execution_unit_rejects_reuse_after_failed_commit_attempt() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_finalize_failure");

    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .get_document(&table, document_id.clone())
        .expect("point read should succeed")
        .expect("document should exist");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let commit_error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(commit_error, Error::Conflict(_)));

    let read_error = execution_unit
        .get_document(&table, document_id.clone())
        .expect_err("conflicted execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Retry"))]),
        )
        .expect_err("conflicted execution unit should reject further writes");
    assert!(matches!(write_error, Error::InvalidInput(message) if message.contains("finalized")));

    let second_commit_error = execution_unit
        .commit()
        .expect_err("conflicted execution unit should reject a second commit");
    assert!(
        matches!(second_commit_error, Error::InvalidInput(message) if message.contains("finalized"))
    );
}

#[test]
fn atomic_write_batch_overwrite_creates_missing_document() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_overwrite_create");
    let document_id = DocumentId::from_key("cities/SF".replace('/', "_"))
        .expect("firestore-style leaf id should parse once isolated");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Set {
                key: locator_key(table.clone(), document_id.clone()),
                document: serde_json::Map::from_iter([
                    ("owner".to_string(), json!("user-123")),
                    ("body".to_string(), json!("San Francisco")),
                ]),
                mode: WriteSetMode::Overwrite,
                precondition: WritePrecondition::default(),
                transforms: Vec::new(),
            }])
            .expect("batch should build"),
        )
        .expect("overwrite batch should succeed");

    assert!(
        outcome.commit.is_some(),
        "overwrite create should emit a commit"
    );
    assert_eq!(outcome.write_results.len(), 1);
    assert_eq!(
        outcome.write_results[0].update_time,
        Some(outcome.commit_time),
        "set writes should surface an update time"
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("created document should exist")
            .get_field("body"),
        Some(&json!("San Francisco"))
    );
}

#[test]
fn staged_atomic_write_batch_keeps_execution_unit_reusable_until_commit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_stage_reuse");
    let document_id = DocumentId::from_key("staged-batch").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let staged = execution_unit
        .stage_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Set {
                key: locator_key(table.clone(), document_id.clone()),
                document: serde_json::Map::from_iter([
                    ("owner".to_string(), json!("user-123")),
                    ("body".to_string(), json!("Before commit")),
                ]),
                mode: WriteSetMode::Overwrite,
                precondition: WritePrecondition::default(),
                transforms: Vec::new(),
            }])
            .expect("batch should build"),
        )
        .expect("staged batch should succeed");

    assert!(
        staged.commit.is_none(),
        "staging should not finalize the execution unit"
    );
    assert_eq!(staged.write_results.len(), 1);
    assert_eq!(
        staged.write_results[0].update_time,
        Some(staged.commit_time),
        "staged set should still surface a provisional update time"
    );

    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("After commit"))]),
        )
        .expect("execution unit should still accept writes after staging");

    let commit = execution_unit
        .commit()
        .expect("final commit should succeed");
    assert!(
        commit.is_some(),
        "final commit should persist staged writes"
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("staged document should commit")
            .get_field("body"),
        Some(&json!("After commit"))
    );
}

#[test]
fn atomic_write_batch_delete_missing_without_precondition_is_a_noop() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_delete_missing");
    let document_id = DocumentId::from_key("missing-doc").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Delete {
                key: locator_key(table.clone(), document_id.clone()),
                precondition: WritePrecondition::default(),
                missing_ok: true,
            }])
            .expect("batch should build"),
        )
        .expect("missing delete should succeed");

    assert!(
        outcome.commit.is_none(),
        "a pure missing-ok delete should not append a logical commit"
    );
    assert_eq!(outcome.write_results.len(), 1);
    assert!(
        outcome.write_results[0].update_time.is_none(),
        "delete write results should not expose update_time"
    );
    assert!(matches!(
        service.get_document(&tenant_id, &table, document_id),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn atomic_write_batch_orders_mixed_results_and_applies_atomically() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_mixed");

    let patch_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Before patch")),
            ]),
        )
        .expect("seed patch document should insert");
    let delete_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Delete me")),
            ]),
        )
        .expect("seed delete document should insert");
    let create_id = DocumentId::from_key("atomic-created").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![
                AtomicWrite::Verify {
                    key: locator_key(table.clone(), patch_id.clone()),
                    precondition: WritePrecondition::exists(true),
                },
                AtomicWrite::Patch {
                    key: locator_key(table.clone(), patch_id.clone()),
                    field_patch: serde_json::Map::from_iter([(
                        "body".to_string(),
                        json!("After patch"),
                    )]),
                    mask: vec!["body".to_string()],
                    precondition: WritePrecondition::exists(true),
                    transforms: Vec::new(),
                },
                AtomicWrite::Set {
                    key: locator_key(table.clone(), create_id.clone()),
                    document: serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-123")),
                        ("body".to_string(), json!("Created")),
                    ]),
                    mode: WriteSetMode::Overwrite,
                    precondition: WritePrecondition::default(),
                    transforms: Vec::new(),
                },
                AtomicWrite::Delete {
                    key: locator_key(table.clone(), delete_id.clone()),
                    precondition: WritePrecondition::exists(true),
                    missing_ok: false,
                },
            ])
            .expect("batch should build"),
        )
        .expect("mixed batch should succeed");

    assert!(outcome.commit.is_some(), "mixed batch should commit");
    assert_eq!(outcome.write_results.len(), 4);
    assert!(outcome.write_results[0].update_time.is_none());
    assert_eq!(
        outcome.write_results[1].update_time,
        Some(outcome.commit_time)
    );
    assert_eq!(
        outcome.write_results[2].update_time,
        Some(outcome.commit_time)
    );
    assert!(outcome.write_results[3].update_time.is_none());
    assert_eq!(
        service
            .get_document(&tenant_id, &table, patch_id.clone())
            .expect("patched document should exist")
            .get_field("body"),
        Some(&json!("After patch"))
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, create_id.clone())
            .expect("created document should exist")
            .get_field("body"),
        Some(&json!("Created"))
    );
    assert!(matches!(
        service.get_document(&tenant_id, &table, delete_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn atomic_write_batch_rolls_back_on_precondition_failure() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_preconditions");

    let existing_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Existing")),
            ]),
        )
        .expect("seed document should insert");
    let staged_id = DocumentId::from_key("staged-before-failure").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let error = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![
                AtomicWrite::Set {
                    key: locator_key(table.clone(), staged_id.clone()),
                    document: serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-123")),
                        ("body".to_string(), json!("Transient")),
                    ]),
                    mode: WriteSetMode::Overwrite,
                    precondition: WritePrecondition::default(),
                    transforms: Vec::new(),
                },
                AtomicWrite::Verify {
                    key: locator_key(table.clone(), existing_id.clone()),
                    precondition: WritePrecondition::exists(false),
                },
            ])
            .expect("batch should build"),
        )
        .expect_err("precondition failure should abort the batch");

    assert!(matches!(error, Error::AlreadyExists(_)));
    assert!(matches!(
        service.get_document(&tenant_id, &table, staged_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn atomic_write_batch_transform_write_creates_missing_document_and_returns_ordered_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_transforms");
    let transformed_id = DocumentId::from_key("transform-created").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), transformed_id.clone()),
                transforms: vec![
                    FieldTransform {
                        field: "count".to_string(),
                        transform: FieldTransformOperation::Increment {
                            operand: NumericValue::Integer { value: 2 },
                        },
                    },
                    FieldTransform {
                        field: "ceiling".to_string(),
                        transform: FieldTransformOperation::Maximum {
                            operand: NumericValue::Double { value: 3.5 },
                        },
                    },
                    FieldTransform {
                        field: "floor".to_string(),
                        transform: FieldTransformOperation::Minimum {
                            operand: NumericValue::Integer { value: 7 },
                        },
                    },
                    FieldTransform {
                        field: "tags".to_string(),
                        transform: FieldTransformOperation::AppendMissingElements {
                            values: vec![json!(2.0), json!("a"), json!("a")],
                        },
                    },
                    FieldTransform {
                        field: "tags".to_string(),
                        transform: FieldTransformOperation::RemoveAllFromArray {
                            values: vec![json!(2)],
                        },
                    },
                ],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("transform-only write should succeed");

    assert_eq!(outcome.write_results.len(), 1);
    assert_eq!(
        outcome.write_results[0].transform_results,
        vec![
            StoredValue::from(json!(2)),
            StoredValue::from(json!(3.5)),
            StoredValue::from(json!(7)),
            StoredValue::from(serde_json::Value::Null),
            StoredValue::from(serde_json::Value::Null)
        ]
    );
    let document = service
        .get_document(&tenant_id, &table, transformed_id.clone())
        .expect("transform write should create the document");
    assert_eq!(document.get_field("count"), Some(&json!(2)));
    assert_eq!(document.get_field("ceiling"), Some(&json!(3.5)));
    assert_eq!(document.get_field("floor"), Some(&json!(7)));
    assert_eq!(document.get_field("tags"), Some(&json!(["a"])));
}

#[test]
fn atomic_write_batch_patch_applies_transforms_after_patch_fields() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_patch_transforms");
    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("count".to_string(), json!(40)),
            ]),
        )
        .expect("seed document should insert");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Patch {
                key: locator_key(table.clone(), document_id.clone()),
                field_patch: serde_json::Map::from_iter([("count".to_string(), json!(1))]),
                mask: vec!["count".to_string()],
                precondition: WritePrecondition::exists(true),
                transforms: vec![FieldTransform {
                    field: "count".to_string(),
                    transform: FieldTransformOperation::Increment {
                        operand: NumericValue::Integer { value: 2 },
                    },
                }],
            }])
            .expect("batch should build"),
        )
        .expect("patch with transforms should succeed");

    assert_eq!(outcome.write_results.len(), 1);
    assert_eq!(
        outcome.write_results[0].transform_results,
        vec![StoredValue::from(json!(3))]
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("patched document should exist")
            .get_field("count"),
        Some(&json!(3))
    );
}

#[test]
fn atomic_write_batch_patch_updates_nested_field_paths() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_nested_patch");
    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                (
                    "profile".to_string(),
                    json!({
                        "active": true,
                        "name": "Tokyo"
                    }),
                ),
            ]),
        )
        .expect("seed document should insert");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Patch {
                key: locator_key(table.clone(), document_id.clone()),
                field_patch: serde_json::Map::from_iter([(
                    "profile".to_string(),
                    json!({
                        "active": false
                    }),
                )]),
                mask: vec!["profile.active".to_string()],
                precondition: WritePrecondition::exists(true),
                transforms: Vec::new(),
            }])
            .expect("batch should build"),
        )
        .expect("nested patch should succeed");

    let document = service
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("patched document should exist");
    assert_eq!(
        document.get_field("profile"),
        Some(&json!({
            "active": false,
            "name": "Tokyo"
        }))
    );
}

#[test]
fn atomic_write_batch_preserves_existing_numeric_type_for_equivalent_extrema() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_equivalent_extrema");
    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("count".to_string(), json!(3)),
            ]),
        )
        .expect("seed document should insert");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![
                    FieldTransform {
                        field: "count".to_string(),
                        transform: FieldTransformOperation::Maximum {
                            operand: NumericValue::Double { value: 3.0 },
                        },
                    },
                    FieldTransform {
                        field: "count".to_string(),
                        transform: FieldTransformOperation::Minimum {
                            operand: NumericValue::Double { value: 3.0 },
                        },
                    },
                ],
                precondition: WritePrecondition::exists(true),
            }])
            .expect("batch should build"),
        )
        .expect("equivalent extrema should succeed");

    assert_eq!(
        outcome.write_results[0].transform_results,
        vec![StoredValue::from(json!(3)), StoredValue::from(json!(3))]
    );
    let count = service
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("document should exist")
        .get_field("count")
        .cloned()
        .expect("count should exist");
    assert_eq!(count, json!(3));
    assert!(
        count.as_i64().is_some(),
        "equivalent extrema should preserve the existing integer representation"
    );
}

#[test]
fn atomic_write_batch_applies_server_timestamp_as_typed_scalar_metadata() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_transforms");
    let document_id = DocumentId::from_key("server-timestamp").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![FieldTransform {
                    field: "updatedAt".to_string(),
                    transform: FieldTransformOperation::ServerTimestamp,
                }],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("server timestamp transform should succeed");

    let [
        StoredValue::TypedScalar {
            value: TypedScalarValue::Timestamp { value },
        },
    ] = outcome.write_results[0].transform_results.as_slice()
    else {
        panic!("server timestamp should return a typed scalar transform result");
    };
    let document = service
        .get_document(&tenant_id, &table, document_id)
        .expect("transformed document should exist");
    assert_eq!(
        document.typed_field("updatedAt"),
        Some(&TypedScalarValue::Timestamp { value: *value })
    );
    assert_eq!(
        document.get_field("updatedAt"),
        Some(&serde_json::Value::Number(serde_json::Number::from(
            value.0
        )))
    );
}

#[test]
fn atomic_write_batch_applies_special_double_extrema_as_typed_scalars() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_atomic_special_doubles");
    let document_id = DocumentId::from_key("special-double").expect("id should parse");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![
                    FieldTransform {
                        field: "ceiling".to_string(),
                        transform: FieldTransformOperation::Maximum {
                            operand: NumericValue::SpecialDouble {
                                value: SpecialDouble::PositiveInfinity,
                            },
                        },
                    },
                    FieldTransform {
                        field: "floor".to_string(),
                        transform: FieldTransformOperation::Minimum {
                            operand: NumericValue::SpecialDouble {
                                value: SpecialDouble::Nan,
                            },
                        },
                    },
                ],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("special double extrema should succeed");

    assert_eq!(
        outcome.write_results[0].transform_results,
        vec![
            StoredValue::TypedScalar {
                value: TypedScalarValue::SpecialDouble {
                    value: SpecialDouble::PositiveInfinity,
                },
            },
            StoredValue::TypedScalar {
                value: TypedScalarValue::SpecialDouble {
                    value: SpecialDouble::Nan,
                },
            },
        ]
    );
    let document = service
        .get_document(&tenant_id, &table, document_id)
        .expect("transformed document should exist");
    assert_eq!(
        document.typed_field("ceiling"),
        Some(&TypedScalarValue::SpecialDouble {
            value: SpecialDouble::PositiveInfinity,
        })
    );
    assert_eq!(document.get_field("ceiling"), Some(&json!("Infinity")));
    assert_eq!(
        document.typed_field("floor"),
        Some(&TypedScalarValue::SpecialDouble {
            value: SpecialDouble::Nan,
        })
    );
    assert_eq!(document.get_field("floor"), Some(&json!("NaN")));
}

fn locator_key(table: nimbus_core::TableName, id: DocumentId) -> WriteKey {
    WriteKey::from(DocumentLocator::new(table, id))
}
