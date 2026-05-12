use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::node_compat::{
    NodeCompatBatchEntrySnapshot, NodeCompatNamedPostludeBehavior, NodeCompatNamedPreludeBehavior,
    core_semantics_batch_snapshot, default_postlude_behavior_for_fixture,
    default_prelude_behavior_for_fixture, fixture_requests_pending_deprecation,
    loader_context_batch_snapshot, loader_context_supplementary_batch_snapshot,
    loader_context_supplementary_global_injection_batch_snapshot,
    loader_context_supplementary_module_bridge_batch_snapshot, networking_batch_snapshot,
    process_and_timing_batch_snapshot, process_and_timing_supplementary_batch_snapshot,
    runtime_supplementary_batch_snapshot, runtime_supplementary_signal_lifecycle_batch_snapshot,
    streams_and_local_io_batch_snapshot,
};
use super::node_compat_manifest_catalog::{
    NodeCompatCapability, NodeCompatExecutionClass, NodeCompatFamilyCatalog,
    NodeCompatFixtureSeedEntry, NodeCompatNamedBehaviorCatalog, NodeCompatPreset,
    NodeCompatSupplementaryCategory, NodeCompatTestTier, load_family_catalogs_from_disk,
    read_sorted_manifest_file_names, repo_root, validate_family_catalogs,
    validate_named_behavior_catalog,
};

const SCHEMA_JSON: &str = include_str!("../node_compat_manifests/schema.json");
const CORE_SEMANTICS_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/core-semantics.json");
const PROCESS_AND_TIMING_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/process-and-timing.json");
const STREAMS_AND_LOCAL_IO_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/streams-and-local-io.json");
const NETWORKING_JSON: &str = include_str!("../node_compat_manifests/fixtures/networking.json");
const LOADER_CONTEXT_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/loader-context.json");
const LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_JSON: &str = include_str!(
    "../node_compat_manifests/fixtures/loader-context-supplementary-global-injection.json"
);
const LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_JSON: &str = include_str!(
    "../node_compat_manifests/fixtures/loader-context-supplementary-module-bridge.json"
);
const LOADER_CONTEXT_SUPPLEMENTARY_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/loader-context-supplementary.json");
const PROCESS_AND_TIMING_SUPPLEMENTARY_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/process-and-timing-supplementary.json");
const RUNTIME_SUPPLEMENTARY_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/runtime-supplementary.json");
const RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/runtime-supplementary-signal-lifecycle.json");
const NODE_COMPAT_RS: &str = include_str!("mod.rs");
const PENDING_DEPRECATION_FIXTURE: &str =
    include_str!("../node_compat_fixtures/node20/test/parallel/test-buffer-pending-deprecation.js");

#[test]
fn node_compat_manifest_topology_schema_documents_family_and_named_behavior_catalogs() {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).expect("schema should parse as JSON");

    let family_catalog = &schema["$defs"]["familyCatalog"];
    let family_required = family_catalog["required"]
        .as_array()
        .expect("familyCatalog.required should be an array");
    let family_properties = family_catalog["properties"]
        .as_object()
        .expect("familyCatalog.properties should be an object");
    for field in [
        "schema_version",
        "family",
        "nlc_item",
        "batch_constant",
        "execution_class",
        "presets",
        "capabilities",
        "lane_batches",
        "manifest_doc",
        "failure_doc",
    ] {
        assert!(
            family_required
                .iter()
                .any(|entry| entry.as_str() == Some(field)),
            "familyCatalog should require {field}",
        );
        assert!(
            family_properties.contains_key(field),
            "familyCatalog should document property {field}",
        );
    }
    assert!(
        family_properties.contains_key("fixture_seeds"),
        "familyCatalog should document optional property fixture_seeds",
    );
    let fixture_seed_properties = schema["$defs"]["fixtureSeedEntry"]["properties"]
        .as_object()
        .expect("fixture seed entry properties should be an object");
    assert!(
        fixture_seed_properties.contains_key("test_tier"),
        "fixture seed entry should document test_tier",
    );
    assert!(
        fixture_seed_properties.contains_key("supplementary_category"),
        "fixture seed entry should document supplementary_category",
    );

    let fixture_seed_lane_sources = &schema["$defs"]["fixtureSeedLaneSources"];
    let pattern_properties = fixture_seed_lane_sources["patternProperties"]
        .as_object()
        .expect("fixtureSeedLaneSources.patternProperties should be an object");
    assert!(
        pattern_properties.contains_key("^node[0-9]+$"),
        "fixtureSeedLaneSources should allow future node lane keys by pattern",
    );
    assert_eq!(
        fixture_seed_lane_sources["minProperties"].as_u64(),
        Some(1),
        "fixtureSeedLaneSources should require at least one lane source",
    );

    let named_behavior_catalog = &schema["$defs"]["namedBehaviorCatalog"];
    let named_behavior_required = named_behavior_catalog["required"]
        .as_array()
        .expect("namedBehaviorCatalog.required should be an array");
    let named_behavior_properties = named_behavior_catalog["properties"]
        .as_object()
        .expect("namedBehaviorCatalog.properties should be an object");
    for field in ["schema_version", "named_behaviors"] {
        assert!(
            named_behavior_required
                .iter()
                .any(|entry| entry.as_str() == Some(field)),
            "namedBehaviorCatalog should require {field}",
        );
        assert!(
            named_behavior_properties.contains_key(field),
            "namedBehaviorCatalog should document property {field}",
        );
    }
}

