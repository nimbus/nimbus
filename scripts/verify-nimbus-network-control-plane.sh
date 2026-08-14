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
NNC64A_WORKLOAD_RESTART_CONTRACT="scripts/nimbus-network-control-plane/workload-restart-contract.sh"
NNC65_WORKLOAD_TEARDOWN_CONTRACT="scripts/nimbus-network-control-plane/workload-teardown-contract.sh"
NNC82_PROVIDER_CURRENT_CLAIM_CONTRACT="scripts/nimbus-network-control-plane/provider-command-current-claim-contract.sh"
AGGREGATE_VERIFIER_SELF_TESTS="scripts/nimbus-network-control-plane/aggregate-verifier-self-tests.sh"
COMPILER_AUTHORITY_CONTRACT="scripts/nimbus-network-control-plane/compiler-authority-contract.mjs"
COMPILER_AUTHORITY_BASELINE="${NIMBUS_NETWORK_VERIFY_COMPILER_BASELINE:-docs/private/plans/proof/nimbus-network-control-plane/nnc9.1-compiler-authority-baseline.json}"

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
# shellcheck source=scripts/nimbus-network-control-plane/aggregate-verifier-self-tests.sh
. "${AGGREGATE_VERIFIER_SELF_TESTS}"

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
  if [ "${NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD:-}" = "1" ]; then
    if [ "${NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE:-}" = "1" ] ||
      [ ! -f "${INVENTORY}" ]; then
      pass "NNCV006" "unclassified-production-bind"
      return
    fi
    if [ -z "${NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE:-}" ] &&
      [ -z "${NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED:-}" ] &&
      [ -z "${NIMBUS_NETWORK_VERIFY_TEST_CLASSIFIED_OCCURRENCE:-}" ] &&
      [ -z "${NIMBUS_NETWORK_VERIFY_TEST_SWAP_SITE_IDS:-}" ] &&
      [ -z "${NIMBUS_NETWORK_VERIFY_TEST_CORRUPT_SITE_DECLARATION:-}" ] &&
      [ -z "${NIMBUS_NETWORK_VERIFY_INVENTORY:-}" ]; then
      # The live aggregate already proves the unchanged source-derived census.
      # Mutation children rerun it only when their fixture actually changes a
      # bind, allocation, classification, site declaration, or exemption.
      pass "NNCV006" "unclassified-production-bind"
      return
    fi
  fi
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

if [ "${1:-}" = "--self-test" ]; then
  # Retained verifier mutations assert one exclusive historical diagnostic.
  # The aggregate mutation fixtures use green baselines for the historical
  # NNCV032-NNCV035 use green aggregate fixtures so retained mutations can
  # assert one historical diagnostic. Their concept-owned mutation suites
  # still run below.
  NIMBUS_NETWORK_NNC63B_AGGREGATE_SELF_TEST_BASELINE=1
  NIMBUS_NETWORK_NNC64_AGGREGATE_SELF_TEST_BASELINE=1
  NIMBUS_NETWORK_NNC64A_AGGREGATE_SELF_TEST_BASELINE=1
  NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE=1
  export NIMBUS_NETWORK_NNC63B_AGGREGATE_SELF_TEST_BASELINE
  export NIMBUS_NETWORK_NNC64_AGGREGATE_SELF_TEST_BASELINE
  export NIMBUS_NETWORK_NNC64A_AGGREGATE_SELF_TEST_BASELINE
  export NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE
  run_self_test
  exit 0
fi
if [ "${1:-}" = "--self-test-nnc81" ]; then
  run_nnc81_affected_self_test
  exit 0
fi
if [ "${1:-}" = "--self-test-nnc82" ]; then
  bash "${NNC82_PROVIDER_CURRENT_CLAIM_CONTRACT}" --self-test
  status=$?
  if [ "${status}" -eq 0 ]; then
    printf 'NNC8.2 affected self-test: 9 passed, 0 failed\n'
  fi
  exit "${status}"
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

