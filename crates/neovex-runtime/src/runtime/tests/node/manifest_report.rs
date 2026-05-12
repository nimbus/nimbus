use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::node_compat_manifest_catalog::{
    NodeCompatCapability, NodeCompatExecutionClass, NodeCompatLaneRole, NodeCompatPreset,
    NodeCompatPublicContractRole, NodeCompatSupplementaryCategory, NodeCompatTestTier,
    load_family_catalogs_from_disk, repo_root,
};

const NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatPlanReport<'a> {
    pub(super) schema_version: u32,
    pub(super) family: &'a str,
    pub(super) nlc_item: &'a str,
    pub(super) slice: &'a str,
    pub(super) test_tier: &'static str,
    pub(super) supplementary_category: Option<&'static str>,
    pub(super) execution_class: &'static str,
    pub(super) presets: Vec<&'static str>,
    pub(super) capabilities: Vec<&'static str>,
    pub(super) slice_summary: NodeCompatSlicePlanSummary,
    pub(super) preset_summaries: Vec<NodeCompatPresetPlanSummary<'a>>,
    pub(super) lane_summaries: Vec<NodeCompatLanePlanSummary<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatSlicePlanSummary {
    pub(super) unique_fixture_count: usize,
    pub(super) lane_count: usize,
    pub(super) total_lane_fixture_entries: usize,
    pub(super) min_lane_fixture_count: usize,
    pub(super) max_lane_fixture_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatPresetPlanSummary<'a> {
    pub(super) preset: &'a str,
    pub(super) unique_fixture_count: usize,
    pub(super) lane_count: usize,
    pub(super) total_lane_fixture_entries: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatLanePlanSummary<'a> {
    pub(super) lane: &'a str,
    pub(super) upstream_fixture_line: &'a str,
    pub(super) lane_role: &'static str,
    pub(super) public_contract_role: &'static str,
    pub(super) runtime_execution_target: &'a str,
    pub(super) runtime_limits_preset: &'a str,
    pub(super) subset_test: &'a str,
    pub(super) fixture_count: usize,
    pub(super) fixture_ids: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatCatalogPlanReport<'a> {
    pub(super) schema_version: u32,
    pub(super) family_count: usize,
    pub(super) slice_count: usize,
    pub(super) total_unique_fixture_seed_count: usize,
    pub(super) total_lane_fixture_entries: usize,
    pub(super) preset_summaries: Vec<NodeCompatCatalogPresetPlanSummary<'a>>,
    pub(super) lane_summaries: Vec<NodeCompatCatalogLaneSummary<'a>>,
    pub(super) slice_reports: Vec<NodeCompatPlanReport<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatCatalogPresetPlanSummary<'a> {
    pub(super) preset: &'a str,
    pub(super) total_unique_fixture_seed_count: usize,
    pub(super) total_lane_fixture_entries: usize,
    pub(super) slice_count_with_entries: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatCatalogLaneSummary<'a> {
    pub(super) lane: &'a str,
    pub(super) upstream_fixture_line: &'a str,
    pub(super) lane_role: &'static str,
    pub(super) public_contract_role: &'static str,
    pub(super) runtime_execution_target: &'a str,
    pub(super) runtime_limits_preset: &'a str,
    pub(super) total_fixture_entries: usize,
    pub(super) slice_count_with_entries: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatObservedFixtureState {
    Pass,
    Skip,
    Fail,
}

#[derive(Debug, Deserialize)]
pub(super) struct NodeCompatObservedLaneFixtureResultRecord {
    pub(super) lane: String,
    pub(super) fixture_id: String,
    pub(super) state: NodeCompatObservedFixtureState,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
pub(super) struct NodeCompatObservedResultCounts {
    pub(super) passed: usize,
    pub(super) skipped: usize,
    pub(super) failed: usize,
    pub(super) missing: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeCompatObservedLaneFixtureResult<'a> {
    pub(super) lane: &'a str,
    pub(super) fixture_id: &'a str,
    pub(super) state: NodeCompatObservedFixtureState,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedPlanReport<'a> {
    pub(super) schema_version: u32,
    pub(super) family: &'a str,
    pub(super) nlc_item: &'a str,
    pub(super) slice: &'a str,
    pub(super) test_tier: &'static str,
    pub(super) supplementary_category: Option<&'static str>,
    pub(super) execution_class: &'static str,
    pub(super) presets: Vec<&'static str>,
    pub(super) capabilities: Vec<&'static str>,
    pub(super) slice_summary: NodeCompatObservedSliceSummary,
    pub(super) preset_summaries: Vec<NodeCompatObservedPresetSummary<'a>>,
    pub(super) lane_summaries: Vec<NodeCompatObservedLaneSummary<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedSliceSummary {
    pub(super) unique_fixture_count: usize,
    pub(super) lane_count: usize,
    pub(super) total_expected_results: usize,
    pub(super) total_observed_results: usize,
    pub(super) counts: NodeCompatObservedResultCounts,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedPresetSummary<'a> {
    pub(super) preset: &'a str,
    pub(super) total_expected_results: usize,
    pub(super) total_observed_results: usize,
    pub(super) counts: NodeCompatObservedResultCounts,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedLaneSummary<'a> {
    pub(super) lane: &'a str,
    pub(super) upstream_fixture_line: &'a str,
    pub(super) lane_role: &'static str,
    pub(super) public_contract_role: &'static str,
    pub(super) runtime_execution_target: &'a str,
    pub(super) runtime_limits_preset: &'a str,
    pub(super) subset_test: &'a str,
    pub(super) expected_fixture_count: usize,
    pub(super) observed_fixture_count: usize,
    pub(super) counts: NodeCompatObservedResultCounts,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeCompatObservedSliceInput<'a> {
    pub(super) family: &'a str,
    pub(super) slice: &'a str,
    pub(super) observed_results: &'a [NodeCompatObservedLaneFixtureResult<'a>],
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedCatalogReport<'a> {
    pub(super) schema_version: u32,
    pub(super) family_count: usize,
    pub(super) slice_count: usize,
    pub(super) total_unique_fixture_seed_count: usize,
    pub(super) total_expected_results: usize,
    pub(super) total_observed_results: usize,
    pub(super) counts: NodeCompatObservedResultCounts,
    pub(super) preset_summaries: Vec<NodeCompatObservedCatalogPresetSummary<'a>>,
    pub(super) lane_summaries: Vec<NodeCompatObservedCatalogLaneSummary<'a>>,
    pub(super) slice_reports: Vec<NodeCompatObservedPlanReport<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedCatalogPresetSummary<'a> {
    pub(super) preset: &'a str,
    pub(super) total_expected_results: usize,
    pub(super) total_observed_results: usize,
    pub(super) slice_count_with_entries: usize,
    pub(super) counts: NodeCompatObservedResultCounts,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeCompatObservedCatalogLaneSummary<'a> {
    pub(super) lane: &'a str,
    pub(super) upstream_fixture_line: &'a str,
    pub(super) lane_role: &'static str,
    pub(super) public_contract_role: &'static str,
    pub(super) runtime_execution_target: &'a str,
    pub(super) runtime_limits_preset: &'a str,
    pub(super) total_expected_results: usize,
    pub(super) total_observed_results: usize,
    pub(super) slice_count_with_entries: usize,
    pub(super) counts: NodeCompatObservedResultCounts,
}

#[derive(Debug)]
pub(super) struct NodeCompatSeededSliceReportArtifactBundle {
    pub(super) artifact_root: PathBuf,
    pub(super) slice_plan_path: PathBuf,
    pub(super) slice_observed_path: PathBuf,
    pub(super) catalog_plan_path: PathBuf,
    pub(super) catalog_observed_path: PathBuf,
}

fn execution_class_label(class: NodeCompatExecutionClass) -> &'static str {
    match class {
        NodeCompatExecutionClass::Parallel => "parallel",
        NodeCompatExecutionClass::Sequential => "sequential",
        NodeCompatExecutionClass::Watchpoint => "watchpoint",
        NodeCompatExecutionClass::ExpectedFailure => "expected_failure",
        NodeCompatExecutionClass::OracleOnly => "oracle_only",
    }
}

fn preset_label(preset: NodeCompatPreset) -> &'static str {
    match preset {
        NodeCompatPreset::Application => "Application",
        NodeCompatPreset::Tooling => "Tooling",
    }
}

