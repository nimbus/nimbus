use serde_json::Value;
use serde_json::json;

use super::node_compat_manifest_catalog::{
    NodeCompatLaneMetadata, NodeCompatLaneRole, NodeCompatPublicContractRole, repo_root,
};

const SCHEMA_JSON: &str = include_str!("../node_compat_manifests/schema.json");
const NODE20_JSON: &str = include_str!("../node_compat_manifests/lanes/node20.json");
const NODE22_JSON: &str = include_str!("../node_compat_manifests/lanes/node22.json");
const NODE24_JSON: &str = include_str!("../node_compat_manifests/lanes/node24.json");
const NODE26_JSON: &str = include_str!("../node_compat_manifests/lanes/node26.json");

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
        "runtime_limits_preset",
        "upstream",
        "fixture_provenance",
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
            NodeCompatLaneRole::Legacy,
            NodeCompatPublicContractRole::Legacy,
            "Node20",
            "application_node20",
            "v20.20.2",
            "3626fea570e44896ad99aaf3bf6e59def5adede5",
            "35e07843146797923006aa01c6daabf4f53a4fb9",
        ),
        (
            "node22",
            NODE22_JSON,
            "Node22",
            NodeCompatLaneRole::Supported,
            NodeCompatPublicContractRole::Supported,
            "Node22",
            "application_node22",
            "v22.23.2",
            "aa4c77582be995286fc6e00aaf530dc7ade102a9",
            "490a9fef8f8adcda5a95bd6f96035b05cb43fe5b",
        ),
        (
            "node24",
            NODE24_JSON,
            "Node24",
            NodeCompatLaneRole::Default,
            NodeCompatPublicContractRole::Default,
            "Node24",
            "application_node24",
            "v24.20.0",
            "71b8b174857e25106d39b61a9e6f30d927da8b01",
            "8392e555cbdef2145d2cd2a2a7d29204d88d4e15",
        ),
        (
            "node26",
            NODE26_JSON,
            "Node26",
            NodeCompatLaneRole::Current,
            NodeCompatPublicContractRole::Current,
            "Node26",
            "application_node26",
            "v26.8.1",
            "7be6d3af31a65adea57c94c41e50c2b071ed0b3a",
            "03c764c3c9fc07333d5fa4fc58c56ee946f56b2f",
        ),
    ];

    for (
        expected_lane,
        json,
        expected_fixture_line,
        expected_lane_role,
        expected_public_contract_role,
        expected_runtime_execution_target,
        expected_runtime_limits_preset,
        expected_tag,
        expected_commit,
        expected_tag_object,
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
            metadata.runtime_limits_preset,
            expected_runtime_limits_preset
        );
        assert_eq!(metadata.upstream.repo, "nodejs/node");
        assert_eq!(metadata.upstream.tag, expected_tag);
        assert_eq!(metadata.upstream.commit, expected_commit);
        assert_eq!(metadata.upstream.tag_object, expected_tag_object);
        assert_eq!(metadata.upstream.fixture_subtree, "test");
        assert_eq!(
            metadata.upstream.source_kind,
            "vendored_official_fixture_corpus"
        );
        assert_eq!(
            metadata.fixture_provenance.selection_command,
            format!(
                "python3 scripts/runtime/node/sync.py --lane {expected_lane} --upstream-tag {expected_tag} --apply"
            )
        );
        assert_eq!(
            metadata.fixture_provenance.nimbus_sync_commit,
            if expected_lane == "node20" {
                "17a6bf48e3d69a5c153ffc89300629cc798346a5"
            } else {
                "af5bf1455bb9fddaa8bc05bb22fd8e89f08e859b"
            }
        );
        assert_eq!(
            metadata.fixture_provenance.synced_at,
            if expected_lane == "node20" {
                "2026-05-11T19:29:29-05:00"
            } else {
                "2026-09-01T05:15:00Z"
            }
        );
        assert_eq!(
            metadata.fixture_provenance.recorded_at,
            if expected_lane == "node20" {
                "2026-05-28"
            } else {
                "2026-09-01"
            }
        );
        let expected_identity_command = if expected_lane == "node20" {
            "git rev-list -n 1"
        } else {
            "git ls-remote"
        };
        assert!(
            metadata
                .fixture_provenance
                .recorded_from
                .contains(expected_identity_command),
            "fixture provenance should explain how commit identity was recorded",
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
        "lane_role": "current",
        "public_contract_role": "current_contract",
        "runtime_execution_target": "Node26",
        "runtime_limits_preset": "application_node26",
        "upstream": {
            "repo": "nodejs/node",
            "tag": "v26.0.0",
            "commit": "0123456789abcdef0123456789abcdef01234567",
            "tag_object": "89abcdef0123456789abcdef0123456789abcdef",
            "fixture_subtree": "test",
            "source_kind": "vendored_official_fixture_corpus"
        },
        "fixture_provenance": {
            "synced_at": "2026-05-28T00:00:00+00:00",
            "selection_command": "python3 scripts/runtime/node/sync.py --lane node26 --upstream-tag v26.0.0 --apply",
            "nimbus_sync_commit": "17a6bf48e3d69a5c153ffc89300629cc798346a5",
            "recorded_at": "2026-05-28",
            "recorded_from": "synthetic test fixture"
        },
        "vendored_fixture_root": "crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/node24/test",
        "manifest_docs": [
            "tests/runtime/node/compat/node-lts-compat/manifests/core-semantics.md"
        ],
        "failure_docs": [
            "tests/runtime/node/compat/node-lts-compat/failures/core-semantics.md"
        ]
    }))
    .expect("synthetic future lane metadata should parse");

    assert_eq!(metadata.lane, "node26");
    assert_eq!(metadata.upstream_fixture_line, "Node26");
    assert_eq!(metadata.upstream.tag, "v26.0.0");
    assert_eq!(
        metadata.fixture_provenance.selection_command,
        "python3 scripts/runtime/node/sync.py --lane node26 --upstream-tag v26.0.0 --apply"
    );
}
