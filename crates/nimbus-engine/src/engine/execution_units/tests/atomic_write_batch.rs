use super::*;

struct AdvancingClock {
    next_ms: AtomicU64,
}

impl AdvancingClock {
    fn new(start_ms: u64) -> Self {
        Self {
            next_ms: AtomicU64::new(start_ms),
        }
    }
}

impl WallClock for AdvancingClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.next_ms.fetch_add(1, Ordering::SeqCst))
    }
}

#[test]
fn atomic_write_batch_overwrite_creates_missing_document() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_overwrite_create");
    let document_id = DocumentId::from_key("cities/SF".replace('/', "_"))
        .expect("firestore-style leaf id should parse once isolated");
    let execution_unit = engine
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
                typed_fields: Default::default(),
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
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("created document should exist")
            .get_field("body"),
        Some(&json!("San Francisco"))
    );
}

#[test]
fn staged_atomic_write_batch_keeps_execution_unit_reusable_until_commit() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_stage_reuse");
    let document_id = DocumentId::from_key("staged-batch").expect("id should parse");
    let execution_unit = engine
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
                typed_fields: Default::default(),
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
        engine
            .get_document(&tenant_id, &table, document_id)
            .expect("staged document should commit")
            .get_field("body"),
        Some(&json!("After commit"))
    );
}

#[test]
fn atomic_write_batch_delete_missing_without_precondition_is_a_noop() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_delete_missing");
    let document_id = DocumentId::from_key("missing-doc").expect("id should parse");
    let execution_unit = engine
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
        engine.get_document(&tenant_id, &table, document_id),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn atomic_write_batch_orders_mixed_results_and_applies_atomically() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_mixed");

    let patch_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Before patch")),
            ]),
        )
        .expect("seed patch document should insert");
    let delete_id = engine
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
    let execution_unit = engine
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
                    typed_fields: Default::default(),
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
                    typed_fields: Default::default(),
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
        engine
            .get_document(&tenant_id, &table, patch_id.clone())
            .expect("patched document should exist")
            .get_field("body"),
        Some(&json!("After patch"))
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, create_id.clone())
            .expect("created document should exist")
            .get_field("body"),
        Some(&json!("Created"))
    );
    assert!(matches!(
        engine.get_document(&tenant_id, &table, delete_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn atomic_write_batch_rolls_back_on_precondition_failure() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_preconditions");

    let existing_id = engine
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
    let execution_unit = engine
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
                    typed_fields: Default::default(),
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
        engine.get_document(&tenant_id, &table, staged_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn atomic_write_batch_enforces_update_time_preconditions() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let clock = Arc::new(ManualWallClock::new(Timestamp(10_000)));
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), clock.clone(), Arc::new(NoopFaultInjector))
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_atomic_update_time_preconditions");

    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("seed document should insert");
    let inserted = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("seed document should be readable");

    clock.advance_ms(1);
    let patch_execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let patch_outcome = patch_execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Patch {
                key: locator_key(table.clone(), document_id.clone()),
                field_patch: serde_json::Map::from_iter([("body".to_string(), json!("Patched"))]),
                typed_fields: Default::default(),
                mask: vec!["body".to_string()],
                precondition: WritePrecondition::update_time(inserted.update_time),
                transforms: Vec::new(),
            }])
            .expect("batch should build"),
        )
        .expect("matching update_time precondition should allow patch");
    assert_eq!(
        patch_outcome.write_results[0].update_time,
        Some(patch_outcome.commit_time)
    );
    let patched = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("patched document should be readable");
    assert_eq!(patched.get_field("body"), Some(&json!("Patched")));
    assert_ne!(
        patched.update_time, inserted.update_time,
        "manual clock advance should make the old precondition stale"
    );

    let staged_id =
        DocumentId::from_key("staged-before-stale-update-time").expect("staged id should parse");
    let stale_execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let stale_error = stale_execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![
                AtomicWrite::Set {
                    key: locator_key(table.clone(), staged_id.clone()),
                    document: serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-123")),
                        ("body".to_string(), json!("Transient")),
                    ]),
                    typed_fields: Default::default(),
                    mode: WriteSetMode::Overwrite,
                    precondition: WritePrecondition::default(),
                    transforms: Vec::new(),
                },
                AtomicWrite::Verify {
                    key: locator_key(table.clone(), document_id.clone()),
                    precondition: WritePrecondition::update_time(inserted.update_time),
                },
            ])
            .expect("batch should build"),
        )
        .expect_err("stale update_time precondition should abort the batch");
    assert!(matches!(stale_error, Error::PreconditionFailed(_)));
    assert!(matches!(
        engine.get_document(&tenant_id, &table, staged_id.clone()),
        Err(Error::DocumentNotFound(_)),
    ));
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("stale precondition should leave document unchanged")
            .get_field("body"),
        Some(&json!("Patched"))
    );

    let missing_id =
        DocumentId::from_key("missing-update-time-target").expect("missing id should parse");
    let missing_execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let missing_error = missing_execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Verify {
                key: locator_key(table.clone(), missing_id),
                precondition: WritePrecondition::update_time(patched.update_time),
            }])
            .expect("batch should build"),
        )
        .expect_err("update_time precondition requires an existing document");
    assert!(matches!(missing_error, Error::DocumentNotFound(_)));

    clock.advance_ms(1);
    let delete_execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    delete_execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Delete {
                key: locator_key(table.clone(), document_id.clone()),
                precondition: WritePrecondition::update_time(patched.update_time),
                missing_ok: false,
            }])
            .expect("batch should build"),
        )
        .expect("matching update_time precondition should allow delete");
    assert!(matches!(
        engine.get_document(&tenant_id, &table, document_id),
        Err(Error::DocumentNotFound(_)),
    ));
}