#[test]
fn node_compat_family_catalog_files_parse_and_point_at_real_docs_and_batches() {
    let repo_root = repo_root();
    let cases = [
        (
            CORE_SEMANTICS_JSON,
            "core-semantics",
            "NLC3",
            "CORE_SEMANTICS_BATCH",
            "docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md",
            "docs/architecture/runtime/node-lts-compat/failures/core-semantics.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node20_supported_lane_executes_official_core_semantics_subset",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node24_supported_lane_core_semantics_watchpoint",
                ),
            ],
        ),
        (
            PROCESS_AND_TIMING_JSON,
            "process-and-timing",
            "NLC4",
            "PROCESS_AND_TIMING_BATCH",
            "docs/architecture/runtime/node-lts-compat/manifests/process-and-timing.md",
            "docs/architecture/runtime/node-lts-compat/failures/process-and-timing.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node20_supported_lane_executes_official_process_and_timing_subset",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node22_default_lane_executes_manifested_process_and_timing_subset",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node24_supported_lane_process_and_timing_watchpoint",
                ),
            ],
        ),
        (
            STREAMS_AND_LOCAL_IO_JSON,
            "streams-and-local-io",
            "NLC5",
            "STREAMS_AND_LOCAL_IO_BATCH",
            "docs/architecture/runtime/node-lts-compat/manifests/streams-and-local-io.md",
            "docs/architecture/runtime/node-lts-compat/failures/streams-and-local-io.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node20_supported_lane_executes_official_streams_and_local_io_subset",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node22_default_lane_executes_manifested_streams_and_local_io_subset",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node24_supported_lane_streams_and_local_io_watchpoint",
                ),
            ],
        ),
        (
            NETWORKING_JSON,
            "networking",
            "NLC6",
            "NETWORKING_BATCH",
            "docs/architecture/runtime/node-lts-compat/manifests/networking.md",
            "docs/architecture/runtime/node-lts-compat/failures/networking.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node20_supported_lane_executes_official_networking_subset",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node22_default_lane_executes_manifested_networking_subset",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node24_supported_lane_networking_watchpoint",
                ),
            ],
        ),
        (
            LOADER_CONTEXT_JSON,
            "loader-context",
            "NLC7",
            "LOADER_CONTEXT_BATCH",
            "docs/architecture/runtime/node-lts-compat/manifests/loader-context.md",
            "docs/architecture/runtime/node-lts-compat/failures/loader-context.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node20_supported_lane_executes_official_loader_context_subset",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node22_default_lane_executes_manifested_loader_context_subset",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node24_supported_lane_loader_context_watchpoint",
                ),
            ],
        ),
        (
            LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_JSON,
            "loader-context-supplementary-global-injection",
            "NCF3",
            "LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH",
            "docs/architecture/runtime/node-compat-supplementary.md",
            "docs/architecture/runtime/node-compat-supplementary-failures.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node_compat_supplementary_global_injection_node20",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node_compat_supplementary_global_injection_node22",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node_compat_supplementary_global_injection_node24",
                ),
            ],
        ),
        (
            LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_JSON,
            "loader-context-supplementary-module-bridge",
            "NCF3",
            "LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH",
            "docs/architecture/runtime/node-compat-supplementary.md",
            "docs/architecture/runtime/node-compat-supplementary-failures.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node_compat_supplementary_module_bridge_node20",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node_compat_supplementary_module_bridge_node22",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node_compat_supplementary_module_bridge_node24",
                ),
            ],
        ),
        (
            LOADER_CONTEXT_SUPPLEMENTARY_JSON,
            "loader-context-supplementary",
            "NCF3",
            "LOADER_CONTEXT_SUPPLEMENTARY_BATCH",
            "docs/architecture/runtime/node-compat-supplementary.md",
            "docs/architecture/runtime/node-compat-supplementary-failures.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node_compat_supplementary_builtin_completeness_node20",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node_compat_supplementary_builtin_completeness_node22",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node_compat_supplementary_builtin_completeness_node24",
                ),
            ],
        ),
        (
            PROCESS_AND_TIMING_SUPPLEMENTARY_JSON,
            "process-and-timing-supplementary",
            "NCF3",
            "PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH",
            "docs/architecture/runtime/node-compat-supplementary.md",
            "docs/architecture/runtime/node-compat-supplementary-failures.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node_compat_supplementary_process_shape_node20",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node_compat_supplementary_process_shape_node22",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node_compat_supplementary_process_shape_node24",
                ),
            ],
        ),
        (
            RUNTIME_SUPPLEMENTARY_JSON,
            "runtime-supplementary",
            "NCF3",
            "RUNTIME_SUPPLEMENTARY_BATCH",
            "docs/architecture/runtime/node-compat-supplementary.md",
            "docs/architecture/runtime/node-compat-supplementary-failures.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node_compat_supplementary_runtime_node20",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node_compat_supplementary_runtime_node22",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node_compat_supplementary_runtime_node24",
                ),
            ],
        ),
        (
            RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_JSON,
            "runtime-supplementary-signal-lifecycle",
            "NCF3",
            "RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH",
            "docs/architecture/runtime/node-compat-supplementary.md",
            "docs/architecture/runtime/node-compat-supplementary-failures.md",
            [
                (
                    "node20",
                    "runtime::tests::node_compat::node_compat_supplementary_signal_lifecycle_watchpoint_node20",
                ),
                (
                    "node22",
                    "runtime::tests::node_compat::node_compat_supplementary_signal_lifecycle_watchpoint_node22",
                ),
                (
                    "node24",
                    "runtime::tests::node_compat::node_compat_supplementary_signal_lifecycle_watchpoint_node24",
                ),
            ],
        ),
    ];

    let mut seen_families = BTreeSet::new();
    for (
        json,
        expected_family,
        expected_nlc_item,
        expected_batch_constant,
        expected_manifest_doc,
        expected_failure_doc,
        expected_lane_batches,
    ) in cases
    {
        let catalog: NodeCompatFamilyCatalog =
            serde_json::from_str(json).expect("family catalog should parse");
        assert_eq!(catalog.schema_version, 1);
        assert_eq!(catalog.family, expected_family);
        assert_eq!(catalog.nlc_item, expected_nlc_item);
        assert_eq!(catalog.batch_constant, expected_batch_constant);
        let expected_execution_class = if matches!(
            expected_family,
            "process-and-timing-supplementary" | "runtime-supplementary-signal-lifecycle"
        ) {
            NodeCompatExecutionClass::ExpectedFailure
        } else {
            NodeCompatExecutionClass::Sequential
        };
        assert_eq!(catalog.execution_class, expected_execution_class);
        assert_eq!(catalog.presets, vec![NodeCompatPreset::Application]);
        if expected_family == "networking" {
            assert_eq!(
                catalog.capabilities,
                vec![
                    NodeCompatCapability::BundleRootFs,
                    NodeCompatCapability::LoopbackNet,
                ]
            );
        } else {
            assert_eq!(
                catalog.capabilities,
                vec![NodeCompatCapability::BundleRootFs]
            );
        }
        assert_eq!(catalog.manifest_doc, expected_manifest_doc);
        assert_eq!(catalog.failure_doc, expected_failure_doc);
        assert!(
            seen_families.insert(catalog.family.clone()),
            "family should only appear once: {}",
            catalog.family,
        );

        let manifest_doc = repo_root.join(&catalog.manifest_doc);
        let failure_doc = repo_root.join(&catalog.failure_doc);
        assert!(
            manifest_doc.is_file(),
            "family manifest doc should exist: {}",
            manifest_doc.display(),
        );
        assert!(
            failure_doc.is_file(),
            "family failure doc should exist: {}",
            failure_doc.display(),
        );
        assert!(
            NODE_COMPAT_RS.contains(&catalog.batch_constant),
            "node_compat.rs should still carry batch constant {}",
            catalog.batch_constant,
        );

        assert_eq!(catalog.lane_batches.len(), 3);
        let actual_lane_set: BTreeSet<&str> = catalog
            .lane_batches
            .iter()
            .map(|entry| entry.lane.as_str())
            .collect();
        assert_eq!(
            actual_lane_set,
            BTreeSet::from(["node20", "node22", "node24"])
        );

        for (expected_lane, expected_subset_test) in expected_lane_batches {
            let batch = catalog
                .lane_batches
                .iter()
                .find(|entry| entry.lane == expected_lane)
                .unwrap_or_else(|| panic!("missing lane batch for {expected_lane}"));
            assert_eq!(batch.subset_test, expected_subset_test);
            let function_name = batch
                .subset_test
                .rsplit("::")
                .next()
                .expect("subset test should end with a function name");
            assert!(
                NODE_COMPAT_RS.contains(function_name),
                "node_compat.rs should still carry subset test function {}",
                function_name,
            );
        }
    }

    assert_eq!(
        seen_families,
        BTreeSet::from([
            "core-semantics".to_string(),
            "process-and-timing".to_string(),
            "streams-and-local-io".to_string(),
            "networking".to_string(),
            "loader-context".to_string(),
            "loader-context-supplementary-global-injection".to_string(),
            "loader-context-supplementary-module-bridge".to_string(),
            "loader-context-supplementary".to_string(),
            "process-and-timing-supplementary".to_string(),
            "runtime-supplementary".to_string(),
            "runtime-supplementary-signal-lifecycle".to_string(),
        ]),
    );
}