fn capability_label(capability: NodeCompatCapability) -> &'static str {
    match capability {
        NodeCompatCapability::Tty => "tty",
        NodeCompatCapability::MainThread => "main-thread",
        NodeCompatCapability::Crypto => "crypto",
        NodeCompatCapability::BundleRootFs => "bundle-root-fs",
        NodeCompatCapability::LoopbackNet => "loopback-net",
        NodeCompatCapability::ExternalNet => "external-net",
        NodeCompatCapability::DnsResultOrder => "dns-result-order",
        NodeCompatCapability::GcExposed => "gc-exposed",
        NodeCompatCapability::ChildProcess => "child-process",
        NodeCompatCapability::WorkerThreads => "worker-threads",
    }
}

fn lane_role_label(role: NodeCompatLaneRole) -> &'static str {
    match role {
        NodeCompatLaneRole::Default => "default",
        NodeCompatLaneRole::Supported => "supported",
    }
}

fn public_contract_role_label(role: NodeCompatPublicContractRole) -> &'static str {
    match role {
        NodeCompatPublicContractRole::DefaultContract => "default_contract",
        NodeCompatPublicContractRole::SupportedContract => "supported_contract",
    }
}

fn test_tier_label(test_tier: NodeCompatTestTier) -> &'static str {
    match test_tier {
        NodeCompatTestTier::UpstreamVendored => "upstream_vendored",
        NodeCompatTestTier::Supplementary => "supplementary",
        NodeCompatTestTier::Canary => "canary",
    }
}

fn supplementary_category_label(category: NodeCompatSupplementaryCategory) -> &'static str {
    match category {
        NodeCompatSupplementaryCategory::BuiltinCompleteness => "builtin_completeness",
        NodeCompatSupplementaryCategory::ModuleResolutionBridge => "module_resolution_bridge",
        NodeCompatSupplementaryCategory::GlobalInjectionFidelity => "global_injection_fidelity",
        NodeCompatSupplementaryCategory::ProcessObjectShape => "process_object_shape",
        NodeCompatSupplementaryCategory::ResourceSafety => "resource_safety",
        NodeCompatSupplementaryCategory::FrameworkMotivatedPatterns => {
            "framework_motivated_patterns"
        }
    }
}

fn slice_fixture_metadata<'a>(
    plan: &super::node_compat_manifest_catalog::NodeCompatResolvedExecutionPlan<'a>,
) -> (&'static str, Option<&'static str>) {
    let fixture = plan
        .lanes
        .iter()
        .flat_map(|lane| lane.fixtures.iter())
        .next()
        .expect("execution plan should carry at least one fixture");
    (
        test_tier_label(fixture.fixture.test_tier),
        fixture
            .fixture
            .supplementary_category
            .map(supplementary_category_label),
    )
}

fn increment_observed_counts(
    counts: &mut NodeCompatObservedResultCounts,
    state: NodeCompatObservedFixtureState,
) {
    match state {
        NodeCompatObservedFixtureState::Pass => counts.passed += 1,
        NodeCompatObservedFixtureState::Skip => counts.skipped += 1,
        NodeCompatObservedFixtureState::Fail => counts.failed += 1,
    }
}

fn write_json_report_artifact<T: Serialize>(
    output_root: &Path,
    file_name: &str,
    report: &T,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(output_root).map_err(|error| {
        format!(
            "failed to create report artifact directory {}: {error}",
            output_root.display()
        )
    })?;
    let output_path = output_root.join(file_name);
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("failed to serialize report artifact {file_name}: {error}"))?;
    std::fs::write(&output_path, bytes).map_err(|error| {
        format!(
            "failed to write report artifact {}: {error}",
            output_path.display()
        )
    })?;
    Ok(output_path)
}

pub(super) fn write_plan_report_artifact<'a>(
    output_root: &Path,
    report: &NodeCompatPlanReport<'a>,
) -> Result<PathBuf, String> {
    write_json_report_artifact(
        output_root,
        &format!("slice-plan-{}-{}.json", report.family, report.slice),
        report,
    )
}

pub(super) fn write_catalog_plan_report_artifact<'a>(
    output_root: &Path,
    report: &NodeCompatCatalogPlanReport<'a>,
) -> Result<PathBuf, String> {
    write_json_report_artifact(output_root, "catalog-plan.json", report)
}

pub(super) fn write_observed_plan_report_artifact<'a>(
    output_root: &Path,
    report: &NodeCompatObservedPlanReport<'a>,
) -> Result<PathBuf, String> {
    write_json_report_artifact(
        output_root,
        &format!("slice-observed-{}-{}.json", report.family, report.slice),
        report,
    )
}

pub(super) fn write_observed_catalog_report_artifact<'a>(
    output_root: &Path,
    report: &NodeCompatObservedCatalogReport<'a>,
) -> Result<PathBuf, String> {
    write_json_report_artifact(output_root, "catalog-observed.json", report)
}

