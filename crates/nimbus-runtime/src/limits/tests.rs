use super::*;
use std::time::Duration;

#[test]
fn runtime_compatibility_target_parses_public_node_lts_aliases() {
    for (raw, expected) in [
        ("\"20\"", RuntimeCompatibilityTarget::Node20),
        ("\"node20\"", RuntimeCompatibilityTarget::Node20),
        ("\"Node20\"", RuntimeCompatibilityTarget::Node20),
        ("\"22\"", RuntimeCompatibilityTarget::Node22),
        ("\"node22\"", RuntimeCompatibilityTarget::Node22),
        ("\"Node22\"", RuntimeCompatibilityTarget::Node22),
        ("\"24\"", RuntimeCompatibilityTarget::Node24),
        ("\"node24\"", RuntimeCompatibilityTarget::Node24),
        ("\"Node24\"", RuntimeCompatibilityTarget::Node24),
    ] {
        let parsed: RuntimeCompatibilityTarget =
            serde_json::from_str(raw).expect("target alias should parse");
        assert_eq!(parsed, expected, "{raw} should parse to {expected:?}");
    }

    assert!(
        serde_json::from_str::<RuntimeCompatibilityTarget>("\"26\"").is_err(),
        "Node26 must not parse until the registry promotes a runtime target"
    );
}

#[test]
fn runtime_node_lts_metadata_is_derived_from_registry() {
    assert_eq!(
        RuntimeCompatibilityTarget::product_default_node_lts_target(),
        RuntimeCompatibilityTarget::Node22
    );
    assert_eq!(
        RuntimeCompatibilityTarget::configured_node_lts_targets(),
        vec![
            RuntimeCompatibilityTarget::Node20,
            RuntimeCompatibilityTarget::Node22,
            RuntimeCompatibilityTarget::Node24,
        ]
    );
    assert_eq!(
        RuntimeCompatibilityTarget::supported_node_lts_targets(),
        vec![
            RuntimeCompatibilityTarget::Node22,
            RuntimeCompatibilityTarget::Node24,
        ],
        "Node20 is EOL legacy and Node26 is preview-current without a runtime target"
    );

    for (target, major, phase, version, tag, codename, module_version, product_default) in [
        (
            RuntimeCompatibilityTarget::Node20,
            20,
            RuntimeNodeSupportPhase::EolLegacy,
            "20.20.2",
            "v20.20.2",
            Some("Iron"),
            "115",
            false,
        ),
        (
            RuntimeCompatibilityTarget::Node22,
            22,
            RuntimeNodeSupportPhase::MaintenanceLts,
            "22.22.3",
            "v22.22.3",
            Some("Jod"),
            "127",
            true,
        ),
        (
            RuntimeCompatibilityTarget::Node24,
            24,
            RuntimeNodeSupportPhase::ActiveLts,
            "24.16.0",
            "v24.16.0",
            Some("Krypton"),
            "137",
            false,
        ),
    ] {
        let metadata = target
            .node_lts_metadata()
            .expect("node target should have registry metadata");
        assert_eq!(target.node_major_version(), Some(major));
        assert_eq!(target.node_support_phase(), Some(phase));
        assert_eq!(target.node_runtime_version_number(), Some(version));
        assert_eq!(target.node_runtime_version(), Some(tag));
        assert_eq!(target.node_release_name(), Some("node"));
        assert_eq!(target.node_release_lts_codename(), codename);
        assert_eq!(target.node_module_version(), Some(module_version));
        assert_eq!(target.is_supported_node_lts(), phase.is_supported_lts());
        assert_eq!(metadata.product_default, product_default);
        assert_eq!(
            metadata.runtime_compatibility_target,
            Some(target),
            "registry target should round-trip for {target:?}"
        );
    }

    assert!(
        !RuntimeCompatibilityTarget::Node20.is_supported_node_lts(),
        "Node20 must not be treated as active enterprise LTS after EOL"
    );
    assert!(
        RuntimeCompatibilityTarget::WebStandardIsolate
            .node_lts_metadata()
            .is_none()
    );
    assert!(
        RuntimeCompatibilityTarget::BunJsc
            .node_lts_metadata()
            .is_none()
    );
}

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
    assert!(node20_limits.grants.net_connect.is_empty());
    assert!(node20_limits.grants.net_listen.is_empty());
    assert!(node20_limits.grants.worker.is_empty());
    assert!(!node20_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node20_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node20
    );

    let node_limits = RuntimeLimits::application_node22().normalized();
    assert_eq!(node_limits.mode, RuntimeMode::Standard);
    assert_eq!(node_limits.preset, RuntimePreset::Application);
    assert!(node_limits.grants.run.is_empty());
    assert!(node_limits.grants.net_connect.is_empty());
    assert!(node_limits.grants.net_listen.is_empty());
    assert!(node_limits.grants.worker.is_empty());
    assert!(!node_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node22
    );

    let node24_limits = RuntimeLimits::application_node24().normalized();
    assert_eq!(node24_limits.mode, RuntimeMode::Standard);
    assert_eq!(node24_limits.preset, RuntimePreset::Application);
    assert!(node24_limits.grants.run.is_empty());
    assert!(node24_limits.grants.net_connect.is_empty());
    assert!(node24_limits.grants.net_listen.is_empty());
    assert!(node24_limits.grants.worker.is_empty());
    assert!(!node24_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node24_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );
}