#[test]
fn node_compat_named_preludes_catalog_matches_default_behavior_registry() {
    let resolved = load_family_catalogs_from_disk();
    let catalog: &NodeCompatNamedBehaviorCatalog = &resolved.named_behavior_catalog;
    assert_eq!(catalog.schema_version, 1);
    validate_named_behavior_catalog(catalog).expect("named behavior catalog should validate");

    let actual_ids: BTreeSet<&str> = catalog
        .named_behaviors
        .iter()
        .map(|behavior| behavior.id.as_str())
        .collect();
    let expected_ids: BTreeSet<&str> = NodeCompatNamedPreludeBehavior::ALL
        .into_iter()
        .map(NodeCompatNamedPreludeBehavior::id)
        .chain(
            NodeCompatNamedPostludeBehavior::ALL
                .into_iter()
                .map(NodeCompatNamedPostludeBehavior::id),
        )
        .collect();
    assert_eq!(actual_ids, expected_ids);

    for behavior in NodeCompatNamedPreludeBehavior::ALL {
        let metadata = catalog
            .named_behaviors
            .iter()
            .find(|entry| entry.id == behavior.id())
            .unwrap_or_else(|| panic!("missing prelude behavior {}", behavior.id()));
        assert_eq!(metadata.phase, behavior.phase());
        assert_eq!(metadata.selection_mode, behavior.selection_mode());
    }
    for behavior in NodeCompatNamedPostludeBehavior::ALL {
        let metadata = catalog
            .named_behaviors
            .iter()
            .find(|entry| entry.id == behavior.id())
            .unwrap_or_else(|| panic!("missing postlude behavior {}", behavior.id()));
        assert_eq!(metadata.phase, behavior.phase());
        assert_eq!(metadata.selection_mode, behavior.selection_mode());
    }

    let prelude_cases = [
        (
            "test/parallel/test-http2-compat-write-early-hints-invalid-argument-type.js",
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel),
        ),
        (
            "test/parallel/test-http2-compat-write-early-hints-invalid-argument-value.js",
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel),
        ),
        (
            "test/parallel/test-cluster-worker-events.js",
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel),
        ),
        (
            "test/parallel/test-cluster-worker-exit.js",
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel),
        ),
        (
            "test/parallel/test-inspector-open.js",
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel),
        ),
        (
            "test/parallel/test-inspector-enabled.js",
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel),
        ),
        (
            "test/parallel/test-readline-interface.js",
            Some(NodeCompatNamedPreludeBehavior::InteractiveTerminal),
        ),
        (
            "test/parallel/test-readline-promises-interface.js",
            Some(NodeCompatNamedPreludeBehavior::InteractiveTerminal),
        ),
        (
            "test/parallel/test-dns-default-order-ipv4.js",
            Some(NodeCompatNamedPreludeBehavior::DnsResultOrderIpv4First),
        ),
        (
            "test/parallel/test-dns-default-order-ipv6.js",
            Some(NodeCompatNamedPreludeBehavior::DnsResultOrderIpv6First),
        ),
        (
            "test/parallel/test-dns-default-order-verbatim.js",
            Some(NodeCompatNamedPreludeBehavior::DnsResultOrderVerbatim),
        ),
        (
            "test/parallel/test-zlib-invalid-input-memory.js",
            Some(NodeCompatNamedPreludeBehavior::ExposeGc),
        ),
        (
            "test/parallel/test-zlib-unused-weak.js",
            Some(NodeCompatNamedPreludeBehavior::ExposeGc),
        ),
        ("test/parallel/test-buffer-equals.js", None),
    ];
    for (fixture, expected_behavior) in prelude_cases {
        assert_eq!(
            default_prelude_behavior_for_fixture(fixture),
            expected_behavior
        );
    }

    let postlude_cases = [
        (
            "test/parallel/test-fs-open-no-close.js",
            Some(NodeCompatNamedPostludeBehavior::ProcessLifecycleDrain),
        ),
        (
            "test/parallel/test-fs-writefile-with-fd.js",
            Some(NodeCompatNamedPostludeBehavior::ProcessLifecycleDrain),
        ),
        (
            "test/parallel/test-trace-events-api.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-cluster-worker-init.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-cluster-worker-isdead.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-cluster-worker-isconnected.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-cluster-worker-disconnect.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-cluster-worker-forced-exit.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-cluster-worker-kill.js",
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle),
        ),
        (
            "test/parallel/test-worker-ref.js",
            Some(NodeCompatNamedPostludeBehavior::ProcessBeforeExitReentry),
        ),
        ("test/parallel/test-buffer-equals.js", None),
    ];
    for (fixture, expected_behavior) in postlude_cases {
        assert_eq!(
            default_postlude_behavior_for_fixture(fixture),
            expected_behavior
        );
    }

    assert!(fixture_requests_pending_deprecation(
        PENDING_DEPRECATION_FIXTURE
    ));
    assert!(!fixture_requests_pending_deprecation("/* no flags */"));
}

