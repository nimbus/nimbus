//! Nimbus server crate.

mod adapters;
mod application_auth;
mod artifact_verifier_effects;
mod construction;
mod error_envelope;
mod execution;
mod http;
mod latency;
mod license;
pub mod local_enforcement;
mod local_server;
mod machine_lifecycle;
mod owned_tasks;
mod protocol;
mod provider_family;
mod router;
mod runtime_host;
mod sandbox;
mod service_manager;
mod service_registry;
mod state;
mod system;
mod system_tenant;
mod tenant;
mod tenant_isolation_drift;
mod ws;

pub use adapters::cloud_functions::CloudFunctionsRegistry;
pub use adapters::convex::ConvexRegistry;
pub use adapters::firebase::FirebaseConfig;
pub use adapters::mongodb::{AuthConfig as MongoDbAuthConfig, MongoDbConfig};
pub use artifact_verifier_effects::{
    ArtifactVerifierCommandBackend, ArtifactVerifierCommandInvocation,
    ArtifactVerifierCommandOutput, ArtifactVerifierCommandRunner, CosignVerifierBackend,
    DEFAULT_ARTIFACT_VERIFIER_TIMEOUT, OfflineVerificationConfig,
    ProcessArtifactVerifierCommandRunner, SbomVerifierBackend, SlsaVerifierBackend,
};
pub mod adapters_mongodb {
    pub use super::adapters::mongodb::bson_bridge;
    pub use super::adapters::mongodb::listener;
    pub use super::adapters::mongodb::wire;
}
pub use construction::{ServeOptions, serve};
pub use license::{
    LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind, LicenseLoadError,
    LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState, LicenseStatus,
    LicenseUsageSnapshot,
};
pub use local_server::{
    LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord, LocalServerPaths, LocalServerPlatform,
    LocalServerSecurityState, SERVER_DISCOVERY_PROTOCOL_VERSIONS, ServerDiscoveryLease,
    ServerDiscoveryRecord, load_local_admin_token, load_or_create_local_admin_token,
    read_live_server_discovery, rotate_local_admin_token_offline,
};
pub use machine_lifecycle::{
    MachineCreateRequest, MachineLifecycleFuture, MachineLifecycleManager,
    MachineLifecycleSnapshot, MachineUpdateRequest,
};
pub use router::{RouterOptions, build_router};
pub use sandbox::{
    EmptySandboxCatalog, EmptySandboxServiceCatalog, SandboxCatalog, SandboxServiceCatalog,
    SandboxServiceLaunch,
};
pub use service_manager::SandboxServiceManager;
pub use tenant::{
    ArtifactAdmission, ArtifactAttestationEvidence, ArtifactImageVerificationProvider,
    ArtifactProvenanceRequirement, ArtifactSignatureEvidence, ArtifactSignatureRequirement,
    ArtifactVerificationEvidence, ArtifactVerificationPolicy, ArtifactVerificationRequest,
    ArtifactVerificationSubject, ArtifactVerificationSubjectKind, ArtifactVerifierBackend,
    ArtifactVerifierBackendIdentity, ArtifactVerifierError, ArtifactVerifierErrorKind,
    ArtifactVerifierResult, CompositeArtifactVerifierBackend, OPERATOR_POLICY_SCHEMA_VERSION,
    OperatorAuditPolicy, OperatorDeniedEgressEvent, OperatorExternalPolicyBackend,
    OperatorExternalPolicyBackendError, OperatorExternalPolicyBackendErrorKind,
    OperatorExternalPolicyBackendIdentity, OperatorExternalPolicyBackendResult,
    OperatorExternalPolicyDecision, OperatorExternalPolicyEngine, OperatorExternalPolicyEvidence,
    OperatorExternalPolicyOutcome, OperatorExternalPolicyRequest, OperatorImagePolicy,
    OperatorImageProvenancePolicy, OperatorImageSignaturePolicy, OperatorNetworkEndpointPolicy,
    OperatorNetworkPolicy, OperatorPolicyAcceptedRisk, OperatorPolicyAdvisory,
    OperatorPolicyAdvisoryKind, OperatorPolicyAdvisorySeverity, OperatorPolicyDecisionEvaluation,
    OperatorPolicyDefaults, OperatorPolicyDiff, OperatorPolicyDiffSummary, OperatorPolicyDocument,
    OperatorPolicyDraft, OperatorPolicyDraftApproval, OperatorPolicyDraftKind,
    OperatorPolicyDraftStatus, OperatorPolicyEvaluation, OperatorPolicyImageSummary,
    OperatorPolicyLifecycle, OperatorPolicyMetadata, OperatorPolicyProofReport,
    OperatorPolicyQuotaSummary, OperatorPolicyReloadOutcome, OperatorPolicyReloadState,
    OperatorPolicyWorkload, OperatorQuotaPolicy, OperatorRuntimePolicy, OperatorRuntimeProfile,
    OperatorSandboxEgressPolicy, OperatorSandboxEgressRulePolicy, OperatorSandboxPolicy,
    OperatorSecretPolicy, OperatorServicePolicy, OperatorStoragePolicy, OperatorVolumePolicy,
    RuntimeIsolationTier, SLSA_PROVENANCE_V1_PREDICATE_TYPE, TENANT_ISOLATION_EVENT_SCHEMA_VERSION,
    TenantAuditRedactionPolicy, TenantImageAdmission, TenantImageAdmissionSource,
    TenantImageAttestationEvidence, TenantImagePolicyDecision, TenantImageProvenanceRequirement,
    TenantImageSignatureEvidence, TenantImageSignatureRequirement, TenantImageVerificationEvidence,
    TenantImageVerificationProvider, TenantImageVerificationRequest, TenantIsolationAuditRecord,
    TenantIsolationAuthorityDecision, TenantIsolationDecision, TenantIsolationDecisionId,
    TenantIsolationEvent, TenantIsolationEventKind, TenantIsolationEventResult,
    TenantIsolationEventValue, TenantIsolationMode, TenantIsolationPolicyInput,
    TenantNetworkEndpointDecision, TenantNetworkPolicyDecision, TenantQuotaPolicyDecision,
    TenantRuntimePolicyAdmission, TenantRuntimePolicyDecision, TenantSecretPolicyDecision,
    TenantServiceAccessDecision, TenantServiceGrantPolicyDecision, TenantStorageAccessDecision,
    TenantStoragePolicyDecision, TenantVolumePolicyDecision, TenantWorkloadIdentity,
    TenantWorkloadKind, TenantWorkloadLocation, TenantWorkloadStableIdentity,
    admit_guest_executable_artifact, admit_runtime_bundle_artifact,
    redact_artifact_verifier_output,
};
pub use tenant_isolation_drift::{
    TenantIsolationDriftReport, TenantIsolationDriftScanConfig, TenantIsolationDriftSurface,
    TenantIsolationDriftViolation, scan_tenant_isolation_drift_async,
};

#[cfg(test)]
mod tests;
