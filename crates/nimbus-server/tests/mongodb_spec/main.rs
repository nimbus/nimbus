#[allow(dead_code)]
mod bson_corpus;
#[allow(dead_code)]
mod executor;
#[allow(dead_code)]
mod runner;
#[allow(dead_code)]
mod wire_client;

use std::path::PathBuf;

use runner::{TestResult, classify_operations, parse_spec_file};

fn spec_test_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home).join("src/github.com/mongodb/specifications/source");
    if path.exists() { Some(path) } else { None }
}

#[test]
fn spec_runner_parses_find_test_file() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let path = spec_dir.join("crud/tests/unified/find.yml");
    if !path.exists() {
        eprintln!("SKIP: find.yml not found");
        return;
    }

    let spec = parse_spec_file(&path).expect("should parse find.yml");
    assert!(!spec.description.is_empty());
    assert!(!spec.tests.is_empty());
    assert!(!spec.create_entities.is_empty());
    assert!(!spec.initial_data.is_empty());

    eprintln!(
        "find.yml: {} tests, {} entities, {} initial data sets",
        spec.tests.len(),
        spec.create_entities.len(),
        spec.initial_data.len(),
    );
}

#[test]
fn spec_runner_parses_insert_test_file() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let path = spec_dir.join("crud/tests/unified/insertOne.yml");
    if !path.exists() {
        eprintln!("SKIP: insertOne.yml not found");
        return;
    }

    let spec = parse_spec_file(&path).expect("should parse insertOne.yml");
    assert!(!spec.description.is_empty());
    assert!(!spec.tests.is_empty());
}

#[test]
fn spec_runner_classifies_find_operations() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let path = spec_dir.join("crud/tests/unified/find.yml");
    if !path.exists() {
        eprintln!("SKIP: find.yml not found");
        return;
    }

    let spec = parse_spec_file(&path).expect("should parse find.yml");
    let classification = classify_operations(&spec);

    eprintln!("find.yml classification:");
    eprintln!("  Supported: {} tests", classification.supported.len());
    eprintln!("  Unsupported: {} tests", classification.unsupported.len());
    for (desc, ops) in &classification.unsupported {
        eprintln!("    {desc}: {:?}", ops);
    }

    assert!(
        !classification.supported.is_empty(),
        "at least some find tests should be classifiable as supported"
    );
}

#[test]
fn spec_runner_crud_classification_report() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let crud_dir = spec_dir.join("crud/tests/unified");
    if !crud_dir.exists() {
        eprintln!("SKIP: CRUD spec tests not found");
        return;
    }

    let mut total_files = 0;
    let mut total_tests = 0;
    let mut total_supported = 0;
    let mut total_unsupported = 0;
    let mut parse_errors = 0;

    let mut entries: Vec<_> = std::fs::read_dir(&crud_dir)
        .expect("should read crud dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in &entries {
        total_files += 1;
        match parse_spec_file(&entry.path()) {
            Ok(spec) => {
                let classification = classify_operations(&spec);
                total_tests += classification.supported.len() + classification.unsupported.len();
                total_supported += classification.supported.len();
                total_unsupported += classification.unsupported.len();
            }
            Err(e) => {
                eprintln!(
                    "  PARSE ERROR: {} — {e}",
                    entry.path().file_name().unwrap().to_string_lossy()
                );
                parse_errors += 1;
            }
        }
    }

    eprintln!("\nCRUD Spec Test Classification Report:");
    eprintln!("  Total files:  {total_files}");
    eprintln!("  Parse errors: {parse_errors}");
    eprintln!("  Total tests:  {total_tests}");
    eprintln!("  Supported:    {total_supported}");
    eprintln!("  Unsupported:  {total_unsupported}");

    if total_tests > 0 {
        let pct = (total_supported as f64 / total_tests as f64) * 100.0;
        eprintln!("  Coverage:     {pct:.1}%");
    }

    assert!(total_files > 0, "should find CRUD spec test files");
    assert!(parse_errors == 0, "all YAML files should parse cleanly");
}

#[test]
fn spec_runner_parses_initial_data_documents() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let path = spec_dir.join("crud/tests/unified/find.yml");
    if !path.exists() {
        eprintln!("SKIP: find.yml not found");
        return;
    };

    let spec = parse_spec_file(&path).expect("should parse");
    assert!(!spec.initial_data.is_empty());

    let first = &spec.initial_data[0];
    assert!(!first.collection_name.is_empty());
    assert!(!first.documents.is_empty());

    let first_doc = &first.documents[0];
    assert!(first_doc.get("_id").is_some(), "documents should have _id");
}