#[test]
fn node_compat_manifest_directory_layout_is_deterministic() {
    let root_files = read_sorted_manifest_file_names(
        "crates/neovex-runtime/src/runtime/tests/node_compat_manifests",
    );
    let lane_files = read_sorted_manifest_file_names(
        "crates/neovex-runtime/src/runtime/tests/node_compat_manifests/lanes",
    );
    let fixture_files = read_sorted_manifest_file_names(
        "crates/neovex-runtime/src/runtime/tests/node_compat_manifests/fixtures",
    );

    assert_eq!(
        root_files,
        vec!["fixtures", "lanes", "preludes.json", "schema.json"]
    );
    assert_eq!(
        lane_files,
        vec!["node20.json", "node22.json", "node24.json"]
    );
    assert_eq!(
        fixture_files,
        vec![
            "core-semantics.json",
            "loader-context-supplementary-global-injection.json",
            "loader-context-supplementary-module-bridge.json",
            "loader-context-supplementary.json",
            "loader-context.json",
            "networking.json",
            "process-and-timing-supplementary.json",
            "process-and-timing.json",
            "runtime-supplementary-signal-lifecycle.json",
            "runtime-supplementary.json",
            "streams-and-local-io.json"
        ]
    );
}

#[test]
fn node_compat_manifest_topology_loader_composes_deterministically_from_disk() {
    let resolved = load_family_catalogs_from_disk();
    assert_eq!(
        resolved.lane_files,
        vec!["node20.json", "node22.json", "node24.json"]
    );
    assert_eq!(
        resolved.family_files,
        vec![
            "core-semantics.json",
            "loader-context-supplementary-global-injection.json",
            "loader-context-supplementary-module-bridge.json",
            "loader-context-supplementary.json",
            "loader-context.json",
            "networking.json",
            "process-and-timing-supplementary.json",
            "process-and-timing.json",
            "runtime-supplementary-signal-lifecycle.json",
            "runtime-supplementary.json",
            "streams-and-local-io.json"
        ]
    );
    let family_map: BTreeMap<&str, &NodeCompatFamilyCatalog> = resolved
        .family_catalogs
        .iter()
        .map(|catalog| (catalog.family.as_str(), catalog))
        .collect();
    assert_eq!(family_map.len(), 11);
    assert_eq!(family_map["core-semantics"].fixture_seeds.len(), 10);
    assert_eq!(family_map["process-and-timing"].fixture_seeds.len(), 10);
    assert_eq!(
        family_map["process-and-timing-supplementary"]
            .fixture_seeds
            .len(),
        1
    );
    assert_eq!(family_map["streams-and-local-io"].fixture_seeds.len(), 10);
    assert_eq!(family_map["networking"].fixture_seeds.len(), 10);
    assert_eq!(family_map["loader-context"].fixture_seeds.len(), 10);
    assert_eq!(
        family_map["loader-context-supplementary-global-injection"]
            .fixture_seeds
            .len(),
        1
    );
    assert_eq!(
        family_map["loader-context-supplementary-module-bridge"]
            .fixture_seeds
            .len(),
        1
    );
    assert_eq!(
        family_map["loader-context-supplementary"]
            .fixture_seeds
            .len(),
        1
    );
    assert_eq!(family_map["runtime-supplementary"].fixture_seeds.len(), 2);
    assert_eq!(
        family_map["runtime-supplementary-signal-lifecycle"]
            .fixture_seeds
            .len(),
        1
    );
    assert_eq!(
        family_map["networking"].capabilities,
        vec![
            NodeCompatCapability::BundleRootFs,
            NodeCompatCapability::LoopbackNet,
        ]
    );
    assert_eq!(
        family_map["loader-context"].execution_class,
        NodeCompatExecutionClass::Sequential
    );
    assert_eq!(
        resolved.named_behavior_catalog.named_behaviors.len(),
        NodeCompatNamedPreludeBehavior::ALL.len() + NodeCompatNamedPostludeBehavior::ALL.len()
    );
    validate_family_catalogs(&resolved.family_catalogs)
        .expect("family catalogs loaded from disk should validate");
    validate_named_behavior_catalog(&resolved.named_behavior_catalog)
        .expect("named behavior catalog loaded from disk should validate");
}

