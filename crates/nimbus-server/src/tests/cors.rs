//! LR5: configurable CORS origins — normalization rules and the
//! configured-origin predicate that extends the always-on loopback
//! allowance.

use std::collections::HashSet;

use axum::http::HeaderValue;

use crate::normalize_cors_origin;
use crate::router::{is_allowed_local_cors_origin, is_configured_cors_origin};

fn allowed(origins: &[&str]) -> HashSet<String> {
    origins
        .iter()
        .map(|origin| normalize_cors_origin(origin).expect("test origin should normalize"))
        .collect()
}

fn header(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).expect("test header value should build")
}

#[test]
fn cors_origin_normalization_canonicalizes_browser_origin_form() {
    assert_eq!(
        normalize_cors_origin("https://App.Example.com").as_deref(),
        Ok("https://app.example.com")
    );
    assert_eq!(
        normalize_cors_origin("https://app.example.com/").as_deref(),
        Ok("https://app.example.com")
    );
    assert_eq!(
        normalize_cors_origin("http://app.example.com:8080").as_deref(),
        Ok("http://app.example.com:8080")
    );
    // Default ports are stripped to match what browsers send.
    assert_eq!(
        normalize_cors_origin("http://app.example.com:80").as_deref(),
        Ok("http://app.example.com")
    );
    assert_eq!(
        normalize_cors_origin("https://app.example.com:443").as_deref(),
        Ok("https://app.example.com")
    );
    // Bracketed IPv6 hosts keep their port split intact.
    assert_eq!(
        normalize_cors_origin("http://[2001:db8::1]:8443").as_deref(),
        Ok("http://[2001:db8::1]:8443")
    );
}

#[test]
fn cors_origin_normalization_rejects_invalid_forms() {
    for (origin, must_mention) in [
        ("*", "wildcard"),
        ("https://*.example.com", "wildcard"),
        ("app.example.com", "scheme"),
        ("ftp://app.example.com", "http or https"),
        ("https://app.example.com/path", "path"),
        ("https://app.example.com?q=1", "path, query, or fragment"),
        ("https://", "host"),
        ("https://app.example.com:notaport", "port"),
        ("", "empty"),
    ] {
        let error = normalize_cors_origin(origin)
            .expect_err(&format!("origin `{origin}` should be rejected"));
        assert!(
            error.contains(must_mention),
            "rejection for `{origin}` should mention `{must_mention}`, got: {error}"
        );
    }
}

#[test]
fn configured_cors_origins_match_exactly_after_normalization() {
    let set = allowed(&["https://app.example.com", "http://app.example.com:8080"]);

    assert!(is_configured_cors_origin(
        &header("https://app.example.com"),
        &set
    ));
    // Browser-sent origin normalizes to the configured form.
    assert!(is_configured_cors_origin(
        &header("https://App.Example.com:443"),
        &set
    ));
    assert!(is_configured_cors_origin(
        &header("http://app.example.com:8080"),
        &set
    ));

    // Different port, scheme, host, or subdomain: rejected.
    assert!(!is_configured_cors_origin(
        &header("https://app.example.com:8443"),
        &set
    ));
    assert!(!is_configured_cors_origin(
        &header("http://app.example.com"),
        &set
    ));
    assert!(!is_configured_cors_origin(
        &header("https://evil.example.com"),
        &set
    ));
    assert!(!is_configured_cors_origin(
        &header("https://sub.app.example.com"),
        &set
    ));
}

#[test]
fn empty_configured_set_grants_nothing_beyond_loopback() {
    let set = HashSet::new();
    assert!(!is_configured_cors_origin(
        &header("https://app.example.com"),
        &set
    ));
    // The loopback predicate is independent of configuration.
    for origin in [
        "http://localhost",
        "http://localhost:3000",
        "http://127.0.0.1:8080",
        "http://[::1]:8080",
    ] {
        assert!(
            is_allowed_local_cors_origin(&header(origin)),
            "loopback origin {origin} must always be allowed"
        );
    }
    assert!(!is_allowed_local_cors_origin(&header(
        "https://app.example.com"
    )));
}
