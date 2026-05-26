use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionAdapterState {
    Linked,
    NotLinked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionAdapterArtifactStatus {
    Linked,
    NotLinked,
    MissingArtifact,
    ChecksumMismatch,
    UnsupportedPlatform,
    InvalidManifest,
    LoadFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionAdapterArtifactSource {
    BuiltIn,
    BuildFeatureDisabled,
    DevelopmentLibraryEnv,
    ManifestEnv,
    PackagedManifest,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeExecutionAdapterExpectedArtifact {
    pub kind: String,
    pub schema_version: u32,
    pub source_repository: String,
    pub source_ref: String,
    pub source_revision: String,
    pub target_triple: String,
    pub platform: String,
    pub manifest_file: String,
    pub library_file: String,
    pub readme_file: String,
    pub abi_name: String,
    pub abi_version: u32,
    pub memory_enforcement: String,
    pub lifecycle: String,
    pub proof_target: String,
    pub simdutf_namespace: String,
    pub required_export_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeExecutionAdapterManifestArtifact {
    pub adapter_version: String,
    pub nimbus_version: String,
    pub source_repository: String,
    pub source_ref: String,
    pub source_revision: String,
    pub target_triple: String,
    pub platform: String,
    pub library_file: String,
    pub library_sha256: String,
    pub abi_name: String,
    pub abi_version: u32,
    pub checksum_file: String,
    pub sbom: String,
    pub slsa: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeExecutionAdapterArtifactDiagnostics {
    pub status: RuntimeExecutionAdapterArtifactStatus,
    pub source: RuntimeExecutionAdapterArtifactSource,
    pub reason_code: String,
    pub install_hint: Option<String>,
    pub expected: Option<RuntimeExecutionAdapterExpectedArtifact>,
    pub manifest: Option<RuntimeExecutionAdapterManifestArtifact>,
}

impl RuntimeExecutionAdapterArtifactDiagnostics {
    pub fn built_in(reason_code: impl Into<String>) -> Self {
        Self {
            status: RuntimeExecutionAdapterArtifactStatus::Linked,
            source: RuntimeExecutionAdapterArtifactSource::BuiltIn,
            reason_code: reason_code.into(),
            install_hint: None,
            expected: None,
            manifest: None,
        }
    }
}