#[test]
fn node_compat_core_semantics_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let core_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "core-semantics")
        .expect("core-semantics catalog should load");
    assert_eq!(core_catalog.fixture_seeds.len(), 10);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        core_semantics_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    for fixture in &core_catalog.fixture_seeds {
        let batch_entry = batch_snapshot
            .get(fixture.test_relative_path.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "fixture seed {} should exist in core batch snapshot",
                    fixture.test_relative_path
                )
            });
        assert_eq!(fixture.id, fixture.test_relative_path);
        assert_eq!(fixture.slice, "assert-and-buffer-foundation");
        assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
        assert!(fixture.named_preludes.is_empty());
        assert!(fixture.named_postludes.is_empty());
    }
}

#[test]
fn node_compat_process_and_timing_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let process_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "process-and-timing")
        .expect("process-and-timing catalog should load");
    assert_eq!(process_catalog.fixture_seeds.len(), 10);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        process_and_timing_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    for fixture in &process_catalog.fixture_seeds {
        let batch_entry = batch_snapshot
            .get(fixture.test_relative_path.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "fixture seed {} should exist in process-and-timing batch snapshot",
                    fixture.test_relative_path
                )
            });
        assert_eq!(fixture.id, fixture.test_relative_path);
        assert_eq!(fixture.slice, "process-foundation");
        assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
        assert!(fixture.named_preludes.is_empty());
        assert!(fixture.named_postludes.is_empty());
    }
}

