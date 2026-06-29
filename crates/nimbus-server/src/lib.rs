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
mod router;
mod service_manager;
mod state;
mod system;
mod system_tenant;
mod tenant;
mod tenant_isolation_drift;
mod tls;
mod ws;

pub use adapters::cloud_functions::CloudFunctionsRegistry;
pub use adapters::cloudflare::{
    CloudflareBindingRegistry, CloudflareConfig, D1DatabaseBinding, DurableObjectBinding,
    KvNamespaceBinding, R2BucketBinding, WranglerConfigError,
};
pub use adapters::convex::ConvexRegistry;
pub use adapters::dynamodb::DynamoDbConfig;
pub use adapters::firebase::{FirebaseConfig, ProjectSpecError, ProjectTenantRegistry};
/// Enables Firebase Emulator token-verification bypass for dev/test servers.
///
/// The default Firebase config rejects unverified emulator tokens and uses a
/// strict empty project registry. This helper opts into the loopback-only
/// dev-mode bypass and identity project registry together, matching local
/// emulator semantics without weakening production defaults.
#[must_use]
pub fn enable_firebase_emulator_token_verification_bypass(
    firebase_config: FirebaseConfig,
) -> FirebaseConfig {
    firebase_config
        .with_emulator_token_verification_bypass()
        .with_project_registry(ProjectTenantRegistry::identity())
}
pub use adapters::mongodb::{
    AuthConfig as MongoDbAuthConfig, CredentialRegistry as MongoDbCredentialRegistry, MongoDbConfig,
};
pub use artifact_verifier_effects::{
    ArtifactVerifierCommandBackend, ArtifactVerifierCommandInvocation,
    ArtifactVerifierCommandOutput, ArtifactVerifierCommandRunner, CosignVerifierBackend,
    DEFAULT_ARTIFACT_VERIFIER_TIMEOUT, OfflineVerificationConfig,
    ProcessArtifactVerifierCommandRunner, SbomVerifierBackend, SlsaVerifierBackend,
    admit_guest_executable_artifact, admit_runtime_bundle_artifact,
};
pub use nimbus_dynamodb::AccessKeyRegistry as DynamoDbAccessKeyRegistry;
pub mod adapters_mongodb {
    pub use super::adapters::mongodb::bson_bridge;
    pub use super::adapters::mongodb::listener;
    pub use super::adapters::mongodb::wire;
}
/// Test-only re-export of the otherwise-crate-private DynamoDB adapter, so the
/// `dynamodb_spec` parity runner can boot the listener (mirrors
/// [`adapters_mongodb`]).
pub mod adapters_dynamodb {
    pub use super::adapters::dynamodb::listener;
}
pub use construction::{ServeOptions, serve};
pub use license::{
    LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind, LicenseLoadError,
    LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState, LicenseStatus,
    LicenseUsageSnapshot,
};
pub use local_server::{
    LOCAL_ADMIN_HEADER_NAME, LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord, LocalServerPaths,
    LocalServerPlatform, LocalServerSecurityState, SERVER_DISCOVERY_PROTOCOL_VERSIONS,
    ServerDiscoveryLease, ServerDiscoveryRecord, load_local_admin_token,
    load_or_create_local_admin_token, read_live_server_discovery, rotate_local_admin_token_offline,
};
pub use machine_lifecycle::{
    MachineCreateRequest, MachineLifecycleFuture, MachineLifecycleManager,
    MachineLifecycleSnapshot, MachineUpdateRequest,
};
pub use nimbus_artifacts::{
    ArtifactAdmission, ArtifactAttestationEvidence, ArtifactProvenanceRequirement,
    ArtifactSignatureEvidence, ArtifactSignatureRequirement, ArtifactVerificationEvidence,
    ArtifactVerificationPolicy, ArtifactVerificationRequest, ArtifactVerificationSubject,
    ArtifactVerificationSubjectKind, ArtifactVerifierBackend, ArtifactVerifierBackendIdentity,
    ArtifactVerifierError, ArtifactVerifierErrorKind, ArtifactVerifierResult,
    CompositeArtifactVerifierBackend,
};
pub use nimbus_services::{
    BuiltInServiceSpec, EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog,
    ExternalServiceSpec, LocalBuildAdmission, ServiceBackend, ServiceDefinitionCatalog,
    ServiceInstanceCatalog, ServiceManager,
};
pub use nimbus_system::SystemTenantStatusEvidenceWriter;
pub use router::{RouterOptions, build_router, normalize_cors_origin};
pub use tenant::{
    ArtifactImageVerificationProvider, OPERATOR_POLICY_SCHEMA_VERSION, OperatorAuditPolicy,
    OperatorDeniedEgressEvent, OperatorExternalPolicyBackend, OperatorExternalPolicyBackendError,
    OperatorExternalPolicyBackendErrorKind, OperatorExternalPolicyBackendIdentity,
    OperatorExternalPolicyBackendResult, OperatorExternalPolicyDecision,
    OperatorExternalPolicyEngine, OperatorExternalPolicyEvidence, OperatorExternalPolicyOutcome,
    OperatorExternalPolicyRequest, OperatorImagePolicy, OperatorImageProvenancePolicy,
    OperatorImageSignaturePolicy, OperatorNetworkEndpointPolicy, OperatorNetworkPolicy,
    OperatorPolicyAcceptedRisk, OperatorPolicyAdvisory, OperatorPolicyAdvisoryKind,
    OperatorPolicyAdvisorySeverity, OperatorPolicyDecisionEvaluation, OperatorPolicyDefaults,
    OperatorPolicyDiff, OperatorPolicyDiffSummary, OperatorPolicyDocument, OperatorPolicyDraft,
    OperatorPolicyDraftApproval, OperatorPolicyDraftKind, OperatorPolicyDraftStatus,
    OperatorPolicyEvaluation, OperatorPolicyImageSummary, OperatorPolicyLifecycle,
    OperatorPolicyMetadata, OperatorPolicyProofReport, OperatorPolicyQuotaSummary,
    OperatorPolicyReloadOutcome, OperatorPolicyReloadState, OperatorPolicyWorkload,
    OperatorQuotaPolicy, OperatorRuntimePolicy, OperatorRuntimeProfile,
    OperatorRuntimeResourceEnvelope, OperatorRuntimeSafetyCaps, OperatorRuntimeScalingQuota,
    OperatorSandboxEgressPolicy, OperatorSandboxEgressRulePolicy, OperatorSandboxPolicy,
    OperatorSecretPolicy, OperatorServicePolicy, OperatorStoragePolicy, OperatorVolumePolicy,
    RuntimeIsolationTier, SLSA_PROVENANCE_V1_PREDICATE_TYPE, TENANT_ISOLATION_EVENT_SCHEMA_VERSION,
    TenantAuditRedactionPolicy, TenantImageAdmission, TenantImageAdmissionSource,
    TenantImageAttestationEvidence, TenantImagePolicyDecision, TenantImageProvenanceRequirement,
    TenantImageSignatureEvidence, TenantImageSignatureRequirement, TenantImageVerificationEvidence,
    TenantImageVerificationProvider, TenantImageVerificationRequest, TenantIsolationAuditRecord,
    TenantIsolationAuthorityDecision, TenantIsolationContext, TenantIsolationDecision,
    TenantIsolationDecisionId, TenantIsolationEvent, TenantIsolationEventKind,
    TenantIsolationEventResult, TenantIsolationEventValue, TenantIsolationMode,
    TenantIsolationPolicyInput, TenantNetworkEndpointDecision, TenantNetworkPolicyDecision,
    TenantQuotaPolicyDecision, TenantRuntimePolicyAdmission, TenantRuntimePolicyDecision,
    TenantRuntimeScalingRequest, TenantSecretPolicyDecision, TenantServiceAccessDecision,
    TenantServiceGrantPolicyDecision, TenantStorageAccessDecision, TenantStoragePolicyDecision,
    TenantVolumePolicyDecision, WorkloadAttributes, WorkloadIdentity, WorkloadKind,
    WorkloadLocation, admit_artifact_subject, normalize_artifact_sha256,
    redact_artifact_verifier_output,
};
pub use tenant_isolation_drift::{
    TenantIsolationDriftReport, TenantIsolationDriftScanConfig, TenantIsolationDriftSurface,
    TenantIsolationDriftViolation, scan_tenant_isolation_drift_async,
};
pub use tls::TlsConfig;

#[cfg(test)]
mod tests;
