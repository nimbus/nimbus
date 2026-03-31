use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use neovex_engine::MonthlyActiveUsersSnapshot;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_LICENSE_PATH: &str = ".neovex/license.json";
pub const LICENSE_FILE_ENV: &str = "NEOVEX_LICENSE_FILE";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseKind {
    Community,
    Trial,
    Enterprise,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseStatus {
    Community,
    TrialActive,
    TrialExpired,
    EnterpriseActive,
    EnterpriseExpired,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseSourceKind {
    CommunityDefault,
    ExplicitFile,
    EnvironmentFile,
    DefaultPath,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LicenseSourceInfo {
    pub kind: LicenseSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LicenseEntitlements {
    #[serde(default)]
    pub hosted_service: bool,
    #[serde(default)]
    pub oem_embedding: bool,
    #[serde(default)]
    pub premium_support: bool,
    #[serde(default)]
    pub custom_terms: bool,
    #[serde(default)]
    pub sso: bool,
    #[serde(default)]
    pub audit_logs: bool,
    #[serde(default)]
    pub backup_api: bool,
    #[serde(default)]
    pub multi_node: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenseDocument {
    #[serde(default = "license_schema_version")]
    pub schema_version: u32,
    pub kind: LicenseKind,
    #[serde(default)]
    pub issued_to: Option<String>,
    #[serde(default)]
    pub issued_by: Option<String>,
    #[serde(default)]
    pub issued_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub trial_expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub revenue_limit_usd: Option<u64>,
    #[serde(default)]
    pub monthly_active_user_limit: Option<u64>,
    #[serde(default)]
    pub entitlements: LicenseEntitlements,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LicenseSnapshot {
    pub source: LicenseSourceInfo,
    pub kind: LicenseKind,
    pub status: LicenseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_expires_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revenue_limit_usd: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_active_user_limit: Option<u64>,
    pub entitlements: LicenseEntitlements,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LicenseUsageSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LicenseUsageSnapshot {
    pub month: String,
    pub month_start_unix_ms: u64,
    pub monthly_active_users: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_recorded_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_exceeded: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct LicenseState {
    source: LicenseSourceInfo,
    document: LicenseDocument,
}

#[derive(Debug, Error)]
pub enum LicenseLoadError {
    #[error("failed to read license file {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse license file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

fn license_schema_version() -> u32 {
    1
}

impl LicenseState {
    pub fn community() -> Self {
        Self {
            source: LicenseSourceInfo {
                kind: LicenseSourceKind::CommunityDefault,
                path: None,
            },
            document: LicenseDocument {
                schema_version: license_schema_version(),
                kind: LicenseKind::Community,
                issued_to: None,
                issued_by: None,
                issued_at_unix_ms: None,
                expires_at_unix_ms: None,
                trial_expires_at_unix_ms: None,
                revenue_limit_usd: Some(10_000_000),
                monthly_active_user_limit: Some(500),
                entitlements: LicenseEntitlements::default(),
                notes: None,
            },
        }
    }

    pub fn from_document(document: LicenseDocument, source: LicenseSourceInfo) -> Self {
        Self { source, document }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, LicenseLoadError> {
        Self::load_path(path.as_ref(), LicenseSourceKind::ExplicitFile)
    }

    pub fn load(explicit_path: Option<&Path>) -> Result<Self, LicenseLoadError> {
        if let Some(path) = explicit_path {
            return Self::load_path(path, LicenseSourceKind::ExplicitFile);
        }

        if let Some(path) = env::var_os(LICENSE_FILE_ENV) {
            return Self::load_path(Path::new(&path), LicenseSourceKind::EnvironmentFile);
        }

        let default_path = PathBuf::from(DEFAULT_LICENSE_PATH);
        if default_path.exists() {
            return Self::load_path(&default_path, LicenseSourceKind::DefaultPath);
        }

        Ok(Self::community())
    }

    pub fn snapshot(&self) -> LicenseSnapshot {
        self.snapshot_with_usage(None)
    }

    pub fn snapshot_with_usage(
        &self,
        usage: Option<MonthlyActiveUsersSnapshot>,
    ) -> LicenseSnapshot {
        let mut warnings = Vec::new();
        let now = current_time_unix_ms();
        let status = match self.document.kind {
            LicenseKind::Community => LicenseStatus::Community,
            LicenseKind::Trial => {
                if let Some(expires_at) = self.document.trial_expires_at_unix_ms {
                    if expires_at <= now {
                        warnings.push("trial license has expired".to_string());
                        LicenseStatus::TrialExpired
                    } else {
                        maybe_warn_about_window(
                            &mut warnings,
                            expires_at,
                            now,
                            14,
                            "trial license expires soon",
                        );
                        LicenseStatus::TrialActive
                    }
                } else {
                    warnings.push("trial license has no expiration timestamp".to_string());
                    LicenseStatus::TrialActive
                }
            }
            LicenseKind::Enterprise => {
                if let Some(expires_at) = self.document.expires_at_unix_ms {
                    if expires_at <= now {
                        warnings.push("enterprise license has expired".to_string());
                        LicenseStatus::EnterpriseExpired
                    } else {
                        maybe_warn_about_window(
                            &mut warnings,
                            expires_at,
                            now,
                            30,
                            "enterprise license expires soon",
                        );
                        LicenseStatus::EnterpriseActive
                    }
                } else {
                    LicenseStatus::EnterpriseActive
                }
            }
        };
        let usage = usage.map(|usage| {
            let limit = self.document.monthly_active_user_limit;
            let limit_exceeded = limit.map(|limit| usage.monthly_active_users > limit);
            if let Some(limit) = limit {
                if usage.monthly_active_users > limit {
                    match self.document.kind {
                        LicenseKind::Enterprise => warnings.push(
                            "observed monthly active users exceed the licensed limit".to_string(),
                        ),
                        LicenseKind::Community | LicenseKind::Trial => warnings.push(
                            "observed monthly active users exceed the configured limit; enterprise licensing may be required depending on annual revenue".to_string(),
                        ),
                    }
                } else if limit > 0
                    && usage.monthly_active_users.saturating_mul(10)
                        >= limit.saturating_mul(9)
                {
                    warnings.push(
                        "observed monthly active users are approaching the configured limit"
                            .to_string(),
                    );
                }
            }
            LicenseUsageSnapshot {
                month: usage.month,
                month_start_unix_ms: usage.month_start_unix_ms,
                monthly_active_users: usage.monthly_active_users,
                last_recorded_at_unix_ms: usage.last_recorded_at_unix_ms,
                limit,
                limit_exceeded,
            }
        });

        LicenseSnapshot {
            source: self.source.clone(),
            kind: self.document.kind,
            status,
            issued_to: self.document.issued_to.clone(),
            issued_by: self.document.issued_by.clone(),
            issued_at_unix_ms: self.document.issued_at_unix_ms,
            expires_at_unix_ms: self.document.expires_at_unix_ms,
            trial_expires_at_unix_ms: self.document.trial_expires_at_unix_ms,
            revenue_limit_usd: self.document.revenue_limit_usd,
            monthly_active_user_limit: self.document.monthly_active_user_limit,
            entitlements: self.document.entitlements.clone(),
            usage,
            warnings,
        }
    }

    fn load_path(path: &Path, source_kind: LicenseSourceKind) -> Result<Self, LicenseLoadError> {
        let display_path = path.display().to_string();
        let raw = fs::read_to_string(path).map_err(|source| LicenseLoadError::Read {
            path: display_path.clone(),
            source,
        })?;
        let document = serde_json::from_str(&raw).map_err(|source| LicenseLoadError::Parse {
            path: display_path.clone(),
            source,
        })?;
        Ok(Self {
            source: LicenseSourceInfo {
                kind: source_kind,
                path: Some(display_path),
            },
            document,
        })
    }
}

fn current_time_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn maybe_warn_about_window(
    warnings: &mut Vec<String>,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    days: u64,
    message: &str,
) {
    let remaining_ms = expires_at_unix_ms.saturating_sub(now_unix_ms);
    let warning_window_ms = days * 24 * 60 * 60 * 1000;
    if remaining_ms <= warning_window_ms {
        warnings.push(message.to_string());
    }
}

#[cfg(test)]
mod tests {
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
}
