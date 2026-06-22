use serde::{Deserialize, Serialize};

use super::axes::{RuntimeBackendKind, RuntimeBundleContentKind, RuntimeCompatibilityTarget};
use super::resources::RuntimeLimits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProfile {
    WebLean,
    NodeFull,
}

impl RuntimeProfile {
    pub fn for_limits(limits: &RuntimeLimits) -> Option<Self> {
        if !matches!(limits.backend_kind, RuntimeBackendKind::V8)
            || !matches!(
                limits.bundle_content_kind,
                RuntimeBundleContentKind::JavaScript
            )
        {
            return None;
        }
        Self::for_compatibility_target(limits.compatibility_target)
    }

    pub fn for_compatibility_target(target: RuntimeCompatibilityTarget) -> Option<Self> {
        match target {
            RuntimeCompatibilityTarget::WebStandardIsolate => Some(Self::WebLean),
            RuntimeCompatibilityTarget::Node20
            | RuntimeCompatibilityTarget::Node22
            | RuntimeCompatibilityTarget::Node24
            | RuntimeCompatibilityTarget::Node26 => Some(Self::NodeFull),
            RuntimeCompatibilityTarget::BunJsc => None,
        }
    }
}
