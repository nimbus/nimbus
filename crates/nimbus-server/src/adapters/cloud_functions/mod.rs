#![allow(dead_code)]
// The Cloud Functions contract types land before the later deploy/runtime
// phases wire them into the live server surface.

pub(crate) mod app_contract;
mod execution;
mod host_bridge;
mod http;
mod registry;
pub(crate) mod runtime_api;

use std::collections::BTreeSet;

use nimbus_core::{DocumentTriggerPattern, Error, FirestoreCloudEventType, Result};
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
pub(crate) use execution::CloudFunctionsTriggerExecutor;
#[allow(unused_imports)]
pub(crate) use http::http_handler;
#[allow(unused_imports)]
pub use registry::CloudFunctionsRegistry;

/// Sibling internal artifact namespace for Cloud Functions-compatible bundles.
///
/// The current Convex registry and manifest loader are intentionally left
/// intact. Cloud Functions artifacts use the same staging, integrity, and
/// generation-activation guarantees, but keep a separate internal layout so we
/// do not force the Convex manifest schema into an artificial "generic" shape.
pub(crate) const CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR: &str = ".nimbus/firebase";
pub(crate) const CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE: &str = "artifact.json";
pub(crate) const CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE: &str = "targets.json";
pub(crate) const CLOUD_FUNCTIONS_RUNTIME_BUNDLE_FILE: &str = "bundle.mjs";
pub(crate) const CLOUD_FUNCTIONS_RUNTIME_BUNDLE_SHA256_FILE: &str = "bundle.sha256";

const CLOUD_FUNCTIONS_ARTIFACT_VERSION: u16 = 1;
const CLOUD_FUNCTIONS_TARGETS_MANIFEST_VERSION: u16 = 1;

const COVERED_IMPORT_SPECIFIERS: &[&str] = &[
    "@google-cloud/functions-framework",
    "firebase-admin/app",
    "firebase-admin/firestore",
    "firebase-functions/v2",
    "firebase-functions/v2/firestore",
    "firebase-functions/v2/https",
];

/// Artifact-family identifier persisted in the Cloud Functions manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeArtifactFamily {
    CloudFunctions,
}

/// Import-resolution contract for first-slice Cloud Functions compatibility.
///
/// Unchanged upstream imports are preserved by a Nimbus-owned build/deploy
/// alias layer. We intentionally do not require user source rewrites or
/// package-manager-level replacement of upstream package names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsImportResolutionStrategy {
    DeployAliasLayer,
}

/// Runtime bundle payload that must pass integrity validation before activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeBundleArtifact {
    pub(crate) entry_file: String,
    pub(crate) sha256_file: String,
}

/// Import-resolution metadata for first-slice compatibility shims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsImportResolution {
    pub(crate) strategy: CloudFunctionsImportResolutionStrategy,
    pub(crate) covered_specifiers: Vec<String>,
}

/// Stable artifact manifest for Cloud Functions-compatible deployments.
///
/// `targets_manifest` reserves the deploy-time target/binding hook that `T0.5`
/// will define in detail. `T0.4` only fixes the file name and artifact-family
/// envelope so later work can reuse the same activation contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsArtifactManifest {
    pub(crate) version: u16,
    pub(crate) family: RuntimeArtifactFamily,
    pub(crate) runtime_bundle: RuntimeBundleArtifact,
    pub(crate) targets_manifest: String,
    pub(crate) import_resolution: CloudFunctionsImportResolution,
}

