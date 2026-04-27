use super::*;

fn workspace_firebase_selftest_dependencies_available(repo_root: &Path) -> bool {
    let root_node_modules = repo_root.join("node_modules");
    let package_node_modules = repo_root.join("packages/firebase/node_modules");
    let has_dependency = |node_modules: &Path, scoped_segments: &[&str]| {
        let mut path = node_modules.to_path_buf();
        for segment in scoped_segments {
            path.push(segment);
        }
        path.is_dir()
    };

    repo_root
        .join("packages/firebase/src/selftest.mjs")
        .is_file()
        && (has_dependency(&root_node_modules, &["esbuild"])
            || has_dependency(&package_node_modules, &["esbuild"]))
        && (has_dependency(&root_node_modules, &["@connectrpc", "connect"])
            || has_dependency(&package_node_modules, &["@connectrpc", "connect"]))
        && (has_dependency(&root_node_modules, &["@connectrpc", "connect-web"])
            || has_dependency(&package_node_modules, &["@connectrpc", "connect-web"]))
        && (has_dependency(&root_node_modules, &["@bufbuild", "protobuf"])
            || has_dependency(&package_node_modules, &["@bufbuild", "protobuf"]))
}

#[tokio::test]
async fn firebase_sdk_crud_selftest_smoke() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist");
    if !workspace_firebase_selftest_dependencies_available(repo_root) {
        eprintln!(
            "skipping firebase SDK smoke selftest because JS workspace dependencies are unavailable"
        );
        return;
    }

    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection("secureSmoke", firebase_owner_read_write_policy()),
        )
        .expect("secureSmoke schema should install");
    let server = ServerFixture::start(build_router_with_firebase(
        service,
        FirebaseConfig::new().with_emulator_mock_user_token_auth(),
    ))
    .await;

    let output = Command::new("node")
        .current_dir(repo_root)
        .arg("./packages/firebase/src/selftest.mjs")
        .arg("--smoke-base-url")
        .arg(server.http_url(""))
        .env(
            "NEOVEX_FIREBASE_SMOKE_MOCK_USER_TOKEN",
            r#"{"sub":"user-1"}"#,
        )
        .output()
        .await
        .expect("firebase SDK smoke selftest should run");

    assert!(
        output.status.success(),
        "firebase SDK smoke selftest should pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
#[tokio::test]
async fn firebase_commit_executes_atomic_batch_and_returns_firestore_commit_response() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" },
                                "population": { "integerValue": "884363" }
                            }
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase commit should send");

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("firebase commit response should deserialize");
    assert_eq!(
        body["writeResults"].as_array().map(Vec::len),
        Some(1),
        "commit should return one write result: {body:?}"
    );
    assert!(
        body["writeResults"][0]["updateTime"].as_str().is_some(),
        "commit should expose updateTime: {body:?}"
    );
    assert!(
        body["commitTime"].as_str().is_some(),
        "commit should expose commitTime: {body:?}"
    );

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("committed document should exist");
    assert_eq!(document.get_field("name"), Some(&json!("San Francisco")));
    assert_eq!(document.get_field("population"), Some(&json!(884363)));
}

#[tokio::test]
async fn firebase_commit_applies_update_transforms_and_returns_transform_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "count": { "integerValue": "1" },
                                "tags": {
                                    "arrayValue": {
                                        "values": [
                                            { "stringValue": "seed" }
                                        ]
                                    }
                                }
                            }
                        },
                        "updateTransforms": [
                            {
                                "fieldPath": "count",
                                "increment": { "integerValue": "2" }
                            },
                            {
                                "fieldPath": "tags",
                                "appendMissingElements": {
                                    "values": [
                                        { "stringValue": "seed" },
                                        { "stringValue": "new" }
                                    ]
                                }
                            }
                        ]
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase commit should send");

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("firebase commit response should deserialize");
    assert_eq!(
        body["writeResults"][0]["transformResults"],
        json!([
            { "integerValue": "3" },
            { "nullValue": null }
        ])
    );

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("committed document should exist");
    assert_eq!(document.get_field("count"), Some(&json!(3)));
    assert_eq!(document.get_field("tags"), Some(&json!(["seed", "new"])));
}

