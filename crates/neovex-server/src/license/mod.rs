use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use neovex_engine::MonthlyActiveUsersSnapshot;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod loading;
mod snapshot;
#[cfg(test)]
mod tests;

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
