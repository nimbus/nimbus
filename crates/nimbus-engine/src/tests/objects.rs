//! Object-metadata writes as fenced journal commits (SUC2.2).
//!
//! Before this lane, manifest and multipart writes ran get-then-insert on the
//! tenant read executor: sequence assignment happened outside the committer,
//! the engine's durable/applied watermarks never advanced, and two writers on
//! the same key could interleave. These tests pin the closed behavior: every
//! object write is one committer-sequenced journal commit, visible in the
//! durable journal and coherent with subsequent document mutations.

use super::*;
use nimbus_storage::{ObjectChecksums, ObjectManifest, ObjectManifestAttributes};

fn manifest_for(key: &str, etag: &str) -> ObjectManifest {
    let mut metadata = serde_json::Map::new();
    metadata.insert("owner".to_string(), json!("engine-object-tests"));
    let mut attributes = ObjectManifestAttributes::new(etag, 1_776_960_000_000);
    attributes.content_type = Some("text/plain".to_string());
    attributes.user_metadata = metadata;
    attributes.checksums = ObjectChecksums {
        content_md5: None,
        crc64nvme: None,
        sha256: Some(
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        ),
    };
    ObjectManifest::whole("bucket", key, 12, "a".repeat(64), attributes)
        .expect("manifest should build")
}

