use super::snapshot::current_time_unix_ms;
use super::*;
use tempfile::tempdir;

#[test]
fn community_snapshot_reports_default_thresholds() {
    let snapshot = LicenseState::community().snapshot();
    assert_eq!(snapshot.kind, LicenseKind::Community);
    assert_eq!(snapshot.status, LicenseStatus::Community);
    assert_eq!(snapshot.revenue_limit_usd, Some(10_000_000));
    assert_eq!(snapshot.monthly_active_user_limit, Some(500));
    assert!(snapshot.warnings.is_empty());
}

#[test]
fn explicit_license_file_loads_and_tracks_path_source() {
    let tempdir = tempdir().expect("license tempdir should build");
    let path = tempdir.path().join("license.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&LicenseDocument {
            schema_version: 1,
            kind: LicenseKind::Trial,
            issued_to: Some("Acme".to_string()),
            issued_by: Some("Neovex".to_string()),
            issued_at_unix_ms: Some(1_700_000_000_000),
            expires_at_unix_ms: None,
            trial_expires_at_unix_ms: Some(current_time_unix_ms() + 60_000),
            revenue_limit_usd: Some(10_000_000),
            monthly_active_user_limit: Some(500),
            entitlements: LicenseEntitlements {
                premium_support: true,
                ..LicenseEntitlements::default()
            },
            notes: Some("trial".to_string()),
        })
        .expect("license document should serialize"),
    )
    .expect("license file should write");

    let state = LicenseState::from_path(&path).expect("license should load");
    let snapshot = state.snapshot();
    assert_eq!(snapshot.source.kind, LicenseSourceKind::ExplicitFile);
    assert_eq!(snapshot.source.path, Some(path.display().to_string()));
    assert_eq!(snapshot.kind, LicenseKind::Trial);
    assert_eq!(snapshot.status, LicenseStatus::TrialActive);
    assert!(snapshot.entitlements.premium_support);
}
