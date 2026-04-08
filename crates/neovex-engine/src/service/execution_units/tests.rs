use std::sync::Arc;

use neovex_core::{
    Error, IndexDefinition, OrderBy, OrderDirection, PrincipalContext, Query, TenantId, Timestamp,
};
use neovex_testing::{BlockingFaultInjector, ServiceFixture};
use serde_json::json;
use tempfile::tempdir;
use tokio::time::{Duration, timeout};

use crate::Service;
use crate::test_support::{
    messages_schema, messages_table, owner_read_write_policy, principal_with_subject,
};
use neovex_storage::{FaultPoint, ManualClock};

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
        .get_document(&table, document_id)
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(document.get_field("body"), Some(&json!("Initial")));
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
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
        .get_document(&table, first_id)
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(read_back.get_field("body"), Some(&json!("First")));
    execution_unit
        .update_document(
            table.clone(),
            first_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            second_id,
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
            .get_document(&tenant_id, &table, first_id)
            .expect("first document should exist")
            .get_field("body"),
        Some(&json!("Tx update"))
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, second_id)
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
            document_id,
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
            .get_document(&tenant_id, &table, document_id)
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
        .delete_document(table.clone(), document_id)
        .expect("staged delete should succeed");

    let commit = execution_unit.commit().expect("commit should succeed");
    assert!(
        commit.is_none(),
        "insert followed by delete should collapse to a no-op"
    );
    let error = service
        .get_document(&tenant_id, &table, document_id)
        .expect_err("transient document should not exist");
    assert!(matches!(error, Error::DocumentNotFound(_)));
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
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("First"))]),
        )
        .expect("first staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Initial"))]),
        )
        .expect("revert staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
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
            .get_document(&tenant_id, &table, document_id)
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
        .insert_document_with_principal(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Hidden")),
            ]),
            &hidden_owner,
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
        .update_document_with_principal(
            &tenant_id,
            table.clone(),
            hidden_id,
            serde_json::Map::from_iter([("owner".to_string(), json!("user-123"))]),
            &hidden_owner,
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
        .get_document(&table, document_id)
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
        .get_document(&table, document_id)
        .expect("point read should succeed")
        .expect("document should exist");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let commit_error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(commit_error, Error::Conflict(_)));

    let read_error = execution_unit
        .get_document(&table, document_id)
        .expect_err("conflicted execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .update_document(
            table.clone(),
            document_id,
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
