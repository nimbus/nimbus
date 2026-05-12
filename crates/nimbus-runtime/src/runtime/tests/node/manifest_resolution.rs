use std::collections::BTreeMap;

use super::node_compat_manifest_catalog::{
    NodeCompatFamilyCatalog, NodeCompatFamilyLaneBatch, NodeCompatFixtureSeedEntry,
    NodeCompatFixtureSeedLaneSources, NodeCompatLaneMetadata, NodeCompatNamedBehaviorCatalog,
    NodeCompatNamedBehaviorMetadata, NodeCompatResolvedManifestCatalog,
    load_family_catalogs_from_disk, validate_resolved_manifest_catalog,
};

const CORE_SEMANTICS_JSON: &str =
    include_str!("../node_compat_manifests/fixtures/core-semantics.json");
const NETWORKING_JSON: &str = include_str!("../node_compat_manifests/fixtures/networking.json");

#[test]
fn node_compat_manifest_resolution_validates_loaded_catalogs_across_files() {
    let resolved = load_family_catalogs_from_disk();

    validate_resolved_manifest_catalog(&resolved)
        .expect("loaded manifest catalogs should validate across files");
}

#[test]
fn node_compat_manifest_resolution_resolves_family_slice_deterministically() {
    let resolved = load_family_catalogs_from_disk();

    let slice = resolved
        .resolve_fixture_seed_slice("networking", "dns-net-foundation")
        .expect("networking seed slice should resolve");
    let actual_ids: Vec<&str> = slice
        .fixtures
        .iter()
        .map(|fixture| fixture.id.as_str())
        .collect();

    assert_eq!(slice.family_catalog.family, "networking");
    assert_eq!(slice.slice, "dns-net-foundation");
    assert_eq!(slice.fixtures.len(), 10);
    assert_eq!(
        actual_ids,
        vec![
            "test/parallel/test-dns-get-server.js",
            "test/parallel/test-dns-set-default-order.js",
            "test/parallel/test-dns-default-order-ipv4.js",
            "test/parallel/test-dns-default-order-ipv6.js",
            "test/parallel/test-dns-default-order-verbatim.js",
            "test/parallel/test-stream-finished.js",
            "test/parallel/test-stream-pipeline.js",
            "test/parallel/test-net-connect-options-invalid.js",
            "test/parallel/test-net-isip.js",
            "test/parallel/test-net-isipv4.js",
        ]
    );
    assert_eq!(
        slice.fixtures[6].lane_sources.get("node24"),
        None,
        "seed resolution should preserve explicit Node24 omissions",
    );
}

#[test]
fn node_compat_manifest_resolution_builds_lane_execution_plan_deterministically() {
    let resolved = load_family_catalogs_from_disk();

    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");

    assert_eq!(plan.family_catalog.family, "networking");
    assert_eq!(plan.slice, "dns-net-foundation");
    assert_eq!(plan.lanes.len(), 3);
    assert_eq!(plan.lanes[0].lane, "node20");
    assert_eq!(plan.lanes[1].lane, "node22");
    assert_eq!(plan.lanes[2].lane, "node24");
    assert_eq!(
        plan.lanes[0].subset_test,
        "runtime::tests::node_compat::node20_supported_lane_executes_official_networking_subset"
    );
    assert_eq!(plan.lanes[0].fixtures.len(), 10);
    assert_eq!(plan.lanes[1].fixtures.len(), 10);
    assert_eq!(plan.lanes[2].fixtures.len(), 9);
    assert_eq!(
        plan.lanes[0].fixtures[0].fixture.id,
        "test/parallel/test-dns-get-server.js"
    );
    assert_eq!(
        plan.lanes[1].fixtures[6].fixture_source_path,
        "node22/test/parallel/test-stream-pipeline.js"
    );
    assert!(
        plan.lanes[2]
            .fixtures
            .iter()
            .all(|fixture| fixture.fixture.id != "test/parallel/test-stream-pipeline.js"),
        "Node24 lane plan should preserve omitted fixture sources",
    );
}