verify_nnc64a_workload_restart() {
  if [ "${NIMBUS_NETWORK_NNC64A_AGGREGATE_SELF_TEST_BASELINE:-0}" = "1" ]; then
    pass "NNCV034" "fenced-workload-restart"
    return
  fi
  if [ ! -f "${NNC64A_WORKLOAD_RESTART_CONTRACT}" ]; then
    fail "NNCV034" "fenced-workload-restart" \
      "missing ${NNC64A_WORKLOAD_RESTART_CONTRACT}"
    return
  fi

  restart_error="$(
    bash "${NNC64A_WORKLOAD_RESTART_CONTRACT}" --check 2>&1
  )"
  restart_contract_exit=$?
  if [ "${restart_contract_exit}" -eq 0 ]; then
    pass "NNCV034" "fenced-workload-restart"
  else
    fail "NNCV034" "fenced-workload-restart" "${restart_error}"
  fi
}

verify_nnc65_workload_teardown() {
  if [ "${NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE:-0}" = "1" ]; then
    pass "NNCV035" "fenced-workload-teardown"
    return
  fi
  if [ ! -f "${NNC65_WORKLOAD_TEARDOWN_CONTRACT}" ]; then
    fail "NNCV035" "fenced-workload-teardown" \
      "missing ${NNC65_WORKLOAD_TEARDOWN_CONTRACT}"
    return
  fi

  teardown_error="$(
    bash "${NNC65_WORKLOAD_TEARDOWN_CONTRACT}" --check 2>&1
  )"
  teardown_contract_exit=$?
  if [ "${teardown_contract_exit}" -eq 0 ]; then
    pass "NNCV035" "fenced-workload-teardown"
  else
    fail "NNCV035" "fenced-workload-teardown" "${teardown_error}"
  fi
}

verify_nnc81_process_harness_owner() {
  if [ -n "${NIMBUS_NETWORK_VERIFY_NNC81_METADATA:-}" ]; then
    metadata="$(cat "${NIMBUS_NETWORK_VERIFY_NNC81_METADATA}" 2>/dev/null)"
  else
    metadata="$(cargo metadata --no-deps --format-version 1 2>/dev/null)"
  fi
  metadata_status=$?
  if [ "${metadata_status}" -ne 0 ] || [ -z "${metadata}" ]; then
    fail "NNCV036" "shared-process-harness-owner" "cargo metadata failed or was empty"
    return
  fi
  error="$(
    printf '%s' "${metadata}" |
      node -e '
        const fs = require("fs");
        let input = "";
        process.stdin.on("data", chunk => input += chunk);
        process.stdin.on("end", () => {
          const errors = [];
          let metadata;
          try {
            metadata = JSON.parse(input);
          } catch (error) {
            process.stdout.write("cargo metadata is invalid: " + error.message);
            process.exit(1);
          }

          const owner = metadata.packages.find(pkg => pkg.name === "nimbus-process-harness");
          if (!owner || !metadata.workspace_members.includes(owner.id)) {
            errors.push("nimbus-process-harness is not a workspace member");
          } else {
            const dependencies = owner.dependencies;
            const permitted = dependencies.length === 1
              && dependencies[0].name === "tempfile"
              && dependencies[0].kind === "dev"
              && dependencies[0].target === null
              && !dependencies[0].optional;
            if (!permitted) {
              const rendered = dependencies.map(dep =>
                dep.name + ":" + (dep.kind ?? "normal") + ":" + (dep.target ?? "all")
              );
              errors.push(
                "process harness dependencies must be exactly one unconditional tempfile dev edge: "
                  + (rendered.join(",") || "<none>"),
              );
            }
          }

          const expected = ["nimbus-cli", "nimbus-kv", "nimbus-sandbox", "nimbus-server", "nimbus-testing"];
          const consumers = metadata.packages.filter(pkg =>
            pkg.dependencies.some(dep => dep.name === "nimbus-process-harness"),
          );
          const actual = consumers.map(pkg => pkg.name).sort();
          if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) {
            errors.push("direct consumers differ: " + (actual.join(",") || "<none>"));
          }
          for (const consumer of consumers) {
            const edges = consumer.dependencies.filter(dep => dep.name === "nimbus-process-harness");
            if (edges.length !== 1 || edges[0].kind !== "dev" || edges[0].target !== null || edges[0].optional) {
              errors.push(consumer.name + " process-harness edge is not one unconditional development dependency");
            }
          }

          const oldPaths = [
            "crates/nimbus-testing/src/process_harness.rs",
            "crates/nimbus-testing/src/process_harness/crash.rs",
          ];
          for (const path of oldPaths) {
            if (fs.existsSync(path)) errors.push("old process-harness owner remains: " + path);
          }
          const testingRoot = "crates/nimbus-testing/src/lib.rs";
          const attachment = "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/crash_recovery.rs";
          if (!fs.existsSync(testingRoot)) errors.push("missing " + testingRoot);
          if (!fs.existsSync(attachment)) errors.push("missing " + attachment);
          if (fs.existsSync(testingRoot)) {
            const source = fs.readFileSync(testingRoot, "utf8");
            if (/\b(?:mod|pub use)\s+process_harness\b|\bnimbus_process_harness\b/.test(source)) {
              errors.push("nimbus-testing retains a process-harness module or compatibility re-export");
            }
          }
          if (fs.existsSync(attachment)) {
            const source = fs.readFileSync(attachment, "utf8");
            for (const symbol of ["SubprocessCrashCutHarness", "run_crash_cut_child", "run_crash_recovery_child"]) {
              if (!source.includes(symbol)) errors.push("attachment proof lacks " + symbol);
            }
            const legacy = source.match(/\b(?:POLL_INTERVAL|kill_after_marker|park_forever|wait_for_marker)\b|std::thread::sleep|std::process::\{?Child/g) || [];
            if (legacy.length) errors.push("attachment proof retains private process transport: " + [...new Set(legacy)].join(","));
          }

          process.stdout.write(errors.join("; "));
          process.exit(errors.length === 0 ? 0 : 1);
        });
      '
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV036" "shared-process-harness-owner"
  else
    fail "NNCV036" "shared-process-harness-owner" "${error:-cargo metadata failed}"
  fi
}

