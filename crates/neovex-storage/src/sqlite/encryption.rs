//! SQLCipher encryption helpers for embedded SQLite.
//!
//! This module provides SQLCipher-based encryption for `SqliteTenantStore`,
//! including connection setup, migration, and rekey operations.
//!
//! # Security Notes
//!
//! - All DEKs are passed as raw 32-byte arrays, never as passphrases
//! - `temp_store = MEMORY` is enforced to prevent plaintext temp file spills
//! - WAL is checkpointed before export to ensure all committed data is captured

use neovex_core::{Error, Result};
use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::backend::map_sqlite_error;

/// Applies SQLCipher encryption key to a connection.
///
/// This must be called before any other database operations. The key
/// is a raw 32-byte DEK that is converted to SQLCipher's hex format.
pub fn apply_encryption_key(conn: &Connection, dek: &[u8; 32]) -> Result<()> {
    let hex_key = hex::encode(dek);
    // SQLCipher raw key format: PRAGMA key = "x'<64 hex chars>'"
    conn.pragma_update(None, "key", format!("x'{hex_key}'"))
        .map_err(map_sqlite_error)?;
    Ok(())
}

/// Hardens temporary storage for encrypted databases.
///
/// Sets `temp_store = MEMORY` to prevent plaintext temp file spills,
/// which is critical when database encryption is enabled.
pub fn harden_temp_storage(conn: &Connection) -> Result<()> {
    // MEMORY mode keeps temp tables and indices in memory only
    conn.pragma_update(None, "temp_store", "MEMORY")
        .map_err(map_sqlite_error)?;
    Ok(())
}

/// Verifies that the encryption key is valid by reading from the database.
///
/// SQLCipher doesn't validate the key until actual data access. This
/// performs a quick read to trigger key validation before returning.
pub(super) fn verify_encryption_key(conn: &Connection) -> Result<()> {
    // Attempt to read the first schema row to trigger key validation without
    // scanning the full catalog on every encrypted open.
    conn.query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))
        .optional()
        .map_err(|error| {
            // SQLCipher returns "file is not a database" for wrong keys
            if error.to_string().contains("file is not a database") {
                Error::PermissionDenied(
                    "invalid encryption key or database is not encrypted".to_string(),
                )
            } else {
                map_sqlite_error(error)
            }
        })?
        .map(|()| ())
        .unwrap_or(());
    Ok(())
}

/// Exports a plaintext database to an encrypted database.
///
/// Uses SQLCipher's `sqlcipher_export` to create an encrypted copy.
/// The destination path must not already exist.
pub fn export_plaintext_to_encrypted(
    plaintext_conn: &Connection,
    encrypted_path: &std::path::Path,
    dek: &[u8; 32],
) -> Result<()> {
    // Fail-fast if target already exists to prevent accidental overwrite
    if encrypted_path.exists() {
        return Err(Error::InvalidInput(format!(
            "target path already exists: {}",
            encrypted_path.display()
        )));
    }

    let hex_key = hex::encode(dek);
    let path_str = validate_path_for_sql(encrypted_path)?;

    // Checkpoint WAL to ensure all committed data is in the main database file
    let _ = plaintext_conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");

    // Attach the encrypted destination using parameterized path.
    // SQLCipher's ATTACH requires the key inline, but the path must be sanitized.
    plaintext_conn
        .execute(
            &format!(
                "ATTACH DATABASE '{}' AS encrypted KEY \"x'{hex_key}'\"",
                path_str
            ),
            [],
        )
        .map_err(map_sqlite_error)?;

    // Export to the encrypted database
    let export_result = plaintext_conn
        .query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))
        .map_err(map_sqlite_error);

    // Always detach, even if export failed
    let detach_result = plaintext_conn
        .execute("DETACH DATABASE encrypted", [])
        .map_err(map_sqlite_error);

    // Return the first error, prioritizing export errors
    export_result?;
    detach_result?;
    Ok(())
}

