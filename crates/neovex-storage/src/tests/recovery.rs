use super::*;

#[test]
fn shadow_materializer_rebuild_from_checkpoint_and_journal_matches_live_state() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");

    let first = sample_document("tasks", "first");
    live.insert(&first).expect("first insert should succeed");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");

    let second = sample_document("tasks", "second");
    live.insert(&second).expect("second insert should succeed");
    live.update(
        &table,
        &first.id,
        &serde_json::Map::from_iter([("title".to_string(), json!("first-updated"))]),
    )
    .expect("update should succeed");

    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let materializer = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint,
        journal_tail,
        ShadowMaterializerConfig {
            compaction_threshold_records: 10,
        },
    )
    .expect("shadow materializer should rebuild");

    let live_snapshot = live
        .export_materialized_journal_snapshot()
        .expect("live snapshot should export");
    assert_eq!(materializer.current_snapshot(), live_snapshot);
}

#[test]
fn shadow_materializer_compaction_is_deterministic_for_same_input() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    for title in ["alpha", "beta", "gamma"] {
        live.insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }
    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 2,
    };

    let left = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint.clone(),
        journal_tail.clone(),
        config,
    )
    .expect("left materializer should build");
    let right = ShadowMaterializer::from_checkpoint_and_journal(checkpoint, journal_tail, config)
        .expect("right materializer should build");

    assert_eq!(left.current_snapshot(), right.current_snapshot());
    assert_eq!(left.manifest(), right.manifest());
}

#[test]
fn shadow_materializer_recovery_from_checkpoint_and_pending_journal_restores_same_state() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    for title in ["alpha", "beta"] {
        live.insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }
    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 10,
    };
    let materializer =
        ShadowMaterializer::from_checkpoint_and_journal(checkpoint.clone(), journal_tail, config)
            .expect("materializer should build");

    let recovered = ShadowMaterializer::recover(
        checkpoint,
        materializer.pending_records().to_vec(),
        materializer.manifest().clone(),
        config,
    )
    .expect("materializer should recover");

    assert_eq!(
        recovered.current_snapshot(),
        materializer.current_snapshot()
    );
    assert_eq!(recovered.manifest(), materializer.manifest());
}

#[test]
fn shadow_materializer_recovery_after_interrupted_compaction_converges_to_clean_rebuild() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "alpha");
    live.insert(&first).expect("first insert should succeed");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");

    for title in ["beta", "gamma", "delta"] {
        live.insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }

    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 2,
    };

    let clean = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint.clone(),
        journal_tail.clone(),
        config,
    )
    .expect("clean shadow materializer should rebuild");

    let interrupted_manifest = ShadowMaterializerManifest {
        version: 1,
        checkpoint_sequence: checkpoint.applied_sequence,
        current_sequence: SequenceNumber(4),
        pending_record_count: journal_tail.len(),
        compaction_runs: 0,
        compaction_threshold_records: config.compaction_threshold_records,
    };
    let recovered =
        ShadowMaterializer::recover(checkpoint, journal_tail, interrupted_manifest, config)
            .expect("recovery after interrupted compaction should succeed");

    assert_eq!(recovered.current_snapshot(), clean.current_snapshot());
    assert_eq!(recovered.manifest(), clean.manifest());
}

#[test]
fn shadow_materializer_rejects_corrupted_journal_input() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "alpha");
    live.insert(&first).expect("first insert should succeed");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    live.insert(&sample_document("tasks", "beta"))
        .expect("second insert should succeed");

    let mut journal_tail = live
        .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
        .expect("journal tail should read");
    journal_tail[0].integrity_sha256[0] ^= 0xff;

    let error = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint,
        journal_tail,
        ShadowMaterializerConfig {
            compaction_threshold_records: 4,
        },
    )
    .expect_err("corrupted journal input should be rejected");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("failed integrity verification"))
    );
}

#[test]
fn shadow_materializer_rejects_corrupted_manifest_recovery_input() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    live.insert(&sample_document("tasks", "alpha"))
        .expect("insert should succeed");

    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 8,
    };
    let materializer = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint.clone(),
        journal_tail.clone(),
        config,
    )
    .expect("materializer should rebuild");

    let mut corrupted_manifest = materializer.manifest().clone();
    corrupted_manifest.pending_record_count += 1;
    let error = ShadowMaterializer::recover(checkpoint, journal_tail, corrupted_manifest, config)
        .expect_err("corrupted manifest should be rejected");
    assert!(matches!(error, Error::InvalidInput(message) if message.contains("pending count")));
}
