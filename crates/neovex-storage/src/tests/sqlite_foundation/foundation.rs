use super::support::*;

#[test]
fn sqlite_store_initializes_wal_foundation_and_empty_progress() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");

    assert_eq!(
        store.journal_mode().expect("journal mode should read"),
        "wal",
        "sqlite foundation should enable WAL mode for tenant files"
    );
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(0),
            applied_head: SequenceNumber(0),
        }
    );
    assert!(
        store
            .metadata_blob("missing")
            .expect("metadata read should succeed")
            .is_none(),
        "new sqlite foundations should start with empty metadata"
    );
}

#[test]
fn sqlite_store_enforces_direct_read_connection_limit() {
    let dir = tempdir().expect("temporary directory should create");
    let store =
        SqliteTenantStore::open_with_max_read_connections(dir.path().join("tenant.sqlite3"), 1)
            .expect("sqlite tenant store should open with explicit read limit");

    let first_snapshot = store
        .read_snapshot()
        .expect("first direct sqlite read snapshot should acquire the only connection");
    let error = match store.read_snapshot() {
        Ok(_) => {
            panic!("second direct sqlite read snapshot should exhaust the explicit pool limit")
        }
        Err(error) => error,
    };
    assert!(
        matches!(error, Error::ResourceExhausted(message) if message.contains("sqlite read connection pool exhausted")),
        "direct callers should get an explicit resource-exhausted error once the store-level pool limit is hit"
    );

    drop(first_snapshot);

    store
        .read_snapshot()
        .expect("released sqlite read connection should be reusable after the snapshot drops");
}

#[test]
fn sqlite_store_reopens_with_typed_scalar_metadata_intact() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let mut document = Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("Typed"))]),
    );
    document.set_typed_field(
        "updatedAt",
        neovex_core::TypedScalarValue::Timestamp {
            value: Timestamp(4_321),
        },
    );
    document.set_typed_field(
        "floor",
        neovex_core::TypedScalarValue::SpecialDouble {
            value: neovex_core::SpecialDouble::Nan,
        },
    );

    {
        let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
        store.insert(&document).expect("insert should succeed");
    }

    let reopened = SqliteTenantStore::open(&path).expect("sqlite tenant store should reopen");
    let fetched = reopened
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(
        fetched.typed_field("updatedAt"),
        Some(&neovex_core::TypedScalarValue::Timestamp {
            value: Timestamp(4_321),
        })
    );
    assert_eq!(fetched.get_field("updatedAt"), Some(&json!(4321_u64)));
    assert_eq!(
        fetched.typed_field("floor"),
        Some(&neovex_core::TypedScalarValue::SpecialDouble {
            value: neovex_core::SpecialDouble::Nan,
        })
    );
    assert_eq!(fetched.get_field("floor"), Some(&json!("NaN")));
}