pub(super) fn build_plan_report<'a>(
    plan: super::node_compat_manifest_catalog::NodeCompatResolvedExecutionPlan<'a>,
) -> NodeCompatPlanReport<'a> {
    let (test_tier, supplementary_category) = slice_fixture_metadata(&plan);
    let lane_summaries: Vec<NodeCompatLanePlanSummary<'a>> = plan
        .lanes
        .iter()
        .map(|lane| NodeCompatLanePlanSummary {
            lane: lane.lane,
            upstream_fixture_line: lane.lane_metadata.upstream_fixture_line.as_str(),
            lane_role: lane_role_label(lane.lane_metadata.lane_role),
            public_contract_role: public_contract_role_label(
                lane.lane_metadata.public_contract_role,
            ),
            runtime_execution_target: lane.lane_metadata.runtime_execution_target.as_str(),
            runtime_limits_preset: lane.lane_metadata.runtime_limits_preset.as_str(),
            subset_test: lane.subset_test,
            fixture_count: lane.fixtures.len(),
            fixture_ids: lane
                .fixtures
                .iter()
                .map(|fixture| fixture.fixture.id.as_str())
                .collect(),
        })
        .collect();
    let unique_fixture_count = plan
        .lanes
        .iter()
        .flat_map(|lane| {
            lane.fixtures
                .iter()
                .map(|fixture| fixture.fixture.id.as_str())
        })
        .collect::<BTreeSet<_>>()
        .len();
    let lane_count = lane_summaries.len();
    let total_lane_fixture_entries = lane_summaries.iter().map(|lane| lane.fixture_count).sum();
    let min_lane_fixture_count = lane_summaries
        .iter()
        .map(|lane| lane.fixture_count)
        .min()
        .unwrap_or(0);
    let max_lane_fixture_count = lane_summaries
        .iter()
        .map(|lane| lane.fixture_count)
        .max()
        .unwrap_or(0);
    let preset_summaries = plan
        .family_catalog
        .presets
        .iter()
        .copied()
        .map(|preset| NodeCompatPresetPlanSummary {
            preset: preset_label(preset),
            unique_fixture_count,
            lane_count,
            total_lane_fixture_entries,
        })
        .collect();

    NodeCompatPlanReport {
        schema_version: NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION,
        family: plan.family_catalog.family.as_str(),
        nlc_item: plan.family_catalog.nlc_item.as_str(),
        slice: plan.slice,
        test_tier,
        supplementary_category,
        execution_class: execution_class_label(plan.family_catalog.execution_class),
        presets: plan
            .family_catalog
            .presets
            .iter()
            .copied()
            .map(preset_label)
            .collect(),
        capabilities: plan
            .family_catalog
            .capabilities
            .iter()
            .copied()
            .map(capability_label)
            .collect(),
        slice_summary: NodeCompatSlicePlanSummary {
            unique_fixture_count,
            lane_count,
            total_lane_fixture_entries,
            min_lane_fixture_count,
            max_lane_fixture_count,
        },
        preset_summaries,
        lane_summaries,
    }
}

pub(super) fn build_catalog_plan_report<'a>(
    resolved: &'a super::node_compat_manifest_catalog::NodeCompatResolvedManifestCatalog,
) -> Result<NodeCompatCatalogPlanReport<'a>, String> {
    let mut slice_reports = Vec::new();
    for family_catalog in &resolved.family_catalogs {
        let slice_names: BTreeSet<&str> = family_catalog
            .fixture_seeds
            .iter()
            .map(|fixture| fixture.slice.as_str())
            .collect();
        for slice_name in slice_names {
            let plan =
                resolved.resolve_lane_execution_plan(family_catalog.family.as_str(), slice_name)?;
            slice_reports.push(build_plan_report(plan));
        }
    }

    let family_count = resolved.family_catalogs.len();
    let slice_count = slice_reports.len();
    let total_unique_fixture_seed_count = slice_reports
        .iter()
        .map(|report| report.slice_summary.unique_fixture_count)
        .sum();
    let total_lane_fixture_entries = slice_reports
        .iter()
        .map(|report| report.slice_summary.total_lane_fixture_entries)
        .sum();
    let preset_ids: BTreeSet<&str> = slice_reports
        .iter()
        .flat_map(|report| report.preset_summaries.iter().map(|preset| preset.preset))
        .collect();
    let preset_summaries = preset_ids
        .into_iter()
        .map(|preset| NodeCompatCatalogPresetPlanSummary {
            preset,
            total_unique_fixture_seed_count: slice_reports
                .iter()
                .flat_map(|report| report.preset_summaries.iter())
                .filter(|summary| summary.preset == preset)
                .map(|summary| summary.unique_fixture_count)
                .sum(),
            total_lane_fixture_entries: slice_reports
                .iter()
                .flat_map(|report| report.preset_summaries.iter())
                .filter(|summary| summary.preset == preset)
                .map(|summary| summary.total_lane_fixture_entries)
                .sum(),
            slice_count_with_entries: slice_reports
                .iter()
                .flat_map(|report| report.preset_summaries.iter())
                .filter(|summary| {
                    summary.preset == preset && summary.total_lane_fixture_entries > 0
                })
                .count(),
        })
        .collect();
    let lane_ids: BTreeSet<&str> = slice_reports
        .iter()
        .flat_map(|report| report.lane_summaries.iter().map(|lane| lane.lane))
        .collect();
    let lane_summaries = lane_ids
        .into_iter()
        .map(|lane| {
            let matching_lane_summaries = slice_reports
                .iter()
                .flat_map(|report| report.lane_summaries.iter())
                .filter(|summary| summary.lane == lane)
                .collect::<Vec<_>>();
            let lane_summary = *matching_lane_summaries
                .first()
                .expect("catalog lane summary should exist");
            NodeCompatCatalogLaneSummary {
                lane,
                upstream_fixture_line: lane_summary.upstream_fixture_line,
                lane_role: lane_summary.lane_role,
                public_contract_role: lane_summary.public_contract_role,
                runtime_execution_target: lane_summary.runtime_execution_target,
                runtime_limits_preset: lane_summary.runtime_limits_preset,
                total_fixture_entries: matching_lane_summaries
                    .iter()
                    .map(|summary| summary.fixture_count)
                    .sum(),
                slice_count_with_entries: matching_lane_summaries
                    .iter()
                    .filter(|summary| summary.fixture_count > 0)
                    .count(),
            }
        })
        .collect();

    Ok(NodeCompatCatalogPlanReport {
        schema_version: NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION,
        family_count,
        slice_count,
        total_unique_fixture_seed_count,
        total_lane_fixture_entries,
        preset_summaries,
        lane_summaries,
        slice_reports,
    })
}