impl CloudFunctionsArtifactManifest {
    pub(crate) fn v1() -> Self {
        Self {
            version: CLOUD_FUNCTIONS_ARTIFACT_VERSION,
            family: RuntimeArtifactFamily::CloudFunctions,
            runtime_bundle: RuntimeBundleArtifact {
                entry_file: CLOUD_FUNCTIONS_RUNTIME_BUNDLE_FILE.to_string(),
                sha256_file: CLOUD_FUNCTIONS_RUNTIME_BUNDLE_SHA256_FILE.to_string(),
            },
            targets_manifest: CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE.to_string(),
            import_resolution: CloudFunctionsImportResolution {
                strategy: CloudFunctionsImportResolutionStrategy::DeployAliasLayer,
                covered_specifiers: covered_import_specifiers(),
            },
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != CLOUD_FUNCTIONS_ARTIFACT_VERSION {
            return Err(Error::InvalidInput(format!(
                "cloud functions artifact manifest version {} is unsupported; expected {}",
                self.version, CLOUD_FUNCTIONS_ARTIFACT_VERSION
            )));
        }
        if !matches!(self.family, RuntimeArtifactFamily::CloudFunctions) {
            return Err(Error::InvalidInput(
                "cloud functions artifact manifest must use `cloud_functions` family".to_string(),
            ));
        }
        if self.runtime_bundle.entry_file.trim().is_empty() {
            return Err(Error::InvalidInput(
                "cloud functions artifact manifest requires a non-empty runtime bundle entry file"
                    .to_string(),
            ));
        }
        if self.runtime_bundle.sha256_file.trim().is_empty() {
            return Err(Error::InvalidInput(
                "cloud functions artifact manifest requires a non-empty runtime bundle sha256 file"
                    .to_string(),
            ));
        }
        if self.targets_manifest.trim().is_empty() {
            return Err(Error::InvalidInput(
                "cloud functions artifact manifest requires a non-empty targets manifest path"
                    .to_string(),
            ));
        }
        if !matches!(
            self.import_resolution.strategy,
            CloudFunctionsImportResolutionStrategy::DeployAliasLayer
        ) {
            return Err(Error::InvalidInput(
                "cloud functions artifact manifest requires `deploy_alias_layer` import resolution"
                    .to_string(),
            ));
        }
        if self.import_resolution.covered_specifiers != covered_import_specifiers() {
            return Err(Error::InvalidInput(
                "cloud functions artifact manifest covered import specifiers do not match the first-slice contract"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn covered_import_specifiers() -> Vec<String> {
    COVERED_IMPORT_SPECIFIERS
        .iter()
        .map(|specifier| (*specifier).to_string())
        .collect()
}

/// Which source-level API declared this target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsAuthoringSurface {
    FirebaseV2,
    FunctionsFramework,
}

/// Runtime function signature advertised by the authoring surface.
///
/// The first slice intentionally rejects legacy `event` signatures even though
/// the standalone Functions Framework can optionally support them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsSignatureType {
    Http,
    CloudEvent,
    Event,
}

/// Execution identity semantics captured at deploy time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsExecutionBinding {
    Service,
    Request,
}

/// HTTP-facing target shape reserved for later T3 runtime routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsHttpExposure {
    Http,
    Callable,
}

/// Root-level `firebase-functions/v2` APIs that affect handler defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsRootApi {
    SetGlobalOptions,
    OnInit,
}

/// Handler families that can inherit root-level defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsDefaultSurface {
    FirestoreDocument,
    HttpsRequest,
    HttpsCallable,
}

/// Explicitly modeled `GlobalOptions` fields for the first slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsGlobalOptionField {
    Retry,
    Region,
    Memory,
    TimeoutSeconds,
    MinInstances,
    MaxInstances,
    Concurrency,
    Cpu,
    ServiceAccount,
    IngressSettings,
    Invoker,
    Labels,
    Secrets,
    EnforceAppCheck,
    PreserveExternalChanges,
    Omit,
    VpcConnector,
    VpcEgress,
    VpcConnectorEgressSettings,
    NetworkInterface,
}

/// Root-level defaults supported by the first Firebase-compatible slice.
///
/// The only inherited field before the later HTTP/runtime work is `retry` for
/// Firestore document triggers. All other `GlobalOptions` fields remain
/// explicit fail-fast boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct CloudFunctionsGlobalDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) retry: Option<bool>,
}

/// Normalized document-trigger defaults after root inheritance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct CloudFunctionsDocumentTriggerDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) retry: Option<bool>,
}

impl CloudFunctionsDocumentTriggerDefaults {
    pub(crate) fn inherit(
        global: &CloudFunctionsGlobalDefaults,
        explicit: &CloudFunctionsDocumentTriggerDefaults,
    ) -> Result<Self> {
        validate_global_option_support(
            CloudFunctionsDefaultSurface::FirestoreDocument,
            CloudFunctionsGlobalOptionField::Retry,
        )?;
        Ok(Self {
            retry: explicit.retry.or(global.retry),
        })
    }
}

/// Stable `targets.json` manifest carried beside the Cloud Functions bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsTargetsManifest {
    pub(crate) version: u16,
    pub(crate) targets: Vec<CloudFunctionsTargetDefinition>,
}