/// Exports an encrypted database to a plaintext database.
///
/// Uses SQLCipher's `sqlcipher_export` to create a plaintext copy.
/// The destination path must not already exist.
pub fn export_encrypted_to_plaintext(
    encrypted_conn: &Connection,
    plaintext_path: &std::path::Path,
) -> Result<()> {
    // Fail-fast if target already exists to prevent accidental overwrite
    if plaintext_path.exists() {
        return Err(Error::InvalidInput(format!(
            "target path already exists: {}",
            plaintext_path.display()
        )));
    }

    let path_str = validate_path_for_sql(plaintext_path)?;

    // Checkpoint WAL to ensure all committed data is in the main database file
    let _ = encrypted_conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");

    // Attach the plaintext destination (empty key means no encryption)
    encrypted_conn
        .execute(
            &format!("ATTACH DATABASE '{}' AS plaintext KEY ''", path_str),
            [],
        )
        .map_err(map_sqlite_error)?;

    // Export to the plaintext database
    let export_result = encrypted_conn
        .query_row("SELECT sqlcipher_export('plaintext')", [], |_| Ok(()))
        .map_err(map_sqlite_error);

    // Always detach, even if export failed
    let detach_result = encrypted_conn
        .execute("DETACH DATABASE plaintext", [])
        .map_err(map_sqlite_error);

    // Return the first error, prioritizing export errors
    export_result?;
    detach_result?;
    Ok(())
}

/// Rotates the DEK of an encrypted database in place.
///
/// Uses SQLCipher's `PRAGMA rekey` to change the encryption key without
/// creating a copy. This rewrites all encrypted pages with the new key.
pub fn rekey_encrypted_database(conn: &Connection, new_dek: &[u8; 32]) -> Result<()> {
    let hex_key = hex::encode(new_dek);
    conn.pragma_update(None, "rekey", format!("x'{hex_key}'"))
        .map_err(map_sqlite_error)?;
    Ok(())
}

/// Validates a path is safe for use in SQL statements.
///
/// Rejects paths containing single quotes (which would break SQL string literals)
/// and non-UTF-8 paths. This prevents SQL injection through file paths.
fn validate_path_for_sql(path: &std::path::Path) -> Result<&str> {
    let path_str = path
        .to_str()
        .ok_or_else(|| Error::InvalidInput("path contains invalid UTF-8".to_string()))?;

    if path_str.contains('\'') {
        return Err(Error::InvalidInput(
            "database path must not contain single quotes".to_string(),
        ));
    }

    Ok(path_str)
}

/// Migrates a plaintext SQLite database to an encrypted database.
///
/// This is a path-based convenience wrapper that opens the source database,
/// exports to an encrypted copy, and optionally validates the result.
pub fn migrate_plaintext_to_encrypted(
    source_path: &std::path::Path,
    target_path: &std::path::Path,
    dek: &[u8; 32],
    validate: bool,
) -> Result<()> {
    // Open plaintext source
    let source_conn = Connection::open(source_path).map_err(map_sqlite_error)?;

    // Export to encrypted target
    export_plaintext_to_encrypted(&source_conn, target_path, dek)?;

    // Validate if requested
    if validate {
        validate_migration(source_path, target_path, dek)?;
    }

    Ok(())
}

/// Exports an encrypted SQLite database to a plaintext database.
///
/// This is a path-based convenience wrapper that opens the encrypted database
/// with the provided DEK and exports to a plaintext copy.
pub fn migrate_encrypted_to_plaintext(
    source_path: &std::path::Path,
    target_path: &std::path::Path,
    dek: &[u8; 32],
) -> Result<()> {
    // Open encrypted source
    let source_conn = Connection::open(source_path).map_err(map_sqlite_error)?;

    // Apply encryption key
    apply_encryption_key(&source_conn, dek)?;

    // Verify we can read from the encrypted database
    verify_encryption_key(&source_conn)?;

    // Export to plaintext target
    export_encrypted_to_plaintext(&source_conn, target_path)?;

    Ok(())
}

