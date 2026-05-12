use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::node_compat::{
    NodeCompatMaterializedSeededFixtureBundle, materialize_seeded_fixture_bundle_for_lane,
    observe_seeded_fixture_runtime_outcome,
};
use super::node_compat_manifest_catalog::{
    NodeCompatLaneRole, NodeCompatPublicContractRole, load_family_catalogs_from_disk, repo_root,
};
use super::node_compat_manifest_report::NodeCompatObservedFixtureState;

const NODE_COMPAT_ORACLE_SCHEMA_VERSION: u32 = 1;
const NODE_COMPAT_PROCESS_EXIT_SENTINEL_MARKER: &str = "__NEOVEX_NODE_COMPAT_PROCESS_EXIT__";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatOracleState {
    Pass,
    Skip,
    Fail,
    HarnessMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeCompatOracleDriftClass {
    AgreementPass,
    AgreementSkip,
    OraclePassRuntimeFail,
    OraclePassRuntimeSkip,
    OracleSkipRuntimeRun,
    OracleFailBoth,
    OracleFailRuntimePass,
    OracleFailRuntimeSkip,
    OracleHarnessMismatch,
}

#[derive(Debug, Serialize)]
struct NodeCompatOracleArtifact {
    schema_version: u32,
    family: String,
    slice: String,
    lane: String,
    upstream_fixture_line: String,
    lane_role: String,
    public_contract_role: String,
    runtime_execution_target: String,
    runtime_limits_profile: String,
    fixture: String,
    fixture_source_path: String,
    node_bin: String,
    node_version: String,
    startup_flags: Vec<String>,
    runtime_state: NodeCompatObservedFixtureState,
    runtime_detail: Option<String>,
    oracle_state: NodeCompatOracleState,
    oracle_error: Option<String>,
    drift_class: NodeCompatOracleDriftClass,
    oracle_stdout: String,
    oracle_stderr: String,
}

#[derive(Debug, Deserialize)]
struct NodeCompatOracleRunnerResult {
    state: NodeCompatOracleState,
    error: Option<String>,
}

#[derive(Debug)]
struct NodeCompatOracleNodeBinary {
    path: PathBuf,
    version: String,
}

fn default_oracle_output_root() -> PathBuf {
    repo_root().join("target/node-compat/oracle")
}

fn lane_role_label(role: NodeCompatLaneRole) -> &'static str {
    match role {
        NodeCompatLaneRole::Primary => "primary",
        NodeCompatLaneRole::Validation => "validation",
        NodeCompatLaneRole::Preview => "preview",
    }
}

fn public_contract_role_label(role: NodeCompatPublicContractRole) -> &'static str {
    match role {
        NodeCompatPublicContractRole::PrimaryContract => "primary_contract",
        NodeCompatPublicContractRole::MeasuredValidationLane => "measured_validation_lane",
        NodeCompatPublicContractRole::PreviewVisibilityLane => "preview_visibility_lane",
    }
}

fn fixture_slug(fixture: &str) -> String {
    fixture
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' => character,
            _ => '-',
        })
        .collect()
}

fn artifact_path(output_root: &Path, lane: &str, fixture: &str) -> PathBuf {
    let slug = fixture_slug(fixture);
    output_root
        .join(lane)
        .join(&slug)
        .join(format!("oracle-{lane}-{slug}.json"))
}

fn node_env_var_for_lane(lane: &str) -> Result<&'static str, String> {
    match lane {
        "node20" => Ok("NEOVEX_NODE20_BIN"),
        "node22" => Ok("NEOVEX_NODE22_BIN"),
        "node24" => Ok("NEOVEX_NODE24_BIN"),
        other => Err(format!("unsupported oracle lane `{other}`")),
    }
}

fn expected_node_major_for_lane(lane: &str) -> Result<u32, String> {
    match lane {
        "node20" => Ok(20),
        "node22" => Ok(22),
        "node24" => Ok(24),
        other => Err(format!("unsupported oracle lane `{other}`")),
    }
}

fn parse_node_major(version: &str) -> Result<u32, String> {
    let version = version.trim();
    let Some(version) = version.strip_prefix('v') else {
        return Err(format!(
            "node version output must start with `v`: {version}"
        ));
    };
    let major = version
        .split('.')
        .next()
        .ok_or_else(|| format!("node version output is missing a major version: {version}"))?;
    major
        .parse::<u32>()
        .map_err(|error| format!("failed to parse node major version `{major}`: {error}"))
}