#[test]
fn node_compat_manifest_resolution_rejects_duplicate_fixture_ids_across_families() {
    let core_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(CORE_SEMANTICS_JSON).expect("core catalog should parse");
    let mut networking_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(NETWORKING_JSON).expect("networking catalog should parse");
    networking_catalog.family = "networking-synthetic".to_string();
    networking_catalog.fixture_seeds[0].id = "test/parallel/test-buffer-alloc.js".to_string();
    let lane_catalogs = load_family_catalogs_from_disk().lane_catalogs;

    let duplicate_fixture_catalog =
        super::node_compat_manifest_catalog::NodeCompatResolvedManifestCatalog {
            lane_files: vec![
                "node20.json".to_string(),
                "node22.json".to_string(),
                "node24.json".to_string(),
            ],
            lane_catalogs,
            named_behavior_catalog:
                super::node_compat_manifest_catalog::NodeCompatNamedBehaviorCatalog {
                    schema_version: 1,
                    named_behaviors: Vec::new(),
                },
            family_files: vec![
                "core-semantics.json".to_string(),
                "networking-synthetic.json".to_string(),
            ],
            family_catalogs: vec![core_catalog, networking_catalog],
        };
    let error = validate_resolved_manifest_catalog(&duplicate_fixture_catalog)
        .expect_err("duplicate fixture ids across families should fail");

    assert!(
        error.contains("duplicate fixture seed id"),
        "duplicate fixture id error should mention fixture ids: {error}",
    );
}

#[test]
fn node_compat_manifest_resolution_supports_future_lane_keys_without_new_rust_fields() {
    let mut core_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(CORE_SEMANTICS_JSON).expect("core catalog should parse");
    core_catalog.family = "core-semantics-future-lane".to_string();
    core_catalog.lane_batches = vec![
        NodeCompatFamilyLaneBatch {
            lane: "node22".to_string(),
            subset_test:
                "runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset"
                    .to_string(),
        },
        NodeCompatFamilyLaneBatch {
            lane: "node26".to_string(),
            subset_test:
                "runtime::tests::node_compat::node26_preview_lane_executes_manifested_core_semantics_subset"
                    .to_string(),
        },
    ];
    core_catalog.fixture_seeds = vec![NodeCompatFixtureSeedEntry {
        id: "test/parallel/test-buffer-alloc.js".to_string(),
        test_relative_path: "test/parallel/test-buffer-alloc.js".to_string(),
        slice: "synthetic-future-lane".to_string(),
        test_tier: super::node_compat_manifest_catalog::NodeCompatTestTier::UpstreamVendored,
        supplementary_category: None,
        lane_sources: NodeCompatFixtureSeedLaneSources(BTreeMap::from([
            (
                "node22".to_string(),
                "node22/test/parallel/test-buffer-alloc.js".to_string(),
            ),
            (
                "node26".to_string(),
                "node26/test/parallel/test-buffer-alloc.js".to_string(),
            ),
        ])),
        named_preludes: Vec::new(),
        named_postludes: Vec::new(),
    }];
    let mut lane_catalogs = load_family_catalogs_from_disk().lane_catalogs;
    lane_catalogs.retain(|metadata| metadata.lane == "node22");
    lane_catalogs.push(
        serde_json::from_value::<NodeCompatLaneMetadata>(serde_json::json!({
            "schema_version": 1,
            "lane": "node26",
            "upstream_fixture_line": "Node26",
            "lane_role": "supported",
            "public_contract_role": "supported_contract",
            "runtime_execution_target": "Node24",
            "runtime_limits_preset": "application_node24",
            "upstream": {
                "repo": "nodejs/node",
                "tag": "v26.0.0",
                "fixture_subtree": "test",
                "source_kind": "vendored_official_fixture_corpus"
            },
            "vendored_fixture_root": "crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/node24/test",
            "manifest_docs": [
                "docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md"
            ],
            "failure_docs": [
                "docs/architecture/runtime/node-lts-compat/failures/core-semantics.md"
            ]
        }))
        .expect("synthetic node26 lane metadata should parse"),
    );
    let resolved = NodeCompatResolvedManifestCatalog {
        lane_files: vec!["node22.json".to_string(), "node26.json".to_string()],
        lane_catalogs,
        named_behavior_catalog: NodeCompatNamedBehaviorCatalog {
            schema_version: 1,
            named_behaviors: Vec::new(),
        },
        family_files: vec!["core-semantics-future-lane.json".to_string()],
        family_catalogs: vec![core_catalog],
    };

    validate_resolved_manifest_catalog(&resolved)
        .expect("future lane keyed seed sources should validate");
    let plan = resolved
        .resolve_lane_execution_plan("core-semantics-future-lane", "synthetic-future-lane")
        .expect("future lane execution plan should resolve");

    assert_eq!(plan.lanes.len(), 2);
    assert_eq!(plan.lanes[0].lane, "node22");
    assert_eq!(plan.lanes[1].lane, "node26");
    assert_eq!(plan.lanes[1].fixtures.len(), 1);
    assert_eq!(
        plan.lanes[1].fixtures[0].fixture_source_path,
        "node26/test/parallel/test-buffer-alloc.js"
    );
}