pub(super) fn build_observed_plan_report<'a>(
    plan: &super::node_compat_manifest_catalog::NodeCompatResolvedExecutionPlan<'a>,
    observed_results: &[NodeCompatObservedLaneFixtureResult<'a>],
) -> Result<NodeCompatObservedPlanReport<'a>, String> {
    let (test_tier, supplementary_category) = slice_fixture_metadata(plan);
    let valid_lane_fixture_keys: BTreeSet<(&str, &str)> = plan
        .lanes
        .iter()
        .flat_map(|lane| {
            lane.fixtures
                .iter()
                .map(move |fixture| (lane.lane, fixture.fixture.id.as_str()))
        })
        .collect();
    let mut seen_observed_keys = BTreeSet::new();
    for observed in observed_results {
        let key = (observed.lane, observed.fixture_id);
        if !valid_lane_fixture_keys.contains(&key) {
            return Err(format!(
                "observed result references unknown lane fixture {}:{}",
                observed.lane, observed.fixture_id
            ));
        }
        if !seen_observed_keys.insert(key) {
            return Err(format!(
                "duplicate observed result for lane fixture {}:{}",
                observed.lane, observed.fixture_id
            ));
        }
    }

    let lane_summaries: Vec<NodeCompatObservedLaneSummary<'a>> = plan
        .lanes
        .iter()
        .map(|lane| {
            let mut counts = NodeCompatObservedResultCounts::default();
            let mut observed_fixture_count = 0usize;
            for fixture in &lane.fixtures {
                let state = observed_results
                    .iter()
                    .find(|observed| {
                        observed.lane == lane.lane
                            && observed.fixture_id == fixture.fixture.id.as_str()
                    })
                    .map(|observed| observed.state);
                match state {
                    Some(state) => {
                        observed_fixture_count += 1;
                        increment_observed_counts(&mut counts, state);
                    }
                    None => counts.missing += 1,
                }
            }
            NodeCompatObservedLaneSummary {
                lane: lane.lane,
                upstream_fixture_line: lane.lane_metadata.upstream_fixture_line.as_str(),
                lane_role: lane_role_label(lane.lane_metadata.lane_role),
                public_contract_role: public_contract_role_label(
                    lane.lane_metadata.public_contract_role,
                ),
                runtime_execution_target: lane.lane_metadata.runtime_execution_target.as_str(),
                runtime_limits_preset: lane.lane_metadata.runtime_limits_preset.as_str(),
                subset_test: lane.subset_test,
                expected_fixture_count: lane.fixtures.len(),
                observed_fixture_count,
                counts,
            }
        })
        .collect();

    let unique_fixture_count = plan
        .lanes
        .iter()
        .flat_map(|lane| {
            lane.fixtures
                .iter()
                .map(|fixture| fixture.fixture.id.as_str())
        })
        .collect::<BTreeSet<_>>()
        .len();
    let lane_count = lane_summaries.len();
    let total_expected_results = lane_summaries
        .iter()
        .map(|lane| lane.expected_fixture_count)
        .sum();
    let total_observed_results = lane_summaries
        .iter()
        .map(|lane| lane.observed_fixture_count)
        .sum();
    let counts = lane_summaries.iter().fold(
        NodeCompatObservedResultCounts::default(),
        |mut acc, lane| {
            acc.passed += lane.counts.passed;
            acc.skipped += lane.counts.skipped;
            acc.failed += lane.counts.failed;
            acc.missing += lane.counts.missing;
            acc
        },
    );
    let preset_summaries = plan
        .family_catalog
        .presets
        .iter()
        .copied()
        .map(|preset| NodeCompatObservedPresetSummary {
            preset: preset_label(preset),
            total_expected_results,
            total_observed_results,
            counts,
        })
        .collect();

    Ok(NodeCompatObservedPlanReport {
        schema_version: NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION,
        family: plan.family_catalog.family.as_str(),
        nlc_item: plan.family_catalog.nlc_item.as_str(),
        slice: plan.slice,
        test_tier,
        supplementary_category,
        execution_class: execution_class_label(plan.family_catalog.execution_class),
        presets: plan
            .family_catalog
            .presets
            .iter()
            .copied()
            .map(preset_label)
            .collect(),
        capabilities: plan
            .family_catalog
            .capabilities
            .iter()
            .copied()
            .map(capability_label)
            .collect(),
        slice_summary: NodeCompatObservedSliceSummary {
            unique_fixture_count,
            lane_count,
            total_expected_results,
            total_observed_results,
            counts,
        },
        preset_summaries,
        lane_summaries,
    })
}

pub(super) fn build_observed_catalog_report<'a>(
    resolved: &'a super::node_compat_manifest_catalog::NodeCompatResolvedManifestCatalog,
    observed_inputs: &[NodeCompatObservedSliceInput<'a>],
) -> Result<NodeCompatObservedCatalogReport<'a>, String> {
    let mut seen_slices = BTreeSet::new();
    let mut slice_reports = Vec::new();
    for input in observed_inputs {
        if !seen_slices.insert((input.family, input.slice)) {
            return Err(format!(
                "duplicate observed slice input {}:{}",
                input.family, input.slice
            ));
        }
        let plan = resolved.resolve_lane_execution_plan(input.family, input.slice)?;
        slice_reports.push(build_observed_plan_report(&plan, input.observed_results)?);
    }

    let family_count = slice_reports
        .iter()
        .map(|report| report.family)
        .collect::<BTreeSet<_>>()
        .len();
    let slice_count = slice_reports.len();
    let total_unique_fixture_seed_count = slice_reports
        .iter()
        .map(|report| report.slice_summary.unique_fixture_count)
        .sum();
    let total_expected_results = slice_reports
        .iter()
        .map(|report| report.slice_summary.total_expected_results)
        .sum();
    let total_observed_results = slice_reports
        .iter()
        .map(|report| report.slice_summary.total_observed_results)
        .sum();
    let counts = slice_reports.iter().fold(
        NodeCompatObservedResultCounts::default(),
        |mut acc, report| {
            acc.passed += report.slice_summary.counts.passed;
            acc.skipped += report.slice_summary.counts.skipped;
            acc.failed += report.slice_summary.counts.failed;
            acc.missing += report.slice_summary.counts.missing;
            acc
        },
    );
    let preset_ids: BTreeSet<&str> = slice_reports
        .iter()
        .flat_map(|report| report.preset_summaries.iter().map(|preset| preset.preset))
        .collect();
    let preset_summaries = preset_ids
        .into_iter()
        .map(|preset| {
            let mut preset_counts = NodeCompatObservedResultCounts::default();
            let mut total_expected_results = 0usize;
            let mut total_observed_results = 0usize;
            let mut slice_count_with_entries = 0usize;
            for summary in slice_reports
                .iter()
                .flat_map(|report| report.preset_summaries.iter())
                .filter(|summary| summary.preset == preset)
            {
                total_expected_results += summary.total_expected_results;
                total_observed_results += summary.total_observed_results;
                if summary.total_expected_results > 0 {
                    slice_count_with_entries += 1;
                }
                preset_counts.passed += summary.counts.passed;
                preset_counts.skipped += summary.counts.skipped;
                preset_counts.failed += summary.counts.failed;
                preset_counts.missing += summary.counts.missing;
            }
            NodeCompatObservedCatalogPresetSummary {
                preset,
                total_expected_results,
                total_observed_results,
                slice_count_with_entries,
                counts: preset_counts,
            }
        })
        .collect();
    let lane_ids: BTreeSet<&str> = slice_reports
        .iter()
        .flat_map(|report| report.lane_summaries.iter().map(|lane| lane.lane))
        .collect();
    let lane_summaries = lane_ids
        .into_iter()
        .map(|lane| {
            let mut lane_counts = NodeCompatObservedResultCounts::default();
            let mut total_expected_results = 0usize;
            let mut total_observed_results = 0usize;
            let mut slice_count_with_entries = 0usize;
            let matching_lane_summaries = slice_reports
                .iter()
                .flat_map(|report| report.lane_summaries.iter())
                .filter(|summary| summary.lane == lane)
                .collect::<Vec<_>>();
            let lane_summary = *matching_lane_summaries
                .first()
                .expect("observed catalog lane summary should exist");
            for summary in matching_lane_summaries {
                total_expected_results += summary.expected_fixture_count;
                total_observed_results += summary.observed_fixture_count;
                if summary.expected_fixture_count > 0 {
                    slice_count_with_entries += 1;
                }
                lane_counts.passed += summary.counts.passed;
                lane_counts.skipped += summary.counts.skipped;
                lane_counts.failed += summary.counts.failed;
                lane_counts.missing += summary.counts.missing;
            }
            NodeCompatObservedCatalogLaneSummary {
                lane,
                upstream_fixture_line: lane_summary.upstream_fixture_line,
                lane_role: lane_summary.lane_role,
                public_contract_role: lane_summary.public_contract_role,
                runtime_execution_target: lane_summary.runtime_execution_target,
                runtime_limits_preset: lane_summary.runtime_limits_preset,
                total_expected_results,
                total_observed_results,
                slice_count_with_entries,
                counts: lane_counts,
            }
        })
        .collect();

    Ok(NodeCompatObservedCatalogReport {
        schema_version: NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION,
        family_count,
        slice_count,
        total_unique_fixture_seed_count,
        total_expected_results,
        total_observed_results,
        counts,
        preset_summaries,
        lane_summaries,
        slice_reports,
    })
}

