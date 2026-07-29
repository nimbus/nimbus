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
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-server tests");
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist");
    if !workspace_firebase_selftest_dependencies_available(repo_root) {
        eprintln!(
            "skipping firebase SDK smoke selftest because JS workspace dependencies are unavailable"
        );
        return;
    }

    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    service
        .set_table_schema(
            &tenant_id,
            firebase_owner_schema_for_collection("secureSmoke", firebase_owner_read_write_policy()),
        )
        .expect("secureSmoke schema should install");
    let server =
        ServerFixture::start(router_for_firebase(service, firebase_verified_config())).await;

    let output = Command::new("node")
        .current_dir(repo_root)
        .arg("./packages/firebase/src/selftest.mjs")
        .arg("--smoke-base-url")
        .arg(server.http_url(""))
        // The smoke surface runs every flow through the dev-mode verification
        // bypass: the token carries the Firebase project issuer so the #24 gate
        // resolves project `demo` to tenant `demo`.
        .env(
            "NIMBUS_FIREBASE_SMOKE_MOCK_USER_TOKEN",
            r#"{"sub":"user-1","iss":"https://securetoken.google.com/demo"}"#,
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let commit_path = "/v1/projects/demo/databases/(default)/documents:commit";
    assert_firebase_rest_anonymous_refused(&server, commit_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
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
async fn firebase_rest_commit_roundtrips_lossless_firestore_field_types() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");
    let document_name = "projects/demo/databases/(default)/documents/events/typed";
    let fields = json!({
        "createdAt": { "timestampValue": "2024-01-02T03:04:05.123456789Z" },
        "payload": { "bytesValue": "AQIDBA==" },
        "owner": {
            "referenceValue": "projects/demo/databases/(default)/documents/users/ada"
        },
        "location": {
            "geoPointValue": { "latitude": 37.7749, "longitude": -122.4194 }
        },
        "score": { "doubleValue": "NaN" },
        "nested": {
            "mapValue": {
                "fields": {
                    "attachment": { "bytesValue": "/wA=" },
                    "label": { "stringValue": "kept" }
                }
            }
        }
    });

    let commit = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [{
                    "update": {
                        "name": document_name,
                        "fields": fields,
                    }
                }]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("typed Firestore commit should send");
    assert_eq!(commit.status(), StatusCode::OK);

    let read = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "documents": [document_name] }).to_string())
        .send()
        .await
        .expect("typed Firestore batch get should send");
    assert_eq!(read.status(), StatusCode::OK);
    let body = response_json_lines(read)
        .await
        .into_iter()
        .next()
        .expect("typed document should be returned");
    // Every field reads back exactly as written except the timestamp, which is
    // stored at Firestore's microsecond precision.
    let mut expected_fields = fields.clone();
    expected_fields["createdAt"] = json!({ "timestampValue": "2024-01-02T03:04:05.123456Z" });
    assert_eq!(body["found"]["fields"], expected_fields);

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["events", "typed"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("typed document should persist");
    for field in [
        "createdAt",
        "payload",
        "owner",
        "location",
        "score",
        "nested",
    ] {
        assert!(
            stored.typed_value(field).is_some(),
            "{field} should retain lossless typed metadata"
        );
    }
}

#[tokio::test]
async fn firebase_commit_applies_update_transforms_and_returns_transform_results() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let commit_path = "/v1/projects/demo/databases/(default)/documents:commit";
    assert_firebase_rest_anonymous_refused(&server, commit_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let commit_path = "/v1/projects/demo/databases/(default)/documents:commit";
    assert_firebase_rest_anonymous_refused(&server, commit_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
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
        matches!(error, nimbus_core::Error::DocumentNotFound(_)),
        "unexpected post-rollback error: {error:?}"
    );
}