fn validate_node_binary_version(
    lane: &str,
    node_bin: PathBuf,
    version_output: &str,
) -> Result<NodeCompatOracleNodeBinary, String> {
    let expected_major = expected_node_major_for_lane(lane)?;
    let actual_major = parse_node_major(version_output)?;
    if actual_major != expected_major {
        return Err(format!(
            "oracle lane `{lane}` requires a Node {expected_major} binary, but `{}` reported `{}`",
            node_bin.display(),
            version_output.trim()
        ));
    }
    Ok(NodeCompatOracleNodeBinary {
        path: node_bin,
        version: version_output.trim().to_string(),
    })
}

fn resolve_node_binary(
    lane: &str,
    override_path: Option<PathBuf>,
) -> Result<NodeCompatOracleNodeBinary, String> {
    let node_bin = match override_path {
        Some(path) => path,
        None => {
            let env_var = node_env_var_for_lane(lane)?;
            let value = std::env::var(env_var).map_err(|_| {
                format!(
                    "oracle lane `{lane}` requires a version-matched Node binary; set {env_var} or pass --node-bin"
                )
            })?;
            PathBuf::from(value)
        }
    };
    let output = Command::new(&node_bin)
        .arg("--version")
        .output()
        .map_err(|error| {
            format!(
                "failed to execute oracle node binary `{}`: {error}",
                node_bin.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "oracle node binary `{}` did not return a version successfully: {}",
            node_bin.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let version_output = String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "oracle node binary `{}` returned invalid UTF-8 version output: {error}",
            node_bin.display()
        )
    })?;
    validate_node_binary_version(lane, node_bin, &version_output)
}

fn classify_harness_mismatch(rendered_error: &str) -> bool {
    [
        "__neovexSyncHostValue",
        "__neovexAsyncHostValue",
        "Neovex node_compat harness is missing",
        "op_neovex_runtime_test_",
    ]
    .iter()
    .any(|marker| rendered_error.contains(marker))
}

fn process_exit_code_from_error(rendered_error: &str) -> Option<i32> {
    let marker = format!("{NODE_COMPAT_PROCESS_EXIT_SENTINEL_MARKER}:");
    let (_, remainder) = rendered_error.split_once(&marker)?;
    let numeric_prefix: String = remainder
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '-')
        .collect();
    if numeric_prefix.is_empty() {
        return None;
    }
    numeric_prefix.parse::<i32>().ok()
}

fn classify_oracle_drift(
    runtime_state: NodeCompatObservedFixtureState,
    oracle_state: NodeCompatOracleState,
) -> NodeCompatOracleDriftClass {
    match (runtime_state, oracle_state) {
        (_, NodeCompatOracleState::HarnessMismatch) => {
            NodeCompatOracleDriftClass::OracleHarnessMismatch
        }
        (NodeCompatObservedFixtureState::Pass, NodeCompatOracleState::Pass) => {
            NodeCompatOracleDriftClass::AgreementPass
        }
        (NodeCompatObservedFixtureState::Skip, NodeCompatOracleState::Skip) => {
            NodeCompatOracleDriftClass::AgreementSkip
        }
        (NodeCompatObservedFixtureState::Fail, NodeCompatOracleState::Pass) => {
            NodeCompatOracleDriftClass::OraclePassRuntimeFail
        }
        (NodeCompatObservedFixtureState::Skip, NodeCompatOracleState::Pass) => {
            NodeCompatOracleDriftClass::OraclePassRuntimeSkip
        }
        (NodeCompatObservedFixtureState::Pass, NodeCompatOracleState::Skip)
        | (NodeCompatObservedFixtureState::Fail, NodeCompatOracleState::Skip) => {
            NodeCompatOracleDriftClass::OracleSkipRuntimeRun
        }
        (NodeCompatObservedFixtureState::Fail, NodeCompatOracleState::Fail) => {
            NodeCompatOracleDriftClass::OracleFailBoth
        }
        (NodeCompatObservedFixtureState::Pass, NodeCompatOracleState::Fail) => {
            NodeCompatOracleDriftClass::OracleFailRuntimePass
        }
        (NodeCompatObservedFixtureState::Skip, NodeCompatOracleState::Fail) => {
            NodeCompatOracleDriftClass::OracleFailRuntimeSkip
        }
    }
}

