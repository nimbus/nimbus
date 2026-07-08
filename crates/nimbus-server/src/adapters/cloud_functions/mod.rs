mod execution;
mod http;

pub(crate) use http::http_handler;
pub use nimbus_cloud_functions::{
    CloudFunctionsHttpExposure, CloudFunctionsRegistry, CloudFunctionsTargetBinding,
};
// The artifact-layout constants are only reached from this crate's
// fixture-building test modules now: the deploy orchestration that used to
// consume them (`http::deploy`) moved to `nimbus_compute::deploy` (CP3),
// which imports them directly from `nimbus_cloud_functions` rather than
// through this re-export.
#[cfg(test)]
pub use nimbus_cloud_functions::{
    CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
    CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
    CloudFunctionsAuthoringSurface, CloudFunctionsExecutionPrincipal, CloudFunctionsSignatureType,
    CloudFunctionsTargetDefinition, CloudFunctionsTargetsManifest,
};