#[test]
fn atomic_write_batch_transform_write_creates_missing_document_and_returns_ordered_results() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_transforms");
    let transformed_id = DocumentId::from_key("transform-created").expect("id should parse");
    let execution_unit = engine
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
                            values: vec![
                                stored(json!(2.0)),
                                stored(json!("a")),
                                stored(json!("a")),
                            ],
                        },
                    },
                    FieldTransform {
                        field: "tags".to_string(),
                        transform: FieldTransformOperation::RemoveAllFromArray {
                            values: vec![stored(json!(2))],
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
    let document = engine
        .get_document(&tenant_id, &table, transformed_id.clone())
        .expect("transform write should create the document");
    assert_eq!(document.get_field("count"), Some(&json!(2)));
    assert_eq!(document.get_field("ceiling"), Some(&json!(3.5)));
    assert_eq!(document.get_field("floor"), Some(&json!(7)));
    assert_eq!(document.get_field("tags"), Some(&json!(["a"])));
}

#[test]
fn atomic_write_batch_applies_array_multiply_and_bitwise_transforms() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_mongodb_transforms");
    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("tags".to_string(), json!(["a"])),
                ("vals".to_string(), json!([1, 2, 3])),
                ("count".to_string(), json!(2)),
                ("flags".to_string(), json!(0b1100_i64)),
            ]),
        )
        .expect("document should insert");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![
                    FieldTransform {
                        field: "tags".to_string(),
                        transform: FieldTransformOperation::AppendElements {
                            values: vec![stored(json!("b")), stored(json!("c"))],
                        },
                    },
                    FieldTransform {
                        field: "vals".to_string(),
                        transform: FieldTransformOperation::PopArray {
                            side: ArrayPopSide::Last,
                        },
                    },
                    FieldTransform {
                        field: "count".to_string(),
                        transform: FieldTransformOperation::Multiply {
                            operand: NumericValue::Integer { value: 3 },
                        },
                    },
                    FieldTransform {
                        field: "flags".to_string(),
                        transform: FieldTransformOperation::Bitwise {
                            operation: BitwiseOperation::And,
                            operand: 0b1010,
                        },
                    },
                    FieldTransform {
                        field: "flags".to_string(),
                        transform: FieldTransformOperation::Bitwise {
                            operation: BitwiseOperation::Or,
                            operand: 0b0001,
                        },
                    },
                ],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("transform-only write should succeed");

    assert_eq!(
        outcome.write_results[0].transform_results,
        vec![
            StoredValue::from(serde_json::Value::Null),
            StoredValue::from(serde_json::Value::Null),
            StoredValue::from(json!(6)),
            StoredValue::from(json!(0b1000)),
            StoredValue::from(json!(0b1001)),
        ]
    );
    let document = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("transformed document should exist");
    assert_eq!(document.get_field("tags"), Some(&json!(["a", "b", "c"])));
    assert_eq!(document.get_field("vals"), Some(&json!([1, 2])));
    assert_eq!(document.get_field("count"), Some(&json!(6)));
    assert_eq!(document.get_field("flags"), Some(&json!(0b1001)));
}