#[tokio::test]
async fn firebase_commit_accepts_transaction_token_and_consumes_session() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let commit_path = "/v1/projects/demo/databases/(default)/documents:commit";
    assert_firebase_rest_anonymous_refused(&server, commit_path, "{}").await;

    let seed_response = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
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

    // The transaction session is engine-bound to its creating principal, so it
    // must be created under the exact principal the verified-path request
    // carries.
    let session = service
        .begin_transaction_session(
            tenant_id.clone(),
            firebase_verified_principal("user-123", "demo"),
            TransactionSessionMode::ReadWrite,
        )
        .expect("transaction session should start");
    let transaction_token =
        base64::engine::general_purpose::STANDARD.encode(session.token.as_str().as_bytes());

    let response = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

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

    let batch_get_path = "/v1/projects/demo/databases/(default)/documents:batchGet";
    assert_firebase_rest_anonymous_refused(&server, batch_get_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(batch_get_path))
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

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

    let batch_get_path = "/v1/projects/demo/databases/(default)/documents:batchGet";
    assert_firebase_rest_anonymous_refused(&server, batch_get_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(batch_get_path))
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    // The transaction session must be created under the same principal the
    // verified-path requests carry (engine sessions are principal-bound).
    let principal = firebase_verified_principal("user-123", "demo");
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

    let batch_get_path = "/v1/projects/demo/databases/(default)/documents:batchGet";
    assert_firebase_rest_anonymous_refused(&server, batch_get_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(batch_get_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .post(server.http_url(batch_get_path))
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let begin_path = "/v1/projects/demo/databases/(default)/documents:beginTransaction";
    assert_firebase_rest_anonymous_refused(&server, begin_path, "{}").await;

    let begin_response = server
        .client()
        .post(server.http_url(begin_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .header(header::AUTHORIZATION, &bearer)
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
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("name", json!("San Francisco")), ("visits", json!(1))],
    );
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let begin_path = "/v1/projects/demo/databases/(default)/documents:beginTransaction";
    assert_firebase_rest_anonymous_refused(&server, begin_path, "{}").await;

    let begin_response = server
        .client()
        .post(server.http_url(begin_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let batch_get_path = "/v1/projects/demo/databases/(default)/documents:batchGet";
    assert_firebase_rest_anonymous_refused(&server, batch_get_path, "{}").await;

    let response = server
        .client()
        .post(server.http_url(batch_get_path))
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

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

    let list_path = "/v1/projects/demo/databases/(default)/documents:listCollectionIds";
    assert_firebase_rest_anonymous_refused(&server, list_path, "{}").await;

    let root_first = server
        .client()
        .post(server.http_url(list_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .post(server.http_url(list_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .header(header::AUTHORIZATION, &bearer)
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
        .header(header::AUTHORIZATION, &bearer)
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let server = ServerFixture::start(router_for_firebase(
        fixture.engine(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");

    let list_path = "/v1/projects/demo/databases/(default)/documents:listCollectionIds";
    assert_firebase_rest_anonymous_refused(&server, list_path, "{}").await;

    let invalid_page_token = server
        .client()
        .post(server.http_url(list_path))
        .header(header::AUTHORIZATION, &bearer)
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
        .post(server.http_url(list_path))
        .header(header::AUTHORIZATION, &bearer)
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

#[tokio::test]
async fn firebase_commit_array_transforms_roundtrip_typed_values() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");
    let commit_path = "/v1/projects/demo/databases/(default)/documents:commit";
    let document_name = "projects/demo/databases/(default)/documents/events/arrays";

    let created_at = json!({ "timestampValue": "2024-01-02T03:04:05.123456789Z" });
    // The stored form of `created_at`: Firestore keeps microsecond precision, so
    // the sub-microsecond digits the client sent are not what reads back.
    let stored_created_at = json!({ "timestampValue": "2024-01-02T03:04:05.123456Z" });
    let payload = json!({ "bytesValue": "AQIDBA==" });
    let owner = json!({
        "referenceValue": "projects/demo/databases/(default)/documents/users/ada"
    });
    let location = json!({
        "geoPointValue": { "latitude": 37.7749, "longitude": -122.4194 }
    });

    let append = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [{
                    "update": {
                        "name": document_name,
                        "fields": {
                            "tags": {
                                "arrayValue": {
                                    "values": [{ "stringValue": "seed" }]
                                }
                            }
                        }
                    },
                    "updateTransforms": [{
                        "fieldPath": "tags",
                        "appendMissingElements": {
                            "values": [
                                { "stringValue": "seed" },
                                created_at,
                                payload,
                                owner,
                                location,
                            ]
                        }
                    }]
                }]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("typed arrayUnion commit should send");
    assert_eq!(
        append.status(),
        StatusCode::OK,
        "arrayUnion carrying Firestore typed values should be accepted: {}",
        append.text().await.unwrap_or_default()
    );

    let read = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "documents": [document_name] }).to_string())
        .send()
        .await
        .expect("typed array batch get should send");
    assert_eq!(read.status(), StatusCode::OK);
    let body = response_json_lines(read)
        .await
        .into_iter()
        .next()
        .expect("typed array document should be returned");
    assert_eq!(
        body["found"]["fields"]["tags"],
        json!({
            "arrayValue": {
                "values": [
                    { "stringValue": "seed" },
                    stored_created_at,
                    payload,
                    owner,
                    location,
                ]
            }
        }),
        "arrayUnion elements must read back as the typed values the client wrote"
    );

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["events", "arrays"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document(&tenant_id, &locator.table, locator.id)
        .expect("typed array document should persist");
    assert!(
        stored
            .typed_value("tags")
            .is_some_and(nimbus_core::StoredValue::contains_typed_metadata),
        "array field holding typed elements should retain typed metadata"
    );

    let remove = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [{
                    "transform": {
                        "document": document_name,
                        "fieldTransforms": [{
                            "fieldPath": "tags",
                            "removeAllFromArray": {
                                "values": [created_at, location]
                            }
                        }]
                    }
                }]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("typed arrayRemove commit should send");
    assert_eq!(
        remove.status(),
        StatusCode::OK,
        "arrayRemove carrying Firestore typed values should be accepted: {}",
        remove.text().await.unwrap_or_default()
    );

    let read_after = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "documents": [document_name] }).to_string())
        .send()
        .await
        .expect("typed array batch get should send");
    let body_after = response_json_lines(read_after)
        .await
        .into_iter()
        .next()
        .expect("typed array document should be returned");
    assert_eq!(
        body_after["found"]["fields"]["tags"],
        json!({
            "arrayValue": {
                "values": [
                    { "stringValue": "seed" },
                    payload,
                    owner,
                ]
            }
        }),
        "arrayRemove must match typed elements by value and leave the rest intact"
    );
}

#[tokio::test]
async fn firebase_commit_array_transforms_dedupe_equivalent_timestamp_spellings() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_firebase(
        service.clone(),
        firebase_verified_config(),
    ))
    .await;
    let bearer = firebase_verified_bearer("user-123", "demo");
    let commit_path = "/v1/projects/demo/databases/(default)/documents:commit";
    let document_name = "projects/demo/databases/(default)/documents/events/timestamps";

    // Four operands, two instants. `+01:00` is the same instant as the `Z` form,
    // and the sub-microsecond spelling is the same stored value as the
    // microsecond one once Firestore's truncation applies.
    let append = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [{
                    "update": { "name": document_name, "fields": {} },
                    "updateTransforms": [{
                        "fieldPath": "seenAt",
                        "appendMissingElements": {
                            "values": [
                                { "timestampValue": "2024-01-02T03:04:05Z" },
                                { "timestampValue": "2024-01-02T04:04:05+01:00" },
                                { "timestampValue": "2024-01-02T03:04:05.123456789Z" },
                                { "timestampValue": "2024-01-02T03:04:05.123456Z" },
                            ]
                        }
                    }]
                }]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("timestamp arrayUnion commit should send");
    assert_eq!(
        append.status(),
        StatusCode::OK,
        "arrayUnion of equivalent timestamp spellings should be accepted: {}",
        append.text().await.unwrap_or_default()
    );

    let read = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "documents": [document_name] }).to_string())
        .send()
        .await
        .expect("timestamp array batch get should send");
    assert_eq!(read.status(), StatusCode::OK);
    let body = response_json_lines(read)
        .await
        .into_iter()
        .next()
        .expect("timestamp array document should be returned");
    assert_eq!(
        body["found"]["fields"]["seenAt"],
        json!({
            "arrayValue": {
                "values": [
                    { "timestampValue": "2024-01-02T03:04:05Z" },
                    { "timestampValue": "2024-01-02T03:04:05.123456Z" },
                ]
            }
        }),
        "equivalent spellings of one instant must dedupe to one canonical element"
    );

    // A spelling the client never wrote must still match for removal, because
    // matching is on the instant rather than on the caller's text.
    let remove = server
        .client()
        .post(server.http_url(commit_path))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [{
                    "transform": {
                        "document": document_name,
                        "fieldTransforms": [{
                            "fieldPath": "seenAt",
                            "removeAllFromArray": {
                                "values": [
                                    { "timestampValue": "2024-01-01T22:04:05-05:00" }
                                ]
                            }
                        }]
                    }
                }]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("timestamp arrayRemove commit should send");
    assert_eq!(
        remove.status(),
        StatusCode::OK,
        "arrayRemove with an equivalent spelling should be accepted: {}",
        remove.text().await.unwrap_or_default()
    );

    let read_after = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchGet"))
        .header(header::AUTHORIZATION, &bearer)
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(json!({ "documents": [document_name] }).to_string())
        .send()
        .await
        .expect("timestamp array batch get should send");
    let body_after = response_json_lines(read_after)
        .await
        .into_iter()
        .next()
        .expect("timestamp array document should be returned");
    assert_eq!(
        body_after["found"]["fields"]["seenAt"],
        json!({
            "arrayValue": {
                "values": [
                    { "timestampValue": "2024-01-02T03:04:05.123456Z" },
                ]
            }
        }),
        "arrayRemove must match an equivalent spelling of the stored instant"
    );
}