fn default_report_artifact_output_root() -> PathBuf {
    repo_root().join("target/node-compat")
}

fn seeded_slice_report_artifact_root(output_root: &Path, family: &str, slice: &str) -> PathBuf {
    output_root.join(family).join(slice)
}

fn borrow_observed_result_records<'a>(
    records: &'a [NodeCompatObservedLaneFixtureResultRecord],
) -> Vec<NodeCompatObservedLaneFixtureResult<'a>> {
    records
        .iter()
        .map(|record| NodeCompatObservedLaneFixtureResult {
            lane: record.lane.as_str(),
            fixture_id: record.fixture_id.as_str(),
            state: record.state,
        })
        .collect()
}

pub(super) fn read_observed_result_records(
    observed_results_path: &Path,
) -> Result<Vec<NodeCompatObservedLaneFixtureResultRecord>, String> {
    let bytes = std::fs::read(observed_results_path).map_err(|error| {
        format!(
            "failed to read observed results file {}: {error}",
            observed_results_path.display()
        )
    })?;
    serde_json::from_slice::<Vec<NodeCompatObservedLaneFixtureResultRecord>>(&bytes).map_err(
        |error| {
            format!(
                "failed to parse observed results file {}: {error}",
                observed_results_path.display()
            )
        },
    )
}

pub(super) fn emit_slice_report_artifacts_with_observed_results(
    output_root: &Path,
    family: &str,
    slice: &str,
    observed_results: &[NodeCompatObservedLaneFixtureResult<'_>],
) -> Result<NodeCompatSeededSliceReportArtifactBundle, String> {
    let resolved = load_family_catalogs_from_disk();
    let slice_plan = resolved.resolve_lane_execution_plan(family, slice)?;
    let slice_plan_report = build_plan_report(slice_plan);
    let slice_observed = resolved.resolve_lane_execution_plan(family, slice)?;
    let slice_observed_report = build_observed_plan_report(&slice_observed, observed_results)?;
    let catalog_plan_report = build_catalog_plan_report(&resolved)?;
    let catalog_observed_report = build_observed_catalog_report(
        &resolved,
        &[NodeCompatObservedSliceInput {
            family,
            slice,
            observed_results,
        }],
    )?;
    let artifact_root = seeded_slice_report_artifact_root(output_root, family, slice);
    let slice_plan_path = write_plan_report_artifact(&artifact_root, &slice_plan_report)?;
    let slice_observed_path =
        write_observed_plan_report_artifact(&artifact_root, &slice_observed_report)?;
    let catalog_plan_path =
        write_catalog_plan_report_artifact(&artifact_root, &catalog_plan_report)?;
    let catalog_observed_path =
        write_observed_catalog_report_artifact(&artifact_root, &catalog_observed_report)?;

    Ok(NodeCompatSeededSliceReportArtifactBundle {
        artifact_root,
        slice_plan_path,
        slice_observed_path,
        catalog_plan_path,
        catalog_observed_path,
    })
}

pub(super) fn emit_seeded_slice_report_artifacts(
    output_root: &Path,
    family: &str,
    slice: &str,
) -> Result<NodeCompatSeededSliceReportArtifactBundle, String> {
    emit_slice_report_artifacts_with_observed_results(output_root, family, slice, &[])
}

pub(super) fn emit_live_seeded_slice_report_artifacts(
    output_root: &Path,
    family: &str,
    slice: &str,
) -> Result<NodeCompatSeededSliceReportArtifactBundle, String> {
    let observed_result_records =
        super::node_compat::collect_seeded_slice_observed_result_records(family, slice)?;
    let observed_results = borrow_observed_result_records(&observed_result_records);
    emit_slice_report_artifacts_with_observed_results(output_root, family, slice, &observed_results)
}

fn unique_report_artifact_test_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("current time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("neovex-node-compat-report-{nanos}"))
}

#[test]
fn node_compat_manifest_report_schema_is_versioned_and_serializes_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let report = build_plan_report(plan);
    let json: Value = serde_json::to_value(&report).expect("plan report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["family"], "networking");
    assert_eq!(json["nlc_item"], "NLC6");
    assert_eq!(json["slice"], "dns-net-foundation");
    assert_eq!(json["execution_class"], "sequential");
    assert_eq!(json["presets"], serde_json::json!(["Application"]));
    assert_eq!(
        json["capabilities"],
        serde_json::json!(["bundle-root-fs", "loopback-net"])
    );
    assert_eq!(json["slice_summary"]["unique_fixture_count"], 10);
    assert_eq!(json["slice_summary"]["lane_count"], 3);
    assert_eq!(json["slice_summary"]["total_lane_fixture_entries"], 29);
    assert_eq!(json["slice_summary"]["min_lane_fixture_count"], 9);
    assert_eq!(json["slice_summary"]["max_lane_fixture_count"], 10);
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([{
            "preset": "Application",
            "unique_fixture_count": 10,
            "lane_count": 3,
            "total_lane_fixture_entries": 29
        }])
    );

    let lane_summaries = json["lane_summaries"]
        .as_array()
        .expect("lane summaries should serialize as an array");
    assert_eq!(lane_summaries.len(), 3);
    assert_eq!(lane_summaries[0]["lane"], "node20");
    assert_eq!(lane_summaries[0]["upstream_fixture_line"], "Node20");
    assert_eq!(lane_summaries[0]["lane_role"], "supported");
    assert_eq!(
        lane_summaries[0]["public_contract_role"],
        "supported_contract"
    );
    assert_eq!(lane_summaries[1]["lane"], "node22");
    assert_eq!(lane_summaries[1]["lane_role"], "default");
    assert_eq!(lane_summaries[2]["lane"], "node24");
    assert_eq!(
        lane_summaries[2]["public_contract_role"],
        "supported_contract"
    );
}