#[test]
fn atomic_write_batch_patch_applies_transforms_after_patch_fields() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_patch_transforms");
    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("count".to_string(), json!(40)),
            ]),
        )
        .expect("seed document should insert");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let outcome = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Patch {
                key: locator_key(table.clone(), document_id.clone()),
                field_patch: serde_json::Map::from_iter([("count".to_string(), json!(1))]),
                typed_fields: Default::default(),
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
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("patched document should exist")
            .get_field("count"),
        Some(&json!(3))
    );
}

#[test]
fn atomic_write_batch_patch_updates_nested_field_paths() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_nested_patch");
    let document_id = engine
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
    let execution_unit = engine
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
                typed_fields: Default::default(),
                mask: vec!["profile.active".to_string()],
                precondition: WritePrecondition::exists(true),
                transforms: Vec::new(),
            }])
            .expect("batch should build"),
        )
        .expect("nested patch should succeed");

    let document = engine
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
fn atomic_write_batch_nested_patch_preserves_and_clears_typed_metadata_by_path() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_nested_typed_patch");
    let document_id = DocumentId::from_key("typed-profile").expect("id should parse");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Set {
                key: locator_key(table.clone(), document_id.clone()),
                document: serde_json::Map::from_iter([
                    ("owner".to_string(), json!("user-123")),
                    (
                        "profile".to_string(),
                        json!({ "active": true, "attachment": "AQID" }),
                    ),
                ]),
                typed_fields: TypedFieldMap::from([(
                    "profile".to_string(),
                    StoredValue::Map {
                        entries: std::collections::BTreeMap::from([
                            ("active".to_string(), StoredValue::from(json!(true))),
                            (
                                "attachment".to_string(),
                                StoredValue::from(TypedScalarValue::Bytes {
                                    data: vec![1, 2, 3],
                                }),
                            ),
                        ]),
                    },
                )]),
                mode: WriteSetMode::Overwrite,
                precondition: WritePrecondition::default(),
                transforms: Vec::new(),
            }])
            .expect("typed seed batch should build"),
        )
        .expect("typed seed should commit");

    let patch = |field_patch, mask| {
        AtomicWriteBatch::new(vec![AtomicWrite::Patch {
            key: locator_key(table.clone(), document_id.clone()),
            field_patch,
            typed_fields: Default::default(),
            mask,
            precondition: WritePrecondition::exists(true),
            transforms: Vec::new(),
        }])
        .expect("patch batch should build")
    };
    engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("patch execution unit should start")
        .execute_atomic_write_batch(patch(
            serde_json::Map::from_iter([("profile".to_string(), json!({ "active": false }))]),
            vec!["profile.active".to_string()],
        ))
        .expect("plain sibling patch should commit");

    let preserved = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("patched document should exist");
    assert_eq!(
        preserved.get_field("profile"),
        Some(&json!({ "active": false, "attachment": "AQID" }))
    );
    assert!(
        preserved
            .typed_value("profile")
            .is_some_and(StoredValue::contains_typed_metadata),
        "patching one nested plain field must preserve a typed sibling"
    );

    engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("replacement execution unit should start")
        .execute_atomic_write_batch(patch(
            serde_json::Map::from_iter([("profile".to_string(), json!({ "attachment": "plain" }))]),
            vec!["profile.attachment".to_string()],
        ))
        .expect("plain typed-field replacement should commit");
    let cleared = engine
        .get_document(&tenant_id, &table, document_id)
        .expect("replaced document should exist");
    assert_eq!(
        cleared.get_field("profile"),
        Some(&json!({ "active": false, "attachment": "plain" }))
    );
    assert!(
        cleared.typed_value("profile").is_none(),
        "replacing the last typed leaf with plain JSON must clear its sidecar"
    );
}

