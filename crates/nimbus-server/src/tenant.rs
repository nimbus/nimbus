mod artifact_provenance;
mod audit_events;
mod authority;
mod context;
mod decision;
mod evidence;
mod identity;
mod image_admission;
mod operator_policy;
mod policy_input;
mod runtime_admission;

#[cfg(test)]
mod tests;

pub use artifact_provenance::{
    ArtifactAdmission, ArtifactAttestationEvidence, ArtifactImageVerificationProvider,
    ArtifactProvenanceRequirement, ArtifactSignatureEvidence, ArtifactSignatureRequirement,
    ArtifactVerificationEvidence, ArtifactVerificationPolicy, ArtifactVerificationRequest,
    ArtifactVerificationSubject, ArtifactVerificationSubjectKind, ArtifactVerifierBackend,
    ArtifactVerifierBackendIdentity, ArtifactVerifierCommandBackend,
    ArtifactVerifierCommandInvocation, ArtifactVerifierCommandOutput,
    ArtifactVerifierCommandRunner, ArtifactVerifierError, ArtifactVerifierErrorKind,
    ArtifactVerifierResult, CompositeArtifactVerifierBackend, CosignVerifierBackend,
    DEFAULT_ARTIFACT_VERIFIER_TIMEOUT, OfflineVerificationConfig,
    ProcessArtifactVerifierCommandRunner, SLSA_PROVENANCE_V1_PREDICATE_TYPE, SbomVerifierBackend,
    SlsaVerifierBackend, admit_guest_executable_artifact, admit_runtime_bundle_artifact,
    redact_artifact_verifier_output,
};
pub use audit_events::{
    TENANT_ISOLATION_EVENT_SCHEMA_VERSION, TenantIsolationEvent, TenantIsolationEventKind,
    TenantIsolationEventResult, TenantIsolationEventValue,
};
pub use authority::{TenantIsolationAuthorityDecision, TenantIsolationMode};
pub(crate) use context::{TenantIsolationContext, admit_runtime_invocation_decision};
pub use decision::{
    TenantIsolationAuditRecord, TenantIsolationDecision, TenantIsolationDecisionId,
    TenantServiceAccessDecision, TenantStorageAccessDecision,
};
pub use identity::{
    TenantWorkloadIdentity, TenantWorkloadKind, TenantWorkloadLocation,
    TenantWorkloadStableIdentity,
};
pub use image_admission::{
    TenantImageAdmission, TenantImageAdmissionSource, TenantImageAttestationEvidence,
    TenantImageProvenanceRequirement, TenantImageSignatureEvidence,
    TenantImageSignatureRequirement, TenantImageVerificationEvidence,
    TenantImageVerificationProvider, TenantImageVerificationRequest,
};
pub use operator_policy::{
    OPERATOR_POLICY_SCHEMA_VERSION, OperatorAuditPolicy, OperatorDeniedEgressEvent,
    OperatorExternalPolicyBackend, OperatorExternalPolicyBackendError,
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
    OperatorSandboxEgressPolicy, OperatorSandboxEgressRulePolicy, OperatorSandboxPolicy,
    OperatorSecretPolicy, OperatorServicePolicy, OperatorStoragePolicy, OperatorVolumePolicy,
};
pub use policy_input::{
    TenantAuditRedactionPolicy, TenantImagePolicyDecision, TenantIsolationPolicyInput,
    TenantNetworkEndpointDecision, TenantNetworkPolicyDecision, TenantQuotaPolicyDecision,
    TenantSecretPolicyDecision, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
    TenantVolumePolicyDecision,
};
#[cfg(test)]
pub(crate) use runtime_admission::RuntimeIsolationRoute;
pub(crate) use runtime_admission::RuntimePolicyAdmission;
pub use runtime_admission::{
    RuntimeIsolationTier, TenantRuntimePolicyAdmission, TenantRuntimePolicyDecision,
};