#[test]
fn node_compat_streams_and_local_io_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let streams_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "streams-and-local-io")
        .expect("streams-and-local-io catalog should load");
    assert_eq!(streams_catalog.fixture_seeds.len(), 10);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        streams_and_local_io_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    for fixture in &streams_catalog.fixture_seeds {
        let batch_entry = batch_snapshot
            .get(fixture.test_relative_path.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "fixture seed {} should exist in streams-and-local-io batch snapshot",
                    fixture.test_relative_path
                )
            });
        assert_eq!(fixture.id, fixture.test_relative_path);
        assert_eq!(fixture.slice, "os-tty-readline-foundation");
        assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
        assert!(fixture.named_preludes.is_empty());
        assert!(fixture.named_postludes.is_empty());
    }
}

#[test]
fn node_compat_networking_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let networking_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "networking")
        .expect("networking catalog should load");
    assert_eq!(networking_catalog.fixture_seeds.len(), 10);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> = networking_batch_snapshot()
        .into_iter()
        .map(|entry| (entry.test_relative_path, entry))
        .collect();

    for fixture in &networking_catalog.fixture_seeds {
        let batch_entry = batch_snapshot
            .get(fixture.test_relative_path.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "fixture seed {} should exist in networking batch snapshot",
                    fixture.test_relative_path
                )
            });
        assert_eq!(fixture.id, fixture.test_relative_path);
        assert_eq!(fixture.slice, "dns-net-foundation");
        assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
        assert!(fixture.named_preludes.is_empty());
        assert!(fixture.named_postludes.is_empty());
    }
}

#[test]
fn node_compat_loader_context_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let loader_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "loader-context")
        .expect("loader-context catalog should load");
    assert_eq!(loader_catalog.fixture_seeds.len(), 10);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        loader_context_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    for fixture in &loader_catalog.fixture_seeds {
        let batch_entry = batch_snapshot
            .get(fixture.test_relative_path.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "fixture seed {} should exist in loader-context batch snapshot",
                    fixture.test_relative_path
                )
            });
        assert_eq!(fixture.id, fixture.test_relative_path);
        assert_eq!(fixture.slice, "module-and-async-foundation");
        assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
        assert!(fixture.named_preludes.is_empty());
        assert!(fixture.named_postludes.is_empty());
    }
}

#[test]
fn node_compat_loader_context_supplementary_global_injection_fixture_seed_entries_align_with_batch_definitions()
 {
    let resolved = load_family_catalogs_from_disk();
    let loader_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "loader-context-supplementary-global-injection")
        .expect("loader-context-supplementary-global-injection catalog should load");
    assert_eq!(loader_catalog.fixture_seeds.len(), 1);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        loader_context_supplementary_global_injection_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    let fixture = &loader_catalog.fixture_seeds[0];
    let batch_entry = batch_snapshot
        .get(fixture.test_relative_path.as_str())
        .expect("supplementary global injection seed should exist in batch snapshot");
    assert_eq!(fixture.id, fixture.test_relative_path);
    assert_eq!(fixture.slice, "supplementary-global-injection-fidelity");
    assert_eq!(fixture.test_tier, NodeCompatTestTier::Supplementary);
    assert_eq!(
        fixture.supplementary_category,
        Some(NodeCompatSupplementaryCategory::GlobalInjectionFidelity)
    );
    assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
}

#[test]
fn node_compat_loader_context_supplementary_module_bridge_fixture_seed_entries_align_with_batch_definitions()
 {
    let resolved = load_family_catalogs_from_disk();
    let loader_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "loader-context-supplementary-module-bridge")
        .expect("loader-context-supplementary-module-bridge catalog should load");
    assert_eq!(loader_catalog.fixture_seeds.len(), 1);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        loader_context_supplementary_module_bridge_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    let fixture = &loader_catalog.fixture_seeds[0];
    let batch_entry = batch_snapshot
        .get(fixture.test_relative_path.as_str())
        .expect("supplementary module bridge seed should exist in batch snapshot");
    assert_eq!(fixture.id, fixture.test_relative_path);
    assert_eq!(fixture.slice, "supplementary-module-resolution-bridge");
    assert_eq!(fixture.test_tier, NodeCompatTestTier::Supplementary);
    assert_eq!(
        fixture.supplementary_category,
        Some(NodeCompatSupplementaryCategory::ModuleResolutionBridge)
    );
    assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
}

#[test]
fn node_compat_loader_context_supplementary_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let loader_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "loader-context-supplementary")
        .expect("loader-context-supplementary catalog should load");
    assert_eq!(loader_catalog.fixture_seeds.len(), 1);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        loader_context_supplementary_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    let fixture = &loader_catalog.fixture_seeds[0];
    let batch_entry = batch_snapshot
        .get(fixture.test_relative_path.as_str())
        .expect("supplementary builtin completeness seed should exist in batch snapshot");
    assert_eq!(fixture.id, fixture.test_relative_path);
    assert_eq!(fixture.slice, "supplementary-builtin-completeness");
    assert_eq!(fixture.test_tier, NodeCompatTestTier::Supplementary);
    assert_eq!(
        fixture.supplementary_category,
        Some(NodeCompatSupplementaryCategory::BuiltinCompleteness)
    );
    assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
}