#[test]
fn atomic_write_batch_merge_all_clears_replaced_typed_metadata() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_merge_all_typed_replacement");
    let document_id = DocumentId::from_key("typed-attachment").expect("id should parse");

    let write = |document, typed_fields, mode| {
        AtomicWriteBatch::new(vec![AtomicWrite::Set {
            key: locator_key(table.clone(), document_id.clone()),
            document,
            typed_fields,
            mode,
            precondition: WritePrecondition::default(),
            transforms: Vec::new(),
        }])
        .expect("set batch should build")
    };

    engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("seed execution unit should start")
        .execute_atomic_write_batch(write(
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("attachment".to_string(), json!("AQID")),
            ]),
            TypedFieldMap::from([(
                "attachment".to_string(),
                StoredValue::from(TypedScalarValue::Bytes {
                    data: vec![1, 2, 3],
                }),
            )]),
            WriteSetMode::Overwrite,
        ))
        .expect("typed seed should commit");

    engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("merge execution unit should start")
        .execute_atomic_write_batch(write(
            serde_json::Map::from_iter([("attachment".to_string(), json!("plain"))]),
            TypedFieldMap::new(),
            WriteSetMode::MergeAll,
        ))
        .expect("plain merge should commit");

    let merged = engine
        .get_document(&tenant_id, &table, document_id)
        .expect("merged document should exist");
    assert_eq!(merged.get_field("attachment"), Some(&json!("plain")));
    assert!(
        merged.typed_value("attachment").is_none(),
        "merge-all replacement with plain JSON must clear stale typed metadata"
    );
}

#[test]
fn atomic_write_batch_preserves_existing_numeric_type_for_equivalent_extrema() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_equivalent_extrema");
    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("count".to_string(), json!(3)),
            ]),
        )
        .expect("seed document should insert");
    let execution_unit = engine
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
    let count = engine
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
fn atomic_write_batch_rejects_non_finite_double_operands_before_storage() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_non_finite_numeric");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");

    let increment_id = DocumentId::from_key("nan-increment").expect("id should parse");
    let increment_error = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), increment_id.clone()),
                transforms: vec![FieldTransform {
                    field: "count".to_string(),
                    transform: FieldTransformOperation::Increment {
                        operand: NumericValue::Double { value: f64::NAN },
                    },
                }],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect_err("non-finite increment operand should be rejected");
    assert!(
        matches!(
            increment_error,
            Error::InvalidInput(ref message)
                if message == "increment transform operand must be a Firestore int64 or finite double"
        ),
        "unexpected increment error: {increment_error}"
    );

    let maximum_id = DocumentId::from_key("infinity-maximum").expect("id should parse");
    let maximum_error = execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), maximum_id.clone()),
                transforms: vec![FieldTransform {
                    field: "ceiling".to_string(),
                    transform: FieldTransformOperation::Maximum {
                        operand: NumericValue::Double {
                            value: f64::INFINITY,
                        },
                    },
                }],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect_err("non-finite maximum operand should be rejected");
    assert!(
        matches!(
            maximum_error,
            Error::InvalidInput(ref message)
                if message == "maximum transform operand must be a Firestore int64, finite double, or special double sentinel"
        ),
        "unexpected maximum error: {maximum_error}"
    );

    assert!(
        matches!(
            engine.get_document(&tenant_id, &table, increment_id),
            Err(Error::DocumentNotFound(_))
        ),
        "rejected increment transform must not create a document"
    );
    assert!(
        matches!(
            engine.get_document(&tenant_id, &table, maximum_id),
            Err(Error::DocumentNotFound(_))
        ),
        "rejected maximum transform must not create a document"
    );
}

#[test]
fn atomic_write_batch_applies_server_timestamp_as_typed_scalar_metadata() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(AdvancingClock::new(10_000)),
            Arc::new(NoopFaultInjector),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_atomic_server_timestamp");
    let document_id = DocumentId::from_key("server-timestamp").expect("id should parse");
    let execution_unit = engine
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
    assert_eq!(
        outcome.write_results[0].update_time,
        Some(outcome.commit_time),
        "write result update_time should be the batch commit time"
    );
    assert_eq!(
        Some(*value),
        outcome.write_results[0].update_time,
        "server timestamp transform result should share the batch commit time"
    );
    assert_eq!(
        outcome.commit.as_ref().map(|commit| commit.timestamp),
        Some(outcome.commit_time),
        "durable commit entry should use the same batch timestamp"
    );
    let document = engine
        .get_document(&tenant_id, &table, document_id)
        .expect("transformed document should exist");
    assert_eq!(
        document.update_time, outcome.commit_time,
        "stored document update_time should share the batch commit time"
    );
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_special_doubles");
    let document_id = DocumentId::from_key("special-double").expect("id should parse");
    let execution_unit = engine
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
    let document = engine
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

