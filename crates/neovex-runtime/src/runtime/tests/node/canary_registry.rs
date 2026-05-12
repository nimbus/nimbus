use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("neovex-runtime should live under crates/")
        .to_path_buf()
}

fn canary_registry_path() -> PathBuf {
    repo_root().join("tests/runtime/node/canary-registry.json")
}

#[derive(Debug, Deserialize)]
struct NodeCompatCanaryClaim {
    id: String,
    package: String,
    runtime_preset: String,
    lane_coverage: Vec<String>,
    nlc_family: String,
    public_claim: String,
    doc_path: String,
}

#[derive(Debug, Deserialize)]
struct NodeCompatCanaryLaneRun {
    lane: String,
    compatibility_target: String,
    cargo_test: String,
}

#[derive(Debug, Deserialize)]
struct NodeCompatCanaryEntry {
    id: String,
    package: String,
    pinned_version: String,
    status: String,
    root: String,
    bundle: String,
    runtime_preset: String,
    nlc_family_dependency: String,
    claim_ids: Vec<String>,
    lane_runs: Vec<NodeCompatCanaryLaneRun>,
}

#[derive(Debug, Deserialize)]
struct NodeCompatCanaryRegistry {
    schema_version: u32,
    claims: Vec<NodeCompatCanaryClaim>,
    canaries: Vec<NodeCompatCanaryEntry>,
}

fn load_canary_registry() -> NodeCompatCanaryRegistry {
    let registry_path = canary_registry_path();
    serde_json::from_slice(
        &std::fs::read(&registry_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", registry_path.display())),
    )
    .unwrap_or_else(|error| panic!("failed to parse {}: {error}", registry_path.display()))
}

fn package_versions_for_root(root: &std::path::Path) -> BTreeMap<String, String> {
    serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(root.join("package.json")).unwrap_or_else(|error| {
            panic!(
                "failed to read {}: {error}",
                root.join("package.json").display()
            )
        }),
    )
    .unwrap_or_else(|error| {
        panic!(
            "failed to parse {}: {error}",
            root.join("package.json").display()
        )
    })["dependencies"]
        .as_object()
        .expect("dependencies should be an object")
        .iter()
        .map(|(package, version)| {
            (
                package.clone(),
                version
                    .as_str()
                    .expect("dependency version should be a string")
                    .to_string(),
            )
        })
        .collect()
}

#[test]
fn node_compat_canary_registry_parses_and_points_at_real_roots() {
    let repo_root = repo_root();
    let registry = load_canary_registry();
    let basic_invocation_source = std::fs::read_to_string(
        repo_root.join("crates/neovex-runtime/src/runtime/tests/basic_invocation.rs"),
    )
    .expect("basic_invocation.rs should read");

    assert_eq!(registry.schema_version, 1);

    let mut seen_claim_ids = BTreeSet::new();
    for claim in &registry.claims {
        assert!(seen_claim_ids.insert(claim.id.as_str()));
        assert!(
            !claim.public_claim.trim().is_empty(),
            "claim {} should describe a real public claim",
            claim.id
        );
        assert!(
            matches!(claim.runtime_preset.as_str(), "Application" | "Tooling"),
            "claim {} should use a supported runtime preset",
            claim.id
        );
        assert!(
            !claim.lane_coverage.is_empty(),
            "claim {} should list lane coverage",
            claim.id
        );
        assert!(
            repo_root.join(&claim.doc_path).is_file(),
            "claim {} should point at a real doc path {}",
            claim.id,
            claim.doc_path
        );
    }

    let mut seen_canary_ids = BTreeSet::new();
    for canary in &registry.canaries {
        assert!(seen_canary_ids.insert(canary.id.as_str()));
        assert_eq!(canary.status, "active");
        assert!(
            matches!(canary.runtime_preset.as_str(), "Application" | "Tooling"),
            "canary {} should use a supported runtime preset",
            canary.id
        );
        let root = repo_root.join(&canary.root);
        assert!(
            root.is_dir(),
            "canary root should exist: {}",
            root.display()
        );
        assert!(
            root.join("bundles").join(&canary.bundle).is_file(),
            "canary bundle should exist: {}",
            root.join("bundles").join(&canary.bundle).display()
        );
        let package_versions = package_versions_for_root(&root);
        let pinned_version = package_versions
            .get(&canary.package)
            .unwrap_or_else(|| panic!("missing package {} in package.json", canary.package));
        assert_eq!(
            pinned_version, &canary.pinned_version,
            "registry version should match canary package.json for {}",
            canary.package
        );
        assert!(
            !canary.lane_runs.is_empty(),
            "canary {} should define at least one lane run",
            canary.id
        );
        for lane_run in &canary.lane_runs {
            assert!(
                matches!(lane_run.lane.as_str(), "node20" | "node22" | "node24"),
                "unsupported lane {} for canary {}",
                lane_run.lane,
                canary.id
            );
            assert!(
                matches!(
                    lane_run.compatibility_target.as_str(),
                    "Node20" | "Node22" | "Node24"
                ),
                "unsupported compatibility target {} for canary {}",
                lane_run.compatibility_target,
                canary.id
            );
            assert!(
                basic_invocation_source.contains(&format!("fn {}()", lane_run.cargo_test)),
                "cargo test {} should exist in basic_invocation.rs",
                lane_run.cargo_test
            );
        }
    }
}

#[test]
fn node_compat_canary_registry_maps_active_claims_to_active_canaries() {
    let repo_root = repo_root();
    let registry = load_canary_registry();
    let surface_matrix = std::fs::read_to_string(
        repo_root.join("docs/architecture/runtime/node-compat-surface-matrix.md"),
    )
    .expect("surface matrix should read");

    let canaries_by_claim: BTreeMap<&str, Vec<&NodeCompatCanaryEntry>> = registry
        .canaries
        .iter()
        .flat_map(|canary| {
            canary
                .claim_ids
                .iter()
                .map(move |claim_id| (claim_id.as_str(), canary))
        })
        .fold(BTreeMap::new(), |mut acc, (claim_id, canary)| {
            acc.entry(claim_id).or_default().push(canary);
            acc
        });

    for claim in &registry.claims {
        let mapped_canaries = canaries_by_claim.get(claim.id.as_str()).unwrap_or_else(|| {
            panic!(
                "claim {} should map to at least one active canary",
                claim.id
            )
        });
        assert!(
            mapped_canaries
                .iter()
                .any(|canary| canary.runtime_preset == claim.runtime_preset),
            "claim {} should keep preset boundaries intact",
            claim.id
        );
        assert!(
            mapped_canaries
                .iter()
                .all(|canary| canary.nlc_family_dependency == claim.nlc_family),
            "claim {} should only map to canaries for the same NLC family",
            claim.id
        );
        assert!(
            surface_matrix.contains(&claim.package),
            "surface matrix should mention claimed package {}",
            claim.package
        );
    }
}