#[tokio::test]
async fn firebase_commit_rolls_back_entire_batch_on_failure() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" }
                            }
                        }
                    },
                    {
                        "verify": "projects/demo/databases/(default)/documents/cities/LA",
                        "currentDocument": {
                            "exists": true
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase commit should send");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("error response should deserialize");
    assert_eq!(body["error"]["status"], json!("NOT_FOUND"));

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let error = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect_err("atomic failure should roll back the earlier write");
    assert!(
        matches!(error, neovex_core::Error::DocumentNotFound(_)),
        "unexpected post-rollback error: {error:?}"
    );
}

#[tokio::test]
async fn firebase_commit_accepts_transaction_token_and_consumes_session() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let seed_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" }
                            }
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("seed firebase commit should send");
    assert_eq!(seed_response.status(), StatusCode::OK);

    let session = service
        .begin_transaction_session(
            tenant_id.clone(),
            PrincipalContext::anonymous(),
            TransactionSessionMode::ReadWrite,
        )
        .expect("transaction session should start");
    let transaction_token =
        base64::engine::general_purpose::STANDARD.encode(session.token.as_str().as_bytes());

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "transaction": transaction_token,
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" },
                                "state": { "stringValue": "CA" }
                            }
                        },
                        "updateMask": {
                            "fieldPaths": ["name", "state"]
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("transactional firebase commit should send");

    assert_eq!(response.status(), StatusCode::OK);
    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let document = service
        .get_document(&tenant_id, &locator.table, locator.id.clone())
        .expect("committed document should exist");
    assert_eq!(document.get_field("state"), Some(&json!("CA")));

    let reused = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "transaction": base64::engine::general_purpose::STANDARD
                    .encode(session.token.as_str().as_bytes()),
                "writes": [
                    {
                        "verify": "projects/demo/databases/(default)/documents/cities/SF",
                        "currentDocument": {
                            "exists": true
                        }
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("reused transaction firebase commit should send");

    assert_eq!(reused.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = reused
        .json()
        .await
        .expect("reused transaction error response should deserialize");
    assert_eq!(body["error"]["status"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn firebase_batch_get_returns_found_missing_and_elides_duplicates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let document_path =
        DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse");
    let locator = crate::adapters::firebase::locator_for_document_path(&document_path)
        .expect("firebase locator should derive");
    service
        .insert_document_with_id(
            &tenant_id,
            locator.table.clone(),
            locator.id.clone(),
            serde_json::Map::from_iter([
                ("name".to_string(), json!("San Francisco")),
                ("population".to_string(), json!(884363)),
                ("state".to_string(), json!("CA")),
            ]),
        )
        .expect("seed document should insert");

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF",
                    "projects/demo/databases/(default)/documents/cities/SF",
                    "projects/demo/databases/(default)/documents/cities/LA"
                ],
                "mask": {
                    "fieldPaths": ["name", "population", "population"]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase batch get should send");

    let status = response.status();
    let body = response
        .text()
        .await
        .expect("run query response body should deserialize to text");
    if status != StatusCode::OK {
        panic!("unexpected run query status {status}: {body}");
    }
    let entries = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("streaming JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        2,
        "duplicate document requests should be elided"
    );
    assert_eq!(
        entries[0]["found"]["name"],
        json!("projects/demo/databases/(default)/documents/cities/SF")
    );
    assert_eq!(
        entries[0]["found"]["fields"]["name"],
        json!({ "stringValue": "San Francisco" })
    );
    assert_eq!(
        entries[0]["found"]["fields"]["population"],
        json!({ "integerValue": "884363" })
    );
    assert!(
        entries[0]["found"]["fields"].get("state").is_none(),
        "field masks should omit non-requested fields: {entries:?}"
    );
    assert!(entries[0]["readTime"].as_str().is_some());
    assert_eq!(
        entries[1]["missing"],
        json!("projects/demo/databases/(default)/documents/cities/LA")
    );
}

#[tokio::test]
async fn firebase_batch_get_reads_nested_document_paths() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let document_path = DocumentPath::from_segments(["cities", "SF", "landmarks", "golden-gate"])
        .expect("nested document path should parse");
    let locator = crate::adapters::firebase::locator_for_document_path(&document_path)
        .expect("firebase locator should derive");
    service
        .insert_document_with_id(
            &tenant_id,
            locator.table.clone(),
            locator.id.clone(),
            serde_json::Map::from_iter([("label".to_string(), json!("Golden Gate Bridge"))]),
        )
        .expect("nested seed document should insert");

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF/landmarks/golden-gate"
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase nested batch get should send");

    let status = response.status();
    let body = response
        .text()
        .await
        .expect("run query response body should deserialize to text");
    if status != StatusCode::OK {
        panic!("unexpected run query status {status}: {body}");
    }
    let entries = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("streaming JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["found"]["name"],
        json!("projects/demo/databases/(default)/documents/cities/SF/landmarks/golden-gate")
    );
    assert_eq!(
        entries[0]["found"]["fields"]["label"],
        json!({ "stringValue": "Golden Gate Bridge" })
    );
}

#[tokio::test]
async fn firebase_batch_get_accepts_active_transaction_tokens_and_rejects_inactive_ones() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let principal = PrincipalContext::anonymous();
    let document_path =
        DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse");
    let locator = crate::adapters::firebase::locator_for_document_path(&document_path)
        .expect("firebase locator should derive");
    service
        .insert_document_with_id(
            &tenant_id,
            locator.table.clone(),
            locator.id.clone(),
            serde_json::Map::from_iter([("name".to_string(), json!("Before"))]),
        )
        .expect("seed document should insert");

    let session = service
        .begin_transaction_session(
            tenant_id.clone(),
            principal.clone(),
            TransactionSessionMode::ReadOnly,
        )
        .expect("read-only transaction session should start");
    let transaction_token =
        base64::engine::general_purpose::STANDARD.encode(session.token.as_str().as_bytes());

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF"
                ],
                "transaction": transaction_token
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase transactional batch get should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(
        entries[0]["found"]["fields"]["name"],
        json!({ "stringValue": "Before" }),
        "transactional batch get should read through the active session path"
    );

    service
        .rollback_transaction_session(&tenant_id, &session.token, &principal)
        .expect("transaction session should roll back");
    let inactive = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF"
                ],
                "transaction": base64::engine::general_purpose::STANDARD
                    .encode(session.token.as_str().as_bytes())
            })
            .to_string(),
        )
        .send()
        .await
        .expect("inactive transaction batch get should send");

    assert_eq!(inactive.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = inactive
        .json()
        .await
        .expect("inactive transaction error response should deserialize");
    assert_eq!(body["error"]["status"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn firebase_rest_begin_transaction_and_rollback_manage_session_tokens() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let begin_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:beginTransaction"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "options": {
                    "readOnly": {}
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase beginTransaction request should send");

    assert_eq!(begin_response.status(), StatusCode::OK);
    let begin_body: serde_json::Value = begin_response
        .json()
        .await
        .expect("beginTransaction response should deserialize");
    let transaction = begin_body["transaction"]
        .as_str()
        .expect("beginTransaction should return a transaction token")
        .to_string();

    let rollback_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:rollback"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "transaction": transaction,
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase rollback request should send");

    assert_eq!(rollback_response.status(), StatusCode::OK);
    let rollback_body: serde_json::Value = rollback_response
        .json()
        .await
        .expect("rollback response should deserialize");
    assert_eq!(rollback_body, json!({}));

    let inactive_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:rollback"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "transaction": begin_body["transaction"],
            })
            .to_string(),
        )
        .send()
        .await
        .expect("inactive rollback request should send");

    assert_eq!(inactive_response.status(), StatusCode::BAD_REQUEST);
    let inactive_body: serde_json::Value = inactive_response
        .json()
        .await
        .expect("inactive rollback response should deserialize");
    assert_eq!(inactive_body["error"]["status"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn firebase_run_query_supports_transaction_selector_with_pinned_snapshot() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco")), ("visits", json!(1))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let begin_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:beginTransaction"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "options": {
                    "readOnly": {}
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase beginTransaction request should send");
    assert_eq!(begin_response.status(), StatusCode::OK);
    let begin_body: serde_json::Value = begin_response
        .json()
        .await
        .expect("beginTransaction response should deserialize");
    let transaction = begin_body["transaction"]
        .as_str()
        .expect("beginTransaction should return a transaction token")
        .to_string();

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    service
        .update_document(
            &tenant_id,
            locator.table.clone(),
            locator.id.clone(),
            serde_json::Map::from_iter([("visits".to_string(), json!(99))]),
        )
        .expect("outside update should commit");

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "transaction": transaction,
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "name" },
                            "op": "EQUAL",
                            "value": { "stringValue": "San Francisco" }
                        }
                    }
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("transactional RunQuery should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(
        entries[0]["document"]["fields"]["visits"],
        json!({ "integerValue": "1" })
    );
}