#[test]
fn atomic_write_batch_array_transforms_preserve_typed_elements_at_every_depth() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_atomic_typed_arrays");
    let document_id = DocumentId::from_key("typed-arrays").expect("id should parse");

    let timestamp = StoredValue::from(TypedScalarValue::FirestoreTimestamp {
        rfc3339: "2024-01-02T03:04:05.123456789Z".to_string(),
    });
    let bytes = StoredValue::from(TypedScalarValue::Bytes {
        data: vec![1, 2, 3, 4],
    });
    let geo_point = StoredValue::from(TypedScalarValue::GeoPoint {
        latitude: 37.7749,
        longitude: -122.4194,
    });
    // A typed scalar buried inside a map inside the array element: the deepest
    // nesting an array transform operand can carry.
    let nested = StoredValue::Map {
        entries: std::collections::BTreeMap::from([
            ("attachment".to_string(), bytes.clone()),
            (
                "label".to_string(),
                StoredValue::Json {
                    value: json!("kept"),
                },
            ),
        ]),
    };

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![FieldTransform {
                    field: "tags".to_string(),
                    transform: FieldTransformOperation::AppendMissingElements {
                        values: vec![
                            stored(json!("seed")),
                            timestamp.clone(),
                            bytes.clone(),
                            geo_point.clone(),
                            nested.clone(),
                            // Repeats of every kind must dedupe against the
                            // element already appended in this same transform.
                            timestamp.clone(),
                            nested.clone(),
                        ],
                    },
                }],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("typed arrayUnion should succeed");

    let document = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("typed array document should exist");
    assert_eq!(
        document.typed_value("tags"),
        Some(&StoredValue::List {
            items: vec![
                stored(json!("seed")),
                timestamp.clone(),
                bytes.clone(),
                geo_point.clone(),
                nested.clone(),
            ]
        }),
        "arrayUnion must keep typed elements and dedupe repeats by typed identity"
    );
    assert_eq!(
        document.get_field("tags"),
        Some(&json!([
            "seed",
            "2024-01-02T03:04:05.123456789Z",
            "AQIDBA==",
            { "latitude": 37.7749, "longitude": -122.4194 },
            { "attachment": "AQIDBA==", "label": "kept" },
        ])),
        "the plain JSON projection must stay in step with the typed tree"
    );

    // arrayRemove matches typed elements by value, including the nested map.
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![FieldTransform {
                    field: "tags".to_string(),
                    transform: FieldTransformOperation::RemoveAllFromArray {
                        values: vec![timestamp, geo_point, nested],
                    },
                }],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("typed arrayRemove should succeed");

    let document = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect("typed array document should exist");
    assert_eq!(
        document.typed_value("tags"),
        Some(&StoredValue::List {
            items: vec![stored(json!("seed")), bytes],
        })
    );

    // Removing the last typed element drops the sidecar so a metadata-free
    // array is stored as plain JSON rather than an inert typed tree.
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .execute_atomic_write_batch(
            AtomicWriteBatch::new(vec![AtomicWrite::Transform {
                key: locator_key(table.clone(), document_id.clone()),
                transforms: vec![FieldTransform {
                    field: "tags".to_string(),
                    transform: FieldTransformOperation::RemoveAllFromArray {
                        values: vec![StoredValue::from(TypedScalarValue::Bytes {
                            data: vec![1, 2, 3, 4],
                        })],
                    },
                }],
                precondition: WritePrecondition::default(),
            }])
            .expect("batch should build"),
        )
        .expect("final typed arrayRemove should succeed");

    let document = engine
        .get_document(&tenant_id, &table, document_id)
        .expect("typed array document should exist");
    assert_eq!(document.typed_value("tags"), None);
    assert_eq!(document.get_field("tags"), Some(&json!(["seed"])));
}

fn locator_key(table: nimbus_core::TableName, id: DocumentId) -> WriteKey {
    WriteKey::from(DocumentLocator::new(table, id))
}

fn stored(value: serde_json::Value) -> StoredValue {
    StoredValue::Json { value }
}
