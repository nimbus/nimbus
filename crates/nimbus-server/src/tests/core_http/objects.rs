//! Native object routes: whole-object upload, listing, download, and delete
//! over the session-authenticated main listener.

use super::*;

const TEXT: &str = "hello from the object routes";

async fn put_text(server: &ServerFixture, path: &str, body: &'static str) -> reqwest::Response {
    server
        .client()
        .put(server.http_url(path))
        .header("content-type", "text/plain; charset=utf-8")
        .body(body)
        .send()
        .await
        .expect("put object")
}

async fn get(server: &ServerFixture, path: &str) -> reqwest::Response {
    server
        .client()
        .get(server.http_url(path))
        .send()
        .await
        .expect("get")
}

#[tokio::test]
async fn uploads_lists_downloads_and_deletes_a_whole_object() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let uploaded = put_text(
        &server,
        "/api/tenants/demo/objects/assets/notes/hello.txt",
        TEXT,
    )
    .await;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let uploaded: serde_json::Value = uploaded.json().await.expect("upload json");
    assert_eq!(uploaded["bucket"], "assets");
    assert_eq!(uploaded["key"], "notes/hello.txt");
    assert_eq!(uploaded["size"], TEXT.len());
    assert_eq!(uploaded["contentType"], "text/plain; charset=utf-8");
    let etag = uploaded["etag"].as_str().expect("etag").to_string();
    assert_eq!(
        etag.len(),
        32,
        "the etag is the hex MD5 the S3 surface writes"
    );
    assert!(uploaded["lastModifiedMillis"].as_u64().unwrap_or(0) > 0);

    let buckets = get(&server, "/api/tenants/demo/objects").await;
    assert_eq!(buckets.status(), StatusCode::OK);
    let buckets: serde_json::Value = buckets.json().await.expect("buckets json");
    assert_eq!(
        buckets,
        json!({ "buckets": [{ "bucket": "assets", "objectCount": 1, "totalBytes": TEXT.len() }] })
    );

    let listed = get(&server, "/api/tenants/demo/objects/assets?prefix=notes/").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: serde_json::Value = listed.json().await.expect("list json");
    assert_eq!(listed["bucket"], "assets");
    assert_eq!(listed["prefix"], "notes/");
    assert_eq!(listed["truncated"], false);
    assert_eq!(listed["objects"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["objects"][0]["key"], "notes/hello.txt");
    assert_eq!(listed["objects"][0]["etag"], etag);

    let other_prefix = get(&server, "/api/tenants/demo/objects/assets?prefix=images/").await;
    let other_prefix: serde_json::Value = other_prefix.json().await.expect("list json");
    assert_eq!(other_prefix["objects"].as_array().map(Vec::len), Some(0));

    let downloaded = get(&server, "/api/tenants/demo/objects/assets/notes/hello.txt").await;
    assert_eq!(downloaded.status(), StatusCode::OK);
    let headers = downloaded.headers().clone();
    assert_eq!(
        headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(
        headers.get("etag").and_then(|v| v.to_str().ok()),
        Some(format!("\"{etag}\"").as_str())
    );
    assert_eq!(
        headers
            .get("content-disposition")
            .and_then(|v| v.to_str().ok()),
        Some("inline; filename=\"hello.txt\"")
    );
    assert_eq!(
        headers
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok()),
        Some("sandbox")
    );
    assert_eq!(downloaded.text().await.expect("body"), TEXT);

    let attachment = get(
        &server,
        "/api/tenants/demo/objects/assets/notes/hello.txt?download=1",
    )
    .await;
    assert_eq!(
        attachment
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok()),
        Some("attachment; filename=\"hello.txt\"")
    );

    let deleted = server
        .client()
        .delete(server.http_url("/api/tenants/demo/objects/assets/notes/hello.txt"))
        .send()
        .await
        .expect("delete");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let gone = get(&server, "/api/tenants/demo/objects/assets/notes/hello.txt").await;
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    let deleted_again = server
        .client()
        .delete(server.http_url("/api/tenants/demo/objects/assets/notes/hello.txt"))
        .send()
        .await
        .expect("delete");
    assert_eq!(deleted_again.status(), StatusCode::NOT_FOUND);

    let buckets = get(&server, "/api/tenants/demo/objects").await;
    let buckets: serde_json::Value = buckets.json().await.expect("buckets json");
    assert_eq!(buckets, json!({ "buckets": [] }));
}