#[test]
fn spec_runner_parses_operations_with_arguments() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let path = spec_dir.join("crud/tests/unified/find.yml");
    if !path.exists() {
        eprintln!("SKIP: find.yml not found");
        return;
    }

    let spec = parse_spec_file(&path).expect("should parse");
    let first_test = &spec.tests[0];
    assert!(!first_test.operations.is_empty());

    let first_op = &first_test.operations[0];
    assert_eq!(first_op.name, "find");
    assert!(!first_op.object.is_empty());
}

#[tokio::test]
async fn spec_executor_runs_find_tests() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let path = spec_dir.join("crud/tests/unified/find.yml");
    if !path.exists() {
        eprintln!("SKIP: find.yml not found");
        return;
    }

    let spec = parse_spec_file(&path).expect("should parse find.yml");
    let fixture = executor::SpecTestFixture::new().await;
    let results = executor::execute_spec_file(&fixture, &spec).await;

    let pass = results
        .iter()
        .filter(|(_, r)| matches!(r, TestResult::Pass))
        .count();
    let fail = results
        .iter()
        .filter(|(_, r)| matches!(r, TestResult::Fail(_)))
        .count();
    let skip = results
        .iter()
        .filter(|(_, r)| matches!(r, TestResult::Skip(_)))
        .count();

    eprintln!("find.yml execution: {pass} pass, {fail} fail, {skip} skip");
    for (desc, result) in &results {
        match result {
            TestResult::Pass => eprintln!("  PASS: {desc}"),
            TestResult::Skip(r) => eprintln!("  SKIP: {desc} — {r}"),
            TestResult::Fail(r) => eprintln!("  FAIL: {desc} — {r}"),
        }
    }

    assert!(
        pass > 0,
        "at least one find test should pass via wire protocol"
    );
}

#[tokio::test]
async fn spec_executor_crud_execution_report() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let crud_dir = spec_dir.join("crud/tests/unified");
    if !crud_dir.exists() {
        eprintln!("SKIP: CRUD spec tests not found");
        return;
    };

    let core_files = [
        "find.yml",
        "insertOne.yml",
        "insertMany.yml",
        "updateOne.yml",
        "updateMany.yml",
        "deleteOne.yml",
        "deleteMany.yml",
        "aggregate.yml",
        "distinct.yml",
    ];

    let mut total_pass = 0;
    let mut total_fail = 0;
    let mut total_skip = 0;

    for file_name in &core_files {
        let path = crud_dir.join(file_name);
        if !path.exists() {
            eprintln!("  SKIP (not found): {file_name}");
            total_skip += 1;
            continue;
        }

        let spec = match parse_spec_file(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  PARSE ERROR: {file_name} — {e}");
                total_fail += 1;
                continue;
            }
        };

        let fixture_per_file = executor::SpecTestFixture::new().await;
        let results = executor::execute_spec_file(&fixture_per_file, &spec).await;

        let pass = results
            .iter()
            .filter(|(_, r)| matches!(r, TestResult::Pass))
            .count();
        let fail = results
            .iter()
            .filter(|(_, r)| matches!(r, TestResult::Fail(_)))
            .count();
        let skip = results
            .iter()
            .filter(|(_, r)| matches!(r, TestResult::Skip(_)))
            .count();

        eprintln!("  {file_name}: {pass} pass, {fail} fail, {skip} skip");
        total_pass += pass;
        total_fail += fail;
        total_skip += skip;
    }

    eprintln!("\nCRUD Core Execution Report:");
    eprintln!(
        "  Total: {} pass, {} fail, {} skip",
        total_pass, total_fail, total_skip
    );

    if total_pass + total_fail > 0 {
        let pct = (total_pass as f64 / (total_pass + total_fail) as f64) * 100.0;
        eprintln!("  Pass rate: {pct:.1}%");
    }

    assert!(total_pass > 0, "at least some CRUD tests should pass");
}

