#!/usr/bin/env bash
# Aggregate static verifier for the transport-free Nimbus network control plane.
#
# NNC0.8 intentionally lands this gate red: later extraction bands remove the
# named legacy authorities and create `nimbus-network`. Missing inputs are hard
# failures. Run `--self-test` to prove missing-input, unclassified-bind, and
# low-level source-contract diagnostics themselves fail closed.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 1

PLAN="${NIMBUS_NETWORK_VERIFY_PLAN:-docs/private/plans/nimbus-network-control-plane-plan.md}"
PLAN_INDEX="docs/private/plans/README.md"
INVENTORY="${NIMBUS_NETWORK_VERIFY_INVENTORY:-docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json}"
DEPENDENCIES="${NIMBUS_NETWORK_VERIFY_DEPENDENCIES:-docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-dependency-graph.json}"
NETWORK_MANIFEST="crates/nimbus-network/Cargo.toml"
CORE_SOURCE_ROOT="${NIMBUS_NETWORK_VERIFY_CORE_SCAN_ROOT:-crates/nimbus-core/src}"
SOURCE_CONTRACT_HELPER="scripts/verify-nimbus-network-source-contract.mjs"
BIND_CENSUS_HELPER="scripts/verify-nimbus-network-bind-census.mjs"
COMPOSITION_CENSUS_HELPER="scripts/verify-nimbus-network-composition-census.mjs"
SOVEREIGNTY_TRIPWIRE_HELPER="${NIMBUS_NETWORK_VERIFY_SOVEREIGNTY_HELPER:-scripts/verify-nimbus-network-sovereignty-tripwire.py}"
NNC52A_ATTACHMENT_ORDERING_HELPER="scripts/verify-nimbus-network-attachment-ordering.mjs"
NNC52D_STARTUP_ORPHAN_HELPER="scripts/verify-nimbus-network-startup-orphan-reconciliation.mjs"
NNC53_ATTACHMENT_READINESS_HELPER="scripts/verify-nimbus-network-attachment-readiness.mjs"
NNC54_ATTACHMENT_CRASH_HELPER="scripts/verify-nimbus-network-attachment-crash-convergence.mjs"
NNC54A_MACHINE_BATCH_HELPER="scripts/verify-nimbus-network-machine-forwarded-batch-convergence.mjs"
COMPOSITION_CENSUS="${NIMBUS_NETWORK_VERIFY_COMPOSITION_CENSUS:-docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json}"
COMPOSITION_CENSUS_SELF_TESTS="scripts/nimbus-network-control-plane/composition-census-self-tests.sh"
BIND_EXEMPTION_SELF_TESTS="scripts/nimbus-network-control-plane/bind-exemption-self-tests.sh"
SOVEREIGNTY_TRIPWIRE_SELF_TESTS="scripts/nimbus-network-control-plane/sovereignty-tripwire-self-tests.sh"
NNC52A_ATTACHMENT_ORDERING_CONTRACT="scripts/nimbus-network-control-plane/attachment-ordering-contract.sh"
NNC52D_STARTUP_ORPHAN_CONTRACT="scripts/nimbus-network-control-plane/startup-orphan-reconciliation-contract.sh"
NNC53_ATTACHMENT_READINESS_CONTRACT="scripts/nimbus-network-control-plane/attachment-readiness-contract.sh"
NNC54_ATTACHMENT_CRASH_CONTRACT="scripts/nimbus-network-control-plane/attachment-crash-convergence-contract.sh"
NNC54A_MACHINE_BATCH_CONTRACT="scripts/nimbus-network-control-plane/machine-forwarded-batch-convergence-contract.sh"
NNC55_EFFECT_LOCALITY_CONTRACT="scripts/nimbus-network-control-plane/effect-locality-contract.sh"
NNC56_SIDE_EFFECT_FREE_INSPECTION_CONTRACT="scripts/nimbus-network-control-plane/side-effect-free-sandbox-inspection-contract.sh"
NNC61_COMPUTE_NETWORK_MANAGER_CONTRACT="scripts/nimbus-network-control-plane/compute-network-manager-injection-contract.sh"
NNC61A_COMPUTE_NODE_COORDINATOR_CONTRACT="scripts/nimbus-network-control-plane/compute-node-workload-coordinator-contract.sh"
NNC61D_WORKLOAD_SAGA_AUTHORITY_CONTRACT="scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh"
NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT="scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh"
NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT="scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh"
NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT="scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh"
NNC63A_WORKLOAD_EXECUTABLE_CONTRACT="scripts/nimbus-network-control-plane/workload-executable-carrier-contract.sh"
NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT="scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh"
NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT="scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh"

# shellcheck source=scripts/nimbus-network-control-plane/attachment-ordering-contract.sh
. "${NNC52A_ATTACHMENT_ORDERING_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/startup-orphan-reconciliation-contract.sh
. "${NNC52D_STARTUP_ORPHAN_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/attachment-readiness-contract.sh
. "${NNC53_ATTACHMENT_READINESS_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/attachment-crash-convergence-contract.sh
. "${NNC54_ATTACHMENT_CRASH_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/machine-forwarded-batch-convergence-contract.sh
. "${NNC54A_MACHINE_BATCH_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/effect-locality-contract.sh
. "${NNC55_EFFECT_LOCALITY_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/side-effect-free-sandbox-inspection-contract.sh
. "${NNC56_SIDE_EFFECT_FREE_INSPECTION_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/compute-network-manager-injection-contract.sh
. "${NNC61_COMPUTE_NETWORK_MANAGER_CONTRACT}"
# shellcheck source=scripts/nimbus-network-control-plane/compute-node-workload-coordinator-contract.sh
. "${NNC61A_COMPUTE_NODE_COORDINATOR_CONTRACT}"

PASS_COUNT=0
FAIL_COUNT=0

pass() {
  PASS_COUNT=$((PASS_COUNT + 1))
  printf 'PASS %s %s\n' "$1" "$2"
}

fail() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  printf 'FAIL %s %s\n' "$1" "$2"
  if [ $# -ge 3 ] && [ -n "$3" ]; then
    printf '     %s\n' "$3"
  fi
}

require_tools() {
  missing=""
  for tool in git rg node cargo; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      missing="${missing}${missing:+, }${tool}"
    fi
  done
  if [ -z "${missing}" ]; then
    pass "NNCV000" "required-tools"
  else
    fail "NNCV000" "required-tools" "missing: ${missing}"
  fi
}

