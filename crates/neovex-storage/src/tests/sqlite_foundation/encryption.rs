//! Integration tests for encrypted SQLite tenant stores.

use super::support::*;

fn test_dek() -> [u8; 32] {
    [0x42u8; 32]
}

fn different_dek() -> [u8; 32] {
    [0x43u8; 32]
}

#[test]
fn encrypted_sqlite_store_create_and_reopen() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    let doc = sample_document("tasks", "encrypted task");

    // Create encrypted store and insert document
    {
        let store = SqliteTenantStore::open_encrypted(&path, &test_dek())
            .expect("encrypted store should open");
        assert!(store.is_encrypted());
        store.insert(&doc).expect("insert should succeed");
    }

    // Reopen and verify data is accessible
    {
        let store = SqliteTenantStore::open_encrypted(&path, &test_dek())
            .expect("encrypted store should reopen");
        let table = TableName::new("tasks").expect("table name should be valid");
        let docs = store.scan_table(&table).expect("scan should succeed");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields["title"], json!("encrypted task"));
    }
}

#[test]
fn encrypted_sqlite_store_wrong_key_fails() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    // Create encrypted store
    {
        let store = SqliteTenantStore::open_encrypted(&path, &test_dek())
            .expect("encrypted store should open");
        let doc = sample_document("tasks", "secret");
        store.insert(&doc).expect("insert should succeed");
    }

    // Try to open with wrong key
    let error = match SqliteTenantStore::open_encrypted(&path, &different_dek()) {
        Ok(_) => panic!("should not open with wrong key"),
        Err(e) => e.to_string(),
    };
    assert!(
        error.contains("invalid encryption key") || error.contains("file is not a database"),
        "unexpected error: {error}"
    );
}

#[test]
fn plaintext_store_cannot_open_encrypted() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    // Create encrypted store
    {
        let store = SqliteTenantStore::open_encrypted(&path, &test_dek())
            .expect("encrypted store should open");
        let doc = sample_document("tasks", "secret");
        store.insert(&doc).expect("insert should succeed");
    }

    // Try to open as plaintext
    let error = match SqliteTenantStore::open(&path) {
        Ok(_) => panic!("plaintext open should fail for encrypted database"),
        Err(e) => e.to_string(),
    };
    assert!(
        error.contains("file is not a database"),
        "unexpected error: {error}"
    );
}

#[test]
fn plaintext_to_encrypted_migration() {
    let dir = tempdir().expect("temporary directory should create");
    let plaintext_path = dir.path().join("plaintext.sqlite3");
    let encrypted_path = dir.path().join("encrypted.sqlite3");

    // Create plaintext store with data
    {
        let store = SqliteTenantStore::open(&plaintext_path).expect("plaintext store should open");
        assert!(!store.is_encrypted());
        let doc = sample_document("tasks", "original");
        store.insert(&doc).expect("insert should succeed");
    }

    // Export to encrypted using raw connection
    {
        let conn = rusqlite::Connection::open(&plaintext_path).expect("connection should open");
        crate::sqlite::encryption::export_plaintext_to_encrypted(
            &conn,
            &encrypted_path,
            &test_dek(),
        )
        .expect("export should succeed");
    }

    // Verify encrypted database has the data
    {
        let store = SqliteTenantStore::open_encrypted(&encrypted_path, &test_dek())
            .expect("encrypted store should open");
        let table = TableName::new("tasks").expect("table name should be valid");
        let docs = store.scan_table(&table).expect("scan should succeed");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields["title"], json!("original"));
    }
}