#[test]
fn node_compat_manifest_report_summarizes_lane_slice_and_preset_counts_from_execution_plan() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let report = build_plan_report(plan);

    assert_eq!(report.slice_summary.unique_fixture_count, 10);
    assert_eq!(report.slice_summary.lane_count, 3);
    assert_eq!(report.slice_summary.total_lane_fixture_entries, 29);
    assert_eq!(report.slice_summary.min_lane_fixture_count, 9);
    assert_eq!(report.slice_summary.max_lane_fixture_count, 10);
    assert_eq!(report.preset_summaries.len(), 1);
    assert_eq!(report.preset_summaries[0].preset, "Application");
    assert_eq!(report.preset_summaries[0].unique_fixture_count, 10);
    assert_eq!(report.preset_summaries[0].lane_count, 3);
    assert_eq!(report.preset_summaries[0].total_lane_fixture_entries, 29);
    assert_eq!(report.lane_summaries.len(), 3);
    assert_eq!(report.lane_summaries[0].upstream_fixture_line, "Node20");
    assert_eq!(report.lane_summaries[0].lane_role, "supported");
    assert_eq!(
        report.lane_summaries[1].public_contract_role,
        "default_contract"
    );
    assert_eq!(report.lane_summaries[0].fixture_count, 10);
    assert_eq!(report.lane_summaries[1].fixture_count, 10);
    assert_eq!(report.lane_summaries[2].fixture_count, 9);
    assert_eq!(
        report.lane_summaries[0].fixture_ids[0],
        "test/parallel/test-dns-get-server.js"
    );
    assert_eq!(
        report.lane_summaries[1].fixture_ids[6],
        "test/parallel/test-stream-pipeline.js"
    );
    assert!(
        !report.lane_summaries[2]
            .fixture_ids
            .contains(&"test/parallel/test-stream-pipeline.js"),
        "report should preserve explicit lane omissions instead of flattening them",
    );
}

#[test]
fn node_compat_manifest_report_builds_catalog_summary_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let report = build_catalog_plan_report(&resolved).expect("catalog plan report should resolve");
    let json: Value = serde_json::to_value(&report).expect("catalog plan report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["family_count"], 11);
    assert_eq!(json["slice_count"], 12);
    assert_eq!(json["total_unique_fixture_seed_count"], 57);
    assert_eq!(json["total_lane_fixture_entries"], 167);
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_unique_fixture_seed_count": 57,
                "total_lane_fixture_entries": 167,
                "slice_count_with_entries": 12
            }
        ])
    );
    assert_eq!(
        json["lane_summaries"],
        serde_json::json!([
            {
                "lane": "node20",
                "upstream_fixture_line": "Node20",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node20",
                "runtime_limits_preset": "application_node20",
                "total_fixture_entries": 54,
                "slice_count_with_entries": 12
            },
            {
                "lane": "node22",
                "upstream_fixture_line": "Node22",
                "lane_role": "default",
                "public_contract_role": "default_contract",
                "runtime_execution_target": "Node22",
                "runtime_limits_preset": "application_node22",
                "total_fixture_entries": 57,
                "slice_count_with_entries": 12
            },
            {
                "lane": "node24",
                "upstream_fixture_line": "Node24",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node24",
                "runtime_limits_preset": "application_node24",
                "total_fixture_entries": 56,
                "slice_count_with_entries": 12
            }
        ])
    );
    let slice_reports = json["slice_reports"]
        .as_array()
        .expect("catalog slice reports should serialize as an array");
    assert_eq!(slice_reports.len(), 12);
    assert_eq!(slice_reports[0]["family"], "core-semantics");
    assert_eq!(slice_reports[11]["family"], "streams-and-local-io");
}

#[test]
fn node_compat_manifest_report_aggregates_observed_results_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let observed_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-set-default-order.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
    ];
    let report = build_observed_plan_report(&plan, &observed_results)
        .expect("observed plan report should build");
    let json: Value = serde_json::to_value(&report).expect("observed plan report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["slice_summary"]["unique_fixture_count"], 10);
    assert_eq!(json["slice_summary"]["lane_count"], 3);
    assert_eq!(json["slice_summary"]["total_expected_results"], 29);
    assert_eq!(json["slice_summary"]["total_observed_results"], 4);
    assert_eq!(
        json["slice_summary"]["counts"],
        serde_json::json!({
            "passed": 2,
            "skipped": 1,
            "failed": 1,
            "missing": 25
        })
    );
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([{
            "preset": "Application",
            "total_expected_results": 29,
            "total_observed_results": 4,
            "counts": {
                "passed": 2,
                "skipped": 1,
                "failed": 1,
                "missing": 25
            }
        }])
    );
    assert_eq!(
        json["lane_summaries"],
        serde_json::json!([
            {
                "lane": "node20",
                "upstream_fixture_line": "Node20",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node20",
                "runtime_limits_preset": "application_node20",
                "subset_test": "runtime::tests::node_compat::node20_supported_lane_executes_official_networking_subset",
                "expected_fixture_count": 10,
                "observed_fixture_count": 2,
                "counts": {
                    "passed": 1,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 8
                }
            },
            {
                "lane": "node22",
                "upstream_fixture_line": "Node22",
                "lane_role": "default",
                "public_contract_role": "default_contract",
                "runtime_execution_target": "Node22",
                "runtime_limits_preset": "application_node22",
                "subset_test": "runtime::tests::node_compat::node22_default_lane_executes_manifested_networking_subset",
                "expected_fixture_count": 10,
                "observed_fixture_count": 1,
                "counts": {
                    "passed": 0,
                    "skipped": 1,
                    "failed": 0,
                    "missing": 9
                }
            },
            {
                "lane": "node24",
                "upstream_fixture_line": "Node24",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node24",
                "runtime_limits_preset": "application_node24",
                "subset_test": "runtime::tests::node_compat::node24_supported_lane_networking_watchpoint",
                "expected_fixture_count": 9,
                "observed_fixture_count": 1,
                "counts": {
                    "passed": 1,
                    "skipped": 0,
                    "failed": 0,
                    "missing": 8
                }
            }
        ])
    );
}

#[test]
fn node_compat_manifest_report_rejects_unknown_or_duplicate_observed_results() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");

    let duplicate_error = build_observed_plan_report(
        &plan,
        &[
            NodeCompatObservedLaneFixtureResult {
                lane: "node20",
                fixture_id: "test/parallel/test-dns-get-server.js",
                state: NodeCompatObservedFixtureState::Pass,
            },
            NodeCompatObservedLaneFixtureResult {
                lane: "node20",
                fixture_id: "test/parallel/test-dns-get-server.js",
                state: NodeCompatObservedFixtureState::Fail,
            },
        ],
    )
    .expect_err("duplicate observed results should fail");
    assert!(
        duplicate_error.contains(
            "duplicate observed result for lane fixture node20:test/parallel/test-dns-get-server.js"
        ),
        "duplicate observed result error should mention the duplicate lane fixture: {duplicate_error}",
    );

    let unknown_error = build_observed_plan_report(
        &plan,
        &[NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-stream-pipeline.js",
            state: NodeCompatObservedFixtureState::Pass,
        }],
    )
    .expect_err("unknown lane fixture result should fail");
    assert!(
        unknown_error.contains(
            "observed result references unknown lane fixture node24:test/parallel/test-stream-pipeline.js"
        ),
        "unknown observed result error should mention the offending lane fixture: {unknown_error}",
    );
}