#[test]
fn node_compat_process_and_timing_supplementary_fixture_seed_entries_align_with_batch_definitions()
{
    let resolved = load_family_catalogs_from_disk();
    let process_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "process-and-timing-supplementary")
        .expect("process-and-timing-supplementary catalog should load");
    assert_eq!(process_catalog.fixture_seeds.len(), 1);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        process_and_timing_supplementary_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    let fixture = &process_catalog.fixture_seeds[0];
    let batch_entry = batch_snapshot
        .get(fixture.test_relative_path.as_str())
        .expect("supplementary process release shape seed should exist in batch snapshot");
    assert_eq!(fixture.id, fixture.test_relative_path);
    assert_eq!(fixture.slice, "supplementary-process-release-shape");
    assert_eq!(fixture.test_tier, NodeCompatTestTier::Supplementary);
    assert_eq!(
        fixture.supplementary_category,
        Some(NodeCompatSupplementaryCategory::ProcessObjectShape)
    );
    assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
}

#[test]
fn node_compat_runtime_supplementary_fixture_seed_entries_align_with_batch_definitions() {
    let resolved = load_family_catalogs_from_disk();
    let runtime_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "runtime-supplementary")
        .expect("runtime-supplementary catalog should load");
    assert_eq!(runtime_catalog.fixture_seeds.len(), 2);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        runtime_supplementary_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    for (expected_path, expected_slice, expected_category) in [
        (
            "supplementary/resource-safety.mjs",
            "supplementary-resource-safety",
            NodeCompatSupplementaryCategory::ResourceSafety,
        ),
        (
            "supplementary/framework-loader-patterns.mjs",
            "supplementary-framework-loader-patterns",
            NodeCompatSupplementaryCategory::FrameworkMotivatedPatterns,
        ),
    ] {
        let fixture = runtime_catalog
            .fixture_seeds
            .iter()
            .find(|fixture| fixture.test_relative_path == expected_path)
            .unwrap_or_else(|| panic!("runtime supplementary fixture {expected_path} should load"));
        let batch_entry = batch_snapshot
            .get(fixture.test_relative_path.as_str())
            .expect("runtime supplementary seed should exist in batch snapshot");
        assert_eq!(fixture.id, fixture.test_relative_path);
        assert_eq!(fixture.slice, expected_slice);
        assert_eq!(fixture.test_tier, NodeCompatTestTier::Supplementary);
        assert_eq!(fixture.supplementary_category, Some(expected_category));
        assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
    }
}

#[test]
fn node_compat_runtime_supplementary_signal_lifecycle_fixture_seed_entries_align_with_batch_definitions()
 {
    let resolved = load_family_catalogs_from_disk();
    let signal_catalog = resolved
        .family_catalogs
        .iter()
        .find(|catalog| catalog.family == "runtime-supplementary-signal-lifecycle")
        .expect("runtime-supplementary-signal-lifecycle catalog should load");
    assert_eq!(signal_catalog.fixture_seeds.len(), 1);

    let batch_snapshot: BTreeMap<&str, NodeCompatBatchEntrySnapshot> =
        runtime_supplementary_signal_lifecycle_batch_snapshot()
            .into_iter()
            .map(|entry| (entry.test_relative_path, entry))
            .collect();

    let fixture = &signal_catalog.fixture_seeds[0];
    let batch_entry = batch_snapshot
        .get(fixture.test_relative_path.as_str())
        .expect("supplementary signal lifecycle seed should exist in batch snapshot");
    assert_eq!(fixture.id, fixture.test_relative_path);
    assert_eq!(fixture.slice, "supplementary-signal-listener-lifecycle");
    assert_eq!(fixture.test_tier, NodeCompatTestTier::Supplementary);
    assert_eq!(
        fixture.supplementary_category,
        Some(NodeCompatSupplementaryCategory::ResourceSafety)
    );
    assert_fixture_lane_sources_match_batch_entry(fixture, batch_entry);
}

