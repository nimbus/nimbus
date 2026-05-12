use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

const PRELUDES_JSON: &str = include_str!("node_compat_manifests/preludes.json");

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatNamedBehaviorCatalog {
    pub(super) schema_version: u32,
    pub(super) named_behaviors: Vec<NodeCompatNamedBehaviorMetadata>,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatNamedBehaviorMetadata {
    pub(super) id: String,
    pub(super) phase: String,
    pub(super) selection_mode: String,
    pub(super) preview_lane_only: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatFamilyLaneBatch {
    pub(super) lane: String,
    pub(super) subset_test: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatLaneRole {
    Primary,
    Validation,
    Preview,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatPublicContractRole {
    PrimaryContract,
    MeasuredValidationLane,
    PreviewVisibilityLane,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatUpstreamMetadata {
    pub(super) repo: String,
    pub(super) tag: String,
    pub(super) fixture_subtree: String,
    pub(super) source_kind: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatLaneMetadata {
    pub(super) schema_version: u32,
    pub(super) lane: String,
    pub(super) upstream_fixture_line: String,
    pub(super) lane_role: NodeCompatLaneRole,
    pub(super) public_contract_role: NodeCompatPublicContractRole,
    pub(super) runtime_execution_target: String,
    pub(super) runtime_limits_profile: String,
    pub(super) upstream: NodeCompatUpstreamMetadata,
    pub(super) vendored_fixture_root: String,
    pub(super) manifest_docs: Vec<String>,
    pub(super) failure_docs: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub(super) struct NodeCompatFixtureSeedLaneSources(pub(super) BTreeMap<String, String>);

impl NodeCompatFixtureSeedLaneSources {
    pub(super) fn get(&self, lane: &str) -> Option<&str> {
        self.0.get(lane).map(String::as_str)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn lane_names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatTestTier {
    #[default]
    UpstreamVendored,
    Supplementary,
    Canary,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatSupplementaryCategory {
    BuiltinCompleteness,
    ModuleResolutionBridge,
    GlobalInjectionFidelity,
    ProcessObjectShape,
    ResourceSafety,
    FrameworkMotivatedPatterns,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatFixtureSeedEntry {
    pub(super) id: String,
    pub(super) test_relative_path: String,
    pub(super) slice: String,
    #[serde(default)]
    pub(super) test_tier: NodeCompatTestTier,
    #[serde(default)]
    pub(super) supplementary_category: Option<NodeCompatSupplementaryCategory>,
    pub(super) lane_sources: NodeCompatFixtureSeedLaneSources,
    #[serde(default)]
    pub(super) named_preludes: Vec<String>,
    #[serde(default)]
    pub(super) named_postludes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NodeCompatProfile {
    #[serde(rename = "Application")]
    Application,
    #[serde(rename = "Tooling")]
    Tooling,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatExecutionClass {
    Parallel,
    Sequential,
    Watchpoint,
    ExpectedFailure,
    OracleOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub(super) enum NodeCompatCapability {
    Tty,
    MainThread,
    Crypto,
    BundleRootFs,
    LoopbackNet,
    ExternalNet,
    DnsResultOrder,
    GcExposed,
    ChildProcess,
    WorkerThreads,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatFamilyCatalog {
    pub(super) schema_version: u32,
    pub(super) family: String,
    pub(super) nlc_item: String,
    pub(super) batch_constant: String,
    pub(super) execution_class: NodeCompatExecutionClass,
    pub(super) profiles: Vec<NodeCompatProfile>,
    pub(super) capabilities: Vec<NodeCompatCapability>,
    pub(super) lane_batches: Vec<NodeCompatFamilyLaneBatch>,
    pub(super) manifest_doc: String,
    pub(super) failure_doc: String,
    #[serde(default)]
    pub(super) fixture_seeds: Vec<NodeCompatFixtureSeedEntry>,
}

#[derive(Debug)]
pub(super) struct NodeCompatResolvedManifestCatalog {
    pub(super) lane_files: Vec<String>,
    pub(super) lane_catalogs: Vec<NodeCompatLaneMetadata>,
    pub(super) named_behavior_catalog: NodeCompatNamedBehaviorCatalog,
    pub(super) family_files: Vec<String>,
    pub(super) family_catalogs: Vec<NodeCompatFamilyCatalog>,
}

#[derive(Debug)]
pub(super) struct NodeCompatResolvedFixtureSeedSlice<'a> {
    pub(super) family_catalog: &'a NodeCompatFamilyCatalog,
    pub(super) slice: &'a str,
    pub(super) fixtures: Vec<&'a NodeCompatFixtureSeedEntry>,
}

#[derive(Debug)]
pub(super) struct NodeCompatResolvedLaneFixture<'a> {
    pub(super) fixture: &'a NodeCompatFixtureSeedEntry,
    pub(super) fixture_source_path: &'a str,
}

#[derive(Debug)]
pub(super) struct NodeCompatResolvedLaneExecutionPlan<'a> {
    pub(super) lane: &'a str,
    pub(super) lane_metadata: &'a NodeCompatLaneMetadata,
    pub(super) subset_test: &'a str,
    pub(super) fixtures: Vec<NodeCompatResolvedLaneFixture<'a>>,
}

#[derive(Debug)]
pub(super) struct NodeCompatResolvedExecutionPlan<'a> {
    pub(super) family_catalog: &'a NodeCompatFamilyCatalog,
    pub(super) slice: &'a str,
    pub(super) lanes: Vec<NodeCompatResolvedLaneExecutionPlan<'a>>,
}

pub(super) fn repo_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(Path::parent)
        .expect("neovex-runtime should live under crates/")
        .to_path_buf()
}

pub(super) fn read_sorted_manifest_file_names(relative_dir: &str) -> Vec<String> {
    let repo_root = repo_root();
    let dir = repo_root.join(relative_dir);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
        .map(|entry| {
            entry
                .expect("manifest directory entry should read")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

pub(super) fn load_family_catalogs_from_disk() -> NodeCompatResolvedManifestCatalog {
    let repo_root = repo_root();
    let lane_files = read_sorted_manifest_file_names(
        "crates/neovex-runtime/src/runtime/tests/node_compat_manifests/lanes",
    );
    let family_files = read_sorted_manifest_file_names(
        "crates/neovex-runtime/src/runtime/tests/node_compat_manifests/fixtures",
    );
    let named_behavior_catalog: NodeCompatNamedBehaviorCatalog =
        serde_json::from_str(PRELUDES_JSON).expect("named behavior catalog should parse");
    let lane_catalogs = lane_files
        .iter()
        .map(|file_name| {
            let path = repo_root.join(format!(
                "crates/neovex-runtime/src/runtime/tests/node_compat_manifests/lanes/{file_name}"
            ));
            serde_json::from_slice::<NodeCompatLaneMetadata>(
                &std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
            )
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
        })
        .collect();
    let family_catalogs = family_files
        .iter()
        .map(|file_name| {
            let path = repo_root.join(format!(
                "crates/neovex-runtime/src/runtime/tests/node_compat_manifests/fixtures/{file_name}"
            ));
            serde_json::from_slice::<NodeCompatFamilyCatalog>(
                &std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
            )
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
        })
        .collect();
    NodeCompatResolvedManifestCatalog {
        lane_files,
        lane_catalogs,
        named_behavior_catalog,
        family_files,
        family_catalogs,
    }
}

pub(super) fn validate_named_behavior_catalog(
    catalog: &NodeCompatNamedBehaviorCatalog,
) -> Result<(), String> {
    let unique_ids: BTreeSet<&str> = catalog
        .named_behaviors
        .iter()
        .map(|behavior| behavior.id.as_str())
        .collect();
    if unique_ids.len() != catalog.named_behaviors.len() {
        return Err("duplicate named behavior ids".to_string());
    }
    Ok(())
}

pub(super) fn validate_resolved_manifest_catalog(
    catalog: &NodeCompatResolvedManifestCatalog,
) -> Result<(), String> {
    validate_lane_catalogs(&catalog.lane_files, &catalog.lane_catalogs)?;
    validate_named_behavior_catalog(&catalog.named_behavior_catalog)?;
    validate_family_catalogs(&catalog.family_catalogs)?;

    let known_lane_ids: BTreeSet<&str> = catalog
        .lane_catalogs
        .iter()
        .map(|lane_metadata| lane_metadata.lane.as_str())
        .collect();
    let known_preludes: BTreeSet<&str> = catalog
        .named_behavior_catalog
        .named_behaviors
        .iter()
        .filter(|behavior| behavior.phase == "prelude")
        .map(|behavior| behavior.id.as_str())
        .collect();
    let known_postludes: BTreeSet<&str> = catalog
        .named_behavior_catalog
        .named_behaviors
        .iter()
        .filter(|behavior| behavior.phase == "postlude")
        .map(|behavior| behavior.id.as_str())
        .collect();
    let mut seen_fixture_ids = BTreeSet::new();

    for family in &catalog.family_catalogs {
        for lane_batch in &family.lane_batches {
            if !known_lane_ids.contains(lane_batch.lane.as_str()) {
                return Err(format!(
                    "family {} references unknown lane {}",
                    family.family, lane_batch.lane
                ));
            }
        }
        for fixture in &family.fixture_seeds {
            if !seen_fixture_ids.insert(fixture.id.as_str()) {
                return Err(format!(
                    "duplicate fixture seed id {} across family catalogs",
                    fixture.id
                ));
            }
            for prelude in &fixture.named_preludes {
                if !known_preludes.contains(prelude.as_str()) {
                    return Err(format!(
                        "fixture seed {} references unknown prelude {}",
                        fixture.id, prelude
                    ));
                }
            }
            for postlude in &fixture.named_postludes {
                if !known_postludes.contains(postlude.as_str()) {
                    return Err(format!(
                        "fixture seed {} references unknown postlude {}",
                        fixture.id, postlude
                    ));
                }
            }
        }
    }

    Ok(())
}

impl NodeCompatResolvedManifestCatalog {
    pub(super) fn family_catalog(&self, family: &str) -> Option<&NodeCompatFamilyCatalog> {
        self.family_catalogs
            .iter()
            .find(|catalog| catalog.family == family)
    }

    pub(super) fn lane_metadata(&self, lane: &str) -> Option<&NodeCompatLaneMetadata> {
        self.lane_catalogs
            .iter()
            .find(|metadata| metadata.lane == lane)
    }

    pub(super) fn resolve_fixture_seed_slice<'a>(
        &'a self,
        family: &str,
        slice: &'a str,
    ) -> Result<NodeCompatResolvedFixtureSeedSlice<'a>, String> {
        let family_catalog = self
            .family_catalog(family)
            .ok_or_else(|| format!("unknown family catalog {family}"))?;
        let fixtures: Vec<&NodeCompatFixtureSeedEntry> = family_catalog
            .fixture_seeds
            .iter()
            .filter(|fixture| fixture.slice == slice)
            .collect();
        if fixtures.is_empty() {
            return Err(format!(
                "family catalog {} has no fixture seed slice {}",
                family_catalog.family, slice
            ));
        }
        Ok(NodeCompatResolvedFixtureSeedSlice {
            family_catalog,
            slice,
            fixtures,
        })
    }

    pub(super) fn resolve_lane_execution_plan<'a>(
        &'a self,
        family: &str,
        slice: &'a str,
    ) -> Result<NodeCompatResolvedExecutionPlan<'a>, String> {
        let resolved_slice = self.resolve_fixture_seed_slice(family, slice)?;
        let lanes = resolved_slice
            .family_catalog
            .lane_batches
            .iter()
            .map(|lane_batch| {
                let lane_metadata =
                    self.lane_metadata(lane_batch.lane.as_str())
                        .ok_or_else(|| {
                            format!(
                                "family {} references unknown lane metadata {}",
                                resolved_slice.family_catalog.family, lane_batch.lane
                            )
                        })?;
                Ok(NodeCompatResolvedLaneExecutionPlan {
                    lane: lane_batch.lane.as_str(),
                    lane_metadata,
                    subset_test: lane_batch.subset_test.as_str(),
                    fixtures: resolved_slice
                        .fixtures
                        .iter()
                        .filter_map(|fixture| {
                            let fixture_source_path =
                                fixture.lane_sources.get(lane_batch.lane.as_str())?;
                            Some(NodeCompatResolvedLaneFixture {
                                fixture,
                                fixture_source_path,
                            })
                        })
                        .collect(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(NodeCompatResolvedExecutionPlan {
            family_catalog: resolved_slice.family_catalog,
            slice: resolved_slice.slice,
            lanes,
        })
    }
}

fn validate_lane_catalogs(
    lane_files: &[String],
    lane_catalogs: &[NodeCompatLaneMetadata],
) -> Result<(), String> {
    let actual_lane_files: BTreeSet<String> = lane_files.iter().cloned().collect();
    let expected_lane_files: BTreeSet<String> = lane_catalogs
        .iter()
        .map(|metadata| format!("{}.json", metadata.lane))
        .collect();
    if actual_lane_files != expected_lane_files {
        return Err(format!(
            "lane metadata files {:?} do not match parsed lane catalogs {:?}",
            actual_lane_files, expected_lane_files
        ));
    }
    let unique_lanes: BTreeSet<&str> = lane_catalogs
        .iter()
        .map(|metadata| metadata.lane.as_str())
        .collect();
    if unique_lanes.len() != lane_catalogs.len() {
        return Err("duplicate lane metadata ids".to_string());
    }
    Ok(())
}

pub(super) fn validate_family_catalogs(catalogs: &[NodeCompatFamilyCatalog]) -> Result<(), String> {
    let mut seen_families = BTreeSet::new();
    for catalog in catalogs {
        if !seen_families.insert(catalog.family.as_str()) {
            return Err(format!("duplicate family catalog {}", catalog.family));
        }
        let unique_profiles: BTreeSet<NodeCompatProfile> =
            catalog.profiles.iter().copied().collect();
        if unique_profiles.len() != catalog.profiles.len() {
            return Err(format!("duplicate profiles for family {}", catalog.family));
        }
        let unique_capabilities: BTreeSet<NodeCompatCapability> =
            catalog.capabilities.iter().copied().collect();
        if unique_capabilities.len() != catalog.capabilities.len() {
            return Err(format!(
                "duplicate capabilities for family {}",
                catalog.family
            ));
        }
        let unique_lanes: BTreeSet<&str> = catalog
            .lane_batches
            .iter()
            .map(|entry| entry.lane.as_str())
            .collect();
        if unique_lanes.len() != catalog.lane_batches.len() {
            return Err(format!(
                "duplicate lane batch entries for family {}",
                catalog.family
            ));
        }
        let known_lanes: BTreeSet<&str> = catalog
            .lane_batches
            .iter()
            .map(|entry| entry.lane.as_str())
            .collect();
        let mut seen_fixture_ids = BTreeSet::new();
        let mut slice_tiers = BTreeMap::new();
        let mut slice_categories = BTreeMap::new();
        for fixture in &catalog.fixture_seeds {
            if !seen_fixture_ids.insert(fixture.id.as_str()) {
                return Err(format!(
                    "duplicate fixture seed ids for family {}",
                    catalog.family
                ));
            }
            match (fixture.test_tier, fixture.supplementary_category) {
                (NodeCompatTestTier::Supplementary, None) => {
                    return Err(format!(
                        "supplementary fixture seed {} for family {} must declare supplementary_category",
                        fixture.id, catalog.family
                    ));
                }
                (NodeCompatTestTier::UpstreamVendored | NodeCompatTestTier::Canary, Some(_)) => {
                    return Err(format!(
                        "non-supplementary fixture seed {} for family {} must not declare supplementary_category",
                        fixture.id, catalog.family
                    ));
                }
                _ => {}
            }
            match slice_tiers.get(fixture.slice.as_str()) {
                Some(existing) if existing != &fixture.test_tier => {
                    return Err(format!(
                        "slice {} for family {} mixes test tiers",
                        fixture.slice, catalog.family
                    ));
                }
                Some(_) => {}
                None => {
                    slice_tiers.insert(fixture.slice.as_str(), fixture.test_tier);
                }
            }
            match slice_categories.get(fixture.slice.as_str()) {
                Some(existing) if existing != &fixture.supplementary_category => {
                    return Err(format!(
                        "slice {} for family {} mixes supplementary categories",
                        fixture.slice, catalog.family
                    ));
                }
                Some(_) => {}
                None => {
                    slice_categories.insert(fixture.slice.as_str(), fixture.supplementary_category);
                }
            }
            if fixture.lane_sources.is_empty() {
                return Err(format!(
                    "fixture seed {} for family {} has no lane sources",
                    fixture.id, catalog.family
                ));
            }
            for lane_name in fixture.lane_sources.lane_names() {
                if !known_lanes.contains(lane_name) {
                    return Err(format!(
                        "fixture seed {} for family {} references unknown lane {}",
                        fixture.id, catalog.family, lane_name
                    ));
                }
            }
        }
    }
    Ok(())
}
