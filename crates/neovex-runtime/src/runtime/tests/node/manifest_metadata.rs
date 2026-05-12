use serde_json::Value;
use serde_json::json;

use super::node_compat_manifest_catalog::{
    NodeCompatLaneMetadata, NodeCompatLaneRole, NodeCompatPublicContractRole, repo_root,
};

const SCHEMA_JSON: &str = include_str!("../node_compat_manifests/schema.json");
const NODE20_JSON: &str = include_str!("../node_compat_manifests/lanes/node20.json");
const NODE22_JSON: &str = include_str!("../node_compat_manifests/lanes/node22.json");
const NODE24_JSON: &str = include_str!("../node_compat_manifests/lanes/node24.json");

#[test]
fn node_compat_lane_metadata_schema_is_valid_json_and_documents_required_fields() {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).expect("schema should parse as JSON");
    let lane_schema = &schema["$defs"]["laneMetadata"];
    let required = lane_schema["required"]
        .as_array()
        .expect("laneMetadata.required should be an array");
    let properties = lane_schema["properties"]
        .as_object()
        .expect("laneMetadata.properties should be an object");

    for field in [
        "schema_version",
        "lane",
        "upstream_fixture_line",
        "lane_role",
        "public_contract_role",
        "runtime_execution_target",
        "runtime_limits_profile",
        "upstream",
        "vendored_fixture_root",
        "manifest_docs",
        "failure_docs",
    ] {
        assert!(
            required.iter().any(|entry| entry.as_str() == Some(field)),
            "schema should require {field}",
        );
        assert!(
            properties.contains_key(field),
            "schema should document property {field}",
        );
    }

    let lane_property = &lane_schema["properties"]["lane"];
    assert_eq!(
        lane_property["pattern"].as_str(),
        Some("^node[0-9]+$"),
        "lane metadata should allow future node lane keys by pattern",
    );
    let upstream_fixture_line_property = &lane_schema["properties"]["upstream_fixture_line"];
    assert_eq!(
        upstream_fixture_line_property["pattern"].as_str(),
        Some("^Node[0-9]+$"),
        "lane metadata should allow future upstream fixture lines by pattern",
    );
}

#[test]
fn node_compat_lane_metadata_files_parse_and_point_at_real_roots() {
    let repo_root = repo_root();
    let cases = [
        (
            "node20",
            NODE20_JSON,
            "Node20",
            NodeCompatLaneRole::Supported,
            NodeCompatPublicContractRole::SupportedContract,
            "Node20",
            "application_node20",
            "v20.20.2",
        ),
        (
            "node22",
            NODE22_JSON,
            "Node22",
            NodeCompatLaneRole::Default,
            NodeCompatPublicContractRole::DefaultContract,
            "Node22",
            "application_node22",
            "v22.15.0",
        ),
        (
            "node24",
            NODE24_JSON,
            "Node24",
            NodeCompatLaneRole::Supported,
            NodeCompatPublicContractRole::SupportedContract,
            "Node24",
            "application_node24",
            "v24.15.0",
        ),
    ];

    for (
        expected_lane,
        json,
        expected_fixture_line,
        expected_lane_role,
        expected_public_contract_role,
        expected_runtime_execution_target,
        expected_runtime_limits_profile,
        expected_tag,
    ) in cases
    {
        let metadata: NodeCompatLaneMetadata =
            serde_json::from_str(json).expect("lane metadata should parse");
        assert_eq!(
            metadata.schema_version, 1,
            "lane schema version should stay pinned"
        );
        assert_eq!(metadata.lane, expected_lane);
        assert_eq!(metadata.upstream_fixture_line, expected_fixture_line);
        assert_eq!(metadata.lane_role, expected_lane_role);
        assert_eq!(metadata.public_contract_role, expected_public_contract_role);
        assert_eq!(
            metadata.runtime_execution_target,
            expected_runtime_execution_target
        );
        assert_eq!(
            metadata.runtime_limits_profile,
            expected_runtime_limits_profile
        );
        assert_eq!(metadata.upstream.repo, "nodejs/node");
        assert_eq!(metadata.upstream.tag, expected_tag);
        assert_eq!(metadata.upstream.fixture_subtree, "test");
        assert_eq!(
            metadata.upstream.source_kind,
            "vendored_official_fixture_corpus"
        );

        let vendored_fixture_root = repo_root.join(&metadata.vendored_fixture_root);
        assert!(
            vendored_fixture_root.is_dir(),
            "vendored fixture root should exist: {}",
            vendored_fixture_root.display(),
        );

        assert_eq!(metadata.manifest_docs.len(), 5);
        assert_eq!(metadata.failure_docs.len(), 5);

        for relative_doc in metadata
            .manifest_docs
            .iter()
            .chain(metadata.failure_docs.iter())
        {
            let doc_path = repo_root.join(relative_doc);
            assert!(
                doc_path.is_file(),
                "lane metadata doc should exist: {}",
                doc_path.display(),
            );
        }
    }
}

#[test]
fn node_compat_lane_metadata_accepts_synthetic_future_lane_values() {
    let metadata: NodeCompatLaneMetadata = serde_json::from_value(json!({
        "schema_version": 1,
        "lane": "node26",
        "upstream_fixture_line": "Node26",
        "lane_role": "supported",
        "public_contract_role": "supported_contract",
        "runtime_execution_target": "Node24",
        "runtime_limits_profile": "application_node24",
        "upstream": {
            "repo": "nodejs/node",
            "tag": "v26.0.0",
            "fixture_subtree": "test",
            "source_kind": "vendored_official_fixture_corpus"
        },
        "vendored_fixture_root": "crates/neovex-runtime/src/runtime/tests/node_compat_fixtures/node24/test",
        "manifest_docs": [
            "docs/architecture/runtime/node-lts-compat/manifests/core-semantics.md"
        ],
        "failure_docs": [
            "docs/architecture/runtime/node-lts-compat/failures/core-semantics.md"
        ]
    }))
    .expect("synthetic future lane metadata should parse");

    assert_eq!(metadata.lane, "node26");
    assert_eq!(metadata.upstream_fixture_line, "Node26");
    assert_eq!(metadata.upstream.tag, "v26.0.0");
}