#[test]
fn node_compat_manifest_report_aggregates_observed_catalog_results_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let networking_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-set-default-order.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
    ];
    let process_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-process-default.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
    ];
    let report = build_observed_catalog_report(
        &resolved,
        &[
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &networking_results,
            },
            NodeCompatObservedSliceInput {
                family: "process-and-timing",
                slice: "process-foundation",
                observed_results: &process_results,
            },
        ],
    )
    .expect("observed catalog report should build");
    let json: Value =
        serde_json::to_value(&report).expect("observed catalog report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["family_count"], 2);
    assert_eq!(json["slice_count"], 2);
    assert_eq!(json["total_unique_fixture_seed_count"], 20);
    assert_eq!(json["total_expected_results"], 58);
    assert_eq!(json["total_observed_results"], 7);
    assert_eq!(
        json["counts"],
        serde_json::json!({
            "passed": 4,
            "skipped": 1,
            "failed": 2,
            "missing": 51
        })
    );
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_expected_results": 58,
                "total_observed_results": 7,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 4,
                    "skipped": 1,
                    "failed": 2,
                    "missing": 51
                }
            }
        ])
    );
    assert_eq!(
        json["lane_summaries"],
        serde_json::json!([
            {
                "lane": "node20",
                "upstream_fixture_line": "Node20",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node20",
                "runtime_limits_preset": "application_node20",
                "total_expected_results": 19,
                "total_observed_results": 3,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 2,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 16
                }
            },
            {
                "lane": "node22",
                "upstream_fixture_line": "Node22",
                "lane_role": "default",
                "public_contract_role": "default_contract",
                "runtime_execution_target": "Node22",
                "runtime_limits_preset": "application_node22",
                "total_expected_results": 20,
                "total_observed_results": 2,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 1,
                    "skipped": 1,
                    "failed": 0,
                    "missing": 18
                }
            },
            {
                "lane": "node24",
                "upstream_fixture_line": "Node24",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node24",
                "runtime_limits_preset": "application_node24",
                "total_expected_results": 19,
                "total_observed_results": 2,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 1,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 17
                }
            }
        ])
    );
    let slice_reports = json["slice_reports"]
        .as_array()
        .expect("observed catalog slice reports should serialize as an array");
    assert_eq!(slice_reports.len(), 2);
    assert_eq!(slice_reports[0]["family"], "networking");
    assert_eq!(slice_reports[1]["family"], "process-and-timing");
}

#[test]
fn node_compat_manifest_report_supports_multi_preset_catalog_summaries() {
    let mut resolved = load_family_catalogs_from_disk();
    let process_family = resolved
        .family_catalogs
        .iter_mut()
        .find(|catalog| catalog.family == "process-and-timing")
        .expect("process-and-timing family catalog should exist");
    process_family.presets.push(NodeCompatPreset::Tooling);

    let plan_report =
        build_catalog_plan_report(&resolved).expect("catalog plan report should resolve");
    let plan_json: Value =
        serde_json::to_value(&plan_report).expect("catalog plan report should serialize");
    assert_eq!(
        plan_json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_unique_fixture_seed_count": 57,
                "total_lane_fixture_entries": 167,
                "slice_count_with_entries": 12
            },
            {
                "preset": "Tooling",
                "total_unique_fixture_seed_count": 10,
                "total_lane_fixture_entries": 29,
                "slice_count_with_entries": 1
            }
        ])
    );

    let networking_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-set-default-order.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
    ];
    let process_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-process-default.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
    ];
    let observed_report = build_observed_catalog_report(
        &resolved,
        &[
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &networking_results,
            },
            NodeCompatObservedSliceInput {
                family: "process-and-timing",
                slice: "process-foundation",
                observed_results: &process_results,
            },
        ],
    )
    .expect("observed catalog report should build");
    let observed_json: Value =
        serde_json::to_value(&observed_report).expect("observed catalog report should serialize");
    assert_eq!(
        observed_json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_expected_results": 58,
                "total_observed_results": 7,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 4,
                    "skipped": 1,
                    "failed": 2,
                    "missing": 51
                }
            },
            {
                "preset": "Tooling",
                "total_expected_results": 29,
                "total_observed_results": 3,
                "slice_count_with_entries": 1,
                "counts": {
                    "passed": 2,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 26
                }
            }
        ])
    );
}

#[test]
fn node_compat_manifest_report_writes_artifacts_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let slice_plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let slice_report = build_plan_report(slice_plan);
    let catalog_plan_report =
        build_catalog_plan_report(&resolved).expect("catalog plan report should resolve");
    let networking_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
    ];
    let observed_catalog_report = build_observed_catalog_report(
        &resolved,
        &[NodeCompatObservedSliceInput {
            family: "networking",
            slice: "dns-net-foundation",
            observed_results: &networking_results,
        }],
    )
    .expect("observed catalog report should resolve");
    let output_root = unique_report_artifact_test_root();

    let slice_path = write_plan_report_artifact(&output_root, &slice_report)
        .expect("slice plan artifact should write");
    let catalog_plan_path = write_catalog_plan_report_artifact(&output_root, &catalog_plan_report)
        .expect("catalog plan artifact should write");
    let observed_catalog_path =
        write_observed_catalog_report_artifact(&output_root, &observed_catalog_report)
            .expect("observed catalog artifact should write");

    assert_eq!(
        slice_path.file_name().and_then(|name| name.to_str()),
        Some("slice-plan-networking-dns-net-foundation.json")
    );
    assert_eq!(
        catalog_plan_path.file_name().and_then(|name| name.to_str()),
        Some("catalog-plan.json")
    );
    assert_eq!(
        observed_catalog_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("catalog-observed.json")
    );

    let slice_json: Value = serde_json::from_slice(
        &std::fs::read(&slice_path).expect("slice artifact should be readable"),
    )
    .expect("slice artifact should parse as json");
    let catalog_plan_json: Value = serde_json::from_slice(
        &std::fs::read(&catalog_plan_path).expect("catalog plan artifact should be readable"),
    )
    .expect("catalog plan artifact should parse as json");
    let observed_catalog_json: Value = serde_json::from_slice(
        &std::fs::read(&observed_catalog_path)
            .expect("observed catalog artifact should be readable"),
    )
    .expect("observed catalog artifact should parse as json");

    assert_eq!(slice_json["family"], "networking");
    assert_eq!(slice_json["slice"], "dns-net-foundation");
    assert_eq!(catalog_plan_json["family_count"], 11);
    assert_eq!(
        catalog_plan_json["preset_summaries"][0]["preset"],
        "Application"
    );
    assert_eq!(observed_catalog_json["slice_count"], 1);
    assert_eq!(
        observed_catalog_json["counts"],
        serde_json::json!({
            "passed": 1,
            "skipped": 1,
            "failed": 0,
            "missing": 27
        })
    );

    std::fs::remove_dir_all(&output_root).expect("temporary report artifact root should clean up");
}

