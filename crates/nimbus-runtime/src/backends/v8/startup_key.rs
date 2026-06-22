use crate::limits::{RuntimeCompatibilityTarget, RuntimeLimits, RuntimeProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeStartupSnapshotKey {
    WebLean,
    WebLeanService,
    NodeFull,
    NodeFullService,
}

impl RuntimeStartupSnapshotKey {
    pub(crate) fn for_limits(limits: &RuntimeLimits) -> Option<Self> {
        let service_extension_enabled =
            limits.service_capability_enabled && limits.grants.has_service_grants();
        match RuntimeProfile::for_limits(limits) {
            Some(RuntimeProfile::WebLean) if service_extension_enabled => {
                Some(Self::WebLeanService)
            }
            Some(RuntimeProfile::WebLean) => Some(Self::WebLean),
            Some(RuntimeProfile::NodeFull) if service_extension_enabled => {
                Some(Self::NodeFullService)
            }
            Some(RuntimeProfile::NodeFull) => Some(Self::NodeFull),
            None => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_target(target: RuntimeCompatibilityTarget) -> Option<Self> {
        match RuntimeProfile::for_compatibility_target(target)? {
            RuntimeProfile::WebLean => Some(Self::WebLean),
            RuntimeProfile::NodeFull => Some(Self::NodeFull),
        }
    }

    pub(crate) fn snapshot_build_target(self) -> RuntimeCompatibilityTarget {
        match self {
            Self::WebLean | Self::WebLeanService => RuntimeCompatibilityTarget::WebStandardIsolate,
            // Node startup snapshots intentionally contain only target-invariant
            // substrate. Exact Node major metadata is installed after runtime
            // construction from the per-invocation contract.
            Self::NodeFull | Self::NodeFullService => RuntimeCompatibilityTarget::Node22,
        }
    }

    pub(crate) const fn service_extension_enabled(self) -> bool {
        match self {
            Self::WebLean | Self::NodeFull => false,
            Self::WebLeanService | Self::NodeFullService => true,
        }
    }

    #[cfg(test)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WebLean => "web_lean",
            Self::WebLeanService => "web_lean_service",
            Self::NodeFull => "node_full",
            Self::NodeFullService => "node_full_service",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_snapshot_key_collapses_node_majors_to_node_full() {
        for target in [
            RuntimeCompatibilityTarget::Node20,
            RuntimeCompatibilityTarget::Node22,
            RuntimeCompatibilityTarget::Node24,
            RuntimeCompatibilityTarget::Node26,
        ] {
            let key = RuntimeStartupSnapshotKey::for_target(target)
                .expect("node target should have a startup snapshot key");
            assert_eq!(key, RuntimeStartupSnapshotKey::NodeFull);
            assert_eq!(key.as_str(), "node_full");
            assert_eq!(
                key.snapshot_build_target(),
                RuntimeCompatibilityTarget::Node22,
                "NodeFull snapshots use the invariant Node substrate build target"
            );
        }
    }

    #[test]
    fn startup_snapshot_key_keeps_web_and_unsupported_targets_separate() {
        assert_eq!(
            RuntimeStartupSnapshotKey::for_target(RuntimeCompatibilityTarget::WebStandardIsolate),
            Some(RuntimeStartupSnapshotKey::WebLean)
        );
        assert_eq!(
            RuntimeStartupSnapshotKey::for_target(RuntimeCompatibilityTarget::BunJsc),
            None
        );
    }

    #[test]
    fn startup_snapshot_key_partitions_optional_service_extension() {
        let mut web_limits = RuntimeLimits::application_web_standard();
        assert_eq!(
            RuntimeStartupSnapshotKey::for_limits(&web_limits),
            Some(RuntimeStartupSnapshotKey::WebLean)
        );
        web_limits.service_capability_enabled = true;
        web_limits.grants.service = vec!["db".to_string()];
        let web_key = RuntimeStartupSnapshotKey::for_limits(&web_limits)
            .expect("WebStandard service snapshot key should exist");
        assert_eq!(web_key, RuntimeStartupSnapshotKey::WebLeanService);
        assert_eq!(web_key.as_str(), "web_lean_service");
        assert!(web_key.service_extension_enabled());

        let mut node_limits = RuntimeLimits::application_node24();
        node_limits.service_capability_enabled = true;
        node_limits.grants.service = vec!["db".to_string()];
        let node_key = RuntimeStartupSnapshotKey::for_limits(&node_limits)
            .expect("NodeFull service snapshot key should exist");
        assert_eq!(node_key, RuntimeStartupSnapshotKey::NodeFullService);
        assert_eq!(
            node_key.snapshot_build_target(),
            RuntimeCompatibilityTarget::Node22
        );
        assert!(node_key.service_extension_enabled());
    }
}