fn write_oracle_runner_script(
    bundle: &NodeCompatMaterializedSeededFixtureBundle,
    result_path: &Path,
) -> Result<PathBuf, String> {
    let runner_path = bundle
        .bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("oracle-runner.mjs");
    let result_path = result_path.to_string_lossy();
    let script = format!(
        r#"
import fs from "node:fs";

const resultPath = {result_path:?};
const processExitMarker = "{NODE_COMPAT_PROCESS_EXIT_SENTINEL_MARKER}:";

function classifyHarnessMismatch(renderedError) {{
  return [
    "__neovexSyncHostValue",
    "__neovexAsyncHostValue",
    "Neovex node_compat harness is missing",
    "op_neovex_runtime_test_",
  ].some((marker) => renderedError.includes(marker));
}}

function processExitCodeFromError(renderedError) {{
  const index = renderedError.indexOf(processExitMarker);
  if (index === -1) {{
    return null;
  }}
  const remainder = renderedError.slice(index + processExitMarker.length);
  const numericPrefix = remainder.match(/^-?\d+/u)?.[0];
  if (!numericPrefix) {{
    return null;
  }}
  return Number.parseInt(numericPrefix, 10);
}}

function writeResult(state, error = null) {{
  fs.writeFileSync(resultPath, JSON.stringify({{ state, error }}), "utf8");
}}

try {{
  await import("./bundle.mjs");
  const result = await globalThis.__neovexInvoke();
  writeResult(result?.skipped === true ? "skip" : "pass");
}} catch (error) {{
  const renderedError =
    typeof error?.stack === "string" ? error.stack : String(error);
  const processExitCode = processExitCodeFromError(renderedError);
  if (processExitCode === 0) {{
    writeResult("pass");
  }} else if (error?.code === "NEOVEX_NODE_COMPAT_SKIP" || error?.__neovexSkip === true) {{
    writeResult("skip", renderedError);
  }} else if (classifyHarnessMismatch(renderedError)) {{
    writeResult("harness_mismatch", renderedError);
  }} else {{
    writeResult("fail", renderedError);
  }}
}}
"#
    );
    std::fs::write(&runner_path, script).map_err(|error| {
        format!(
            "failed to write oracle runner script {}: {error}",
            runner_path.display()
        )
    })?;
    Ok(runner_path)
}

fn read_runner_result(result_path: &Path) -> Result<NodeCompatOracleRunnerResult, String> {
    let bytes = std::fs::read(result_path).map_err(|error| {
        format!(
            "failed to read oracle runner result {}: {error}",
            result_path.display()
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "failed to parse oracle runner result {}: {error}",
            result_path.display()
        )
    })
}