impl CloudFunctionsTargetsManifest {
    pub(crate) fn v1(targets: Vec<CloudFunctionsTargetDefinition>) -> Result<Self> {
        let manifest = Self {
            version: CLOUD_FUNCTIONS_TARGETS_MANIFEST_VERSION,
            targets,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != CLOUD_FUNCTIONS_TARGETS_MANIFEST_VERSION {
            return Err(Error::InvalidInput(format!(
                "cloud functions targets manifest version {} is unsupported; expected {}",
                self.version, CLOUD_FUNCTIONS_TARGETS_MANIFEST_VERSION
            )));
        }
        let mut seen_names = BTreeSet::new();
        let mut seen_http_paths = BTreeSet::new();
        for target in &self.targets {
            if !seen_names.insert(target.name.as_str()) {
                return Err(Error::InvalidInput(format!(
                    "cloud functions targets manifest cannot reuse target name `{}`",
                    target.name
                )));
            }
            if let CloudFunctionsTargetBinding::Https { path, .. } = &target.binding
                && !seen_http_paths.insert(path.as_str())
            {
                return Err(Error::InvalidInput(format!(
                    "cloud functions targets manifest cannot reuse HTTP path `{path}`"
                )));
            }
            target.validate()?;
        }
        Ok(())
    }
}

/// One named handler target resolved during build or deploy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsTargetDefinition {
    pub(crate) name: String,
    pub(crate) entrypoint: String,
    pub(crate) authoring_surface: CloudFunctionsAuthoringSurface,
    pub(crate) signature_type: CloudFunctionsSignatureType,
    pub(crate) binding: CloudFunctionsTargetBinding,
}

impl CloudFunctionsTargetDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "cloud functions target name cannot be empty".to_string(),
            ));
        }
        if self.entrypoint.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "cloud functions target `{}` requires a non-empty runtime entrypoint",
                self.name
            )));
        }
        if matches!(self.signature_type, CloudFunctionsSignatureType::Event) {
            return Err(Error::InvalidInput(format!(
                "cloud functions target `{}` uses unsupported legacy `event` signature type; first slice only covers `http` and `cloudevent`",
                self.name
            )));
        }

        self.binding.validate(self)
    }
}

/// Deploy-time binding contract shared by Firebase and Functions Framework.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "binding_kind", rename_all = "snake_case")]
pub(crate) enum CloudFunctionsTargetBinding {
    FirestoreDocument {
        event_type: FirestoreCloudEventType,
        database: String,
        document: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        execution: CloudFunctionsExecutionBinding,
    },
    Https {
        exposure: CloudFunctionsHttpExposure,
        path: String,
        execution: CloudFunctionsExecutionBinding,
    },
}