async fn engine_with_tenant(data_dir: &TempDir, tenant: &str) -> (Arc<Engine>, TenantId) {
    let engine = Arc::new(
        Engine::new_with_embedded_provider(data_dir.path(), EmbeddedProviderKind::Sqlite)
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new(tenant).expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    (engine, tenant_id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn object_meta_writes_are_sequenced_journal_commits() {
    let data_dir = tempdir().expect("object commit tempdir should build");
    let (engine, tenant_id) = engine_with_tenant(&data_dir, "object-meta-commits").await;
    let runtime = engine
        .registered_runtime_for_testing(&tenant_id)
        .expect("tenant runtime should resolve");
    let meta = engine
        .tenant_object_meta(tenant_id.clone())
        .await
        .expect("object meta handle should resolve");

    let baseline = runtime.durable_head();
    let commit = meta
        .put_manifest(manifest_for("reports/summary.txt", "\"etag-1\""))
        .await
        .expect("manifest put should commit");
    assert_eq!(
        commit.sequence,
        SequenceNumber(baseline.0 + 1),
        "manifest put must consume the next committer-assigned sequence"
    );
    assert_eq!(
        runtime.durable_head(),
        commit.sequence,
        "engine durable watermark must advance with the object commit"
    );
    assert_eq!(
        runtime.applied_head(),
        commit.sequence,
        "engine applied watermark must advance with the object commit"
    );

    // The bypass's sharpest observable: a document mutation after an object
    // write. With out-of-band sequence assignment the journal's next append
    // collides with the sequence the object write already consumed; on the
    // fenced path the mutation lands on the next sequence.
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after-object-write"))]),
        )
        .await
        .expect("document insert after an object write must not conflict");
    assert_eq!(
        runtime.durable_head(),
        SequenceNumber(commit.sequence.0 + 1),
        "document mutation must land on the sequence after the object commit"
    );

    // The commit is a real durable-journal record carrying a document write
    // on the manifest system table — not an untracked side effect.
    let records = runtime
        .store
        .read_durable_journal_from(commit.sequence)
        .expect("durable journal should read");
    let record = records
        .iter()
        .find(|record| record.sequence == commit.sequence)
        .expect("object commit must exist in the durable journal");
    assert_eq!(record.writes.len(), 1);
    assert_eq!(
        record.writes[0].table.as_str(),
        nimbus_storage::OBJECT_MANIFEST_TABLE,
        "object commit must be classified as a document write on the manifest table"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manifest_replace_preserves_creation_identity_and_delete_returns_previous() {
    let data_dir = tempdir().expect("object replace tempdir should build");
    let (engine, tenant_id) = engine_with_tenant(&data_dir, "object-meta-replace").await;
    let runtime = engine
        .registered_runtime_for_testing(&tenant_id)
        .expect("tenant runtime should resolve");
    let meta = engine
        .tenant_object_meta(tenant_id.clone())
        .await
        .expect("object meta handle should resolve");

    let first = manifest_for("logs/app.txt", "\"etag-1\"");
    let document_id = first.document_id().expect("document id should derive");
    meta.put_manifest(first)
        .await
        .expect("first manifest put should commit");
    let stored_first = runtime
        .store
        .get(
            &TableName::new(nimbus_storage::OBJECT_MANIFEST_TABLE).expect("table name"),
            &document_id,
        )
        .expect("stored manifest should read")
        .expect("stored manifest should exist");

    meta.put_manifest(manifest_for("logs/app.txt", "\"etag-2\""))
        .await
        .expect("manifest replace should commit");
    let stored_second = runtime
        .store
        .get(
            &TableName::new(nimbus_storage::OBJECT_MANIFEST_TABLE).expect("table name"),
            &document_id,
        )
        .expect("replaced manifest should read")
        .expect("replaced manifest should exist");
    assert_eq!(
        stored_second.creation_time, stored_first.creation_time,
        "replace must preserve the stored row's creation identity"
    );
    let replaced = meta
        .get_manifest("bucket".to_string(), "logs/app.txt".to_string())
        .await
        .expect("manifest should read")
        .expect("manifest should exist");
    assert_eq!(replaced.etag, "\"etag-2\"");

    let head_before_delete = runtime.durable_head();
    let (delete_commit, removed) = meta
        .delete_manifest("bucket".to_string(), "logs/app.txt".to_string())
        .await
        .expect("manifest delete should commit")
        .expect("manifest delete should find the stored manifest");
    assert_eq!(removed.etag, "\"etag-2\"");
    assert_eq!(
        delete_commit.sequence,
        SequenceNumber(head_before_delete.0 + 1)
    );

    // Deleting an absent manifest consumes no sequence and commits nothing.
    let head_after_delete = runtime.durable_head();
    let absent = meta
        .delete_manifest("bucket".to_string(), "logs/app.txt".to_string())
        .await
        .expect("absent delete should succeed");
    assert!(absent.is_none(), "absent delete must report no manifest");
    assert_eq!(
        runtime.durable_head(),
        head_after_delete,
        "absent delete must not consume a journal sequence"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multipart_upload_writes_commit_and_roundtrip() {
    let data_dir = tempdir().expect("multipart tempdir should build");
    let (engine, tenant_id) = engine_with_tenant(&data_dir, "object-meta-multipart").await;
    let runtime = engine
        .registered_runtime_for_testing(&tenant_id)
        .expect("tenant runtime should resolve");
    let meta = engine
        .tenant_object_meta(tenant_id.clone())
        .await
        .expect("object meta handle should resolve");

    let upload = nimbus_storage::ObjectMultipartUpload::new(
        "upload-0001",
        "bucket",
        "big/file.bin",
        Some("application/octet-stream".to_string()),
        serde_json::Map::new(),
        1_776_960_000_000,
    )
    .expect("upload should build");
    let baseline = runtime.durable_head();
    let commit = meta
        .put_multipart_upload(upload)
        .await
        .expect("multipart put should commit");
    assert_eq!(commit.sequence, SequenceNumber(baseline.0 + 1));
    assert_eq!(
        commit.writes[0].table.as_str(),
        nimbus_storage::OBJECT_MULTIPART_TABLE
    );

    let read_back = meta
        .get_multipart_upload("upload-0001".to_string())
        .await
        .expect("multipart get should read")
        .expect("multipart upload should exist");
    assert_eq!(read_back.key, "big/file.bin");

    let (delete_commit, removed) = meta
        .delete_multipart_upload("upload-0001".to_string())
        .await
        .expect("multipart delete should commit")
        .expect("multipart delete should find the upload");
    assert_eq!(removed.upload_id, "upload-0001");
    assert_eq!(
        delete_commit.sequence,
        SequenceNumber(commit.sequence.0 + 1)
    );
    assert!(
        meta.get_multipart_upload("upload-0001".to_string())
            .await
            .expect("multipart get should read")
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_object_and_document_writers_serialize_without_conflict() {
    let data_dir = tempdir().expect("object race tempdir should build");
    let (engine, tenant_id) = engine_with_tenant(&data_dir, "object-meta-race").await;
    let runtime = engine
        .registered_runtime_for_testing(&tenant_id)
        .expect("tenant runtime should resolve");

    let baseline = runtime.durable_head();
    const WRITERS: usize = 4;
    const ROUNDS: usize = 8;
    let mut handles = Vec::new();
    for writer in 0..WRITERS {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        handles.push(tokio::spawn(async move {
            let meta = engine
                .tenant_object_meta(tenant_id.clone())
                .await
                .expect("object meta handle should resolve");
            for round in 0..ROUNDS {
                if writer % 2 == 0 {
                    // Even writers hammer the same manifest key.
                    meta.put_manifest(manifest_for(
                        "contended/key.txt",
                        &format!("\"etag-{writer}-{round}\""),
                    ))
                    .await
                    .expect("contended manifest put should commit");
                } else {
                    // Odd writers interleave ordinary document mutations.
                    engine
                        .insert_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([(
                                "writer".to_string(),
                                json!(format!("{writer}-{round}")),
                            )]),
                        )
                        .await
                        .expect("interleaved document insert should commit");
                }
            }
        }));
    }
    for handle in handles {
        handle.await.expect("writer task should join");
    }

    let total = (WRITERS * ROUNDS) as u64;
    assert_eq!(
        runtime.durable_head(),
        SequenceNumber(baseline.0 + total),
        "every object and document write must consume exactly one sequence"
    );
    assert_eq!(runtime.applied_head(), runtime.durable_head());

    let meta = engine
        .tenant_object_meta(tenant_id.clone())
        .await
        .expect("object meta handle should resolve");
    let survivor = meta
        .get_manifest("bucket".to_string(), "contended/key.txt".to_string())
        .await
        .expect("contended manifest should read")
        .expect("contended manifest should exist");
    assert!(
        survivor.etag.starts_with("\"etag-"),
        "surviving manifest must be one of the racing writers' images"
    );
}