#[tokio::test]
async fn firebase_batch_get_rejects_unsupported_read_time_selector() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "documents": [
                    "projects/demo/databases/(default)/documents/cities/SF"
                ],
                "readTime": "2026-04-25T00:00:00Z"
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase batch get error request should send");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("firebase batch get error response should deserialize");
    assert_eq!(body["error"]["status"], json!("INVALID_ARGUMENT"));
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("readTime")),
        "unsupported selector should mention readTime: {body:?}"
    );
}

#[tokio::test]
async fn firebase_list_collection_ids_lists_root_and_nested_parents_with_pagination() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["countries", "JP"],
        [("name", json!("Japan"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["regions", "west"],
        [("name", json!("West"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "bridge"],
        [("label", json!("Golden Gate Bridge"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "neighborhoods", "soma"],
        [("label", json!("SoMa"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "bridge", "photos", "p1"],
        [("label", json!("Photo"))],
    );

    let root_first = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:listCollectionIds"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "pageSize": 2 }).to_string())
        .send()
        .await
        .expect("root ListCollectionIds should send");
    assert_eq!(root_first.status(), StatusCode::OK);
    let root_first: serde_json::Value = root_first
        .json()
        .await
        .expect("root ListCollectionIds response should deserialize");
    assert_eq!(root_first["collectionIds"], json!(["cities", "countries"]));
    let next_page_token = root_first["nextPageToken"]
        .as_str()
        .expect("page token should be a string")
        .to_string();
    assert!(!next_page_token.is_empty());

    let root_second = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:listCollectionIds"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "pageToken": next_page_token }).to_string())
        .send()
        .await
        .expect("paged ListCollectionIds should send");
    assert_eq!(root_second.status(), StatusCode::OK);
    let root_second: serde_json::Value = root_second
        .json()
        .await
        .expect("paged ListCollectionIds response should deserialize");
    assert_eq!(root_second["collectionIds"], json!(["regions"]));
    assert_eq!(root_second["nextPageToken"], json!(""));

    let nested = server
        .client()
        .post(server.http_url(
            "/v1/projects/demo/databases/(default)/documents/cities/SF:listCollectionIds",
        ))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body("{}".to_string())
        .send()
        .await
        .expect("nested ListCollectionIds should send");
    assert_eq!(nested.status(), StatusCode::OK);
    let nested: serde_json::Value = nested
        .json()
        .await
        .expect("nested ListCollectionIds response should deserialize");
    assert_eq!(
        nested["collectionIds"],
        json!(["landmarks", "neighborhoods"])
    );

    let deep = server
        .client()
        .post(server.http_url(
            "/v1/projects/demo/databases/(default)/documents/cities/SF/landmarks/bridge:listCollectionIds",
        ))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body("{}".to_string())
        .send()
        .await
        .expect("deep ListCollectionIds should send");
    assert_eq!(deep.status(), StatusCode::OK);
    let deep: serde_json::Value = deep
        .json()
        .await
        .expect("deep ListCollectionIds response should deserialize");
    assert_eq!(deep["collectionIds"], json!(["photos"]));
}

#[tokio::test]
async fn firebase_list_collection_ids_rejects_invalid_page_tokens_and_read_time() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let invalid_page_token = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:listCollectionIds"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "pageToken": "not-base64!" }).to_string())
        .send()
        .await
        .expect("invalid page token request should send");
    assert_eq!(invalid_page_token.status(), StatusCode::BAD_REQUEST);
    let invalid_page_token: serde_json::Value = invalid_page_token
        .json()
        .await
        .expect("invalid page token error should deserialize");
    assert_eq!(
        invalid_page_token["error"]["status"],
        json!("INVALID_ARGUMENT")
    );
    assert!(
        invalid_page_token["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("pageToken")),
        "invalid page token errors should mention pageToken: {invalid_page_token:?}"
    );

    let read_time = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:listCollectionIds"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "readTime": "2024-01-01T00:00:00Z" }).to_string())
        .send()
        .await
        .expect("readTime request should send");
    assert_eq!(read_time.status(), StatusCode::BAD_REQUEST);
    let read_time: serde_json::Value = read_time
        .json()
        .await
        .expect("readTime error should deserialize");
    assert_eq!(read_time["error"]["status"], json!("INVALID_ARGUMENT"));
    assert!(
        read_time["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("readTime")),
        "unsupported selector should mention readTime: {read_time:?}"
    );
}