#[test]
fn bson_corpus_roundtrip_report() {
    let Some(spec_dir) = spec_test_dir() else {
        eprintln!("SKIP: MongoDB spec repo not found");
        return;
    };

    let corpus_dir = spec_dir.join("bson-corpus/tests");
    if !corpus_dir.exists() {
        eprintln!("SKIP: BSON corpus tests not found");
        return;
    }

    let mut entries: Vec<_> = std::fs::read_dir(&corpus_dir)
        .expect("should read corpus dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.path());

    let mut total_valid_pass = 0;
    let mut total_valid_fail = 0;
    let mut total_valid_skip = 0;
    let mut total_roundtrip_pass = 0;
    let mut total_roundtrip_fail = 0;
    let mut total_decode_error_pass = 0;
    let mut total_decode_error_fail = 0;
    let mut total_files = 0;
    let mut all_failures: Vec<String> = Vec::new();

    for entry in &entries {
        let file_name = entry
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let corpus = match bson_corpus::parse_corpus_file(&entry.path()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  PARSE ERROR: {file_name} — {e}");
                continue;
            }
        };

        total_files += 1;
        let result = bson_corpus::run_corpus_file(&corpus, &file_name);

        total_valid_pass += result.valid_pass;
        total_valid_fail += result.valid_fail;
        total_valid_skip += result.valid_skip;
        total_roundtrip_pass += result.roundtrip_pass;
        total_roundtrip_fail += result.roundtrip_fail;
        total_decode_error_pass += result.decode_error_pass;
        total_decode_error_fail += result.decode_error_fail;
        all_failures.extend(result.failures);
    }

    eprintln!("\nBSON Corpus Report:");
    eprintln!("  Files:              {total_files}");
    eprintln!(
        "  Valid decode:       {total_valid_pass} pass, {total_valid_fail} fail, {total_valid_skip} skip"
    );
    eprintln!("  Bridge roundtrip:   {total_roundtrip_pass} pass, {total_roundtrip_fail} fail");
    eprintln!(
        "  Decode errors:      {total_decode_error_pass} pass, {total_decode_error_fail} fail"
    );

    if total_roundtrip_pass + total_roundtrip_fail > 0 {
        let pct = (total_roundtrip_pass as f64
            / (total_roundtrip_pass + total_roundtrip_fail) as f64)
            * 100.0;
        eprintln!("  Roundtrip rate:     {pct:.1}%");
    }

    for f in &all_failures {
        eprintln!("  FAIL: {f}");
    }

    assert!(total_files > 0, "should find BSON corpus files");
    assert!(total_valid_pass > 0, "should decode valid BSON");
    assert!(total_roundtrip_pass > 0, "should roundtrip through bridge");
}

#[tokio::test]
async fn handshake_hello_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "hello": 1, "helloOk": true, "$db": "admin" })
        .await;

    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_i32("maxBsonObjectSize").is_ok());
    assert!(resp.get_i32("maxMessageSizeBytes").is_ok());
    assert!(resp.get_i32("maxWriteBatchSize").is_ok());
    assert!(resp.get_i32("minWireVersion").is_ok());
    assert!(resp.get_i32("maxWireVersion").is_ok());
    assert!(resp.get_i64("connectionId").is_ok());
    assert!(!resp.get_bool("readOnly").unwrap());
    assert!(resp.get_bool("helloOk").unwrap());
    assert!(resp.get_bool("isWritablePrimary").unwrap());

    eprintln!(
        "hello response: maxWireVersion={}",
        resp.get_i32("maxWireVersion").unwrap()
    );
}

#[tokio::test]
async fn handshake_ismaster_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "isMaster": 1, "$db": "admin" })
        .await;

    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_bool("ismaster").unwrap());
    assert!(resp.get_i32("maxWireVersion").is_ok());
    assert!(resp.get_i32("maxBsonObjectSize").is_ok());
}

#[tokio::test]
async fn handshake_build_info_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "buildInfo": 1, "$db": "admin" })
        .await;

    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_str("version").is_ok());
    assert!(resp.get_str("gitVersion").is_ok());
    let version_array = resp.get_array("versionArray").unwrap();
    assert_eq!(version_array.len(), 4);
    assert_eq!(resp.get_i32("bits").unwrap(), 64);
}

#[tokio::test]
async fn handshake_sasl_supported_mechs() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! {
            "hello": 1,
            "saslSupportedMechs": "admin.testuser",
            "$db": "admin",
        })
        .await;

    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    let mechs = resp.get_array("saslSupportedMechs").unwrap();
    assert!(!mechs.is_empty());
    assert_eq!(mechs[0].as_str().unwrap(), "SCRAM-SHA-256");
}

#[tokio::test]
async fn collection_create_and_list_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "create": "wire_test_col", "$db": "testdb" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);

    let resp = client
        .command(&bson::doc! { "listCollections": 1, "$db": "testdb" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    let cursor = resp.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    let names: Vec<&str> = batch
        .iter()
        .filter_map(|b| b.as_document().and_then(|d| d.get_str("name").ok()))
        .collect();
    assert!(names.contains(&"wire_test_col"));
}

#[tokio::test]
async fn collection_create_drop_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "create": "to_drop", "$db": "testdb" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);

    let resp = client.drop_collection("testdb", "to_drop").await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);

    let resp = client
        .command(&bson::doc! { "listCollections": 1, "$db": "testdb" })
        .await;
    let cursor = resp.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    let names: Vec<&str> = batch
        .iter()
        .filter_map(|b| b.as_document().and_then(|d| d.get_str("name").ok()))
        .collect();
    assert!(!names.contains(&"to_drop"));
}