#[test]
fn node_compat_manifest_resolution_rejects_unknown_named_behavior_phase_links() {
    let mut core_catalog: NodeCompatFamilyCatalog =
        serde_json::from_str(CORE_SEMANTICS_JSON).expect("core catalog should parse");
    core_catalog.fixture_seeds[0]
        .named_preludes
        .push("fork_child_settle".to_string());
    let lane_catalogs = load_family_catalogs_from_disk().lane_catalogs;
    let resolved = super::node_compat_manifest_catalog::NodeCompatResolvedManifestCatalog {
        lane_files: vec![
            "node20.json".to_string(),
            "node22.json".to_string(),
            "node24.json".to_string(),
        ],
        lane_catalogs,
        named_behavior_catalog: NodeCompatNamedBehaviorCatalog {
            schema_version: 1,
            named_behaviors: vec![
                NodeCompatNamedBehaviorMetadata {
                    id: "interactive_terminal".to_string(),
                    phase: "prelude".to_string(),
                    selection_mode: "default_fixture_mapping".to_string(),
                },
                NodeCompatNamedBehaviorMetadata {
                    id: "fork_child_settle".to_string(),
                    phase: "postlude".to_string(),
                    selection_mode: "default_fixture_mapping".to_string(),
                },
            ],
        },
        family_files: vec!["core-semantics.json".to_string()],
        family_catalogs: vec![core_catalog],
    };
    let error = validate_resolved_manifest_catalog(&resolved)
        .expect_err("postlude linked as prelude should fail");

    assert!(
        error.contains("unknown prelude fork_child_settle"),
        "wrong-phase named behavior error should mention the prelude id: {error}",
    );
}

#[test]
fn node_compat_manifest_resolution_rejects_missing_family_or_slice() {
    let resolved = load_family_catalogs_from_disk();

    let family_error = resolved
        .resolve_fixture_seed_slice("not-a-family", "anything")
        .expect_err("unknown family should fail");
    assert!(
        family_error.contains("unknown family catalog not-a-family"),
        "missing family error should mention the requested family: {family_error}",
    );

    let slice_error = resolved
        .resolve_fixture_seed_slice("core-semantics", "not-a-slice")
        .expect_err("unknown slice should fail");
    assert!(
        slice_error.contains("has no fixture seed slice not-a-slice"),
        "missing slice error should mention the requested slice: {slice_error}",
    );
}
