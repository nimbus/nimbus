use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::metrics::{RuntimeMetrics, RuntimeMetricsSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendKind {
    #[serde(rename = "v8")]
    V8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCompatibilityTarget {
    WebStandardIsolate,
    Node20,
    Node22,
    Node24,
}

impl RuntimeCompatibilityTarget {
    pub fn is_node(self) -> bool {
        matches!(self, Self::Node20 | Self::Node22 | Self::Node24)
    }

    pub fn node_major_version(self) -> Option<u16> {
        match self {
            Self::Node20 => Some(20),
            Self::Node22 => Some(22),
            Self::Node24 => Some(24),
            Self::WebStandardIsolate => None,
        }
    }

    pub fn node_runtime_version(self) -> Option<&'static str> {
        match self {
            Self::Node20 => Some("v20.0.0-nimbus"),
            Self::Node22 => Some("v22.0.0-nimbus"),
            Self::Node24 => Some("v24.0.0-nimbus"),
            Self::WebStandardIsolate => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionModel {
    RunToCompletion,
    CooperativeLocker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Restricted,
    Standard,
    Privileged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLanguage {
    JavaScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePreset {
    Application,
    Tooling,
    Oracle,
    Operator,
    Code,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeGrants {
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub net_connect: Vec<String>,
    pub net_listen: Vec<String>,
    pub env_read: Vec<String>,
    pub env_write: Vec<String>,
    pub secret: Vec<String>,
    pub identity: Vec<String>,
    pub service: Vec<String>,
    pub run: Vec<String>,
    pub sys: Vec<String>,
    pub ffi: Vec<String>,
    pub worker: Vec<String>,
    pub tool: Vec<String>,
}

impl RuntimeGrants {
    pub fn restricted() -> Self {
        Self::default()
    }

    pub fn application_web_standard() -> Self {
        Self {
            read: vec!["$generated_root".to_string()],
            write: vec!["$generated_root".to_string()],
            env_read: vec!["NODE_TLS_REJECT_UNAUTHORIZED".to_string()],
            sys: vec![
                "hostname".to_string(),
                "gid".to_string(),
                "statfs".to_string(),
                "uid".to_string(),
            ],
            ..Self::default()
        }
    }

    pub fn application_node() -> Self {
        let mut grants = Self::application_web_standard();
        grants.net_connect = vec!["127.0.0.1".to_string(), "localhost".to_string()];
        grants.net_listen = vec![
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "0.0.0.0".to_string(),
            "[::1]".to_string(),
            "[::]".to_string(),
        ];
        grants.sys.push("inspector".to_string());
        grants.worker = vec!["thread".to_string()];
        grants
    }

    pub fn tooling() -> Self {
        Self {
            read: vec![
                "$app_root".to_string(),
                "$generated_root".to_string(),
                "$temp_root".to_string(),
                "$cache_root".to_string(),
            ],
            write: vec![
                "$generated_root".to_string(),
                "$temp_root".to_string(),
                "$cache_root".to_string(),
            ],
            net_connect: vec!["127.0.0.1".to_string(), "localhost".to_string()],
            env_read: vec![
                "ESBUILD_BINARY_PATH".to_string(),
                "ESBUILD_MAX_BUFFER".to_string(),
                "ESBUILD_WORKER_THREADS".to_string(),
                "HOME".to_string(),
                "NODE_ENV".to_string(),
                "NODE_TLS_REJECT_UNAUTHORIZED".to_string(),
                "NODE_INSPECTOR_IPC".to_string(),
                "NODE_V8_COVERAGE".to_string(),
                "PATH".to_string(),
                "PWD".to_string(),
                "TEMP".to_string(),
                "TMP".to_string(),
                "TMPDIR".to_string(),
                "TSC_NONPOLLING_WATCHER".to_string(),
                "TSC_WATCHDIRECTORY".to_string(),
                "TSC_WATCHFILE".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE_HIGH".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE_LOW".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE_MEDIUM".to_string(),
                "TSC_WATCH_POLLINGINTERVAL".to_string(),
                "TSC_WATCH_POLLINGINTERVAL_HIGH".to_string(),
                "TSC_WATCH_POLLINGINTERVAL_LOW".to_string(),
                "TSC_WATCH_POLLINGINTERVAL_MEDIUM".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS_HIGH".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS_LOW".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS_MEDIUM".to_string(),
                "VSCODE_INSPECTOR_OPTIONS".to_string(),
                "npm_config_cache".to_string(),
                "npm_config_user_agent".to_string(),
                "npm_execpath".to_string(),
            ],
            run: vec![
                "$discovered_tooling".to_string(),
                "$runtime_self_exec".to_string(),
                "$runtime_host_exec".to_string(),
            ],
            worker: vec!["thread".to_string()],
            sys: vec![
                "hostname".to_string(),
                "gid".to_string(),
                "statfs".to_string(),
                "uid".to_string(),
                "inspector".to_string(),
            ],
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRoutingAffinity {
    None,
    Tenant,
    Function,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePoolKind {
    /// Reuse the worker-local bootstrap snapshot, then build a fresh JsRuntime
    /// for every invocation.
    ///
    /// This preserves the freshest execution boundary and is currently the
    /// default low-latency mode.
    StartupSnapshotCache,
    /// Retain whole JsRuntime instances with evaluated modules alive across
    /// invocations. No realm reset, no module reload — only surgical
    /// per-request state cleanup via `reset_request_state()`.
    ///
    /// Requires `CooperativeLocker` execution model. Fails fast with
    /// `RunToCompletion`.
    WarmPool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeModuleStateSemantics {
    FreshPerInvocation,
    /// Modules persist across invocations by contract. Module-level side
    /// effects (e.g. `let counter = 0`) accumulate across requests.
    WarmPerBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeResetCapabilities {
    pub op_state_per_invocation: bool,
    pub bootstrap_state_per_invocation: bool,
    pub user_module_state_per_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeLimits {
    pub backend_kind: RuntimeBackendKind,
    pub compatibility_target: RuntimeCompatibilityTarget,
    pub execution_model: RuntimeExecutionModel,
    pub mode: RuntimeMode,
    pub language: RuntimeLanguage,
    pub preset: RuntimePreset,
    pub grants: RuntimeGrants,
    pub runtime_pool_kind: RuntimePoolKind,
    pub routing_affinity: RuntimeRoutingAffinity,
    pub routing_affinity_max_entries: usize,
    pub max_warm_pool_entries_per_worker: usize,
    pub max_warm_reuses: usize,
    pub max_heap_mb: usize,
    pub initial_heap_mb: usize,
    pub execution_timeout: Duration,
    pub max_concurrent_runtime_instances: usize,
    pub worker_threads: usize,
    pub max_active_top_level_invocations_per_tenant: usize,
    pub max_in_flight_top_level_invocations_per_tenant: usize,
    pub max_queued_top_level_invocations_per_tenant: usize,
    pub max_nested_runtime_invocations: usize,
}

impl RuntimeLimits {
    pub fn restricted_code() -> Self {
        Self {
            mode: RuntimeMode::Restricted,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Code,
            grants: RuntimeGrants::restricted(),
            ..Self::default()
        }
    }

    pub fn privileged_operator() -> Self {
        Self {
            mode: RuntimeMode::Privileged,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Operator,
            grants: RuntimeGrants::restricted(),
            ..Self::default()
        }
    }

    pub fn application_web_standard() -> Self {
        Self {
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_web_standard(),
            ..Self::default()
        }
    }

    pub fn application_node22() -> Self {
        Self::application_node(RuntimeCompatibilityTarget::Node22)
    }

    pub fn application_node20() -> Self {
        Self::application_node(RuntimeCompatibilityTarget::Node20)
    }

    pub fn application_node24() -> Self {
        Self::application_node(RuntimeCompatibilityTarget::Node24)
    }

    pub fn application_node(target: RuntimeCompatibilityTarget) -> Self {
        assert!(target.is_node(), "application_node requires a Node target");
        Self {
            compatibility_target: target,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_node(),
            ..Self::default()
        }
    }

    pub fn tooling_node22() -> Self {
        Self {
            compatibility_target: RuntimeCompatibilityTarget::Node22,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Tooling,
            grants: RuntimeGrants::tooling(),
            ..Self::default()
        }
    }

    pub fn module_state_semantics(&self) -> RuntimeModuleStateSemantics {
        match self.runtime_pool_kind {
            RuntimePoolKind::WarmPool => RuntimeModuleStateSemantics::WarmPerBundle,
            _ => RuntimeModuleStateSemantics::FreshPerInvocation,
        }
    }

    pub fn reset_capabilities(&self) -> RuntimeResetCapabilities {
        match self.runtime_pool_kind {
            RuntimePoolKind::WarmPool => RuntimeResetCapabilities {
                op_state_per_invocation: true,
                bootstrap_state_per_invocation: true,
                user_module_state_per_invocation: false,
            },
            RuntimePoolKind::StartupSnapshotCache => RuntimeResetCapabilities {
                op_state_per_invocation: true,
                bootstrap_state_per_invocation: true,
                user_module_state_per_invocation: true,
            },
        }
    }

    pub fn normalized(&self) -> Self {
        if matches!(self.preset, RuntimePreset::Tooling)
            && !matches!(
                self.compatibility_target,
                RuntimeCompatibilityTarget::Node22
            )
        {
            panic!(
                "Tooling runtime preset currently requires Node22 compatibility target, \
                 got {:?}",
                self.compatibility_target
            );
        }

        if !self.grants.run.is_empty()
            && !matches!(
                self.compatibility_target,
                RuntimeCompatibilityTarget::Node22
            )
        {
            panic!(
                "runtime run grants currently require Node22 compatibility target, got {:?}",
                self.compatibility_target
            );
        }

        if self
            .grants
            .run
            .iter()
            .any(|grant| grant == "$discovered_tooling")
            && !matches!(self.preset, RuntimePreset::Tooling)
        {
            panic!(
                "$discovered_tooling run grant requires Tooling runtime preset, got {:?}",
                self.preset
            );
        }

        let grants = if matches!(self.preset, RuntimePreset::Application)
            && self.compatibility_target.is_node()
            && self.grants == RuntimeGrants::application_web_standard()
        {
            RuntimeGrants::application_node()
        } else {
            self.grants.clone()
        };
        validate_mode_grant_ceiling(self.mode, &grants);

        // WarmPool requires CooperativeLocker — fail fast.
        if matches!(self.runtime_pool_kind, RuntimePoolKind::WarmPool)
            && !matches!(
                self.execution_model,
                RuntimeExecutionModel::CooperativeLocker
            )
        {
            panic!(
                "WarmPool requires CooperativeLocker execution model, \
                 got {:?}",
                self.execution_model
            );
        }

        let max_concurrent_runtime_instances = self.max_concurrent_runtime_instances.max(1);
        let worker_threads = self
            .worker_threads
            .max(max_concurrent_runtime_instances)
            .max(1);
        let max_heap_mb = self.max_heap_mb.max(1);
        let initial_heap_mb = self.initial_heap_mb.max(1).min(max_heap_mb);
        let max_active_top_level_invocations_per_tenant = self
            .max_active_top_level_invocations_per_tenant
            .max(1)
            .min(max_concurrent_runtime_instances);
        let max_in_flight_top_level_invocations_per_tenant = self
            .max_in_flight_top_level_invocations_per_tenant
            .max(max_active_top_level_invocations_per_tenant)
            .min(worker_threads);
        Self {
            backend_kind: self.backend_kind,
            compatibility_target: self.compatibility_target,
            execution_model: self.execution_model,
            mode: self.mode,
            language: self.language,
            preset: self.preset,
            grants,
            runtime_pool_kind: self.runtime_pool_kind,
            routing_affinity: self.routing_affinity,
            routing_affinity_max_entries: self.routing_affinity_max_entries.max(1),
            max_warm_pool_entries_per_worker: self.max_warm_pool_entries_per_worker.max(1),
            max_warm_reuses: self.max_warm_reuses.max(1),
            max_heap_mb,
            initial_heap_mb,
            execution_timeout: self.execution_timeout,
            max_concurrent_runtime_instances,
            worker_threads,
            max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant: self
                .max_queued_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: self.max_nested_runtime_invocations,
        }
    }
}

fn validate_mode_grant_ceiling(mode: RuntimeMode, grants: &RuntimeGrants) {
    match mode {
        RuntimeMode::Restricted => {
            assert_grant_family_empty(mode, "env_write", &grants.env_write);
            assert_grant_family_empty(mode, "identity", &grants.identity);
            assert_grant_family_empty(mode, "run", &grants.run);
            assert_grant_family_empty(mode, "ffi", &grants.ffi);
            assert_grant_family_empty(mode, "worker", &grants.worker);
            assert_grant_family_empty(mode, "tool", &grants.tool);
        }
        RuntimeMode::Standard => {
            assert_grant_family_empty(mode, "ffi", &grants.ffi);
        }
        RuntimeMode::Privileged => {}
    }
}

fn assert_grant_family_empty(mode: RuntimeMode, family: &str, grants: &[String]) {
    assert!(
        grants.is_empty(),
        "{family} grants exceed the {mode:?} runtime mode ceiling"
    );
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        let max_concurrent_runtime_instances = std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::MIN)
            .get();
        let worker_threads = max_concurrent_runtime_instances.saturating_mul(2).max(1);
        let max_active_top_level_invocations_per_tenant =
            max_concurrent_runtime_instances.saturating_sub(1).max(1);
        let max_in_flight_top_level_invocations_per_tenant =
            max_active_top_level_invocations_per_tenant
                .saturating_mul(2)
                .min(worker_threads)
                .max(max_active_top_level_invocations_per_tenant);
        let routing_affinity_max_entries = worker_threads.saturating_mul(256).max(1024);
        Self {
            backend_kind: RuntimeBackendKind::V8,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_web_standard(),
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            routing_affinity: RuntimeRoutingAffinity::Tenant,
            routing_affinity_max_entries,
            max_warm_pool_entries_per_worker: 4,
            max_warm_reuses: 10_000,
            max_heap_mb: 128,
            initial_heap_mb: 8,
            execution_timeout: Duration::from_secs(30),
            max_concurrent_runtime_instances,
            worker_threads,
            max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant:
                max_in_flight_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: 64,
        }
    }
}

#[derive(Debug)]
pub struct RuntimePolicy {
    limits: RuntimeLimits,
    runtime_instance_semaphore: Arc<Semaphore>,
    metrics: Arc<RuntimeMetrics>,
}

impl RuntimePolicy {
    pub fn new(limits: RuntimeLimits) -> Self {
        let limits = limits.normalized();
        Self {
            runtime_instance_semaphore: Arc::new(Semaphore::new(
                limits.max_concurrent_runtime_instances,
            )),
            metrics: Arc::new(RuntimeMetrics::default()),
            limits,
        }
    }

    pub fn limits(&self) -> &RuntimeLimits {
        &self.limits
    }

    pub(crate) fn runtime_instance_semaphore(&self) -> Arc<Semaphore> {
        self.runtime_instance_semaphore.clone()
    }

    pub fn metrics(&self) -> Arc<RuntimeMetrics> {
        self.metrics.clone()
    }

    pub fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.metrics.snapshot()
    }
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self::new(RuntimeLimits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_preset_supports_node_lts_targets() {
        let web_limits = RuntimeLimits::application_web_standard().normalized();
        assert_eq!(web_limits.mode, RuntimeMode::Standard);
        assert_eq!(web_limits.language, RuntimeLanguage::JavaScript);
        assert_eq!(web_limits.preset, RuntimePreset::Application);
        assert!(web_limits.grants.run.is_empty());
        assert_eq!(
            web_limits.compatibility_target,
            RuntimeCompatibilityTarget::WebStandardIsolate
        );

        let node20_limits = RuntimeLimits::application_node20().normalized();
        assert_eq!(node20_limits.mode, RuntimeMode::Standard);
        assert_eq!(node20_limits.preset, RuntimePreset::Application);
        assert!(node20_limits.grants.run.is_empty());
        assert_eq!(
            node20_limits.compatibility_target,
            RuntimeCompatibilityTarget::Node20
        );

        let node_limits = RuntimeLimits::application_node22().normalized();
        assert_eq!(node_limits.mode, RuntimeMode::Standard);
        assert_eq!(node_limits.preset, RuntimePreset::Application);
        assert!(node_limits.grants.run.is_empty());
        assert_eq!(
            node_limits.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        );

        let node24_limits = RuntimeLimits::application_node24().normalized();
        assert_eq!(node24_limits.mode, RuntimeMode::Standard);
        assert_eq!(node24_limits.preset, RuntimePreset::Application);
        assert!(node24_limits.grants.run.is_empty());
        assert_eq!(
            node24_limits.compatibility_target,
            RuntimeCompatibilityTarget::Node24
        );
    }

    #[test]
    fn tooling_preset_requires_node22_target() {
        let valid = RuntimeLimits::tooling_node22().normalized();
        assert_eq!(valid.mode, RuntimeMode::Standard);
        assert_eq!(valid.preset, RuntimePreset::Tooling);
        assert_eq!(
            valid.grants.run,
            vec![
                "$discovered_tooling".to_string(),
                "$runtime_self_exec".to_string(),
                "$runtime_host_exec".to_string(),
            ]
        );
        assert_eq!(
            valid.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        );

        let err = std::panic::catch_unwind(|| {
            RuntimeLimits {
                preset: RuntimePreset::Tooling,
                grants: RuntimeGrants::tooling(),
                compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
                ..RuntimeLimits::default()
            }
            .normalized()
        });
        assert!(err.is_err());
    }

    #[test]
    fn runtime_self_exec_run_grant_requires_node22_target() {
        let valid = RuntimeLimits {
            grants: RuntimeGrants {
                run: vec!["$runtime_self_exec".to_string()],
                ..RuntimeGrants::application_node()
            },
            ..RuntimeLimits::application_node22()
        }
        .normalized();
        assert_eq!(valid.grants.run, vec!["$runtime_self_exec".to_string()]);

        let err = std::panic::catch_unwind(|| {
            RuntimeLimits {
                grants: RuntimeGrants {
                    run: vec!["$runtime_self_exec".to_string()],
                    ..RuntimeGrants::application_node()
                },
                compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
                ..RuntimeLimits::default()
            }
            .normalized()
        });
        assert!(err.is_err());
    }

    #[test]
    fn runtime_modes_enforce_grant_ceilings() {
        let restricted = RuntimeLimits::restricted_code().normalized();
        assert_eq!(restricted.mode, RuntimeMode::Restricted);
        assert_eq!(restricted.language, RuntimeLanguage::JavaScript);
        assert_eq!(restricted.preset, RuntimePreset::Code);
        assert_eq!(restricted.grants, RuntimeGrants::restricted());

        let restricted_run = std::panic::catch_unwind(|| {
            RuntimeLimits {
                mode: RuntimeMode::Restricted,
                preset: RuntimePreset::Code,
                grants: RuntimeGrants {
                    run: vec!["node".to_string()],
                    ..RuntimeGrants::restricted()
                },
                ..RuntimeLimits::default()
            }
            .normalized()
        });
        assert!(restricted_run.is_err());

        let restricted_node_preset_rewrite = std::panic::catch_unwind(|| {
            RuntimeLimits {
                mode: RuntimeMode::Restricted,
                preset: RuntimePreset::Application,
                compatibility_target: RuntimeCompatibilityTarget::Node22,
                grants: RuntimeGrants::application_web_standard(),
                ..RuntimeLimits::default()
            }
            .normalized()
        });
        assert!(
            restricted_node_preset_rewrite.is_err(),
            "effective node grants must be checked against the final Restricted ceiling"
        );

        let standard_ffi = std::panic::catch_unwind(|| {
            RuntimeLimits {
                mode: RuntimeMode::Standard,
                grants: RuntimeGrants {
                    ffi: vec!["/usr/lib/libexample.dylib".to_string()],
                    ..RuntimeGrants::application_node()
                },
                ..RuntimeLimits::application_node22()
            }
            .normalized()
        });
        assert!(standard_ffi.is_err());

        let privileged = RuntimeLimits {
            grants: RuntimeGrants {
                ffi: vec!["/usr/lib/libexample.dylib".to_string()],
                ..RuntimeGrants::restricted()
            },
            ..RuntimeLimits::privileged_operator()
        }
        .normalized();
        assert_eq!(privileged.mode, RuntimeMode::Privileged);
        assert_eq!(privileged.preset, RuntimePreset::Operator);
        assert_eq!(privileged.grants.ffi, vec!["/usr/lib/libexample.dylib"]);
    }

    #[test]
    fn runtime_preset_and_execution_model_are_independent_axes() {
        let run_to_completion = RuntimeLimits {
            preset: RuntimePreset::Application,
            compatibility_target: RuntimeCompatibilityTarget::Node22,
            execution_model: RuntimeExecutionModel::RunToCompletion,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        }
        .normalized();
        assert_eq!(run_to_completion.preset, RuntimePreset::Application);
        assert_eq!(
            run_to_completion.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        );
        assert_eq!(
            run_to_completion.execution_model,
            RuntimeExecutionModel::RunToCompletion
        );

        let cooperative = RuntimeLimits {
            preset: RuntimePreset::Application,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        }
        .normalized();
        assert_eq!(cooperative.preset, RuntimePreset::Application);
        assert_eq!(
            cooperative.compatibility_target,
            RuntimeCompatibilityTarget::WebStandardIsolate
        );
        assert_eq!(
            cooperative.execution_model,
            RuntimeExecutionModel::CooperativeLocker
        );
    }
}