#[test]
fn node_permission_profiles_are_separated_by_deployment_intent() {
    let production = RuntimeLimits::application_node22().normalized();
    assert_eq!(
        production.grants,
        RuntimeGrants::application_node_production_in_process()
    );
    assert!(production.grants.net_connect.is_empty());
    assert!(production.grants.net_listen.is_empty());
    assert!(production.grants.worker.is_empty());
    assert!(production.grants.run.is_empty());
    assert!(production.grants.ffi.is_empty());
    assert!(!production.grants.sys.contains(&"inspector".to_string()));
    assert!(
        !production
            .grants
            .env_read
            .contains(&"NODE_TLS_REJECT_UNAUTHORIZED".to_string()),
        "production in-process Node must not inherit ambient TLS-disable env"
    );

    let local_dev = RuntimeLimits::application_node22_local_development().normalized();
    assert_eq!(
        local_dev.grants,
        RuntimeGrants::application_node_local_development()
    );
    assert!(
        local_dev
            .grants
            .net_connect
            .contains(&"localhost".to_string())
    );
    assert!(local_dev.grants.net_listen.contains(&"0.0.0.0".to_string()));
    assert!(local_dev.grants.worker.contains(&"thread".to_string()));
    assert!(local_dev.grants.sys.contains(&"inspector".to_string()));
    assert!(
        local_dev
            .grants
            .env_read
            .contains(&"NODE_TLS_REJECT_UNAUTHORIZED".to_string()),
        "local development keeps the compatibility env escape explicit"
    );

    let service = RuntimeLimits::application_node22_service_microvm().normalized();
    assert_eq!(
        service.grants,
        RuntimeGrants::application_node_service_microvm()
    );
    assert!(service.grants.net_listen.contains(&"[::]".to_string()));
    assert!(service.grants.worker.contains(&"thread".to_string()));
}

#[test]
fn tooling_preset_requires_node_target() {
    for valid in [
        RuntimeLimits::tooling_node20().normalized(),
        RuntimeLimits::tooling_node22().normalized(),
        RuntimeLimits::tooling_node24().normalized(),
    ] {
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
        assert!(valid.compatibility_target.is_node());
    }

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
fn runtime_self_exec_run_grant_requires_node_target() {
    let valid = RuntimeLimits {
        grants: RuntimeGrants {
            run: vec!["$runtime_self_exec".to_string()],
            ..RuntimeGrants::application_node()
        },
        ..RuntimeLimits::application_node24()
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

#[test]
fn runtime_policy_accepts_current_v8_javascript_axis_combinations() {
    for compatibility_target in [
        RuntimeCompatibilityTarget::WebStandardIsolate,
        RuntimeCompatibilityTarget::Node20,
        RuntimeCompatibilityTarget::Node22,
        RuntimeCompatibilityTarget::Node24,
    ] {
        let run_to_completion = RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            compatibility_target,
            execution_model: RuntimeExecutionModel::RunToCompletion,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        });
        assert_eq!(
            run_to_completion.limits().backend_kind,
            RuntimeBackendKind::V8
        );
        assert_eq!(
            run_to_completion.limits().backend_lifecycle_policy,
            RuntimeBackendLifecyclePolicy::V8DenoCorePool
        );
        assert_eq!(
            run_to_completion.limits().backend_trust_tier,
            RuntimeBackendTrustTier::InProcessUntrusted
        );
        assert_eq!(
            run_to_completion.limits().backend_lockdown_profile,
            RuntimeBackendLockdownProfile::V8DenoCore
        );
        assert_eq!(
            run_to_completion.limits().bundle_content_kind,
            RuntimeBundleContentKind::JavaScript
        );
        assert_eq!(
            run_to_completion.limits().javascript_evaluation_format,
            RuntimeJavaScriptEvaluationFormat::EsModule
        );

        let cooperative_snapshot = RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            compatibility_target,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        });
        assert_eq!(
            cooperative_snapshot.limits().execution_model,
            RuntimeExecutionModel::CooperativeLocker
        );
    }

    let cooperative_warm_pool = RuntimePolicy::new(RuntimeLimits {
        backend_kind: RuntimeBackendKind::V8,
        bundle_content_kind: RuntimeBundleContentKind::JavaScript,
        compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmPool,
        ..RuntimeLimits::default()
    });
    assert_eq!(
        cooperative_warm_pool.limits().runtime_pool_kind,
        RuntimePoolKind::WarmPool
    );
}