fn emit_oracle_artifact(
    output_root: &Path,
    lane: &str,
    fixture: &str,
    node_bin_override: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let runtime_outcome = observe_seeded_fixture_runtime_outcome(lane, fixture)?;
    let bundle = materialize_seeded_fixture_bundle_for_lane(lane, fixture)?;
    let resolved = load_family_catalogs_from_disk();
    let lane_metadata = resolved
        .lane_metadata(lane)
        .ok_or_else(|| format!("missing lane metadata for oracle lane `{lane}`"))?;
    let node_binary = resolve_node_binary(lane, node_bin_override)?;
    let oracle_result_path = bundle
        .bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("oracle-result.json");
    let runner_path = write_oracle_runner_script(&bundle, &oracle_result_path)?;
    let output = Command::new(&node_binary.path)
        .args(&bundle.startup_flags)
        .arg(&runner_path)
        .current_dir(
            bundle
                .bundle_path
                .parent()
                .expect("bundle parent should resolve"),
        )
        .output()
        .map_err(|error| {
            format!(
                "failed to execute oracle runner for `{lane}` `{fixture}` with `{}`: {error}",
                node_binary.path.display()
            )
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let runner_result = if oracle_result_path.is_file() {
        read_runner_result(&oracle_result_path)?
    } else {
        let rendered_error = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        let state = if let Some(exit_code) = process_exit_code_from_error(&rendered_error) {
            if exit_code == 0 {
                NodeCompatOracleState::Pass
            } else {
                NodeCompatOracleState::Fail
            }
        } else if classify_harness_mismatch(&rendered_error) {
            NodeCompatOracleState::HarnessMismatch
        } else {
            NodeCompatOracleState::Fail
        };
        NodeCompatOracleRunnerResult {
            state,
            error: if rendered_error.is_empty() {
                None
            } else {
                Some(rendered_error)
            },
        }
    };
    let drift_class = classify_oracle_drift(runtime_outcome.state, runner_result.state);
    let artifact = NodeCompatOracleArtifact {
        schema_version: NODE_COMPAT_ORACLE_SCHEMA_VERSION,
        family: bundle.family,
        slice: bundle.slice,
        lane: bundle.lane.clone(),
        upstream_fixture_line: lane_metadata.upstream_fixture_line.clone(),
        lane_role: lane_role_label(lane_metadata.lane_role).to_string(),
        public_contract_role: public_contract_role_label(lane_metadata.public_contract_role)
            .to_string(),
        runtime_execution_target: lane_metadata.runtime_execution_target.clone(),
        runtime_limits_profile: lane_metadata.runtime_limits_profile.clone(),
        fixture: bundle.test_relative_path.clone(),
        fixture_source_path: bundle.fixture_source_path,
        node_bin: node_binary.path.to_string_lossy().into_owned(),
        node_version: node_binary.version,
        startup_flags: bundle.startup_flags,
        runtime_state: runtime_outcome.state,
        runtime_detail: runtime_outcome.detail,
        oracle_state: runner_result.state,
        oracle_error: runner_result.error,
        drift_class,
        oracle_stdout: stdout,
        oracle_stderr: stderr,
    };
    let artifact_path = artifact_path(output_root, lane, fixture);
    let artifact_dir = artifact_path
        .parent()
        .expect("oracle artifact parent should resolve");
    std::fs::create_dir_all(artifact_dir).map_err(|error| {
        format!(
            "failed to create oracle artifact directory {}: {error}",
            artifact_dir.display()
        )
    })?;
    let bytes = serde_json::to_vec_pretty(&artifact)
        .map_err(|error| format!("failed to serialize oracle artifact: {error}"))?;
    std::fs::write(&artifact_path, bytes).map_err(|error| {
        format!(
            "failed to write oracle artifact {}: {error}",
            artifact_path.display()
        )
    })?;
    drop(bundle.tempdir);
    Ok(artifact_path)
}

#[test]
fn node_compat_oracle_classification_covers_drift_matrix() {
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Pass,
            NodeCompatOracleState::Pass
        ),
        NodeCompatOracleDriftClass::AgreementPass
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Skip,
            NodeCompatOracleState::Skip
        ),
        NodeCompatOracleDriftClass::AgreementSkip
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Fail,
            NodeCompatOracleState::Pass
        ),
        NodeCompatOracleDriftClass::OraclePassRuntimeFail
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Skip,
            NodeCompatOracleState::Pass
        ),
        NodeCompatOracleDriftClass::OraclePassRuntimeSkip
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Pass,
            NodeCompatOracleState::Skip
        ),
        NodeCompatOracleDriftClass::OracleSkipRuntimeRun
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Fail,
            NodeCompatOracleState::Fail
        ),
        NodeCompatOracleDriftClass::OracleFailBoth
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Pass,
            NodeCompatOracleState::Fail
        ),
        NodeCompatOracleDriftClass::OracleFailRuntimePass
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Skip,
            NodeCompatOracleState::Fail
        ),
        NodeCompatOracleDriftClass::OracleFailRuntimeSkip
    );
    assert_eq!(
        classify_oracle_drift(
            NodeCompatObservedFixtureState::Pass,
            NodeCompatOracleState::HarnessMismatch
        ),
        NodeCompatOracleDriftClass::OracleHarnessMismatch
    );
}

#[test]
fn node_compat_oracle_classification_rejects_mismatched_node_major_versions() {
    let error = validate_node_binary_version("node22", PathBuf::from("/tmp/node"), "v25.9.0\n")
        .expect_err("wrong node major should fail");
    assert!(
        error.contains("requires a Node 22 binary"),
        "version mismatch error should mention the expected lane major: {error}",
    );
}

#[test]
#[ignore = "manual node-compat oracle artifact entrypoint"]
fn node_compat_oracle_entrypoint_emits_fixture_artifact() {
    let lane = std::env::var("NEOVEX_NODE_COMPAT_ORACLE_LANE").expect("oracle lane should be set");
    let fixture =
        std::env::var("NEOVEX_NODE_COMPAT_ORACLE_FIXTURE").expect("oracle fixture should be set");
    let output_root = std::env::var("NEOVEX_NODE_COMPAT_ORACLE_OUTPUT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_oracle_output_root());
    let node_bin_override = std::env::var("NEOVEX_NODE_COMPAT_ORACLE_NODE_BIN")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let artifact_path = emit_oracle_artifact(&output_root, &lane, &fixture, node_bin_override)
        .expect("oracle artifact should emit from manual entrypoint");
    println!("oracle_artifact={}", artifact_path.display());
}