verify_plan_in_head() {
  if [ ! -f "${PLAN}" ]; then
    fail "NNCV001" "plan-in-HEAD" "missing working-tree input: ${PLAN}"
    return
  fi
  case "${PLAN}" in
    /*)
      fail "NNCV001" "plan-in-HEAD" "canonical plan override is not repository-relative: ${PLAN}"
      ;;
    *)
      if git cat-file -e "HEAD:${PLAN}" 2>/dev/null; then
        pass "NNCV001" "plan-in-HEAD"
      else
        fail "NNCV001" "plan-in-HEAD" "${PLAN} exists only outside HEAD or is unreadable from HEAD"
      fi
      ;;
  esac
}

verify_baseline_inputs() {
  error="$(
    node - "${INVENTORY}" "${DEPENDENCIES}" <<'NODE'
const fs = require("fs");
const [inventoryPath, dependencyPath] = process.argv.slice(2);
const errors = [];

function readJson(path, label) {
  if (!fs.existsSync(path)) {
    errors.push(label + " missing: " + path);
    return null;
  }
  let text;
  try {
    text = fs.readFileSync(path, "utf8");
  } catch (error) {
    errors.push(label + " unreadable: " + path + ": " + error.message);
    return null;
  }
  if (!text.trim()) {
    errors.push(label + " empty: " + path);
    return null;
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    errors.push(label + " invalid JSON: " + path + ": " + error.message);
    return null;
  }
}

const inventory = readJson(inventoryPath, "bind inventory");
const dependencies = readJson(dependencyPath, "dependency baseline");

if (inventory) {
  if (inventory.schema_version !== 2) errors.push("bind inventory schema_version must be 2");
  if (!Array.isArray(inventory.production_sites) || inventory.production_sites.length === 0) {
    errors.push("bind inventory production_sites must be non-empty");
  }
  if (!Array.isArray(inventory.authority_occurrences) || inventory.authority_occurrences.length === 0) {
    errors.push("bind inventory authority_occurrences must be non-empty");
  }
  if (!Array.isArray(inventory.non_authority_occurrences)) {
    errors.push("bind inventory non_authority_occurrences must be an array");
  }
  if (!Array.isArray(inventory.non_production_exemptions)) {
    errors.push("bind inventory non_production_exemptions must be an array");
  }
  const exemptionMechanisms = new Set(
    (inventory.non_production_exemptions || []).map(entry => entry.mechanism),
  );
  for (const mechanism of [
    "path-convention",
    "cfg-test-item",
    "path-owned-test-module",
    "test-support-crate",
  ]) {
    if (!exemptionMechanisms.has(mechanism)) {
      errors.push("bind inventory lacks exemption mechanism: " + mechanism);
    }
  }
  if ((inventory.non_production_exemptions || []).some(entry => "examples" in entry)) {
    errors.push("bind inventory exemptions must not exempt whole production files through examples");
  }
  for (const exemption of inventory.non_production_exemptions || []) {
    if (exemption.mechanism !== "path-owned-test-module") continue;
    if (!Array.isArray(exemption.files) || exemption.files.length === 0) {
      errors.push("bind inventory path-owned-test-module exemption must name exact files");
      continue;
    }
    for (const evidence of exemption.files) {
      for (const field of ["path", "declared_from", "cfg_owner", "owner_module", "module"]) {
        if (typeof evidence[field] !== "string" || !evidence[field].trim()) {
          errors.push("bind inventory path-owned test exemption lacks " + field);
        }
      }
    }
  }
  if (!inventory.summary || inventory.summary.production_sites !== inventory.production_sites?.length) {
    errors.push("bind inventory summary.production_sites does not match production_sites");
  }
  if (inventory.summary?.authority_occurrences !== inventory.authority_occurrences?.length) {
    errors.push("bind inventory summary.authority_occurrences does not match authority_occurrences");
  }
  if (inventory.summary?.non_authority_occurrences !== inventory.non_authority_occurrences?.length) {
    errors.push("bind inventory summary.non_authority_occurrences does not match non_authority_occurrences");
  }
  if (inventory.summary?.unclassified_production_sites !== 0) {
    errors.push("bind inventory baseline records unclassified production sites");
  }
  const ids = new Set();
  let activeSites = 0;
  let retiredSites = 0;
  for (const site of inventory.production_sites || []) {
    for (const field of [
      "id",
      "status",
      "verification",
      "path",
      "current_owner",
      "current_truth",
      "disposition",
      "target_owner",
      "owner_item",
    ]) {
      if (typeof site[field] !== "string" || !site[field].trim()) {
        errors.push("bind inventory site " + (site.id || "<unknown>") + " lacks " + field);
      }
    }
    if (ids.has(site.id)) errors.push("duplicate bind inventory id: " + site.id);
    ids.add(site.id);
    if (site.path && !fs.existsSync(site.path)) errors.push("bind inventory path missing: " + site.path);
    if (site.status === "active") activeSites += 1;
    else if (site.status === "retired") {
      retiredSites += 1;
      if (typeof site.retired_item !== "string" || !site.retired_item.trim()) {
        errors.push("retired bind inventory site lacks retired_item: " + site.id);
      }
    } else {
      errors.push("bind inventory site has invalid status: " + site.id);
    }
    if (site.status === "active" && site.verification === "source-occurrence") {
      if (!Array.isArray(site.authority_kinds) || site.authority_kinds.length === 0) {
        errors.push("source-occurrence site lacks authority_kinds: " + site.id);
      }
      if (!Array.isArray(site.authority_symbols) || site.authority_symbols.length === 0) {
        errors.push("source-occurrence site lacks authority_symbols: " + site.id);
      }
      if (site.authority_paths !== undefined) {
        if (
          !Array.isArray(site.authority_paths) ||
          site.authority_paths.length === 0 ||
          !site.authority_paths.every(path => typeof path === "string" && path.trim()) ||
          !site.authority_paths.includes(site.path)
        ) {
          errors.push("source-occurrence site has invalid authority_paths: " + site.id);
        }
      }
    } else if (site.status === "active" && site.verification === "symbol-presence") {
      if (typeof site.declaration_name !== "string" || !site.declaration_name.trim()) {
        errors.push("symbol-presence site lacks declaration_name: " + site.id);
      }
    } else if (site.status === "active") {
      errors.push("active bind inventory site has invalid verification: " + site.id);
    }
  }
  if (inventory.summary?.active_production_sites !== activeSites) {
    errors.push("bind inventory summary.active_production_sites does not match production_sites");
  }
  if (inventory.summary?.retired_production_sites !== retiredSites) {
    errors.push("bind inventory summary.retired_production_sites does not match production_sites");
  }
  const occurrenceKeys = new Set();
  for (const occurrence of inventory.authority_occurrences || []) {
    for (const field of ["site_id", "path", "kind", "symbol"]) {
      if (typeof occurrence[field] !== "string" || !occurrence[field].trim()) {
        errors.push("bind inventory authority occurrence lacks " + field);
      }
    }
    if (!ids.has(occurrence.site_id)) {
      errors.push("bind inventory authority occurrence references unknown site: " + occurrence.site_id);
    }
    if (!Number.isInteger(occurrence.ordinal) || occurrence.ordinal < 1) {
      errors.push("bind inventory authority occurrence has invalid ordinal: " + occurrence.site_id);
    }
    if (!Number.isInteger(occurrence.line) || occurrence.line < 1) {
      errors.push("bind inventory authority occurrence has invalid line: " + occurrence.site_id);
    }
    const key = [occurrence.path, occurrence.kind, occurrence.symbol, occurrence.ordinal].join("|");
    if (occurrenceKeys.has(key)) errors.push("duplicate bind inventory authority occurrence: " + key);
    occurrenceKeys.add(key);
  }
  const nonAuthorityKeys = new Set();
  for (const occurrence of inventory.non_authority_occurrences || []) {
    for (const field of ["path", "kind", "symbol", "reason"]) {
      if (typeof occurrence[field] !== "string" || !occurrence[field].trim()) {
        errors.push("bind inventory non-authority occurrence lacks " + field);
      }
    }
    if (!Number.isInteger(occurrence.ordinal) || occurrence.ordinal < 1) {
      errors.push("bind inventory non-authority occurrence has invalid ordinal: " + occurrence.path);
    }
    if (!Number.isInteger(occurrence.line) || occurrence.line < 1) {
      errors.push("bind inventory non-authority occurrence has invalid line: " + occurrence.path);
    }
    const key = [occurrence.path, occurrence.kind, occurrence.symbol, occurrence.ordinal].join("|");
    if (nonAuthorityKeys.has(key)) errors.push("duplicate bind inventory non-authority occurrence: " + key);
    nonAuthorityKeys.add(key);
  }
  if (typeof inventory.source_head !== "string" || !/^[0-9a-f]{7,40}$/.test(inventory.source_head)) {
    errors.push("bind inventory source_head is missing or invalid");
  }
}

if (dependencies) {
  if (dependencies.schema_version !== 1) errors.push("dependency baseline schema_version must be 1");
  if (!Array.isArray(dependencies.profiles) || dependencies.profiles.length < 4) {
    errors.push("dependency baseline must contain at least four profiles");
  }
  if (!Array.isArray(dependencies.targets) || dependencies.targets.length < 2) {
    errors.push("dependency baseline must contain multiple targets");
  }
  if (typeof dependencies.source_head !== "string" || !/^[0-9a-f]{7,40}$/.test(dependencies.source_head)) {
    errors.push("dependency baseline source_head is missing or invalid");
  }
}

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  node_status=$?

  ancestry_error=""
  if [ "${node_status}" -eq 0 ]; then
    for input in "${INVENTORY}" "${DEPENDENCIES}"; do
      source_head="$(node -e 'const fs=require("fs"); process.stdout.write(JSON.parse(fs.readFileSync(process.argv[1],"utf8")).source_head)' "${input}" 2>/dev/null)"
      if [ -z "${source_head}" ] || ! git merge-base --is-ancestor "${source_head}" HEAD 2>/dev/null; then
        ancestry_error="${ancestry_error}${ancestry_error:+; }${input} source_head ${source_head:-<missing>} is not an ancestor of HEAD"
      fi
    done
  fi

  if [ "${node_status}" -eq 0 ] && [ -z "${ancestry_error}" ]; then
    pass "NNCV002" "required-baseline-inputs"
  else
    fail "NNCV002" "required-baseline-inputs" "${error}${error:+; }${ancestry_error}"
  fi
}

verify_network_crate() {
  if [ ! -f "${NETWORK_MANIFEST}" ]; then
    fail "NNCV003" "nimbus-network-crate" "missing ${NETWORK_MANIFEST}"
    return
  fi
  if cargo metadata --no-deps --format-version 1 2>/dev/null |
    node -e '
      let text = "";
      process.stdin.on("data", chunk => text += chunk);
      process.stdin.on("end", () => {
        const metadata = JSON.parse(text);
        const pkg = metadata.packages.find(candidate => candidate.name === "nimbus-network");
        process.exit(pkg && metadata.workspace_members.includes(pkg.id) ? 0 : 1);
      });
    '; then
    pass "NNCV003" "nimbus-network-crate"
  else
    fail "NNCV003" "nimbus-network-crate" "manifest exists but package is not a workspace member"
  fi
}

verify_network_dependency_contract() {
  if [ ! -f "${NETWORK_MANIFEST}" ]; then
    fail "NNCV004" "network-dependency-contract" "unavailable because nimbus-network crate is missing"
    return
  fi
  error="$(
    cargo metadata --no-deps --format-version 1 2>/dev/null |
      node -e '
        let text = "";
        process.stdin.on("data", chunk => text += chunk);
        process.stdin.on("end", () => {
          const metadata = JSON.parse(text);
          const pkg = metadata.packages.find(candidate => candidate.name === "nimbus-network");
          if (!pkg) {
            process.stdout.write("nimbus-network package absent from cargo metadata");
            process.exit(1);
          }
          const workspaceNames = new Set(metadata.packages.map(candidate => candidate.name));
          const testCase = process.env.NIMBUS_NETWORK_VERIFY_TEST_DEPENDENCY_CONTRACT_CASE || "";
          if (testCase === "core-dev") {
            const core = pkg.dependencies.find(dep => dep.name === "nimbus-core");
            if (core) core.kind = "dev";
          } else if (testCase === "core-feature") {
            const core = pkg.dependencies.find(dep => dep.name === "nimbus-core");
            if (core) core.features.push("effect-surface");
          } else if (testCase === "core-no-default") {
            const core = pkg.dependencies.find(dep => dep.name === "nimbus-core");
            if (core) core.uses_default_features = false;
          } else if (testCase === "serde-no-default") {
            const serde = pkg.dependencies.find(dep => dep.name === "serde");
            if (serde) serde.uses_default_features = false;
          } else if (testCase === "tokio") {
            pkg.dependencies.push({
              name: "tokio", source: "registry+self-test", kind: null, optional: false,
              target: null, rename: null, features: [], uses_default_features: true,
            });
          } else if (testCase === "windows-networking") {
            const windows = pkg.dependencies.find(dep => dep.name === "windows-sys");
            if (windows) windows.features.push("Win32_Networking_WinSock");
          }
          const workspaceEdges = pkg.dependencies
            .filter(dep => dep.source === null && workspaceNames.has(dep.name));
          const core = workspaceEdges.filter(dep => dep.name === "nimbus-core");
          const exactCore = core.length === 1 &&
            core[0].kind === null &&
            core[0].target === null &&
            core[0].optional === false &&
            core[0].rename === null &&
            core[0].uses_default_features === true &&
            JSON.stringify([...core[0].features].sort()) === "[]";
          if (!exactCore || workspaceEdges.length !== 1) {
            const edges = workspaceEdges.map(dep =>
              dep.name + ":" + (dep.kind || "normal") + ":" + (dep.target || "all") +
              (dep.optional ? ":optional" : ""),
            );
            process.stdout.write("workspace dependency must be one normal unconditional non-optional nimbus-core edge; found " + (edges.join(", ") || "<none>"));
            process.exit(1);
          }
          const approved = new Map([
            ["fs2", {kind: null, target: null, features: [], defaultFeatures: true}],
            ["serde", {kind: null, target: null, features: ["derive"], defaultFeatures: true}],
            ["serde_json", {kind: null, target: null, features: ["raw_value"], defaultFeatures: true}],
            ["sha2", {kind: null, target: null, features: [], defaultFeatures: true}],
            ["ulid", {kind: null, target: null, features: ["serde"], defaultFeatures: true}],
            ["libc", {kind: null, target: "cfg(unix)", features: [], defaultFeatures: true}],
            ["windows-sys", {kind: null, target: "cfg(windows)", features: ["Win32_Storage_FileSystem"], defaultFeatures: true}],
            ["proptest", {kind: "dev", target: null, features: [], defaultFeatures: true}],
            ["tempfile", {kind: "dev", target: null, features: [], defaultFeatures: true}],
          ]);
          const externals = pkg.dependencies.filter(dep => dep.source !== null);
          const errors = [];
          for (const dep of externals) {
            const expected = approved.get(dep.name);
            const actualFeatures = [...dep.features].sort();
            if (!expected ||
                dep.kind !== expected.kind ||
                dep.target !== expected.target ||
                dep.optional !== false ||
                dep.rename !== null ||
                dep.uses_default_features !== expected.defaultFeatures ||
                JSON.stringify(actualFeatures) !== JSON.stringify([...expected.features].sort())) {
              errors.push(dep.name + ":" + (dep.kind || "normal") + ":" +
                (dep.target || "all") + ":default-features=" +
                dep.uses_default_features + ":features=" + actualFeatures.join(","));
            }
          }
          if (externals.length !== approved.size || errors.length) {
            process.stdout.write("nimbus-network dependency envelope changed: " +
              (errors.join("; ") || "missing approved dependency"));
            process.exit(1);
          }
        });
      '
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV004" "network-dependency-contract"
  else
    fail "NNCV004" "network-dependency-contract" "${error:-cargo metadata failed}"
  fi
}

verify_single_port_authority() {
  legacy="$(
    rg -n \
      -e 'PortManager' \
      -e 'port_manager' \
      -e 'fn resolve_listener_port' \
      -e 'fn ephemeral_port' \
      -e 'fn allocate_machine_ssh_port' \
      -e 'fn machine_port_is_available' \
      crates/nimbus-sandbox/src \
      crates/nimbus-cli/src 2>&1
  )"
  scan_status=$?
  if [ "${NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD:-}" = "1" ] &&
    [ -n "${NIMBUS_NETWORK_VERIFY_TEST_LEGACY_PORT_AUTHORITY:-}" ]; then
    legacy="${legacy}${legacy:+
}${NIMBUS_NETWORK_VERIFY_TEST_LEGACY_PORT_AUTHORITY}"
    scan_status=0
  fi
  if [ "${scan_status}" -gt 1 ]; then
    fail "NNCV005" "no-duplicate-port-allocation-authority" "source scan failed: ${legacy}"
  elif [ -z "${legacy}" ] && [ -f "${NETWORK_MANIFEST}" ]; then
    pass "NNCV005" "no-duplicate-port-allocation-authority"
  else
    detail="${legacy:-nimbus-network lease authority is absent}"
    fail "NNCV005" "no-duplicate-port-allocation-authority" "${detail}"
  fi
}

verify_bind_census() {
  census_output="$(mktemp "${TMPDIR:-/tmp}/nimbus-network-bind-census.XXXXXX")" || {
    fail "NNCV006" "unclassified-production-bind" "unable to create census output file"
    return
  }
  node "${BIND_CENSUS_HELPER}" --inventory "${INVENTORY}" >"${census_output}"
  status=$?
  error="$(<"${census_output}")"
  rm -f "${census_output}"
  if [ "${status}" -eq 0 ]; then
    pass "NNCV006" "unclassified-production-bind"
  else
    fail "NNCV006" "unclassified-production-bind" "${error}"
  fi
}

verify_dependency_baseline() {
  error="$(
    node - "${DEPENDENCIES}" <<'NODE'
const fs = require("fs");
const input = process.argv[2];
const errors = [];
if (!fs.existsSync(input)) {
  process.stdout.write("dependency baseline missing: " + input);
  process.exit(1);
}
let graph;
try {
  graph = JSON.parse(fs.readFileSync(input, "utf8"));
} catch (error) {
  process.stdout.write("dependency baseline invalid: " + error.message);
  process.exit(1);
}
const profiles = graph.profiles || [];
const names = new Set();
for (const profile of profiles) {
  if (!profile.name || names.has(profile.name)) errors.push("missing or duplicate profile name: " + (profile.name || "<empty>"));
  names.add(profile.name);
  if (!Array.isArray(profile.edges)) errors.push((profile.name || "<unknown>") + " lacks edges");
  if (!Array.isArray(profile.cycles)) errors.push((profile.name || "<unknown>") + " lacks cycles");
  else if (profile.cycles.length) errors.push(profile.name + " contains " + profile.cycles.length + " dependency cycle(s)");
}
for (const fragment of ["normal", "dev", "all-feature"]) {
  if (![...names].some(name => name.includes(fragment))) errors.push("missing " + fragment + " dependency profile");
}
process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV007" "dependency-profiles-acyclic"
  else
    fail "NNCV007" "dependency-profiles-acyclic" "${error}"
  fi
}

verify_checkpoint_ledger() {
  error="$(
    node - "${PLAN}" <<'NODE'
const fs = require("fs");
const {spawnSync} = require("child_process");
const input = process.argv[2];
if (!fs.existsSync(input)) {
  process.stdout.write("plan missing: " + input);
  process.exit(1);
}
const text = fs.readFileSync(input, "utf8");
const marker = "## Item Checkpoint Ledger";
const offset = text.indexOf(marker);
const errors = [];
if (offset < 0) {
  process.stdout.write("Item Checkpoint Ledger heading missing");
  process.exit(1);
}
const itemPattern = /^\| (NNC\d+\.\d+(?:[a-z]\d*)?) \|/gm;
const planned = [...text.slice(0, offset).matchAll(itemPattern)].map(match => match[1]);
const tick = String.fromCharCode(96);
const rows = text.slice(offset).split("\n")
  .filter(line => /^\| NNC\d+\.\d+(?:[a-z]\d*)? \|/.test(line))
  .map(line => line.split("|").slice(1, -1).map(cell => cell.trim()))
  .map(cells => ({id: cells[0], status: cells[1].replaceAll(tick, ""), evidence: cells.slice(2).join("|").trim()}));
const unique = values => new Set(values).size === values.length;
if (!planned.length) errors.push("no implementation-band items found");
if (!unique(planned)) errors.push("duplicate implementation-band item IDs");
if (!unique(rows.map(row => row.id))) errors.push("duplicate checkpoint-ledger item IDs");
const plannedSet = new Set(planned);
const ledgerSet = new Set(rows.map(row => row.id));
const missing = planned.filter(item => !ledgerSet.has(item));
const extra = rows.map(row => row.id).filter(item => !plannedSet.has(item));
if (missing.length || extra.length) errors.push("band/ledger mismatch: missing=" + (missing.join(",") || "<none>") + " extra=" + (extra.join(",") || "<none>"));
const inProgress = rows.filter(row => row.status === "in_progress");
if (inProgress.length !== 1) errors.push("expected exactly one in_progress row, found " + inProgress.length);
for (const row of rows) {
  if (!["done", "in_progress", "todo"].includes(row.status)) errors.push(row.id + " has invalid status " + row.status);
  if (row.status === "done" && (!row.evidence.trim() || row.evidence.trim() === "—")) errors.push(row.id + " done without evidence");
  if (row.status === "in_progress") {
    for (const field of ["Owned paths", "Last green", "Next", "Blocker"]) {
      if (!row.evidence.toLowerCase().includes(field.toLowerCase())) errors.push(row.id + " recovery checkpoint lacks " + field);
    }
  }
}
const currentItemLine = text.split("\n").find(line => line.startsWith("| Current item |"));
const currentItem = currentItemLine?.split("|")[2]?.trim().replaceAll(tick, "");
if (!currentItem || !inProgress[0] || !currentItem.startsWith(inProgress[0].id + " ")) {
  errors.push("Recovery Header current item does not match the in_progress ledger row");
}
const checkpointLine = text.split("\n").find(line => line.startsWith("| Last checkpoint commit |"));
const checkpointHashes = checkpointLine?.match(/\b[0-9a-f]{40}\b/g) || [];
if (checkpointHashes.length !== 1) {
  errors.push("Recovery Header must contain exactly one full Last checkpoint commit hash");
} else {
  const checkpoint = checkpointHashes[0];
  const resolution = spawnSync("git", ["cat-file", "-e", checkpoint + "^{commit}"], {
    stdio: "ignore",
  });
  if (resolution.status !== 0) {
    errors.push("Recovery Header Last checkpoint commit does not resolve: " + checkpoint);
  }
}
process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV008" "checkpoint-ledger-recoverable"
  else
    fail "NNCV008" "checkpoint-ledger-recoverable" "${error}"
  fi
}

verify_routing_owner() {
  error="$(
    node - "${PLAN}" "${PLAN_INDEX}" <<'NODE'
const fs = require("fs");
const [planPath, indexPath] = process.argv.slice(2);
const errors = [];
if (!fs.existsSync(planPath)) errors.push("plan missing: " + planPath);
if (!fs.existsSync(indexPath)) errors.push("plan index missing: " + indexPath);
if (!errors.length) {
  const plan = fs.readFileSync(planPath, "utf8");
  const index = fs.readFileSync(indexPath, "utf8");
  const normalizedIndex = index.replace(/\s+/g, " ");
  const tick = String.fromCharCode(96);
  const statusLine = plan.split("\n").find(line => line.startsWith("Status: "));
  const status = statusLine?.slice("Status: ".length).replaceAll(tick, "");
  if (!index.includes("nimbus-network-control-plane-plan.md")) errors.push("plan index does not route the canonical network owner");
  const expectedRoute = "nimbus-network-control-plane-plan.md" + tick + " - " + tick + status + tick;
  if (!status || !normalizedIndex.includes(expectedRoute)) {
    errors.push("plan index status does not match canonical status: " + (status || "<missing>"));
  }
  const ownerClaims = (index.match(/nimbus-network-control-plane-plan\.md/g) || []).length;
  if (ownerClaims !== 1) errors.push("plan index contains " + ownerClaims + " network-plan routes; expected exactly one");
}
process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV009" "sole-plan-routing-owner"
  else
    fail "NNCV009" "sole-plan-routing-owner" "${error}"
  fi
}

verify_foundation_invariants() {
  metadata_error="$(
    cargo metadata --no-deps --format-version 1 2>/dev/null |
      node -e '
        let text = "";
        process.stdin.on("data", chunk => text += chunk);
        process.stdin.on("end", () => {
          let metadata;
          try { metadata = JSON.parse(text); }
          catch (error) {
            process.stdout.write("cargo metadata invalid: " + error.message);
            process.exit(1);
          }
          const runtime = metadata.packages.find(pkg => pkg.name === "nimbus-runtime");
          if (!runtime) {
            process.stdout.write("nimbus-runtime missing from cargo metadata");
            process.exit(1);
          }
          const workspaceNames = new Set(metadata.packages.map(pkg => pkg.name));
          const workspaceEdges = runtime.dependencies
            .filter(dep => dep.source === null && workspaceNames.has(dep.name))
            .map(dep => dep.name);
          if (workspaceEdges.length) {
            process.stdout.write("nimbus-runtime workspace dependencies: " + workspaceEdges.join(", "));
            process.exit(1);
          }
        });
      '
  )"
  metadata_status=$?
  core_io="$(
    rg -n \
      -e '\b(?:TcpListener|TcpStream|UdpSocket)::' \
      -e '\b(?:File|OpenOptions)::(?:open|create|new)' \
      -e '\bstd::fs::' \
      -e '\btokio::(?:fs|net)::' \
      "${CORE_SOURCE_ROOT}" 2>&1
  )"
  core_scan_status=$?
  if [ "${core_scan_status}" -gt 1 ]; then
    fail "NNCV010" "core-runtime-foundation-invariants" "nimbus-core source scan failed: ${core_io}"
  elif [ "${metadata_status}" -eq 0 ] && [ -z "${core_io}" ]; then
    pass "NNCV010" "core-runtime-foundation-invariants"
  else
    fail "NNCV010" "core-runtime-foundation-invariants" "${metadata_error}${metadata_error:+; }${core_io}"
  fi
}

verify_portable_vocabulary_owner() {
  legacy="$(
    rg -n \
      -e 'pub struct NetworkSegment' \
      -e 'trait NetworkSegmentAllocator' \
      -e 'pub (enum|struct) PublishedEndpoint' \
      crates/nimbus-core/src \
      crates/nimbus-sandbox/src 2>&1
  )"
  scan_status=$?
  required_missing=""
  if [ ! -d "crates/nimbus-network/src" ]; then
    required_missing="nimbus-network portable vocabulary owner is absent"
  elif ! rg -q 'NetworkAttachmentId' crates/nimbus-network/src; then
    required_missing="nimbus-network lacks NetworkAttachmentId"
  fi
  if [ "${scan_status}" -gt 1 ]; then
    fail "NNCV011" "single-portable-vocabulary-owner" "source scan failed: ${legacy}"
  elif [ -z "${legacy}" ] && [ -z "${required_missing}" ]; then
    pass "NNCV011" "single-portable-vocabulary-owner"
  else
    fail "NNCV011" "single-portable-vocabulary-owner" "${required_missing}${required_missing:+; }${legacy}"
  fi
}

verify_forbidden_network_dependencies_effects() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV012" "forbidden-network-dependencies-effects" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" forbidden-dependencies-effects 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV012" "forbidden-network-dependencies-effects"
  else
    fail "NNCV012" "forbidden-network-dependencies-effects" "${error}"
  fi
}

verify_single_network_definition_owner() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV013" "single-network-definition-owner" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" single-definition-owner 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV013" "single-network-definition-owner"
  else
    fail "NNCV013" "single-network-definition-owner" "${error}"
  fi
}

verify_address_is_not_network_identity() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV014" "address-is-not-network-identity" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" address-is-not-identity 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV014" "address-is-not-network-identity"
  else
    fail "NNCV014" "address-is-not-network-identity" "${error}"
  fi
}

verify_local_network_composition_census() {
  if [ "${NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD:-}" = "1" ] &&
    [ "${NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE:-}" != "1" ]; then
    pass "NNCV015" "local-network-composition-census"
    return
  fi
  if [ ! -f "${COMPOSITION_CENSUS_HELPER}" ]; then
    fail "NNCV015" "local-network-composition-census" "missing ${COMPOSITION_CENSUS_HELPER}"
    return
  fi
  error="$(
    node "${COMPOSITION_CENSUS_HELPER}" \
      --inventory "${INVENTORY}" \
      --census "${COMPOSITION_CENSUS}" 2>&1
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV015" "local-network-composition-census"
  else
    fail "NNCV015" "local-network-composition-census" "${error}"
  fi
}

verify_sovereignty_tripwire_contract() {
  if [ ! -f "${SOVEREIGNTY_TRIPWIRE_HELPER}" ]; then
    fail "NNCV016" "sovereignty-tripwire-contract" "missing ${SOVEREIGNTY_TRIPWIRE_HELPER}"
    return
  fi
  error="$(python3 "${SOVEREIGNTY_TRIPWIRE_HELPER}" 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV016" "sovereignty-tripwire-contract"
  else
    fail "NNCV016" "sovereignty-tripwire-contract" "${error}"
  fi
}

run_nnc61e_recovery_decision_self_tests() {
  script="$1"
  temporary="$2"
  nnc61e_fail=0
  nnc61e_mutations=(
    missing-tenant-cursor
    missing-cursor-ordering
    missing-quiescent-matrix
    missing-selector
    missing-action-row
    missing-cleanup-retention
    missing-successor-promotion
    missing-bounded-reader
    missing-kill-reap-proof
    snapshot-handoff
  )

  for mutation in "${nnc61e_mutations[@]}"; do
    fixture="${temporary}/nnc61e-${mutation}"
    mkdir -p "${fixture}"
    cp crates/nimbus-workloads/src/store.rs "${fixture}/store.rs"
    cp crates/nimbus-compute/src/workload_saga.rs "${fixture}/compute-root.rs"
    cp crates/nimbus-compute/src/workload_saga/recovery.rs "${fixture}/compute.rs"
    cp crates/nimbus-server/src/workload_saga_store/tenant_enumeration.rs \
      "${fixture}/tenant-adapter.rs"
    cp crates/nimbus-server/src/workload_saga_store/tests/composition.rs \
      "${fixture}/process.rs"
    cp crates/nimbus-server/src/workload_saga_store/tests/recovery.rs \
      "${fixture}/matrix.rs"

    if ! node - "${mutation}" "${fixture}" <<'NODE'
const fs = require("fs");
const [mutation, root] = process.argv.slice(2);

function replaceOne(name, before, after) {
  const path = `${root}/${name}`;
  const source = fs.readFileSync(path, "utf8");
  const index = source.indexOf(before);
  if (index < 0) throw new Error(`${mutation}: missing mutation anchor ${before}`);
  fs.writeFileSync(path, source.slice(0, index) + after + source.slice(index + before.length));
}

switch (mutation) {
  case "missing-tenant-cursor":
    replaceOne("store.rs", "pub struct WorkloadSagaTenantCursor", "pub struct MissingWorkloadSagaTenantCursor");
    break;
  case "missing-cursor-ordering":
    replaceOne(
      "store.rs",
      "workload saga tenant page is duplicated, identity-unsorted, or cursor-regressing",
      "workload saga tenant page ordering guard removed",
    );
    break;
  case "missing-quiescent-matrix":
    {
      const path = `${root}/matrix.rs`;
      const source = fs.readFileSync(path, "utf8");
      if (!source.includes("process-recorded-quiescent")) {
        throw new Error(`${mutation}: missing quiescent matrix anchor`);
      }
      fs.writeFileSync(path, source.replaceAll("process-recorded-quiescent", "process-recorded-omitted"));
    }
    break;
  case "missing-selector":
    replaceOne("compute.rs", "pub enum WorkloadSagaAction", "pub enum MissingWorkloadSagaAction");
    break;
  case "missing-action-row":
    replaceOne(
      "compute.rs",
      "WorkloadSagaAction::Provision(decision)",
      "WorkloadSagaAction::OmittedProvision(decision)",
    );
    break;
  case "missing-cleanup-retention":
    replaceOne(
      "compute.rs",
      "retained_references: detail.retained_references().clone()",
      "retained_references: WorkloadEffectReferences::default()",
    );
    break;
  case "missing-successor-promotion":
    replaceOne("compute.rs", "WorkloadSagaAction::PromoteSuccessor", "WorkloadSagaAction::OmittedSuccessorPromotion");
    break;
  case "missing-bounded-reader":
    replaceOne("compute.rs", "plan_recoverable_page", "omitted_recoverable_page");
    break;
  case "missing-kill-reap-proof":
    replaceOne("process.rs", "killed-at-boundary-and-reaped", "unbounded-child-cleanup");
    break;
  case "snapshot-handoff":
    fs.appendFileSync(`${root}/process.rs`, "\nconst SNAPSHOT_HANDOFF_PAYLOAD: &[u8] = b\"forbidden\";\n");
    break;
  default:
    throw new Error(`unknown NNC6.1e mutation ${mutation}`);
}
NODE
    then
      printf 'SELFTEST FAIL NNC6.1e mutation fixture %s could not be built\n' "${mutation}"
      nnc61e_fail=$((nnc61e_fail + 1))
      continue
    fi

    output="${temporary}/nnc61e-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_RECOVERY_STORE_SOURCE="${fixture}/store.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_COMPUTE_ROOT_SOURCE="${fixture}/compute-root.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_COMPUTE_SOURCE="${fixture}/compute.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_TENANT_ADAPTER_SOURCE="${fixture}/tenant-adapter.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_PROCESS_SOURCE="${fixture}/process.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_MATRIX_SOURCE="${fixture}/matrix.rs" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNC6.1e mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc61e_fail=$((nnc61e_fail + 1))
    elif ! grep -q '^FAIL NNCV027 durable-workload-saga-authority' "${output}" ||
      grep -q '^PASS NNCV027 durable-workload-saga-authority' "${output}" ||
      [ "$(grep -c '^FAIL NNCV' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL NNC6.1e mutation %s did not fail exclusively as NNCV027\n' "${mutation}"
      nnc61e_fail=$((nnc61e_fail + 1))
    else
      printf 'SELFTEST PASS NNC6.1e mutation %s fails closed as NNCV027\n' "${mutation}"
    fi
  done

  return "${nnc61e_fail}"
}

run_self_test() {
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-network-verifier-self-test.XXXXXX")" || {
    printf 'SELFTEST FAIL unable to create temporary directory\n'
    exit 1
  }
  trap 'rm -rf "${temporary}"' EXIT
  script="${REPO_ROOT}/scripts/verify-nimbus-network-control-plane.sh"
  self_fail=0

  run_parallel_self_test_batch() {
    launched_lanes=""
    for lane_spec in "$@"; do
      IFS='|' read -r lane_name lane_function missing_label <<<"${lane_spec}"
      if ! declare -F "${lane_function}" >/dev/null 2>&1; then
        printf 'SELFTEST FAIL %s contract helper is missing\n' "${missing_label}"
        self_fail=$((self_fail + 1))
        continue
      fi
      launched_lanes="${launched_lanes}${launched_lanes:+ }${lane_name}"
      (
        "${lane_function}" "${script}" "${temporary}"
        lane_status=$?
        printf '%d\n' "${lane_status}" >"${temporary}/${lane_name}.status"
      ) >"${temporary}/${lane_name}.out" 2>&1 &
    done

    wait
    for lane_name in ${launched_lanes}; do
      if [ ! -f "${temporary}/${lane_name}.status" ]; then
        printf 'SELFTEST FAIL %s lane did not record its result\n' "${lane_name}"
        self_fail=$((self_fail + 1))
        continue
      fi
      cat "${temporary}/${lane_name}.out"
      lane_status="$(cat "${temporary}/${lane_name}.status")"
      case "${lane_status}" in
        '' | *[!0-9]*)
          printf 'SELFTEST FAIL %s lane recorded an invalid result\n' "${lane_name}"
          self_fail=$((self_fail + 1))
          ;;
        *) self_fail=$((self_fail + lane_status)) ;;
      esac
    done
  }

  if NIMBUS_NETWORK_VERIFY_PLAN="${temporary}/missing-plan.md" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-plan.out" 2>&1; then
    printf 'SELFTEST FAIL missing plan unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV001 plan-in-HEAD' "${temporary}/missing-plan.out" ||
    grep -q '^PASS NNCV001 plan-in-HEAD' "${temporary}/missing-plan.out"; then
    printf 'SELFTEST FAIL missing plan did not produce an exclusive NNCV001 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing plan fails closed as NNCV001\n'
  fi

  if NIMBUS_NETWORK_VERIFY_INVENTORY="${temporary}/missing-inventory.json" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-inventory.out" 2>&1; then
    printf 'SELFTEST FAIL missing inventory unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV002 required-baseline-inputs' "${temporary}/missing-inventory.out" ||
    grep -q '^PASS NNCV002 required-baseline-inputs' "${temporary}/missing-inventory.out"; then
    printf 'SELFTEST FAIL missing inventory did not produce an exclusive NNCV002 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing inventory fails closed as NNCV002\n'
  fi

  if NIMBUS_NETWORK_VERIFY_DEPENDENCIES="${temporary}/missing-dependencies.json" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-dependencies.out" 2>&1; then
    printf 'SELFTEST FAIL missing dependency baseline unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV002 required-baseline-inputs' "${temporary}/missing-dependencies.out" ||
    grep -q '^PASS NNCV002 required-baseline-inputs' "${temporary}/missing-dependencies.out"; then
    printf 'SELFTEST FAIL missing dependency baseline did not produce an exclusive NNCV002 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing dependency baseline fails closed as NNCV002\n'
  fi

  invalid_checkpoint_plan="${temporary}/invalid-checkpoint-plan.md"
  if ! node - "${PLAN}" "${invalid_checkpoint_plan}" <<'NODE'
const fs = require("fs");
const [source, target] = process.argv.slice(2);
const text = fs.readFileSync(source, "utf8");
const invalid = text.replace(
  /^(\| Last checkpoint commit \|.*?`)[0-9a-f]{40}(`.*\|)$/m,
  "$1" + "0".repeat(40) + "$2",
);
if (invalid === text) process.exit(1);
fs.writeFileSync(target, invalid);
NODE
  then
    printf 'SELFTEST FAIL nonexistent checkpoint fixture could not be built\n'
    self_fail=$((self_fail + 1))
  elif NIMBUS_NETWORK_VERIFY_PLAN="${invalid_checkpoint_plan}" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/invalid-checkpoint.out" 2>&1; then
    printf 'SELFTEST FAIL nonexistent checkpoint unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV008 checkpoint-ledger-recoverable' "${temporary}/invalid-checkpoint.out" ||
    grep -q '^PASS NNCV008 checkpoint-ledger-recoverable' "${temporary}/invalid-checkpoint.out" ||
    ! grep -q 'Last checkpoint commit does not resolve: 0000000000000000000000000000000000000000' "${temporary}/invalid-checkpoint.out"; then
    printf 'SELFTEST FAIL nonexistent checkpoint did not produce the exact exclusive NNCV008 diagnostic\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS nonexistent checkpoint fails closed as NNCV008\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_LEGACY_PORT_AUTHORITY='synthetic.rs:1:struct PortManager' \
    "${script}" >"${temporary}/legacy-port-authority.out" 2>&1; then
    printf 'SELFTEST FAIL injected legacy port authority unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV005 no-duplicate-port-allocation-authority' "${temporary}/legacy-port-authority.out" ||
    grep -q '^PASS NNCV005 no-duplicate-port-allocation-authority' "${temporary}/legacy-port-authority.out" ||
    [ "$(grep -c '^FAIL ' "${temporary}/legacy-port-authority.out")" -ne 1 ] ||
    ! grep -q '^Summary: 33 passed, 1 failed$' "${temporary}/legacy-port-authority.out"; then
    printf 'SELFTEST FAIL legacy port authority did not produce an exclusive NNCV005 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS legacy port authority fails closed as NNCV005\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED="synthetic-unclassified.rs:1:TcpListener::bind" \
    "${script}" >"${temporary}/unclassified.out" 2>&1; then
    printf 'SELFTEST FAIL injected unclassified bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/unclassified.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/unclassified.out"; then
    printf 'SELFTEST FAIL injected bind did not produce an exclusive NNCV006 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nuse std::net::TcpListener;\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-cfg.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after test-only item unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-cfg.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-cfg.out"; then
    printf 'SELFTEST FAIL production bind after test-only item escaped NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS production bind after test-only item fails as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'struct Fixture {\n    #[cfg(test)]\n    test_only: bool,\n}\nimpl Fixture {\n    fn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n}\n' \
    "${script}" >"${temporary}/production-after-test-field.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) field unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-field.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-field.out"; then
    printf 'SELFTEST FAIL cfg(test) field hid a later production bind from NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) field cannot hide a later production bind from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority(\n    #[cfg(test)]\n    test_only: bool\n) { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-parameter.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) parameter unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-parameter.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-parameter.out"; then
    printf 'SELFTEST FAIL cfg(test) parameter hid its production function from NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) parameter cannot hide its production function from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nmacro_rules! listener_fixture { () => {}; }\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-macro.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) macro unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-macro.out" ||
    ! grep -q 'tcp-bind|production_authority' "${temporary}/production-after-test-macro.out"; then
    printf 'SELFTEST FAIL cfg(test) macro hid the following production bind from NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) macro cannot hide the following production bind from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nfixture! {\n    fn hidden() { TcpListener::bind("127.0.0.1:0"); }\n}\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-macro-invocation.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) brace macro unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-macro-invocation.out" ||
    ! grep -q 'tcp-bind|production_authority' "${temporary}/production-after-test-macro-invocation.out" ||
    grep -q 'tcp-bind|hidden' "${temporary}/production-after-test-macro-invocation.out"; then
    printf 'SELFTEST FAIL cfg(test) brace macro hid or leaked authority across its AST item boundary\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) brace macro cannot hide the following production bind from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'use std::net::TcpListener as Listener;\nfn production_authority() { Listener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/aliased-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL aliased listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/aliased-listener-bind.out" ||
    ! grep -q 'ambiguous socket authority alias is forbidden:' "${temporary}/aliased-listener-bind.out"; then
    printf 'SELFTEST FAIL aliased listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS aliased listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'type Listener = TcpListener;\nfn production_authority() { Listener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/type-aliased-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL type-aliased listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/type-aliased-listener-bind.out" ||
    ! grep -q 'socket authority type alias is forbidden:' "${temporary}/type-aliased-listener-bind.out"; then
    printf 'SELFTEST FAIL type-aliased listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS type-aliased listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority() { unsafe { UdpSocket::from_raw_socket(1); } }\n' \
    "${script}" >"${temporary}/udp-raw-socket-adoption.out" 2>&1; then
    printf 'SELFTEST FAIL UDP raw-socket adoption unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/udp-raw-socket-adoption.out" ||
    ! grep -q 'udp-from-raw-socket' "${temporary}/udp-raw-socket-adoption.out"; then
    printf 'SELFTEST FAIL UDP raw-socket adoption did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS UDP raw-socket adoption fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority(listener: &Socket) { listener.bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/instance-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL instance listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/instance-listener-bind.out" ||
    ! grep -q 'unclassified ambiguous bind/adoption operation:' "${temporary}/instance-listener-bind.out"; then
    printf 'SELFTEST FAIL instance listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS instance listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority(sockets: &[Socket], index: usize) { sockets[index].bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/postfix-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL postfix listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/postfix-listener-bind.out" ||
    ! grep -q 'ambiguous-instance-bind' "${temporary}/postfix-listener-bind.out"; then
    printf 'SELFTEST FAIL postfix listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS postfix listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority() { UnixDatagram::bind("/tmp/nimbus-verifier.sock"); }\n' \
    "${script}" >"${temporary}/unix-datagram-bind.out" 2>&1; then
    printf 'SELFTEST FAIL Unix datagram bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/unix-datagram-bind.out" ||
    ! grep -q 'unix-datagram-bind|production_authority' "${temporary}/unix-datagram-bind.out"; then
    printf 'SELFTEST FAIL Unix datagram bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS Unix datagram bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'struct Held(pub TcpListener);\n' \
    "${script}" >"${temporary}/tuple-listener-ownership.out" 2>&1; then
    printf 'SELFTEST FAIL tuple listener ownership unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/tuple-listener-ownership.out" ||
    ! grep -q 'listener-ownership-slot|<module>' "${temporary}/tuple-listener-ownership.out"; then
    printf 'SELFTEST FAIL tuple listener ownership did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS tuple listener ownership fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn handoff()\n    -> Result<\n        TcpListener,\n        std::io::Error,\n    >\n{\n    unreachable!()\n}\n' \
    "${script}" >"${temporary}/multiline-listener-return.out" 2>&1; then
    printf 'SELFTEST FAIL multiline listener return unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/multiline-listener-return.out" ||
    ! grep -q 'listener-return-handoff|handoff' "${temporary}/multiline-listener-return.out"; then
    printf 'SELFTEST FAIL multiline listener return did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS multiline listener return fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[path = "tests/fixture.rs"]\nmod fixture;\n' \
    "${script}" >"${temporary}/production-test-path-inclusion.out" 2>&1; then
    printf 'SELFTEST FAIL production inclusion of a test-exempt path unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-test-path-inclusion.out" ||
    ! grep -q 'production module/include references test-exempt source:' "${temporary}/production-test-path-inclusion.out"; then
    printf 'SELFTEST FAIL production inclusion of a test-exempt path did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS production inclusion of a test-exempt path fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn indirect() { let bind_listener = TcpListener::bind; let _ = bind_listener("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/bind-function-value.out" 2>&1; then
    printf 'SELFTEST FAIL bind function value unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/bind-function-value.out" ||
    ! grep -q 'tcp-bind|indirect' "${temporary}/bind-function-value.out"; then
    printf 'SELFTEST FAIL bind function value did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS bind function value fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'use libc::bind;\nfn bare() { unsafe { bind(0, std::ptr::null(), 0); } }\n' \
    "${script}" >"${temporary}/bare-bind-import.out" 2>&1; then
    printf 'SELFTEST FAIL bare imported bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/bare-bind-import.out" ||
    ! grep -q 'ambiguous-bind-function-import' "${temporary}/bare-bind-import.out" ||
    ! grep -q 'ambiguous-bare-bind-call' "${temporary}/bare-bind-import.out"; then
    printf 'SELFTEST FAIL bare imported bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS bare imported bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'trait Kind { type Listener; }\nstruct Host;\nimpl Kind for Host { type Listener = TcpListener; }\n' \
    "${script}" >"${temporary}/associated-listener-alias.out" 2>&1; then
    printf 'SELFTEST FAIL associated listener alias unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/associated-listener-alias.out" ||
    ! grep -q 'associated socket authority type alias is forbidden:' "${temporary}/associated-listener-alias.out"; then
    printf 'SELFTEST FAIL associated listener alias did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS associated listener alias fails closed as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'struct Fixture;\nimpl Fixture {\n    #[cfg(test)]\n    fixture! { TcpListener host_port }\n}\nfn production_without_bind() {\n    #[cfg(test)]\n    fixture! { TcpListener host_port }\n    let _ = match 1 {\n        #[cfg(test)]\n        0 => TcpListener::bind("127.0.0.1:0"),\n        _ => 1,\n    };\n}\n' \
    "${script}" >"${temporary}/cfg-associated-statement-nodes.out" 2>&1 || true
  if ! grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/cfg-associated-statement-nodes.out" ||
    grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/cfg-associated-statement-nodes.out"; then
    printf 'SELFTEST FAIL cfg(test) associated/statement nodes were misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) associated/statement nodes remain test-only for NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn consume_prebound(listener: TcpListener) { drop(listener); }\n' \
    "${script}" >"${temporary}/prebound-listener-consumer.out" 2>&1; then
    printf 'SELFTEST FAIL pre-bound listener consumer unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/prebound-listener-consumer.out" ||
    ! grep -q 'listener-ownership-slot|consume_prebound' "${temporary}/prebound-listener-consumer.out"; then
    printf 'SELFTEST FAIL pre-bound listener consumer did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS pre-bound listener consumer fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn find_available_listener_port() -> u16 { 7000 }\n' \
    "${script}" >"${temporary}/suspicious-port-allocation.out" 2>&1; then
    printf 'SELFTEST FAIL suspicious port allocator unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/suspicious-port-allocation.out" ||
    ! grep -q 'suspicious-port-allocation-definition' "${temporary}/suspicious-port-allocation.out"; then
    printf 'SELFTEST FAIL suspicious port allocator did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS suspicious port allocator fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn reserve_port() -> u16 { 7000 }\n' \
    "${script}" >"${temporary}/reserve-port-allocation.out" 2>&1; then
    printf 'SELFTEST FAIL reserve-port allocator unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/reserve-port-allocation.out" ||
    ! grep -q 'suspicious-port-allocation-definition|reserve_port' "${temporary}/reserve-port-allocation.out"; then
    printf 'SELFTEST FAIL reserve-port allocator did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS reserve-port allocator fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn configure_gvproxy() { let args = ["-ssh-port"]; }\n' \
    "${script}" >"${temporary}/provider-port-request.out" 2>&1; then
    printf 'SELFTEST FAIL provider port request unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/provider-port-request.out" ||
    ! grep -q 'gvproxy-ssh-port-request' "${temporary}/provider-port-request.out"; then
    printf 'SELFTEST FAIL provider port request did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS provider port request fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn configure_provider() {\n    let generic = ProviderRequest { host_port: 7000 };\n    let forward = crate::MachinePortForwardRequest { local: "127.0.0.1:7000" };\n    let netavark = crate::NetavarkRequest { port_mappings: mappings };\n}\n' \
    "${script}" >"${temporary}/generic-provider-port-request.out" 2>&1; then
    printf 'SELFTEST FAIL generic provider port request unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/generic-provider-port-request.out" ||
    ! grep -q 'provider-port-request|configure_provider' "${temporary}/generic-provider-port-request.out" ||
    ! grep -q 'machine-forwarder-port-request|configure_provider' "${temporary}/generic-provider-port-request.out" ||
    ! grep -q 'netavark-port-mapping-request|configure_provider' "${temporary}/generic-provider-port-request.out"; then
    printf 'SELFTEST FAIL generic provider port request did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS generic provider port request fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn configure_provider(host_port: u16) { let request = ProviderRequest { host_port }; }\n' \
    "${script}" >"${temporary}/shorthand-provider-port-request.out" 2>&1; then
    printf 'SELFTEST FAIL shorthand provider port request unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/shorthand-provider-port-request.out" ||
    ! grep -q 'provider-port-request|configure_provider' "${temporary}/shorthand-provider-port-request.out"; then
    printf 'SELFTEST FAIL shorthand provider port request did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS shorthand provider port request fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_SWAP_SITE_IDS='cli-main-direct-listener,cli-main-systemd-listener' \
    "${script}" >"${temporary}/same-path-site-swap.out" 2>&1; then
    printf 'SELFTEST FAIL same-path site swap unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/same-path-site-swap.out" ||
    ! grep -Eq 'authority (kind|symbol) .* is invalid for site' "${temporary}/same-path-site-swap.out"; then
    printf 'SELFTEST FAIL same-path site swap did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS same-path site swap fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_CORRUPT_SITE_DECLARATION='sandbox-pep-port-allocation' \
    "${script}" >"${temporary}/stale-site-declaration.out" 2>&1; then
    printf 'SELFTEST FAIL stale site declaration unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/stale-site-declaration.out" ||
    ! grep -q 'active site declaration missing or stale: sandbox-pep-port-allocation' "${temporary}/stale-site-declaration.out"; then
    printf 'SELFTEST FAIL stale site declaration did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS stale site declaration fails closed as NNCV006\n'
  fi

  bind_exemption_self_fail=0
  if [ ! -f "${BIND_EXEMPTION_SELF_TESTS}" ]; then
    printf 'SELFTEST FAIL bind-exemption self-test helper is missing: %s\n' "${BIND_EXEMPTION_SELF_TESTS}"
    self_fail=$((self_fail + 1))
  else
    # shellcheck source=scripts/nimbus-network-control-plane/bind-exemption-self-tests.sh
    . "${BIND_EXEMPTION_SELF_TESTS}"
    run_nnc46f_bind_exemption_self_tests "${script}" "${temporary}" "${INVENTORY}" ||
      bind_exemption_self_fail=$?
    self_fail=$((self_fail + bind_exemption_self_fail))
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_CLASSIFIED_OCCURRENCE="__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs|tcp-bind|first_authority|1" \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn first_authority() { TcpListener::bind("127.0.0.1:0"); }\nfn second_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/second-bind-in-classified-file.out" 2>&1 || true
  if grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/second-bind-in-classified-file.out" ||
    ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/second-bind-in-classified-file.out"; then
    printf 'SELFTEST FAIL second bind in a classified file escaped NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS second bind in a classified file fails as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_CLASSIFIED_OCCURRENCE="__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs|tcp-bind|deleted_authority|1" \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_without_bind() {}\n' \
    "${script}" >"${temporary}/stale-bind-classification.out" 2>&1 || true
  if ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/stale-bind-classification.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/stale-bind-classification.out" ||
    ! grep -q 'stale authority occurrence classification: .*deleted_authority' "${temporary}/stale-bind-classification.out"; then
    printf 'SELFTEST FAIL stale bind classification did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS stale bind classification fails as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nmod tests { fn listener_fixture() { TcpListener::bind("127.0.0.1:0"); } }\nfn production_without_bind() {}\n' \
    "${script}" >"${temporary}/test-module-bind.out" 2>&1 || true
  if ! grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/test-module-bind.out" ||
    grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/test-module-bind.out"; then
    printf 'SELFTEST FAIL cfg(test) module bind was misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) module bind remains test-only for NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_CORE_SCAN_ROOT="${temporary}/missing-core-source" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-core-source.out" 2>&1; then
    printf 'SELFTEST FAIL missing core source unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV010 core-runtime-foundation-invariants' "${temporary}/missing-core-source.out" ||
    grep -q '^PASS NNCV010 core-runtime-foundation-invariants' "${temporary}/missing-core-source.out"; then
    printf 'SELFTEST FAIL missing core source did not produce an exclusive NNCV010 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing core source fails closed as NNCV010\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='nimbus-tenant' \
    "${script}" >"${temporary}/forbidden-workspace-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected upper workspace dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-workspace-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-workspace-dependency.out"; then
    printf 'SELFTEST FAIL upper workspace dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS upper workspace dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='axum' \
    "${script}" >"${temporary}/forbidden-transport-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected transport dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-transport-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-transport-dependency.out"; then
    printf 'SELFTEST FAIL transport dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS transport dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='aws-sdk-ec2' \
    "${script}" >"${temporary}/forbidden-cloud-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected cloud SDK dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-cloud-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-cloud-dependency.out"; then
    printf 'SELFTEST FAIL cloud SDK dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cloud SDK dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT='fn bind_effect() { std::net::TcpListener::bind("127.0.0.1:0"); }' \
    "${script}" >"${temporary}/forbidden-effect.out" 2>&1; then
    printf 'SELFTEST FAIL injected provider effect unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-effect.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-effect.out"; then
    printf 'SELFTEST FAIL injected provider effect did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected provider effect fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_DUPLICATE_DEFINITION='pub struct NetworkPlan;' \
    "${script}" >"${temporary}/duplicate-definition.out" 2>&1; then
    printf 'SELFTEST FAIL injected duplicate definition unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV013 single-network-definition-owner' "${temporary}/duplicate-definition.out" ||
    grep -q '^PASS NNCV013 single-network-definition-owner' "${temporary}/duplicate-definition.out"; then
    printf 'SELFTEST FAIL injected duplicate definition did not produce an exclusive NNCV013 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected duplicate definition fails closed as NNCV013\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_ADDRESS_IDENTITY='pub struct NetworkSegmentId(Cidr);' \
    "${script}" >"${temporary}/address-identity.out" 2>&1; then
    printf 'SELFTEST FAIL injected address identity unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV014 address-is-not-network-identity' "${temporary}/address-identity.out" ||
    grep -q '^PASS NNCV014 address-is-not-network-identity' "${temporary}/address-identity.out"; then
    printf 'SELFTEST FAIL injected address identity did not produce an exclusive NNCV014 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected address identity fails closed as NNCV014\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT=$'// TcpListener::bind is documentation only.\nconst NOTE: &str = "std::net::TcpListener::bind";\nfn pure_contract() {}\n' \
    "${script}" >"${temporary}/non-code-effect-terms.out" 2>&1 || true
  if ! grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/non-code-effect-terms.out" ||
    grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/non-code-effect-terms.out"; then
    printf 'SELFTEST FAIL comment/string provider terms were misclassified as effects\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS comment/string provider terms remain non-effects for NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_NETWORK_SCAN_ROOT="${temporary}/missing-network-source" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-network-source.out" 2>&1; then
    printf 'SELFTEST FAIL missing network source unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/missing-network-source.out" ||
    ! grep -q '^FAIL NNCV013 single-network-definition-owner' "${temporary}/missing-network-source.out" ||
    ! grep -q '^FAIL NNCV014 address-is-not-network-identity' "${temporary}/missing-network-source.out"; then
    printf 'SELFTEST FAIL missing network source did not fail all source-contract conditions\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing network source fails closed for NNCV012-NNCV014\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn bad() { LocalPortLeaseAuthority::open("foreign"); }' \
    "${script}" >"${temporary}/composition-primitive-open.out" 2>&1; then
    printf 'SELFTEST FAIL unclassified primitive composition open unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-primitive-open.out" ||
    grep -q '^PASS NNCV015 local-network-composition-census' "${temporary}/composition-primitive-open.out" ||
    ! grep -q 'primitive-port-authority-open|bad' "${temporary}/composition-primitive-open.out"; then
    printf 'SELFTEST FAIL primitive composition open did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS primitive composition open fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn bad(config: Config) { KrunSandboxBackend::new(config); }' \
    "${script}" >"${temporary}/composition-direct-backend.out" 2>&1; then
    printf 'SELFTEST FAIL unclassified direct backend construction unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-direct-backend.out" ||
    ! grep -q 'direct-krun-backend-construction|bad' "${temporary}/composition-direct-backend.out"; then
    printf 'SELFTEST FAIL direct backend construction did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS direct backend construction fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn bad() { NetworkAttachmentProviderRegistration::new(provider(), endpoints(), lifecycle(), sovereignty()); }' \
    "${script}" >"${temporary}/composition-fabricated-registration.out" 2>&1; then
    printf 'SELFTEST FAIL fabricated attachment registration unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-fabricated-registration.out" ||
    ! grep -q 'attachment-registration-construction|bad' "${temporary}/composition-fabricated-registration.out"; then
    printf 'SELFTEST FAIL fabricated attachment registration did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS fabricated attachment registration fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='use nimbus_network::LocalNetworkManager as HiddenManager;' \
    "${script}" >"${temporary}/composition-import-alias.out" 2>&1; then
    printf 'SELFTEST FAIL composition authority import alias unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-import-alias.out" ||
    ! grep -q 'composition-type-import-alias|<module>' "${temporary}/composition-import-alias.out"; then
    printf 'SELFTEST FAIL composition import alias did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS composition import alias fails closed as NNCV015\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nfn hidden() { LocalPortLeaseAuthority::open("test-only"); }\nfn production_without_composition() {}\n' \
    "${script}" >"${temporary}/composition-cfg-test.out" 2>&1 || true
  if ! grep -q '^PASS NNCV015 local-network-composition-census' "${temporary}/composition-cfg-test.out" ||
    grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-cfg-test.out"; then
    printf 'SELFTEST FAIL cfg(test) composition call was misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) composition call remains test-only for NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_DROP_COMPOSITION_KEY='crates/nimbus-cli/src/network_composition.rs|manager-bootstrap|claim|1' \
    "${script}" >"${temporary}/composition-stale-classification.out" 2>&1; then
    printf 'SELFTEST FAIL stale composition classification unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-stale-classification.out" ||
    ! grep -q 'stale production network authority classification: crates/nimbus-cli/src/network_composition.rs|manager-bootstrap|claim|1' "${temporary}/composition-stale-classification.out"; then
    printf 'SELFTEST FAIL stale composition classification did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS stale composition classification fails closed as NNCV015\n'
  fi

  composition_self_fail=0
  if [ ! -f "${COMPOSITION_CENSUS_SELF_TESTS}" ]; then
    printf 'SELFTEST FAIL composition-census self-test helper is missing: %s\n' "${COMPOSITION_CENSUS_SELF_TESTS}"
    self_fail=$((self_fail + 1))
  else
    # shellcheck source=scripts/nimbus-network-control-plane/composition-census-self-tests.sh
    . "${COMPOSITION_CENSUS_SELF_TESTS}"
    run_nnc46f_composition_census_self_tests "${script}" "${temporary}" ||
      composition_self_fail=$?
    self_fail=$((self_fail + composition_self_fail))
  fi

  if NIMBUS_NETWORK_VERIFY_SOVEREIGNTY_HELPER="${temporary}/missing-sovereignty-helper.py" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-sovereignty-helper.out" 2>&1; then
    printf 'SELFTEST FAIL missing sovereignty helper did not fail closed\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV016 sovereignty-tripwire-contract' "${temporary}/missing-sovereignty-helper.out" ||
    grep -q '^PASS NNCV016 sovereignty-tripwire-contract' "${temporary}/missing-sovereignty-helper.out"; then
    printf 'SELFTEST FAIL missing sovereignty helper did not fail NNCV016 exclusively\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing sovereignty helper fails closed as NNCV016\n'
  fi

  if [ ! -f "${SOVEREIGNTY_TRIPWIRE_SELF_TESTS}" ]; then
    printf 'SELFTEST FAIL sovereignty-tripwire self-test helper is missing: %s\n' "${SOVEREIGNTY_TRIPWIRE_SELF_TESTS}"
    self_fail=$((self_fail + 1))
  elif ! timeout 120 bash "${SOVEREIGNTY_TRIPWIRE_SELF_TESTS}" \
    >"${temporary}/sovereignty-tripwire-self-tests.out" 2>&1; then
    printf 'SELFTEST FAIL sovereignty-tripwire mutation suite failed\n'
    sed -n '1,200p' "${temporary}/sovereignty-tripwire-self-tests.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^Ran 70 tests' "${temporary}/sovereignty-tripwire-self-tests.out" ||
    ! grep -q '^OK$' "${temporary}/sovereignty-tripwire-self-tests.out"; then
    printf 'SELFTEST FAIL sovereignty-tripwire mutation suite count/result is not exact\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS sovereignty-tripwire mutation suite passes 70 tests\n'
  fi

  # Each lane invokes the same read-only aggregate child with disjoint
  # mutation variables and output paths. Batches cap concurrent repository
  # scans at two so the bind scanner retains enough process and memory headroom.
  # The overlap does not change any exclusive failure assertion.
  run_parallel_self_test_batch \
    'nnc52a|run_nnc52a_attachment_ordering_self_tests|attachment-ordering' \
    'nnc52d|run_nnc52d_startup_orphan_self_tests|startup-orphan'
  run_parallel_self_test_batch \
    'nnc53|run_nnc53_attachment_readiness_self_tests|attachment-readiness' \
    'nnc54|run_nnc54_attachment_crash_self_tests|attachment-crash'
  run_parallel_self_test_batch \
    'nnc54a|run_nnc54a_machine_forwarded_batch_self_tests|machine-forwarded-batch' \
    'nnc55|run_nnc55_effect_locality_self_tests|effect-locality'
  run_parallel_self_test_batch \
    'nnc56|run_nnc56_side_effect_free_inspection_self_tests|side-effect-free inspection' \
    'nnc61|run_nnc61_compute_network_manager_self_tests|compute-network-manager'
  run_parallel_self_test_batch \
    'nnc61a|run_nnc61a_compute_node_workload_coordinator_self_tests|compute-node-workload-coordinator' \
    'nnc61e|run_nnc61e_recovery_decision_self_tests|recovery-decision'

  if [ ! -f "${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV028 workload-network-plan compiler helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}" --self-test \
    >"${temporary}/nnc62-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV028 workload-network-plan compiler mutation suite failed\n'
    sed -n '1,120p' "${temporary}/nnc62-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.2 contract self-test: 7 passed, 0 failed$' \
    "${temporary}/nnc62-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV028 workload-network-plan compiler mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,120p' "${temporary}/nnc62-contract-self-test.out"
  fi

  if [ ! -f "${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV029 workload-network-plan durability helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}" --self-test \
    >"${temporary}/nnc62a-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV029 workload-network-plan durability mutation suite failed\n'
    sed -n '1,160p' "${temporary}/nnc62a-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.2a contract self-test: 10 passed, 0 failed$' \
    "${temporary}/nnc62a-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV029 workload-network-plan durability mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,160p' "${temporary}/nnc62a-contract-self-test.out"
  fi

  if [ ! -f "${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV030 workload-saga ingress helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}" --self-test \
    >"${temporary}/nnc61e1-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV030 workload-saga ingress mutation suite failed\n'
    sed -n '1,180p' "${temporary}/nnc61e1-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.1e1 ingress contract self-test: 13 passed, 0 failed$' \
    "${temporary}/nnc61e1-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV030 workload-saga ingress mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,180p' "${temporary}/nnc61e1-contract-self-test.out"
  fi

  if [ ! -f "${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV031 workload executable helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}" --self-test \
    >"${temporary}/nnc63a-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV031 workload executable mutation suite failed\n'
    sed -n '1,180p' "${temporary}/nnc63a-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.3a executable contract self-test: 13 passed, 0 failed$' \
    "${temporary}/nnc63a-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV031 workload executable mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,180p' "${temporary}/nnc63a-contract-self-test.out"
  fi

  if [ ! -f "${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV032 workload provision decision helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}" --self-test \
    >"${temporary}/nnc63b-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV032 workload provision decision mutation suite failed\n'
    sed -n '1,220p' "${temporary}/nnc63b-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC6\.3b provision contract self-test: 36 passed, 0 failed$' \
    "${temporary}/nnc63b-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV032 workload provision decision mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,220p' "${temporary}/nnc63b-contract-self-test.out"
  fi

  if [ ! -f "${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV033 workload provision dispatch helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}" --self-test \
    >"${temporary}/nnc64-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV033 workload provision dispatch mutation suite failed\n'
    sed -n '1,260p' "${temporary}/nnc64-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC6\.4 provider dispatch contract self-test: 48 passed, 0 failed$' \
    "${temporary}/nnc64-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV033 workload provision dispatch mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,260p' "${temporary}/nnc64-contract-self-test.out"
  fi

  if [ "${self_fail}" -ne 0 ]; then
    printf 'self-test: %d failed\n' "${self_fail}"
    exit 1
  fi
  printf 'self-test: 325 passed, 0 failed\n'
}

if [ "${1:-}" = "--self-test" ]; then
  # Retained verifier mutations assert one exclusive historical diagnostic.
  # The aggregate mutation fixtures use green baselines for the historical
  # NNCV032 contract and the intentionally red NNCV033 contract. Their
  # concept-owned 36- and 48-mutation suites still run below.
  NIMBUS_NETWORK_NNC63B_AGGREGATE_SELF_TEST_BASELINE=1
  NIMBUS_NETWORK_NNC64_AGGREGATE_SELF_TEST_BASELINE=1
  export NIMBUS_NETWORK_NNC63B_AGGREGATE_SELF_TEST_BASELINE
  export NIMBUS_NETWORK_NNC64_AGGREGATE_SELF_TEST_BASELINE
  run_self_test
  exit 0
fi
if [ "${1:-}" = "--self-test-nnc61e" ]; then
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-network-nnc61e-self-test.XXXXXX")" || {
    printf 'NNC6.1e self-test: unable to create temporary directory\n'
    exit 1
  }
  trap 'rm -rf "${temporary}"' EXIT
  run_nnc61e_recovery_decision_self_tests \
    "${REPO_ROOT}/scripts/verify-nimbus-network-control-plane.sh" "${temporary}"
  status=$?
  if [ "${status}" -eq 0 ]; then
    printf 'NNC6.1e self-test: 10 passed, 0 failed\n'
    exit 0
  fi
  printf 'NNC6.1e self-test: %d failed\n' "${status}"
  exit 1
fi
if [ $# -ne 0 ]; then
  printf 'usage: %s [--self-test|--self-test-nnc61e]\n' "$0" >&2
  exit 2
fi

verify_nnc61d_durable_workload_saga_store() {
  if [ ! -f "${NNC61D_WORKLOAD_SAGA_AUTHORITY_CONTRACT}" ]; then
    fail "NNCV027" "durable-workload-saga-authority" \
      "missing ${NNC61D_WORKLOAD_SAGA_AUTHORITY_CONTRACT}"
    return
  fi

  durable_error="$(
    bash "${NNC61D_WORKLOAD_SAGA_AUTHORITY_CONTRACT}" durable-store 2>&1
  )"
  durable_status=$?
  recovery_error="$(
    bash "${NNC61D_WORKLOAD_SAGA_AUTHORITY_CONTRACT}" recovery-decisions 2>&1
  )"
  recovery_status=$?
  if [ "${durable_status}" -eq 0 ] && [ "${recovery_status}" -eq 0 ]; then
    pass "NNCV027" "durable-workload-saga-authority"
  else
    error="${durable_error}"
    if [ -n "${recovery_error}" ]; then
      error="${error}${error:+$'\n'}${recovery_error}"
    fi
    fail "NNCV027" "durable-workload-saga-authority" "${error}"
  fi
}

verify_nnc62_workload_network_plan_compiler() {
  if [ ! -f "${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}" ]; then
    fail "NNCV028" "workload-network-plan-compiler" \
      "missing ${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}"
    return
  fi

  compiler_error="$(
    bash "${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}" --check 2>&1
  )"
  compiler_contract_exit=$?
  if [ "${compiler_contract_exit}" -eq 0 ]; then
    pass "NNCV028" "workload-network-plan-compiler"
  else
    fail "NNCV028" "workload-network-plan-compiler" "${compiler_error}"
  fi
}

verify_nnc62a_workload_network_plan_durability() {
  if [ ! -f "${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}" ]; then
    fail "NNCV029" "workload-network-plan-durability" \
      "missing ${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}"
    return
  fi

  durability_error="$(
    bash "${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}" --check 2>&1
  )"
  durability_contract_exit=$?
  if [ "${durability_contract_exit}" -eq 0 ]; then
    pass "NNCV029" "workload-network-plan-durability"
  else
    fail "NNCV029" "workload-network-plan-durability" "${durability_error}"
  fi
}

verify_nnc61e1_workload_saga_ingress() {
  if [ ! -f "${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}" ]; then
    fail "NNCV030" "durable-workload-saga-ingress" \
      "missing ${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}"
    return
  fi

  ingress_error="$(
    bash "${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}" --check 2>&1
  )"
  ingress_contract_exit=$?
  if [ "${ingress_contract_exit}" -eq 0 ]; then
    pass "NNCV030" "durable-workload-saga-ingress"
  else
    fail "NNCV030" "durable-workload-saga-ingress" "${ingress_error}"
  fi
}

verify_nnc63a_workload_executable_carrier() {
  if [ ! -f "${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}" ]; then
    fail "NNCV031" "strict-workload-executable-carrier" \
      "missing ${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}"
    return
  fi

  executable_error="$(
    bash "${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}" --check 2>&1
  )"
  executable_contract_exit=$?
  if [ "${executable_contract_exit}" -eq 0 ]; then
    pass "NNCV031" "strict-workload-executable-carrier"
  else
    fail "NNCV031" "strict-workload-executable-carrier" "${executable_error}"
  fi
}

verify_nnc63b_workload_provision_decision() {
  if [ "${NIMBUS_NETWORK_NNC63B_AGGREGATE_SELF_TEST_BASELINE:-0}" = "1" ]; then
    pass "NNCV032" "pure-workload-provision-decision"
    return
  fi
  if [ ! -f "${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}" ]; then
    fail "NNCV032" "pure-workload-provision-decision" \
      "missing ${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}"
    return
  fi

  provision_error="$(
    bash "${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}" --check 2>&1
  )"
  provision_contract_exit=$?
  if [ "${provision_contract_exit}" -eq 0 ]; then
    pass "NNCV032" "pure-workload-provision-decision"
  else
    fail "NNCV032" "pure-workload-provision-decision" "${provision_error}"
  fi
}

verify_nnc64_workload_provision_dispatch() {
  if [ "${NIMBUS_NETWORK_NNC64_AGGREGATE_SELF_TEST_BASELINE:-0}" = "1" ]; then
    pass "NNCV033" "atomic-workload-provision-dispatch"
    return
  fi
  if [ ! -f "${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}" ]; then
    fail "NNCV033" "atomic-workload-provision-dispatch" \
      "missing ${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}"
    return
  fi

  dispatch_error="$(
    bash "${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}" --check 2>&1
  )"
  dispatch_contract_exit=$?
  if [ "${dispatch_contract_exit}" -eq 0 ]; then
    pass "NNCV033" "atomic-workload-provision-dispatch"
  else
    fail "NNCV033" "atomic-workload-provision-dispatch" "${dispatch_error}"
  fi
}

printf 'Nimbus network control-plane static verifier\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

require_tools
verify_plan_in_head
verify_baseline_inputs
verify_network_crate
verify_network_dependency_contract
verify_single_port_authority
verify_bind_census
verify_dependency_baseline
verify_checkpoint_ledger
verify_routing_owner
verify_foundation_invariants
verify_portable_vocabulary_owner
verify_forbidden_network_dependencies_effects
verify_single_network_definition_owner
verify_address_is_not_network_identity
verify_local_network_composition_census
verify_sovereignty_tripwire_contract
verify_nnc52a_attachment_effect_ordering
verify_nnc52d_startup_orphan_reconciliation
verify_nnc53_attachment_readiness
verify_nnc54_attachment_crash_convergence
verify_nnc54a_machine_forwarded_batch_convergence
verify_nnc55_sandbox_effect_locality
verify_nnc55_sealed_effect_capabilities
verify_nnc56_side_effect_free_sandbox_inspection
verify_nnc61_compute_network_manager_injection
verify_nnc61a_compute_node_workload_coordinator
verify_nnc61d_durable_workload_saga_store
verify_nnc62_workload_network_plan_compiler
verify_nnc62a_workload_network_plan_durability
verify_nnc61e1_workload_saga_ingress
verify_nnc63a_workload_executable_carrier
verify_nnc63b_workload_provision_decision
verify_nnc64_workload_provision_dispatch

printf '\nSummary: %d passed, %d failed\n' "${PASS_COUNT}" "${FAIL_COUNT}"
if [ "${FAIL_COUNT}" -ne 0 ]; then
  exit 1
fi