#[test]
fn node_compat_preset_capability_model_rejects_ambiguous_seed_entries() {
    let mut duplicate_preset_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(CORE_SEMANTICS_JSON).expect("core semantics catalog should parse");
    duplicate_preset_catalog.presets =
        vec![NodeCompatPreset::Application, NodeCompatPreset::Application];
    let duplicate_preset_error = validate_family_catalogs(&[duplicate_preset_catalog])
        .expect_err("duplicate preset should fail");
    assert!(
        duplicate_preset_error.contains("duplicate presets"),
        "duplicate preset error should mention presets: {duplicate_preset_error}",
    );

    let mut duplicate_capability_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(NETWORKING_JSON).expect("networking catalog should parse");
    duplicate_capability_catalog.capabilities = vec![
        NodeCompatCapability::BundleRootFs,
        NodeCompatCapability::LoopbackNet,
        NodeCompatCapability::LoopbackNet,
    ];
    let duplicate_capability_error = validate_family_catalogs(&[duplicate_capability_catalog])
        .expect_err("duplicate capability should fail");
    assert!(
        duplicate_capability_error.contains("duplicate capabilities"),
        "duplicate capability error should mention capabilities: {duplicate_capability_error}",
    );

    let invalid_execution_class = serde_json::json!({
        "schema_version": 1,
        "family": "core-semantics",
        "nlc_item": "NLC3",
        "batch_constant": "CORE_SEMANTICS_BATCH",
        "execution_class": "nonsense",
        "presets": ["Application"],
        "capabilities": ["bundle-root-fs"],
        "lane_batches": [
            {
                "lane": "node22",
                "subset_test": "runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset"
            }
        ],
        "manifest_doc": "docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md",
        "failure_doc": "docs/architecture/runtime/node-lts-compat/failures/core-semantics.md"
    });
    assert!(
        serde_json::from_value::<NodeCompatFamilyCatalog>(invalid_execution_class).is_err(),
        "unknown execution class should fail to parse",
    );

    let mut missing_lane_source_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(CORE_SEMANTICS_JSON).expect("core semantics catalog should parse");
    missing_lane_source_catalog.fixture_seeds = vec![NodeCompatFixtureSeedEntry {
        id: "synthetic".to_string(),
        test_relative_path: "test/parallel/test-synthetic.js".to_string(),
        slice: "synthetic".to_string(),
        test_tier: NodeCompatTestTier::UpstreamVendored,
        supplementary_category: None,
        lane_sources: super::node_compat_manifest_catalog::NodeCompatFixtureSeedLaneSources(
            BTreeMap::new(),
        ),
        named_preludes: Vec::new(),
        named_postludes: Vec::new(),
    }];
    let missing_lane_source_error = validate_family_catalogs(&[missing_lane_source_catalog])
        .expect_err("fixture seed without lane sources should fail");
    assert!(
        missing_lane_source_error.contains("has no lane sources"),
        "missing lane source error should mention lane sources: {missing_lane_source_error}",
    );

    let duplicate_named_behavior_catalog = NodeCompatNamedBehaviorCatalog {
        schema_version: 1,
        named_behaviors: vec![
            super::node_compat_manifest_catalog::NodeCompatNamedBehaviorMetadata {
                id: "interactive_terminal".to_string(),
                phase: "prelude".to_string(),
                selection_mode: "default_fixture_mapping".to_string(),
            },
            super::node_compat_manifest_catalog::NodeCompatNamedBehaviorMetadata {
                id: "interactive_terminal".to_string(),
                phase: "prelude".to_string(),
                selection_mode: "default_fixture_mapping".to_string(),
            },
        ],
    };
    let duplicate_named_behavior_error =
        validate_named_behavior_catalog(&duplicate_named_behavior_catalog)
            .expect_err("duplicate named behavior ids should fail");
    assert!(
        duplicate_named_behavior_error.contains("duplicate named behavior ids"),
        "duplicate named behavior error should mention ids: {duplicate_named_behavior_error}",
    );

    let mut unknown_lane_source_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(CORE_SEMANTICS_JSON).expect("core semantics catalog should parse");
    unknown_lane_source_catalog.fixture_seeds = vec![NodeCompatFixtureSeedEntry {
        id: "synthetic".to_string(),
        test_relative_path: "test/parallel/test-synthetic.js".to_string(),
        slice: "synthetic".to_string(),
        test_tier: NodeCompatTestTier::UpstreamVendored,
        supplementary_category: None,
        lane_sources: super::node_compat_manifest_catalog::NodeCompatFixtureSeedLaneSources(
            BTreeMap::from([(
                "node26".to_string(),
                "node26/test/parallel/test-synthetic.js".to_string(),
            )]),
        ),
        named_preludes: Vec::new(),
        named_postludes: Vec::new(),
    }];
    let unknown_lane_source_error = validate_family_catalogs(&[unknown_lane_source_catalog])
        .expect_err("fixture seed referencing undeclared future lane should fail");
    assert!(
        unknown_lane_source_error.contains("references unknown lane node26"),
        "unknown lane source error should mention the future lane key: {unknown_lane_source_error}",
    );

    let mut missing_supplementary_category_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(LOADER_CONTEXT_SUPPLEMENTARY_JSON)
            .expect("supplementary catalog should parse");
    missing_supplementary_category_catalog.fixture_seeds[0].supplementary_category = None;
    let missing_supplementary_category_error =
        validate_family_catalogs(&[missing_supplementary_category_catalog])
            .expect_err("supplementary fixture without category should fail");
    assert!(
        missing_supplementary_category_error.contains("must declare supplementary_category"),
        "supplementary category error should mention the missing category: {missing_supplementary_category_error}",
    );
}

fn assert_fixture_lane_sources_match_batch_entry(
    fixture: &NodeCompatFixtureSeedEntry,
    batch_entry: &NodeCompatBatchEntrySnapshot,
) {
    assert_eq!(
        fixture.lane_sources.get("node20"),
        batch_entry.node20_fixture_source_path
    );
    assert_eq!(
        fixture.lane_sources.get("node22"),
        batch_entry.node22_fixture_source_path
    );
    assert_eq!(
        fixture.lane_sources.get("node24"),
        batch_entry.node24_fixture_source_path
    );
}
