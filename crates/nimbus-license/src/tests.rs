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
            issued_by: Some("Nimbus".to_string()),
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

#[test]
fn expired_license_snapshots_do_not_report_active_entitlements() {
    for (kind, expires_at, expected_status) in [
        (
            LicenseKind::Trial,
            (None, Some(1)),
            LicenseStatus::TrialExpired,
        ),
        (
            LicenseKind::Enterprise,
            (Some(1), None),
            LicenseStatus::EnterpriseExpired,
        ),
    ] {
        let state = LicenseState {
            source: LicenseSourceInfo {
                kind: LicenseSourceKind::ExplicitFile,
                path: None,
            },
            document: LicenseDocument {
                schema_version: 1,
                kind,
                issued_to: None,
                issued_by: None,
                issued_at_unix_ms: None,
                expires_at_unix_ms: expires_at.0,
                trial_expires_at_unix_ms: expires_at.1,
                revenue_limit_usd: None,
                monthly_active_user_limit: None,
                entitlements: LicenseEntitlements {
                    premium_support: true,
                    audit_logs: true,
                    ..LicenseEntitlements::default()
                },
                notes: None,
            },
        };

        let snapshot = state.snapshot();
        assert_eq!(snapshot.status, expected_status);
        assert_eq!(snapshot.entitlements, LicenseEntitlements::default());
    }
}