impl CloudFunctionsTargetBinding {
    fn validate(&self, target: &CloudFunctionsTargetDefinition) -> Result<()> {
        match self {
            Self::FirestoreDocument {
                database,
                document,
                namespace,
                execution,
                ..
            } => {
                if !matches!(
                    target.signature_type,
                    CloudFunctionsSignatureType::CloudEvent
                ) {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` binds a Firestore document event but uses `{}` signature type",
                        target.name,
                        target.signature_type.as_str()
                    )));
                }
                if database.trim().is_empty() {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` requires a non-empty Firestore database id",
                        target.name
                    )));
                }
                if let Some(namespace) = namespace {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` uses unsupported Firestore namespace `{namespace}`; first slice only covers default namespace document triggers",
                        target.name
                    )));
                }
                validate_trigger_pattern(target, document)?;
                if !matches!(execution, CloudFunctionsExecutionBinding::Service) {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` must run Firestore document triggers as a trusted service principal",
                        target.name
                    )));
                }
            }
            Self::Https {
                path, execution, ..
            } => {
                if !matches!(target.signature_type, CloudFunctionsSignatureType::Http) {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` binds HTTP exposure but uses `{}` signature type",
                        target.name,
                        target.signature_type.as_str()
                    )));
                }
                if !path.starts_with('/') {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` requires an HTTP path beginning with `/`",
                        target.name
                    )));
                }
                if is_reserved_cloud_functions_http_path(path) {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` cannot use reserved HTTP path `{path}`",
                        target.name
                    )));
                }
                if !matches!(execution, CloudFunctionsExecutionBinding::Request) {
                    return Err(Error::InvalidInput(format!(
                        "cloud functions target `{}` must use request-scoped execution for HTTP exposure",
                        target.name
                    )));
                }
            }
        }

        Ok(())
    }
}

impl CloudFunctionsSignatureType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::CloudEvent => "cloudevent",
            Self::Event => "event",
        }
    }
}

impl CloudFunctionsGlobalOptionField {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Region => "region",
            Self::Memory => "memory",
            Self::TimeoutSeconds => "timeoutSeconds",
            Self::MinInstances => "minInstances",
            Self::MaxInstances => "maxInstances",
            Self::Concurrency => "concurrency",
            Self::Cpu => "cpu",
            Self::ServiceAccount => "serviceAccount",
            Self::IngressSettings => "ingressSettings",
            Self::Invoker => "invoker",
            Self::Labels => "labels",
            Self::Secrets => "secrets",
            Self::EnforceAppCheck => "enforceAppCheck",
            Self::PreserveExternalChanges => "preserveExternalChanges",
            Self::Omit => "omit",
            Self::VpcConnector => "vpcConnector",
            Self::VpcEgress => "vpcEgress",
            Self::VpcConnectorEgressSettings => "vpcConnectorEgressSettings",
            Self::NetworkInterface => "networkInterface",
        }
    }
}

pub(crate) fn validate_root_api(api: CloudFunctionsRootApi) -> Result<()> {
    match api {
        CloudFunctionsRootApi::SetGlobalOptions => Ok(()),
        CloudFunctionsRootApi::OnInit => Err(Error::InvalidInput(
            "firebase-functions/v2 root API `onInit()` is deferred; first slice only covers `setGlobalOptions()` plus namespace imports".to_string(),
        )),
    }
}

pub(crate) fn validate_global_option_support(
    surface: CloudFunctionsDefaultSurface,
    field: CloudFunctionsGlobalOptionField,
) -> Result<()> {
    if matches!(
        (surface, field),
        (
            CloudFunctionsDefaultSurface::FirestoreDocument,
            CloudFunctionsGlobalOptionField::Retry
        )
    ) {
        return Ok(());
    }

    Err(Error::InvalidInput(format!(
        "global option `{}` is not covered for `{}` in the first cloud functions slice",
        field.as_str(),
        surface.as_str()
    )))
}

impl CloudFunctionsDefaultSurface {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::FirestoreDocument => "firestore_document",
            Self::HttpsRequest => "https_request",
            Self::HttpsCallable => "https_callable",
        }
    }
}

fn validate_trigger_pattern(target: &CloudFunctionsTargetDefinition, document: &str) -> Result<()> {
    let segments = document.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(Error::InvalidInput(format!(
            "cloud functions target `{}` cannot use an empty segment in document pattern `{document}`",
            target.name
        )));
    }
    DocumentTriggerPattern::from_segments(segments).map_err(|error| {
        Error::InvalidInput(format!(
            "cloud functions target `{}` has invalid document pattern `{document}`: {error}",
            target.name
        ))
    })?;
    Ok(())
}

fn is_reserved_cloud_functions_http_path(path: &str) -> bool {
    matches!(
        path,
        "/health"
            | "/ws"
            | "/api"
            | "/convex"
            | "/debug"
            | "/demos"
            | "/ui"
            | "/v1"
            | "/google.firestore.v1.Firestore"
    ) || path.starts_with("/api/")
        || path.starts_with("/convex/")
        || path.starts_with("/debug/")
        || path.starts_with("/demos/")
        || path.starts_with("/ui/")
        || path.starts_with("/v1/")
        || path.starts_with("/google.firestore.v1.Firestore/")
}

#[cfg(test)]
mod tests {
    use super::{
        CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
        CLOUD_FUNCTIONS_RUNTIME_BUNDLE_FILE, CLOUD_FUNCTIONS_RUNTIME_BUNDLE_SHA256_FILE,
        CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
        CloudFunctionsAuthoringSurface, CloudFunctionsDefaultSurface,
        CloudFunctionsDocumentTriggerDefaults, CloudFunctionsExecutionBinding,
        CloudFunctionsGlobalDefaults, CloudFunctionsGlobalOptionField, CloudFunctionsHttpExposure,
        CloudFunctionsImportResolutionStrategy, CloudFunctionsRootApi, CloudFunctionsSignatureType,
        CloudFunctionsTargetBinding, CloudFunctionsTargetDefinition, CloudFunctionsTargetsManifest,
        RuntimeArtifactFamily, covered_import_specifiers, validate_global_option_support,
        validate_root_api,
    };
    use nimbus_core::FirestoreCloudEventType;

    #[test]
    fn cloud_functions_artifacts_use_sibling_firebase_namespace() {
        assert_eq!(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR, ".nimbus/firebase");
        assert_eq!(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, "artifact.json");
        assert_eq!(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, "targets.json");
        assert_eq!(CLOUD_FUNCTIONS_RUNTIME_BUNDLE_FILE, "bundle.mjs");
        assert_eq!(CLOUD_FUNCTIONS_RUNTIME_BUNDLE_SHA256_FILE, "bundle.sha256");
    }

    #[test]
    fn artifact_manifest_v1_reserves_target_manifest_and_bundle_pair() {
        let manifest = CloudFunctionsArtifactManifest::v1();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.family, RuntimeArtifactFamily::CloudFunctions);
        assert_eq!(manifest.runtime_bundle.entry_file, "bundle.mjs");
        assert_eq!(manifest.runtime_bundle.sha256_file, "bundle.sha256");
        assert_eq!(manifest.targets_manifest, "targets.json");
        assert_eq!(
            manifest.import_resolution.strategy,
            CloudFunctionsImportResolutionStrategy::DeployAliasLayer
        );
    }

    #[test]
    fn covered_import_specifiers_match_first_slice_contract_in_sorted_order() {
        assert_eq!(
            covered_import_specifiers(),
            vec![
                "@google-cloud/functions-framework".to_string(),
                "firebase-admin/app".to_string(),
                "firebase-admin/firestore".to_string(),
                "firebase-functions/v2".to_string(),
                "firebase-functions/v2/firestore".to_string(),
                "firebase-functions/v2/https".to_string(),
            ]
        );
    }

    #[test]
    fn artifact_manifest_roundtrips_through_json() {
        let manifest = CloudFunctionsArtifactManifest::v1();
        let encoded = serde_json::to_value(&manifest).expect("manifest should encode");
        let decoded: CloudFunctionsArtifactManifest =
            serde_json::from_value(encoded).expect("manifest should decode");

        assert_eq!(decoded, manifest);
    }

    #[test]
    fn targets_manifest_accepts_firestore_and_http_bindings() {
        let manifest = CloudFunctionsTargetsManifest::v1(vec![
            CloudFunctionsTargetDefinition {
                name: "syncUser".to_string(),
                entrypoint: "exports.syncUser".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::CloudEvent,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Written,
                    database: "(default)".to_string(),
                    document: "users/{userId}".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            },
            CloudFunctionsTargetDefinition {
                name: "helloWorld".to_string(),
                entrypoint: "registry.helloWorld".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            },
        ])
        .expect("manifest should validate");

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.targets.len(), 2);
    }

    #[test]
    fn firestore_binding_rejects_http_signature_type() {
        let error = CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
            name: "invalidTrigger".to_string(),
            entrypoint: "exports.invalidTrigger".to_string(),
            authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
            signature_type: CloudFunctionsSignatureType::Http,
            binding: CloudFunctionsTargetBinding::FirestoreDocument {
                event_type: FirestoreCloudEventType::Created,
                database: "(default)".to_string(),
                document: "users/{userId}".to_string(),
                namespace: None,
                execution: CloudFunctionsExecutionBinding::Service,
            },
        }])
        .expect_err("manifest should reject mismatched signature");

        assert!(
            error
                .to_string()
                .contains("binds a Firestore document event but uses `http` signature type")
        );
    }

    #[test]
    fn target_manifest_rejects_unsupported_runtime_parity_claims() {
        let legacy_signature =
            CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
                name: "legacyEvent".to_string(),
                entrypoint: "registry.legacyEvent".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Event,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Written,
                    database: "(default)".to_string(),
                    document: "users/{userId}".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            }])
            .expect_err("legacy event signature should be rejected");
        assert!(
            legacy_signature
                .to_string()
                .contains("unsupported legacy `event` signature type")
        );

        let namespace = CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
            name: "withNamespace".to_string(),
            entrypoint: "exports.withNamespace".to_string(),
            authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
            signature_type: CloudFunctionsSignatureType::CloudEvent,
            binding: CloudFunctionsTargetBinding::FirestoreDocument {
                event_type: FirestoreCloudEventType::Updated,
                database: "(default)".to_string(),
                document: "users/{userId}".to_string(),
                namespace: Some("tenant-a".to_string()),
                execution: CloudFunctionsExecutionBinding::Service,
            },
        }])
        .expect_err("namespace should be rejected");
        assert!(
            namespace
                .to_string()
                .contains("unsupported Firestore namespace")
        );
    }

    #[test]
    fn target_manifest_rejects_invalid_pattern_duplicate_name_and_http_execution() {
        let invalid_pattern =
            CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
                name: "badPattern".to_string(),
                entrypoint: "exports.badPattern".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::CloudEvent,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Deleted,
                    database: "(default)".to_string(),
                    document: "users/{userId}/messages".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            }])
            .expect_err("collection-terminal patterns should be rejected");
        assert!(
            invalid_pattern
                .to_string()
                .contains("invalid document pattern")
        );

        let duplicate_name = CloudFunctionsTargetsManifest::v1(vec![
            CloudFunctionsTargetDefinition {
                name: "duplicate".to_string(),
                entrypoint: "exports.one".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::CloudEvent,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Written,
                    database: "(default)".to_string(),
                    document: "users/{userId}".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            },
            CloudFunctionsTargetDefinition {
                name: "duplicate".to_string(),
                entrypoint: "exports.two".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/dup".to_string(),
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            },
        ])
        .expect_err("duplicate names should be rejected");
        assert!(
            duplicate_name
                .to_string()
                .contains("cannot reuse target name")
        );

        let invalid_http_execution =
            CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
                name: "badHttp".to_string(),
                entrypoint: "registry.badHttp".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Callable,
                    path: "/call".to_string(),
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            }])
            .expect_err("http bindings must remain request-scoped");
        assert!(
            invalid_http_execution
                .to_string()
                .contains("must use request-scoped execution for HTTP exposure")
        );

        let duplicate_http_path = CloudFunctionsTargetsManifest::v1(vec![
            CloudFunctionsTargetDefinition {
                name: "hello".to_string(),
                entrypoint: "registry.hello".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            },
            CloudFunctionsTargetDefinition {
                name: "helloAgain".to_string(),
                entrypoint: "registry.helloAgain".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            },
        ])
        .expect_err("duplicate HTTP paths should be rejected");
        assert!(
            duplicate_http_path
                .to_string()
                .contains("cannot reuse HTTP path")
        );

        let reserved_http_path =
            CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
                name: "reserved".to_string(),
                entrypoint: "registry.reserved".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/api/admin/deploy".to_string(),
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            }])
            .expect_err("reserved server paths should be rejected");
        assert!(
            reserved_http_path
                .to_string()
                .contains("cannot use reserved HTTP path")
        );
    }

    #[test]
    fn document_trigger_defaults_inherit_retry_and_explicit_override_wins() {
        let inherited = CloudFunctionsDocumentTriggerDefaults::inherit(
            &CloudFunctionsGlobalDefaults { retry: Some(true) },
            &CloudFunctionsDocumentTriggerDefaults::default(),
        )
        .expect("root retry should inherit");
        assert_eq!(inherited.retry, Some(true));

        let overridden = CloudFunctionsDocumentTriggerDefaults::inherit(
            &CloudFunctionsGlobalDefaults { retry: Some(true) },
            &CloudFunctionsDocumentTriggerDefaults { retry: Some(false) },
        )
        .expect("explicit retry should override");
        assert_eq!(overridden.retry, Some(false));
    }

    #[test]
    fn unsupported_global_option_fields_and_root_apis_fail_fast() {
        let https_retry = validate_global_option_support(
            CloudFunctionsDefaultSurface::HttpsRequest,
            CloudFunctionsGlobalOptionField::Retry,
        )
        .expect_err("https retry inheritance should be deferred");
        assert!(
            https_retry
                .to_string()
                .contains("global option `retry` is not covered for `https_request`")
        );

        let region = validate_global_option_support(
            CloudFunctionsDefaultSurface::FirestoreDocument,
            CloudFunctionsGlobalOptionField::Region,
        )
        .expect_err("region should be deferred");
        assert!(
            region
                .to_string()
                .contains("global option `region` is not covered for `firestore_document`")
        );

        let on_init = validate_root_api(CloudFunctionsRootApi::OnInit)
            .expect_err("onInit should be deferred");
        assert!(
            on_init
                .to_string()
                .contains("root API `onInit()` is deferred")
        );
    }
}