#[tokio::test]
async fn index_list_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    client
        .insert(
            "testdb",
            "idx_col",
            &[bson::doc! { "_id": "d1", "name": "Alice", "age": 30 }],
        )
        .await;

    let resp = client
        .command(&bson::doc! { "listIndexes": "idx_col", "$db": "testdb" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    let cursor = resp.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    let id_idx = batch[0].as_document().unwrap();
    assert_eq!(id_idx.get_str("name").unwrap(), "_id_");

    let resp = client
        .command(&bson::doc! { "listIndexes": "nonexistent", "$db": "testdb" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 0.0);
    assert_eq!(resp.get_i32("code").unwrap(), 26);
}

#[tokio::test]
async fn admin_server_status_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "serverStatus": 1, "$db": "admin" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert_eq!(resp.get_str("version").unwrap(), "7.0.0");
    assert_eq!(resp.get_str("process").unwrap(), "nimbus");
    assert!(resp.get_document("connections").is_ok());
}

#[tokio::test]
async fn admin_whatsmyuri_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "whatsmyuri": 1, "$db": "admin" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_str("you").is_ok());
}

#[tokio::test]
async fn admin_get_log_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    let resp = client
        .command(&bson::doc! { "getLog": "*", "$db": "admin" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_array("names").is_ok());

    let resp = client
        .command(&bson::doc! { "getLog": "global", "$db": "admin" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_array("log").is_ok());
}

#[tokio::test]
async fn list_databases_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    client
        .insert("mydb", "docs", &[bson::doc! { "_id": "x", "v": 1 }])
        .await;

    let resp = client
        .command(&bson::doc! { "listDatabases": 1, "$db": "admin" })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    let databases = resp.get_array("databases").unwrap();
    assert!(!databases.is_empty());
}

#[tokio::test]
async fn transaction_commit_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    client
        .insert("testdb", "txn_col", &[bson::doc! { "_id": "seed", "v": 0 }])
        .await;

    let session_resp = client.start_session().await;
    assert_eq!(session_resp.get_f64("ok").unwrap(), 1.0);
    let lsid = session_resp.get_document("id").unwrap().clone();

    let resp = client
        .command(&bson::doc! {
            "insert": "txn_col",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": &lsid,
            "documents": [{ "_id": "txn1", "v": 1 }],
        })
        .await;
    assert_eq!(
        resp.get_f64("ok").unwrap(),
        1.0,
        "insert in txn failed: {:?}",
        resp
    );

    let resp = client
        .command(&bson::doc! {
            "commitTransaction": 1,
            "$db": "admin",
            "lsid": &lsid,
        })
        .await;
    assert_eq!(
        resp.get_f64("ok").unwrap(),
        1.0,
        "commit failed: {:?}",
        resp
    );

    let docs = client
        .find(
            "testdb",
            "txn_col",
            bson::doc! { "_id": "txn1" },
            bson::Document::new(),
        )
        .await
        .expect("find should succeed");
    assert_eq!(docs.len(), 1);
}

#[tokio::test]
async fn transaction_abort_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    client
        .insert(
            "testdb",
            "abort_col",
            &[bson::doc! { "_id": "seed", "v": 0 }],
        )
        .await;

    let session_resp = client.start_session().await;
    let lsid = session_resp.get_document("id").unwrap().clone();

    let resp = client
        .command(&bson::doc! {
            "insert": "abort_col",
            "$db": "testdb",
            "startTransaction": true,
            "lsid": &lsid,
            "documents": [{ "_id": "aborted", "v": 99 }],
        })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);

    let resp = client
        .command(&bson::doc! {
            "abortTransaction": 1,
            "$db": "admin",
            "lsid": &lsid,
        })
        .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0, "abort failed: {:?}", resp);
}

#[tokio::test]
async fn change_stream_over_wire() {
    let fixture = executor::SpecTestFixture::new().await;
    let mut client = wire_client::WireClient::connect(fixture.addr).await;

    client
        .insert("testdb", "cs_col", &[bson::doc! { "_id": "s1", "v": 1 }])
        .await;

    let resp = client
        .command(&bson::doc! {
            "aggregate": "cs_col",
            "$db": "testdb",
            "pipeline": [bson::Bson::Document(bson::doc! { "$changeStream": {} })],
            "cursor": {},
        })
        .await;
    assert_eq!(
        resp.get_f64("ok").unwrap(),
        1.0,
        "aggregate $changeStream failed: {:?}",
        resp
    );
    let cursor = resp.get_document("cursor").unwrap();
    let cursor_id = cursor.get_i64("id").unwrap();
    assert!(cursor_id != 0, "change stream cursor should be non-zero");
    let first_batch = cursor.get_array("firstBatch").unwrap();
    assert!(
        first_batch.is_empty(),
        "initial batch should be empty for change stream"
    );
}
