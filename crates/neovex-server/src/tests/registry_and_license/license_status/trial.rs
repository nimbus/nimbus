use super::*;

#[tokio::test]
async fn license_status_route_returns_trial_license_details() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let license_state = LicenseState::from_document(
        LicenseDocument {
            schema_version: 1,
            kind: LicenseKind::Trial,
            issued_to: Some("Acme Corp".to_string()),
            issued_by: Some("Neovex".to_string()),
            issued_at_unix_ms: Some(1_700_000_000_000),
            expires_at_unix_ms: None,
            trial_expires_at_unix_ms: Some(u64::MAX),
            revenue_limit_usd: Some(10_000_000),
            monthly_active_user_limit: Some(500),
            entitlements: LicenseEntitlements {
                premium_support: true,
                custom_terms: true,
                ..LicenseEntitlements::default()
            },
            notes: None,
        },
        LicenseSourceInfo {
            kind: LicenseSourceKind::ExplicitFile,
            path: Some("/tmp/license.json".to_string()),
        },
    );
    let server =
        ServerFixture::start(build_router_with_license(fixture.service(), license_state)).await;
    let api = HttpApiFixture::new(&server);

    let response = api.license_status().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("license status json should parse");
    assert_eq!(body["kind"], json!("trial"));
    assert_eq!(body["status"], json!("trial_active"));
    assert_eq!(body["issued_to"], json!("Acme Corp"));
    assert_eq!(body["source"]["kind"], json!("explicit_file"));
    assert_eq!(body["source"]["path"], json!("/tmp/license.json"));
    assert_eq!(body["entitlements"]["premium_support"], json!(true));
    assert_eq!(body["entitlements"]["custom_terms"], json!(true));
    assert_eq!(body["usage"]["monthly_active_users"], json!(0));
}