#[test]
fn encrypted_to_plaintext_export() {
    let dir = tempdir().expect("temporary directory should create");
    let encrypted_path = dir.path().join("encrypted.sqlite3");
    let plaintext_path = dir.path().join("plaintext.sqlite3");

    // Create encrypted store with data
    {
        let store = SqliteTenantStore::open_encrypted(&encrypted_path, &test_dek())
            .expect("encrypted store should open");
        let doc = sample_document("tasks", "secret data");
        store.insert(&doc).expect("insert should succeed");
    }

    // Export to plaintext
    {
        let conn = rusqlite::Connection::open(&encrypted_path).expect("connection should open");
        crate::sqlite::encryption::apply_encryption_key(&conn, &test_dek())
            .expect("key should apply");
        crate::sqlite::encryption::export_encrypted_to_plaintext(&conn, &plaintext_path)
            .expect("export should succeed");
    }

    // Verify plaintext database has the data
    {
        let store = SqliteTenantStore::open(&plaintext_path).expect("plaintext store should open");
        let table = TableName::new("tasks").expect("table name should be valid");
        let docs = store.scan_table(&table).expect("scan should succeed");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields["title"], json!("secret data"));
    }
}

#[test]
fn rekey_encrypted_database() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    // Create encrypted store with data
    {
        let store = SqliteTenantStore::open_encrypted(&path, &test_dek())
            .expect("encrypted store should open");
        let doc = sample_document("tasks", "rekey test");
        store.insert(&doc).expect("insert should succeed");
    }

    // Rekey with a raw connection
    {
        let conn = rusqlite::Connection::open(&path).expect("connection should open");
        crate::sqlite::encryption::apply_encryption_key(&conn, &test_dek())
            .expect("key should apply");
        crate::sqlite::encryption::rekey_encrypted_database(&conn, &different_dek())
            .expect("rekey should succeed");
    }

    // Old key should not work
    {
        let result = SqliteTenantStore::open_encrypted(&path, &test_dek());
        assert!(result.is_err());
    }

    // New key should work
    {
        let store = SqliteTenantStore::open_encrypted(&path, &different_dek())
            .expect("rekeyed store should open");
        let table = TableName::new("tasks").expect("table name should be valid");
        let docs = store.scan_table(&table).expect("scan should succeed");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields["title"], json!("rekey test"));
    }
}

#[test]
fn encrypted_store_schema_operations() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    let store =
        SqliteTenantStore::open_encrypted(&path, &test_dek()).expect("encrypted store should open");

    // Define schema
    let schema = ranked_tasks_schema();
    store
        .execute_write(|txn| txn.save_table_schema(&schema))
        .expect("schema save should succeed");

    // Insert document
    let doc = ranked_document(&schema.table, "high priority", 1);
    store.insert(&doc).expect("insert should succeed");

    // Verify schema persisted
    let loaded_schema = store
        .read_snapshot()
        .expect("snapshot should read")
        .load_schema()
        .expect("schema should load");
    assert_eq!(loaded_schema.tables.len(), 1);
}

#[test]
fn encrypted_store_journal_operations() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    let store =
        SqliteTenantStore::open_encrypted(&path, &test_dek()).expect("encrypted store should open");

    // Insert document (generates commit log entry)
    let doc = sample_document("tasks", "journaled");
    let entry = store.insert(&doc).expect("insert should succeed");

    // Verify commit was recorded
    assert!(entry.sequence.0 > 0);

    // Verify journal can be read back
    let snapshot = store.read_snapshot().expect("snapshot should read");
    let bootstrap = snapshot
        .export_durable_journal_bootstrap()
        .expect("bootstrap should read");
    // The applied sequence should match what we inserted
    assert!(bootstrap.snapshot.applied_sequence.0 >= entry.sequence.0);
}

#[test]
fn encrypted_store_multiple_writes_and_reads() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("encrypted.sqlite3");

    let store =
        SqliteTenantStore::open_encrypted(&path, &test_dek()).expect("encrypted store should open");

    // Insert multiple documents
    let table = TableName::new("tasks").expect("table name should be valid");
    for i in 0..10 {
        let doc = sample_document("tasks", &format!("task {i}"));
        store.insert(&doc).expect("insert should succeed");
    }

    // Verify all documents are accessible
    let docs = store.scan_table(&table).expect("scan should succeed");
    assert_eq!(docs.len(), 10);

    // Verify concurrent read connections work (acquire two snapshots at once)
    let _snapshot1 = store.read_snapshot().expect("first snapshot should read");
    let _snapshot2 = store.read_snapshot().expect("second snapshot should read");
    // If we got here, concurrent reads work
}