/// Rotates the DEK of an encrypted database at the given path.
///
/// This is a path-based convenience wrapper that opens the encrypted database,
/// applies the current DEK, and rekeys to the new DEK.
pub fn rekey_encrypted_database_at_path(
    path: &std::path::Path,
    current_dek: &[u8; 32],
    new_dek: &[u8; 32],
) -> Result<()> {
    // Open encrypted database
    let conn = Connection::open(path).map_err(map_sqlite_error)?;

    // Apply current encryption key
    apply_encryption_key(&conn, current_dek)?;

    // Verify we can read from the encrypted database
    verify_encryption_key(&conn)?;

    // Rekey to new DEK
    rekey_encrypted_database(&conn, new_dek)?;

    Ok(())
}

/// Checkpoints an encrypted SQLite database so committed WAL state is flushed
/// into the main database file before backup or cutover operations.
pub fn checkpoint_encrypted_database_at_path(
    path: &std::path::Path,
    current_dek: &[u8; 32],
) -> Result<()> {
    let conn = Connection::open(path).map_err(map_sqlite_error)?;
    apply_encryption_key(&conn, current_dek)?;
    verify_encryption_key(&conn)?;
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .map_err(map_sqlite_error)?;
    Ok(())
}

/// Validates that a migration was successful by comparing schema and row counts.
///
/// This performs three levels of validation:
/// 1. Table count equality (quick structural check)
/// 2. Schema DDL equality per table (ensures structure was preserved)
/// 3. Row count equality per table (ensures data was fully exported)
fn validate_migration(
    source_path: &std::path::Path,
    target_path: &std::path::Path,
    dek: &[u8; 32],
) -> Result<()> {
    // Open plaintext source
    let source_conn = Connection::open(source_path).map_err(map_sqlite_error)?;

    // Open encrypted target
    let target_conn = Connection::open(target_path).map_err(map_sqlite_error)?;
    apply_encryption_key(&target_conn, dek)?;
    verify_encryption_key(&target_conn)?;

    // Collect source table names and their DDL
    let source_tables = collect_table_info(&source_conn)?;
    let target_tables = collect_table_info(&target_conn)?;

    // Validate table count
    if source_tables.len() != target_tables.len() {
        return Err(Error::Internal(format!(
            "migration validation failed: source has {} tables, target has {}",
            source_tables.len(),
            target_tables.len()
        )));
    }

    // Validate schema equality and row counts per table
    for (name, source_sql, source_rows) in &source_tables {
        let target_entry = target_tables.iter().find(|(n, _, _)| n == name);
        match target_entry {
            None => {
                return Err(Error::Internal(format!(
                    "migration validation failed: table '{}' missing from target",
                    name
                )));
            }
            Some((_, target_sql, target_rows)) => {
                if source_sql != target_sql {
                    return Err(Error::Internal(format!(
                        "migration validation failed: schema mismatch for table '{}'",
                        name
                    )));
                }
                if source_rows != target_rows {
                    return Err(Error::Internal(format!(
                        "migration validation failed: row count mismatch for table '{}' \
                         (source={}, target={})",
                        name, source_rows, target_rows
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Collects table names, DDL, and row counts from a database connection.
///
/// Returns a Vec of (table_name, create_sql, row_count) tuples for all
/// user tables (excluding sqlite_* internal tables).
fn collect_table_info(conn: &Connection) -> Result<Vec<(String, String, i64)>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, sql FROM sqlite_master \
             WHERE type='table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .map_err(map_sqlite_error)?;

    let tables: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(map_sqlite_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(map_sqlite_error)?;

    let mut result = Vec::with_capacity(tables.len());
    for (name, sql) in tables {
        // Count rows in each table. Table names from sqlite_master are safe
        // identifiers (they were created by SQLite itself), but we quote them
        // defensively to handle names with special characters.
        let quoted_name = format!("\"{}\"", name.replace('"', "\"\""));
        let count: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM {}", quoted_name),
                [],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)?;
        result.push((name, sql, count));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    fn different_key() -> [u8; 32] {
        [0x43u8; 32]
    }

    #[test]
    fn test_path_with_quote_is_rejected() {
        let path = std::path::Path::new("/tmp/it's-a-trap.db");
        let result = validate_path_for_sql(path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("single quotes"));
    }

    #[test]
    fn test_apply_encryption_key_creates_encrypted_db() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");

        // Create encrypted database
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            harden_temp_storage(&conn).unwrap();
            conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", [])
                .unwrap();
            conn.execute("INSERT INTO test (id) VALUES (1)", [])
                .unwrap();
        }

        // Verify it can be reopened with correct key
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            verify_encryption_key(&conn).unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM test", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn test_wrong_key_fails_verification() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");

        // Create encrypted database
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", [])
                .unwrap();
        }

        // Try to open with wrong key
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &different_key()).unwrap();
            let result = verify_encryption_key(&conn);
            assert!(result.is_err());
            let error_msg = result.unwrap_err().to_string();
            assert!(
                error_msg.contains("invalid encryption key")
                    || error_msg.contains("file is not a database")
            );
        }
    }

    #[test]
    fn test_plaintext_to_encrypted_export() {
        let dir = TempDir::new().unwrap();
        let plaintext_path = dir.path().join("plain.db");
        let encrypted_path = dir.path().join("encrypted.db");

        // Create plaintext database
        {
            let conn = Connection::open(&plaintext_path).unwrap();
            conn.execute("CREATE TABLE test (value TEXT)", []).unwrap();
            conn.execute("INSERT INTO test (value) VALUES ('hello')", [])
                .unwrap();
        }

        // Export to encrypted
        {
            let conn = Connection::open(&plaintext_path).unwrap();
            export_plaintext_to_encrypted(&conn, &encrypted_path, &test_key()).unwrap();
        }

        // Verify encrypted database
        {
            let conn = Connection::open(&encrypted_path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            verify_encryption_key(&conn).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM test", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "hello");
        }
    }

    #[test]
    fn test_encrypted_to_plaintext_export() {
        let dir = TempDir::new().unwrap();
        let encrypted_path = dir.path().join("encrypted.db");
        let plaintext_path = dir.path().join("plain.db");

        // Create encrypted database
        {
            let conn = Connection::open(&encrypted_path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            conn.execute("CREATE TABLE test (value TEXT)", []).unwrap();
            conn.execute("INSERT INTO test (value) VALUES ('secret')", [])
                .unwrap();
        }

        // Export to plaintext
        {
            let conn = Connection::open(&encrypted_path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            export_encrypted_to_plaintext(&conn, &plaintext_path).unwrap();
        }

        // Verify plaintext database (no key needed)
        {
            let conn = Connection::open(&plaintext_path).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM test", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "secret");
        }
    }

    #[test]
    fn test_rekey_encrypted_database() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");

        // Create encrypted database with original key
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            conn.execute("CREATE TABLE test (value TEXT)", []).unwrap();
            conn.execute("INSERT INTO test (value) VALUES ('data')", [])
                .unwrap();
        }

        // Rekey to new key
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            rekey_encrypted_database(&conn, &different_key()).unwrap();
        }

        // Verify old key no longer works
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &test_key()).unwrap();
            assert!(verify_encryption_key(&conn).is_err());
        }

        // Verify new key works
        {
            let conn = Connection::open(&path).unwrap();
            apply_encryption_key(&conn, &different_key()).unwrap();
            verify_encryption_key(&conn).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM test", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "data");
        }
    }

    #[test]
    fn test_temp_store_is_memory() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");

        let conn = Connection::open(&path).unwrap();
        apply_encryption_key(&conn, &test_key()).unwrap();
        harden_temp_storage(&conn).unwrap();

        let temp_store: i64 = conn
            .query_row("PRAGMA temp_store", [], |row| row.get(0))
            .unwrap();
        // temp_store: 0 = DEFAULT, 1 = FILE, 2 = MEMORY
        assert_eq!(temp_store, 2);
    }
}