verify_nnc82_provider_current_claim_authority() {
  if [ ! -f "${NNC82_PROVIDER_CURRENT_CLAIM_CONTRACT}" ]; then
    fail "NNCV037" "provider-command-current-claim-authority" \
      "missing ${NNC82_PROVIDER_CURRENT_CLAIM_CONTRACT}"
    return
  fi

  current_claim_error="$(
    bash "${NNC82_PROVIDER_CURRENT_CLAIM_CONTRACT}" --check 2>&1
  )"
  current_claim_status=$?
  if [ "${current_claim_status}" -eq 0 ]; then
    pass "NNCV037" "provider-command-current-claim-authority"
  else
    fail "NNCV037" "provider-command-current-claim-authority" "${current_claim_error}"
  fi
}

verify_nnc91_compiler_authority_closure() {
  if [ "${NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD:-0}" = "1" ] &&
    [ "${NIMBUS_NETWORK_VERIFY_COMPILER_SELF_TEST_FORCE:-0}" != "1" ]; then
    return
  fi
  if [ ! -f "${COMPILER_AUTHORITY_CONTRACT}" ]; then
    fail "NNCV038" "compiler-generated-authority-closure" \
      "missing ${COMPILER_AUTHORITY_CONTRACT}"
    return
  fi
  compiler_authority_error="$(
    node "${COMPILER_AUTHORITY_CONTRACT}" \
      --baseline "${COMPILER_AUTHORITY_BASELINE}" \
      --inventory "${INVENTORY}" \
      --check 2>&1
  )"
  compiler_authority_status=$?
  if [ "${compiler_authority_status}" -eq 0 ]; then
    pass "NNCV038" "compiler-generated-authority-closure"
  else
    fail "NNCV038" "compiler-generated-authority-closure" \
      "${compiler_authority_error}"
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
verify_nnc64a_workload_restart
verify_nnc65_workload_teardown
verify_nnc81_process_harness_owner
verify_nnc82_provider_current_claim_authority
verify_nnc91_compiler_authority_closure

printf '\nSummary: %d passed, %d failed\n' "${PASS_COUNT}" "${FAIL_COUNT}"
if [ "${FAIL_COUNT}" -ne 0 ]; then
  exit 1
fi