#[test]
fn runtime_policy_accepts_bun_jsc_only_with_proven_lockdown_profile() {
    let policy = RuntimePolicy::new(RuntimeLimits::application_bun_jsc());
    let limits = policy.limits();
    assert_eq!(limits.backend_kind, RuntimeBackendKind::BunJsc);
    assert_eq!(
        limits.backend_trust_tier,
        RuntimeBackendTrustTier::InProcessUntrusted
    );
    assert_eq!(
        limits.backend_lockdown_profile,
        RuntimeBackendLockdownProfile::BunJscInProcessUntrusted
    );
    assert_eq!(
        limits.backend_lifecycle_policy,
        RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired
    );
    assert_eq!(
        limits.javascript_evaluation_format,
        RuntimeJavaScriptEvaluationFormat::ProgramWrapper
    );
    assert_eq!(
        limits.compatibility_target,
        RuntimeCompatibilityTarget::BunJsc
    );
    assert_eq!(
        limits.execution_model,
        RuntimeExecutionModel::BackendOwnedEventLoop
    );
    assert_eq!(
        limits.runtime_pool_kind,
        RuntimePoolKind::BunJscFreshDiscard
    );
    assert_eq!(
        limits.memory_enforcement,
        RuntimeMemoryEnforcement::OuterQuotaRequired
    );
    assert!(limits.grants.run.is_empty());
    assert!(limits.grants.ffi.is_empty());
    assert!(limits.grants.net_connect.is_empty());
    assert!(limits.grants.net_listen.is_empty());
}

#[test]
fn runtime_pool_kind_exposes_engine_owned_diagnostics() {
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::StartupSnapshotCache).unwrap(),
        serde_json::json!("startup_snapshot_cache")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::WarmPool).unwrap(),
        serde_json::json!("warm_pool")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::BunJscTrustedRetained).unwrap(),
        serde_json::json!("bun_jsc_trusted_retained")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::BunJscFreshDiscard).unwrap(),
        serde_json::json!("bun_jsc_fresh_discard")
    );
    assert_eq!(
        serde_json::to_value(RuntimeMemoryEnforcement::V8IsolateHeapLimit).unwrap(),
        serde_json::json!("v8_isolate_heap_limit")
    );
    assert_eq!(
        serde_json::to_value(RuntimeMemoryEnforcement::OuterQuotaRequired).unwrap(),
        serde_json::json!("outer_quota_required")
    );

    let bun_trusted_retained = RuntimeLimits {
        runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
        ..RuntimeLimits::default()
    };
    assert_eq!(
        bun_trusted_retained.module_state_semantics(),
        RuntimeModuleStateSemantics::WarmPerBundle
    );
    assert_eq!(
        bun_trusted_retained.reset_capabilities(),
        RuntimeResetCapabilities {
            op_state_per_invocation: true,
            bootstrap_state_per_invocation: true,
            user_module_state_per_invocation: false,
        }
    );

    let bun_fresh_discard = RuntimeLimits {
        runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
        ..RuntimeLimits::default()
    };
    assert_eq!(
        bun_fresh_discard.module_state_semantics(),
        RuntimeModuleStateSemantics::FreshPerInvocation
    );
    assert_eq!(
        bun_fresh_discard.reset_capabilities(),
        RuntimeResetCapabilities {
            op_state_per_invocation: true,
            bootstrap_state_per_invocation: true,
            user_module_state_per_invocation: true,
        }
    );
}

#[test]
fn runtime_limits_expose_tenant_budget_from_normalized_limits() {
    let mut limits = RuntimeLimits::application_web_standard();
    limits.max_concurrent_runtime_instances = 8;
    limits.worker_threads = 16;
    limits.max_active_top_level_invocations_per_tenant = 3;
    limits.max_in_flight_top_level_invocations_per_tenant = 5;
    limits.max_queued_top_level_invocations_per_tenant = 7;
    limits.max_heap_mb = 256;
    limits.execution_timeout = Duration::from_secs(9);
    limits.max_nested_runtime_invocations = 11;

    let budget = limits.tenant_budget();

    assert_eq!(budget.max_active_runtime_slots, 3);
    assert_eq!(budget.max_in_flight_top_level_invocations, 5);
    assert_eq!(budget.max_queued_top_level_invocations, 7);
    assert_eq!(budget.max_worker_thread_slots, 3);
    assert_eq!(budget.max_heap_mb_per_runtime, 256);
    assert_eq!(
        budget.memory_enforcement,
        RuntimeMemoryEnforcement::V8IsolateHeapLimit
    );
    assert_eq!(budget.max_active_heap_mb, 768);
    assert_eq!(budget.execution_timeout, Duration::from_secs(9));
    assert_eq!(budget.max_nested_runtime_invocations_per_top_level, 11);
    assert_eq!(RuntimePolicy::new(limits).tenant_budget(), budget);
}