#[tokio::test]
async fn replaces_an_object_in_place_and_serves_binary_bytes_as_octet_stream() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let first = put_text(&server, "/api/tenants/demo/objects/assets/a.txt", "one").await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let second = server
        .client()
        .put(server.http_url("/api/tenants/demo/objects/assets/a.txt"))
        .body(vec![0_u8, 1, 2, 3])
        .send()
        .await
        .expect("put");
    assert_eq!(second.status(), StatusCode::CREATED);

    let listed: serde_json::Value = get(&server, "/api/tenants/demo/objects/assets")
        .await
        .json()
        .await
        .expect("list json");
    assert_eq!(listed["objects"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["objects"][0]["size"], 4);
    assert_eq!(listed["objects"][0]["contentType"], serde_json::Value::Null);

    let downloaded = get(&server, "/api/tenants/demo/objects/assets/a.txt").await;
    assert_eq!(
        downloaded
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/octet-stream")
    );
    assert_eq!(
        downloaded.bytes().await.expect("bytes").as_ref(),
        &[0, 1, 2, 3]
    );
}

#[tokio::test]
async fn lists_with_a_limit_and_reports_truncation() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for key in ["c.txt", "a.txt", "b.txt"] {
        let path = format!("/api/tenants/demo/objects/assets/{key}");
        assert_eq!(
            put_text(&server, &path, "x").await.status(),
            StatusCode::CREATED
        );
    }

    let page: serde_json::Value = get(&server, "/api/tenants/demo/objects/assets?limit=2")
        .await
        .json()
        .await
        .expect("list json");
    assert_eq!(page["truncated"], true);
    assert_eq!(page["objects"][0]["key"], "a.txt");
    assert_eq!(page["objects"][1]["key"], "b.txt");
    assert_eq!(page["objects"].as_array().map(Vec::len), Some(2));

    let all: serde_json::Value = get(&server, "/api/tenants/demo/objects/assets")
        .await
        .json()
        .await
        .expect("list json");
    assert_eq!(all["truncated"], false);
    assert_eq!(all["objects"].as_array().map(Vec::len), Some(3));
}

#[tokio::test]
async fn rejects_unknown_tenants_invalid_buckets_and_oversized_bodies() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    // An upload never creates a tenant as a side effect.
    let ghost = put_text(&server, "/api/tenants/ghost/objects/assets/a.txt", "x").await;
    assert_eq!(ghost.status(), StatusCode::NOT_FOUND);
    let tenants: serde_json::Value = api.list_tenants().await.json().await.expect("tenants");
    assert!(
        !tenants.to_string().contains("ghost"),
        "upload to an unknown tenant must not create it: {tenants}"
    );
    assert_eq!(
        get(&server, "/api/tenants/ghost/objects").await.status(),
        StatusCode::NOT_FOUND
    );

    // Bucket names follow the manifest rules: no slash, at most 63 bytes.
    let long_bucket = "b".repeat(64);
    let invalid = put_text(
        &server,
        &format!("/api/tenants/demo/objects/{long_bucket}/a.txt"),
        "x",
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        get(&server, &format!("/api/tenants/demo/objects/{long_bucket}"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    // A missing key is a 404, not an empty body.
    assert_eq!(
        get(&server, "/api/tenants/demo/objects/assets/missing.txt")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // The whole-object route is bounded; larger objects take the S3
    // multipart path.
    let oversized = server
        .client()
        .put(server.http_url("/api/tenants/demo/objects/assets/big.bin"))
        .body(vec![7_u8; 16 * 1024 * 1024 + 1])
        .send()
        .await
        .expect("put");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let listed: serde_json::Value = get(&server, "/api/tenants/demo/objects/assets")
        .await
        .json()
        .await
        .expect("list json");
    assert_eq!(listed["objects"].as_array().map(Vec::len), Some(0));
}
