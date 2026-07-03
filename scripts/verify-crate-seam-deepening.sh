#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

plan_active="docs/private/plans/crate-seam-deepening-plan.md"
plan_archived="docs/private/plans/archive/crate-seam-deepening-plan.md"
if [ -f "$plan_active" ]; then
  plan="$plan_active"
else
  plan="$plan_archived"
fi
index="docs/private/plans/README.md"
predecessor_active="docs/private/plans/crate-architecture-modularity-plan.md"
predecessor_archived="docs/private/plans/archive/crate-architecture-modularity-plan.md"
if [ -f "$predecessor_active" ]; then
  predecessor="$predecessor_active"
else
  predecessor="$predecessor_archived"
fi
proof_root="docs/private/plans/proof/crate-seam-deepening"
cas0_proof="$proof_root/cas0-baseline.md"

passed=0
failed=0

pass() {
  printf 'PASS: %s\n' "$1"
  passed=$((passed + 1))
}

fail() {
  printf 'FAIL: %s\n' "$1"
  failed=$((failed + 1))
}

phase_status() {
  local phase="$1"
  sed -n "s/^| ${phase} | \`\([^\\\`]*\)\` |.*/\1/p" "$plan" | head -n 1
}

phase_done() {
  [ "$(phase_status "$1")" = "done" ]
}

all_phase_rows_machine_checkable() {
  local phase status
  for phase in CAS0 CAS1 CAS2 CAS3 CAS4 CAS5 CAS6 CAS7 CAS8; do
    status="$(phase_status "$phase")"
    case "$status" in
      todo|in_progress|blocked|done|deferred) ;;
      *) return 1 ;;
    esac
  done
}

metadata_file="$(mktemp "${TMPDIR:-/tmp}/nimbus-cas-metadata.XXXXXX.json")"
metadata_ok=0
if cargo metadata --no-deps --format-version 1 >"$metadata_file" 2>/dev/null; then
  metadata_ok=1
fi
trap 'rm -f "$metadata_file"' EXIT

normal_workspace_dep_exists() {
  local package="$1"
  local dep="$2"
  [ "$metadata_ok" -eq 1 ] || return 1
  jq -e --arg package "$package" --arg dep "$dep" '
    .packages[]
    | select(.name == $package)
    | .dependencies[]
    | select(.name == $dep and (.kind == null or .kind == "normal"))
  ' "$metadata_file" >/dev/null
}

runtime_workspace_deps() {
  [ "$metadata_ok" -eq 1 ] || return 1
  jq -r '
    [.packages[] | select(.source == null) | .name] as $workspace
    | .packages[]
    | select(.name == "nimbus-runtime")
    | .dependencies[]
    | select(.name as $name | $workspace | index($name))
    | "\(.name) kind=\(.kind // "normal")"
  ' "$metadata_file"
}

core_io_hits() {
  rg -n \
    '(std::(fs|process|os::unix::net)|tokio::(fs|net|process)|reqwest|hyper::|TcpListener|TcpStream|UdpSocket|Unix(Stream|Listener)|Command::new|File::(open|create)|OpenOptions|read_to_string|write_all)' \
    crates/nimbus-core/src crates/nimbus-core/Cargo.toml || true
}

condition_1() {
  [ -f "$plan" ] \
    && [ -f "$index" ] \
    && { if [ "$plan" = "$plan_active" ]; then
      grep -q 'crate-seam-deepening-plan.md' "$index"
    else
      ! grep -q 'crate-seam-deepening-plan.md' "$index"
    fi; } \
    && grep -Eq '(Closed|Completed) predecessor' "$predecessor"
}

condition_2() {
  [ -f "$cas0_proof" ] \
    && grep -q '/Users/jack/.codex/worktrees/crate-seam-deepening/nimbus' "$cas0_proof" \
    && grep -q 'git worktree list --porcelain' "$cas0_proof" \
    && grep -q 'origin/main' "$cas0_proof" \
    && grep -q 'Crate inventory' "$cas0_proof" \
    && grep -q 'Dependency graph' "$cas0_proof" \
    && grep -q 'Public export inventory' "$cas0_proof" \
    && grep -q 'Large-file inventory' "$cas0_proof" \
    && grep -q 'PR #68' "$cas0_proof"
}

condition_3() {
  [ -x scripts/verify-crate-seam-deepening.sh ] && all_phase_rows_machine_checkable
}

condition_4() {
  [ -z "$(core_io_hits)" ] && [ -z "$(runtime_workspace_deps)" ]
}