#[test]
fn runtime_policy_rejects_unsupported_engine_axis_combinations() {
    let run_to_completion_warm_pool = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::RunToCompletion,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        run_to_completion_warm_pool.is_err(),
        "WarmPool must not be accepted with RunToCompletion"
    );

    let wasm_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::WasmComponent,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        wasm_on_v8.is_err(),
        "V8 must reject non-JavaScript bundle content"
    );

    let program_wrapper_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        program_wrapper_on_v8.is_err(),
        "V8 must reject Bun/JSC program-wrapper evaluation"
    );

    let bun_lifecycle_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_lifecycle_on_v8.is_err(),
        "V8 must reject Bun/JSC lifecycle policies"
    );

    for runtime_pool_kind in [
        RuntimePoolKind::BunJscTrustedRetained,
        RuntimePoolKind::BunJscFreshDiscard,
    ] {
        let bun_pool_on_v8 = std::panic::catch_unwind(|| {
            RuntimePolicy::new(RuntimeLimits {
                backend_kind: RuntimeBackendKind::V8,
                runtime_pool_kind,
                ..RuntimeLimits::default()
            })
        });
        assert!(
            bun_pool_on_v8.is_err(),
            "V8 must reject Bun/JSC pool kind {runtime_pool_kind:?}"
        );
    }

    let bun_target_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_target_on_v8.is_err(),
        "V8 must reject Bun/JSC compatibility target"
    );

    let outer_quota_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            memory_enforcement: RuntimeMemoryEnforcement::OuterQuotaRequired,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        outer_quota_on_v8.is_err(),
        "V8 must reject outer-quota-only memory enforcement"
    );

    let bun_jsc_without_matching_profile = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_without_matching_profile.is_err(),
        "Bun/JSC must require an explicit matching lockdown profile"
    );

    let bun_jsc_proof_only = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::ProofOnly,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscProofOnly,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_proof_only.is_err(),
        "Bun/JSC proof-only profile must remain non-selectable"
    );

    let bun_jsc_trusted_only = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessTrustedOnly,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_trusted_only.is_err(),
        "Bun/JSC trusted-only generated-wrapper profile must not create a product route"
    );

    let bun_jsc_untrusted_wrong_target = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessUntrusted,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            backend_lifecycle_policy:
                RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::Node22,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_untrusted_wrong_target.is_err(),
        "Bun/JSC in-process-untrusted profile must not be labeled as a Node target"
    );

    let bun_jsc_untrusted_without_outer_quota_memory = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            memory_enforcement: RuntimeMemoryEnforcement::V8IsolateHeapLimit,
            ..RuntimeLimits::application_bun_jsc()
        })
    });
    assert!(
        bun_jsc_untrusted_without_outer_quota_memory.is_err(),
        "Bun/JSC in-process-untrusted profile must require outer-quota memory enforcement"
    );

    let bun_jsc_untrusted_missing_outer_quota_lifecycle = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessUntrusted,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_untrusted_missing_outer_quota_lifecycle.is_err(),
        "Bun/JSC in-process-untrusted profile must require the fresh-VM outer-quota lifecycle policy"
    );

    let bun_jsc_trusted_profile_with_v8_pool = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessTrustedOnly,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_trusted_profile_with_v8_pool.is_err(),
        "Bun/JSC must reject V8/Deno pool kinds"
    );
}

#[test]
fn runtime_policy_rejects_bundle_content_kind_mismatches() {
    let policy = RuntimePolicy::new(RuntimeLimits::default());
    assert!(
        policy
            .validate_bundle_content_kind(RuntimeBundleContentKind::JavaScript)
            .is_ok()
    );

    let error = policy
        .validate_bundle_content_kind(RuntimeBundleContentKind::WasmComponent)
        .expect_err("V8 JavaScript policy should reject Wasm content");
    assert!(
        error
            .to_string()
            .contains("runtime bundle content kind WasmComponent does not match"),
        "unexpected content-kind mismatch error: {error}"
    );
}