#[test]
fn node_compat_manifest_report_emits_seeded_slice_artifact_bundle_under_stable_root() {
    let output_root = unique_report_artifact_test_root();
    let artifacts =
        emit_seeded_slice_report_artifacts(&output_root, "networking", "dns-net-foundation")
            .expect("seeded slice artifact bundle should emit");

    assert_eq!(
        artifacts.artifact_root,
        output_root.join("networking").join("dns-net-foundation")
    );
    assert_eq!(
        artifacts
            .slice_plan_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("slice-plan-networking-dns-net-foundation.json")
    );
    assert_eq!(
        artifacts
            .slice_observed_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("slice-observed-networking-dns-net-foundation.json")
    );
    assert_eq!(
        artifacts
            .catalog_plan_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("catalog-plan.json")
    );
    assert_eq!(
        artifacts
            .catalog_observed_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("catalog-observed.json")
    );

    let slice_plan_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.slice_plan_path).expect("slice plan artifact should be readable"),
    )
    .expect("slice plan artifact should parse as json");
    let slice_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.slice_observed_path)
            .expect("slice observed artifact should be readable"),
    )
    .expect("slice observed artifact should parse as json");
    let catalog_plan_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.catalog_plan_path)
            .expect("catalog plan artifact should be readable"),
    )
    .expect("catalog plan artifact should parse as json");
    let catalog_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.catalog_observed_path)
            .expect("catalog observed artifact should be readable"),
    )
    .expect("catalog observed artifact should parse as json");

    assert_eq!(slice_plan_json["family"], "networking");
    assert_eq!(slice_observed_json["slice"], "dns-net-foundation");
    assert_eq!(
        slice_observed_json["slice_summary"]["counts"]["missing"],
        29
    );
    assert_eq!(catalog_plan_json["family_count"], 11);
    assert_eq!(catalog_observed_json["slice_count"], 1);
    assert_eq!(catalog_observed_json["counts"]["missing"], 29);

    std::fs::remove_dir_all(&output_root).expect("temporary seeded artifact root should clean up");
}

#[test]
#[ignore = "manual harness report artifact entrypoint"]
fn node_compat_manifest_report_entrypoint_emits_slice_artifacts() {
    let family = std::env::var("NEOVEX_NODE_COMPAT_REPORT_FAMILY")
        .expect("NEOVEX_NODE_COMPAT_REPORT_FAMILY should be set");
    let slice = std::env::var("NEOVEX_NODE_COMPAT_REPORT_SLICE")
        .expect("NEOVEX_NODE_COMPAT_REPORT_SLICE should be set");
    let output_root = std::env::var("NEOVEX_NODE_COMPAT_REPORT_OUTPUT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_report_artifact_output_root());
    let capture_mode = std::env::var("NEOVEX_NODE_COMPAT_REPORT_CAPTURE_MODE")
        .unwrap_or_else(|_| "seeded".to_string());
    let observed_results_path = std::env::var("NEOVEX_NODE_COMPAT_REPORT_OBSERVED_RESULTS")
        .ok()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let artifacts = match capture_mode.as_str() {
        "seeded" => {
            let observed_result_records = observed_results_path
                .as_ref()
                .map(|path| read_observed_result_records(path))
                .transpose()
                .expect("observed results path should parse when present")
                .unwrap_or_default();
            let observed_results = borrow_observed_result_records(&observed_result_records);
            emit_slice_report_artifacts_with_observed_results(
                &output_root,
                &family,
                &slice,
                &observed_results,
            )
            .expect("seeded slice artifacts should emit from manual entrypoint")
        }
        "live" => {
            assert!(
                observed_results_path.is_none(),
                "live capture mode cannot also consume NEOVEX_NODE_COMPAT_REPORT_OBSERVED_RESULTS",
            );
            emit_live_seeded_slice_report_artifacts(&output_root, &family, &slice)
                .expect("live slice artifacts should emit from manual entrypoint")
        }
        other => panic!("unsupported node-compat report capture mode `{other}`"),
    };

    println!("artifact_root={}", artifacts.artifact_root.display());
    println!("slice_plan={}", artifacts.slice_plan_path.display());
    println!("slice_observed={}", artifacts.slice_observed_path.display());
    println!("catalog_plan={}", artifacts.catalog_plan_path.display());
    println!(
        "catalog_observed={}",
        artifacts.catalog_observed_path.display()
    );
}

#[test]
fn node_compat_manifest_report_emits_observed_results_from_json_input() {
    let output_root = unique_report_artifact_test_root();
    let observed_results_path = output_root.join("observed-results.json");
    std::fs::create_dir_all(&output_root).expect("temporary observed-results root should create");
    std::fs::write(
        &observed_results_path,
        serde_json::to_vec_pretty(&serde_json::json!([
            {
                "lane": "node20",
                "fixture_id": "test/parallel/test-dns-get-server.js",
                "state": "pass"
            },
            {
                "lane": "node22",
                "fixture_id": "test/parallel/test-dns-get-server.js",
                "state": "skip"
            }
        ]))
        .expect("observed results fixture should serialize"),
    )
    .expect("observed results fixture should write");
    let observed_result_records = read_observed_result_records(&observed_results_path)
        .expect("observed results file should parse");
    let observed_results = borrow_observed_result_records(&observed_result_records);
    let artifacts = emit_slice_report_artifacts_with_observed_results(
        &output_root,
        "networking",
        "dns-net-foundation",
        &observed_results,
    )
    .expect("observed artifact bundle should emit");

    let slice_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.slice_observed_path)
            .expect("slice observed artifact should be readable"),
    )
    .expect("slice observed artifact should parse as json");
    let catalog_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.catalog_observed_path)
            .expect("catalog observed artifact should be readable"),
    )
    .expect("catalog observed artifact should parse as json");

    assert_eq!(
        slice_observed_json["slice_summary"]["total_observed_results"],
        2
    );
    assert_eq!(
        slice_observed_json["slice_summary"]["counts"],
        serde_json::json!({
            "passed": 1,
            "skipped": 1,
            "failed": 0,
            "missing": 27
        })
    );
    assert_eq!(catalog_observed_json["slice_count"], 1);
    assert_eq!(catalog_observed_json["total_observed_results"], 2);

    std::fs::remove_dir_all(&output_root)
        .expect("temporary observed-results artifact root should clean up");
}

#[test]
fn node_compat_manifest_report_rejects_duplicate_observed_catalog_slice_inputs() {
    let resolved = load_family_catalogs_from_disk();
    let error = build_observed_catalog_report(
        &resolved,
        &[
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &[],
            },
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &[],
            },
        ],
    )
    .expect_err("duplicate observed slice inputs should fail");
    assert!(
        error.contains("duplicate observed slice input networking:dns-net-foundation"),
        "duplicate observed slice input error should mention the duplicate key: {error}",
    );
}