condition_5() {
  phase_done CAS1 \
    && ! rg -n 'pub use (tenant|artifact_verifier_effects|nimbus_artifacts|nimbus_services|machine_lifecycle|local_enforcement)::|pub use nimbus_(tenant|artifacts|services)::' crates/nimbus-server/src/lib.rs >/dev/null
}

condition_6() {
  phase_done CAS2 \
    && ! rg -n 'struct (RuntimeUserIdentity|VerifiedUserIdentity|InvocationAuth)|enum VerifiedUserIdentityKind' crates/nimbus-runtime/src >/dev/null \
    && ! normal_workspace_dep_exists nimbus-auth nimbus-runtime \
    && [ -z "$(runtime_workspace_deps)" ]
}

condition_7() {
  phase_done CAS3 \
    && [ -f crates/nimbus-workloads/Cargo.toml ] \
    && ! normal_workspace_dep_exists nimbus-services nimbus-node
}

condition_8() {
  phase_done CAS4 \
    && [ -f crates/nimbus-machine/Cargo.toml ] \
    && rg -n 'PROTOCOL_VERSION|MachineApi[A-Za-z0-9_]+(Request|Response)|machine API protocol|machine-api' crates/nimbus-machine/src >/dev/null \
    && ! rg -n '^(axum|hyper|tokio)\\s*=' crates/nimbus-machine/Cargo.toml >/dev/null
}

condition_9() {
  phase_done CAS5 \
    && [ -f crates/nimbus-cli/Cargo.toml ] \
    && normal_workspace_dep_exists nimbus-bin nimbus-cli \
    && rg -n 'nimbus_cli' crates/nimbus-bin/src/main.rs >/dev/null
}

condition_10() {
  phase_done CAS6 \
    && [ -f "$proof_root/cas6-concept-heavy-modules.md" ] \
    && grep -q 'server router' "$proof_root/cas6-concept-heavy-modules.md" \
    && grep -q 'node root' "$proof_root/cas6-concept-heavy-modules.md" \
    && grep -q 'egress root' "$proof_root/cas6-concept-heavy-modules.md" \
    && grep -q 'storage SQL' "$proof_root/cas6-concept-heavy-modules.md"
}

condition_11() {
  phase_done CAS7 \
    && [ -f "$proof_root/cas7-dependency-discipline.md" ] \
    && grep -q 'cargo tree -d' "$proof_root/cas7-dependency-discipline.md" \
    && grep -q 'make deny' "$proof_root/cas7-dependency-discipline.md"
}

condition_12() {
  local phase
  for phase in CAS0 CAS1 CAS2 CAS3 CAS4 CAS5 CAS6 CAS7 CAS8; do
    phase_done "$phase" || return 1
  done
  [ -f "$proof_root/cas8-closeout.md" ] \
    && grep -q 'cargo fmt --all --check' "$proof_root/cas8-closeout.md" \
    && grep -q 'make check' "$proof_root/cas8-closeout.md" \
    && grep -q 'make clippy' "$proof_root/cas8-closeout.md" \
    && grep -q 'make deny' "$proof_root/cas8-closeout.md" \
    && grep -q 'make verify-third-party-attribution' "$proof_root/cas8-closeout.md" \
    && grep -q 'Summary: 12 passed, 0 failed' "$proof_root/cas8-closeout.md" \
    && grep -q 'PR' "$proof_root/cas8-closeout.md" \
    && grep -q 'CI green' "$proof_root/cas8-closeout.md"
}

run_condition() {
  local number="$1"
  local description="$2"
  local function_name="$3"
  if "$function_name"; then
    pass "$number $description"
  else
    fail "$number $description"
  fi
}

run_condition 1 'plan routing and predecessor marker' condition_1
run_condition 2 'CAS0 baseline proof records required inventories' condition_2
run_condition 3 'verifier exists and CAS rows are machine-checkable' condition_3
run_condition 4 'nimbus-core zero-I/O and nimbus-runtime zero workspace deps' condition_4
run_condition 5 'nimbus-server public exports narrowed' condition_5
run_condition 6 'identity moved out of nimbus-runtime' condition_6
run_condition 7 'nimbus-workloads owns workload-control seam' condition_7
run_condition 8 'machine protocol lives in nimbus-machine without heavy transport deps' condition_8
run_condition 9 'nimbus-cli owns CLI application logic and nimbus-bin is launcher' condition_9
run_condition 10 'concept-heavy modules split or proof-backed exceptions recorded' condition_10
run_condition 11 'dependency discipline proof and deny checks recorded' condition_11
run_condition 12 'all rows done, local gates, PR, and CI closeout recorded' condition_12

printf 'Summary: %s passed, %s failed\n' "$passed" "$failed"

if [ "$failed" -ne 0 ]; then
  exit 1
fi
