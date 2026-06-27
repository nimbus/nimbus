use nimbus_proxy::redact_egress_decision_log_value;

#[test]
fn egress_proxy_redacts_query_bearer_cookie_and_userinfo_in_decision_logs() {
    let redacted_url = redact_egress_decision_log_value(
        "url",
        "https://user:password@example.test/path?token=secret&account=123",
    );
    assert!(
        !redacted_url.contains("password")
            && !redacted_url.contains("secret")
            && !redacted_url.contains("123"),
        "userinfo and query parameter values must be redacted: {redacted_url}"
    );
    assert!(
        redacted_url.contains("token=") && redacted_url.contains("account="),
        "query keys should remain useful while values are redacted: {redacted_url}"
    );

    assert_eq!(
        redact_egress_decision_log_value("Authorization", "Bearer secret-token"),
        "<redacted>"
    );
    assert_eq!(
        redact_egress_decision_log_value("Cookie", "session=secret"),
        "<redacted>"
    );
}
