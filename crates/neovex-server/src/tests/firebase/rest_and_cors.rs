use super::*;

#[tokio::test]
async fn firebase_rest_preflight_allows_text_plain_and_sdk_headers_from_loopback_origin() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;

    let allowed = server
        .client()
        .request(
            reqwest::Method::OPTIONS,
            server.http_url("/v1/projects/demo/databases/(default)/documents:commit"),
        )
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type,google-cloud-resource-prefix,x-firebase-gmpid,x-goog-api-client,x-goog-request-params",
        )
        .send()
        .await
        .expect("firebase rest preflight should send");
    assert!(
        allowed.status().is_success(),
        "firebase rest preflight should succeed: {allowed:?}"
    );
    assert_eq!(
        allowed
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:5173")
    );
    let allow_headers = header_csv_values(&allowed, "access-control-allow-headers");
    for expected in [
        "authorization",
        "content-type",
        "google-cloud-resource-prefix",
        "x-firebase-gmpid",
        "x-goog-api-client",
        "x-goog-request-params",
    ] {
        assert!(
            allow_headers.contains(expected),
            "missing {expected} from firebase rest allow headers: {allow_headers:?}"
        );
    }

    let denied = server
        .client()
        .request(
            reqwest::Method::OPTIONS,
            server.http_url("/v1/projects/demo/databases/(default)/documents:commit"),
        )
        .header("origin", "http://example.com")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .expect("firebase rest denied preflight should send");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn firebase_grpc_web_preflight_allows_firestore_headers_and_exposes_grpc_trailers() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;

    let allowed = server
        .client()
        .request(
            reqwest::Method::OPTIONS,
            server.http_url("/google.firestore.v1.Firestore/Commit"),
        )
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "content-type,google-cloud-resource-prefix,grpc-timeout,x-firebase-appcheck,x-goog-api-key,x-goog-request-params,x-grpc-web",
        )
        .send()
        .await
        .expect("firebase grpc-web preflight should send");
    assert!(
        allowed.status().is_success(),
        "firebase grpc-web preflight should succeed: {allowed:?}"
    );
    assert_eq!(
        allowed
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:5173")
    );
    let allow_headers = header_csv_values(&allowed, "access-control-allow-headers");
    for expected in [
        "content-type",
        "google-cloud-resource-prefix",
        "grpc-timeout",
        "x-firebase-appcheck",
        "x-goog-api-key",
        "x-goog-request-params",
        "x-grpc-web",
    ] {
        assert!(
            allow_headers.contains(expected),
            "missing {expected} from firebase grpc-web allow headers: {allow_headers:?}"
        );
    }

    let grpc_web_response = server
        .client()
        .post(server.http_url("/google.firestore.v1.Firestore/Commit"))
        .header("origin", "http://localhost:5173")
        .header("x-grpc-web", "1")
        .header("content-type", "application/grpc-web+proto")
        .body(Vec::new())
        .send()
        .await
        .expect("firebase grpc-web request should send");
    assert_eq!(grpc_web_response.status(), StatusCode::NOT_FOUND);
    let exposed_headers = header_csv_values(&grpc_web_response, "access-control-expose-headers");
    for expected in ["grpc-status", "grpc-message", "grpc-status-details-bin"] {
        assert!(
            exposed_headers.contains(expected),
            "missing {expected} from grpc-web exposed headers: {exposed_headers:?}"
        );
    }
}

#[tokio::test]
async fn firebase_enabled_routes_grpc_and_grpc_web_requests_to_firestore_service() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let grpc_response = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("http2 prior knowledge client should build")
        .post(server.http_url("/google.firestore.v1.Firestore/Commit"))
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(empty_grpc_frame())
        .send()
        .await
        .expect("firebase grpc request should send");
    assert_ne!(grpc_response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        grpc_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/grpc")
    );

    let grpc_web_response = server
        .client()
        .post(server.http_url("/google.firestore.v1.Firestore/Commit"))
        .header("origin", "http://localhost:5173")
        .header("x-grpc-web", "1")
        .header("content-type", "application/grpc-web+proto")
        .body(empty_grpc_frame())
        .send()
        .await
        .expect("firebase grpc-web request should send");
    assert_ne!(grpc_web_response.status(), StatusCode::NOT_FOUND);
    let exposed_headers = header_csv_values(&grpc_web_response, "access-control-expose-headers");
    for expected in ["grpc-status", "grpc-message", "grpc-status-details-bin"] {
        assert!(
            exposed_headers.contains(expected),
            "missing {expected} from grpc-web exposed headers: {exposed_headers:?}"
        );
    }
}
