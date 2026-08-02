#!/usr/bin/env bash
# Static NNC6.2 contract for the pure admitted workload-network plan compiler.

set -u

REPO_ROOT="${NIMBUS_NETWORK_NNC62_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
SCRIPT_PATH="${REPO_ROOT}/scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh"
ITEM_COMMIT="0977c17d93f3b39f18b33d504193c6eee6e9ba50"
COMPUTE_COMPILER="crates/nimbus-compute/src/workload_network_plan.rs"
PORTABLE_PAYLOAD="crates/nimbus-workloads/src/network_plan.rs"
WORKLOAD_SAGA="crates/nimbus-workloads/src/saga.rs"
NETWORK_IDENTITY="crates/nimbus-network/src/identity.rs"
OWNER_PLAN="docs/private/plans/nimbus-network-control-plane-plan.md"

add_error() {
  NNC62_ERRORS+=("$1")
}

require_nonempty_file() {
  target="$1"
  label="$2"
  if [ ! -s "${target}" ]; then
    add_error "missing or empty ${label}: ${target}"
    return 1
  fi
  return 0
}

match_count() {
  pattern="$1"
  shift
  rg -n --count-matches "${pattern}" "$@" 2>/dev/null |
    awk -F: '{ total += $NF } END { print total + 0 }'
}

verify_compiler_owner() {
  if ! require_nonempty_file "${COMPUTE_COMPILER}" "compute compiler target"; then
    return
  fi

  compiler_structs="$(match_count '^pub struct WorkloadNetworkPlanCompiler\b' "${COMPUTE_COMPILER}")"
  compiler_methods="$(match_count '^[[:space:]]*pub fn compile[[:space:]]*\(' "${COMPUTE_COMPILER}")"
  production_owners="$({
    rg -l 'pub struct WorkloadNetworkPlanCompiler\b|impl WorkloadNetworkPlanCompiler\b' \
      crates --glob '*.rs' --glob '!**/tests.rs' --glob '!**/tests/**' || true
  } | sort -u)"
  compiler_traits="$({
    rg -n '^[[:space:]]*(pub[[:space:]]+)?trait[[:space:]]+[A-Za-z0-9_]*NetworkPlanCompiler\b' \
      crates --glob '*.rs' --glob '!**/tests.rs' --glob '!**/tests/**' || true
  })"

  if [ "${compiler_structs}" -ne 1 ]; then
    add_error "expected one WorkloadNetworkPlanCompiler struct in ${COMPUTE_COMPILER}; found ${compiler_structs}"
  fi
  if [ "${compiler_methods}" -ne 1 ]; then
    add_error "expected one direct public compile method in ${COMPUTE_COMPILER}; found ${compiler_methods}"
  fi
  if [ "${production_owners}" != "${COMPUTE_COMPILER}" ]; then
    add_error "compute must be the only production compiler owner; found: ${production_owners:-none}"
  fi
  if [ -n "${compiler_traits}" ]; then
    add_error "compiler trait is forbidden until real substitution earns it: ${compiler_traits}"
  fi
  if rg -n '^[[:space:]]*(pub[[:space:]]+)?trait[[:space:]]+' "${COMPUTE_COMPILER}" >/dev/null; then
    add_error "compute compiler file must expose the direct compiler only, not a speculative trait"
  fi

  stripped_compiler="$(
    node - "${COMPUTE_COMPILER}" <<'NODE'
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
  )"
  if [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "decision-bound-identity" ]; then
    stripped_compiler="${stripped_compiler}
let forbidden_identity = workload.workload_uid();"
  fi
  forbidden_compiler_pattern='std::fs|std::env|std::time|tokio::net|TcpListener|TcpStream|UdpSocket|UnixListener|UnixStream|NetworkProviderHandle|LocalNetworkManager|LocalPortLeaseAuthority|LocalNetworkAttachmentAuthority|NetworkLeaseEpoch|lease_epoch|SandboxId|Ulid::new|rand::|SystemTime|Instant|Command::new|async[[:space:]]+fn|\.await\b|PublishedEndpointId::generate|IngressRouteId::generate|workload_uid|decision\.id[[:space:]]*\('
  forbidden_compiler_matches="$(
    printf '%s\n' "${stripped_compiler}" | rg -n "${forbidden_compiler_pattern}" || true
  )"
  if [ -n "${forbidden_compiler_matches}" ]; then
    add_error "pure compiler gained effects, ambient input, epoch assignment, random identity, or provider-handle authority: ${forbidden_compiler_matches}"
  fi

  if ! printf '%s\n' "${stripped_compiler}" |
    rg -q 'network_workload_incarnation_key[[:space:]]*\('; then
    add_error "compiler lacks the address-independent admitted workload-subject identity seam"
  fi
  if ! printf '%s\n' "${stripped_compiler}" |
    rg -q 'workload_identity[[:space:]]*\(\)\.subject[[:space:]]*\('; then
    add_error "compiler identity seam does not derive from the admitted workload subject"
  fi

  for seam in \
    'WorkloadNetworkPlanIdentity::new' \
    'WorkloadNetworkAttachmentBlueprint::new' \
    'WorkloadNetworkRouteBlueprint::new' \
    'WorkloadNetworkListenerBlueprint::new' \
    'CompiledWorkloadNetworkPlan::from_content'; do
    if ! printf '%s\n' "${stripped_compiler}" | rg -q -F "${seam}"; then
      add_error "compiler does not route exact identity/envelope construction through ${seam}"
    fi
  done
  if printf '%s\n' "${stripped_compiler}" |
    rg -n 'NetworkPlan::new|NetworkPlanContentDigest::sha256' >/dev/null; then
    add_error "compute independently assembles the plan envelope instead of deriving it from portable content"
  fi
}

verify_portable_payload() {
  if ! require_nonempty_file "${PORTABLE_PAYLOAD}" "portable compiled-plan payload"; then
    return
  fi
  payload_structs="$(match_count '^pub struct CompiledWorkloadNetworkPlan\b' "${PORTABLE_PAYLOAD}")"
  content_structs="$(match_count '^pub struct WorkloadNetworkPlanContent\b' "${PORTABLE_PAYLOAD}")"
  identity_structs="$(match_count '^pub struct WorkloadNetworkPlanIdentity\b' "${PORTABLE_PAYLOAD}")"
  if [ "${payload_structs}" -ne 1 ]; then
    add_error "expected one CompiledWorkloadNetworkPlan portable payload; found ${payload_structs}"
  fi
  if [ "${content_structs}" -ne 1 ]; then
    add_error "expected one WorkloadNetworkPlanContent retained payload; found ${content_structs}"
  fi
  if [ "${identity_structs}" -ne 1 ]; then
    add_error "expected one tenant-qualified WorkloadNetworkPlanIdentity; found ${identity_structs}"
  fi
  if ! rg -q '#\[serde\([^]]*deny_unknown_fields' "${PORTABLE_PAYLOAD}"; then
    add_error "portable compiled-plan payload lacks strict unknown-field rejection"
  fi

  stripped_payload="$(
    node - "${PORTABLE_PAYLOAD}" <<'NODE'
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
  )"
  for retained in \
    'identity: WorkloadNetworkPlanIdentity' \
    'capability_requirements: NetworkCapabilityRequirements' \
    'dependency_listeners: Vec<WorkloadNetworkDependencyListenerBlueprint>'; do
    if ! printf '%s\n' "${stripped_payload}" | rg -q -F "${retained}"; then
      add_error "portable content does not retain exact envelope provenance: ${retained}"
    fi
  done
  if [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "uncorrelated-envelope" ] ||
    ! printf '%s\n' "${stripped_payload}" | rg -q 'pub fn from_content[[:space:]]*\('; then
    add_error "compiled payload lacks a content-derived envelope constructor"
  fi
  for mismatch in \
    PlanIdentityMismatch \
    PlanGenerationMismatch \
    PlanSovereigntyMismatch \
    PlanCapabilityRequirementsMismatch \
    PlanReadinessRequirementsMismatch; do
    if ! printf '%s\n' "${stripped_payload}" | rg -q "${mismatch}"; then
      add_error "compiled payload does not fail closed on ${mismatch}"
    fi
  done
  for derivation in \
    'identity.attachment_id' \
    'identity.route_id' \
    'identity.listener_id' \
    'identity.endpoint_id' \
    'PortLeaseId::for_listener'; do
    if { [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "uncorrelated-resource-id" ] &&
      [ "${derivation}" = "identity.attachment_id" ]; } ||
      ! printf '%s\n' "${stripped_payload}" | rg -q -F "${derivation}"; then
      add_error "portable content does not rederive tenant-qualified resource identity through ${derivation}"
    fi
  done

  if ! require_nonempty_file "${WORKLOAD_SAGA}" "portable workload saga"; then
    return
  fi
  if rg -n 'CompiledWorkloadNetworkPlan|WorkloadNetworkPlanContent' "${WORKLOAD_SAGA}" >/dev/null; then
    add_error "NNC6.2 embedded the compiled payload in saga.rs; NNC6.2a owns durable embedding"
  fi
  item_commit="${ITEM_COMMIT}"
  if [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "missing-item-commit" ]; then
    item_commit="0000000000000000000000000000000000000000"
  fi
  if ! git cat-file -e "${item_commit}^{commit}" 2>/dev/null; then
    add_error "NNC6.2 item commit is unavailable for ownership verification: ${item_commit}"
  elif ! saga_changes="$(
    git diff-tree --no-commit-id --name-only -r "${item_commit}" -- "${WORKLOAD_SAGA}" 2>/dev/null
  )"; then
    add_error "NNC6.2 item commit could not be inspected for saga ownership: ${item_commit}"
  elif [ -n "${saga_changes}" ]; then
    add_error "NNC6.2 item commit changed saga.rs even though NNC6.2a owns durable embedding: ${saga_changes}"
  fi
}

verify_deterministic_identity_contract() {
  if ! require_nonempty_file "${NETWORK_IDENTITY}" "network identity contract"; then
    return
  fi
  identity_error="$(node - "${NETWORK_IDENTITY}" <<'NODE'
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
const errors = [];

function functionBody(name) {
  const start = source.indexOf(`pub fn ${name}`);
  if (start < 0) {
    errors.push(`missing deterministic ${name} constructor`);
    return null;
  }
  const open = source.indexOf("{", start);
  if (open < 0) {
    errors.push(`${name} constructor has no body`);
    return null;
  }
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(start, index + 1);
    }
  }
  errors.push(`${name} constructor body is incomplete`);
  return null;
}

const endpoint = functionBody("for_workload_endpoint");
if (endpoint) {
  if (!/pub fn for_workload_endpoint\(\s*workload_incarnation_key: &str,\s*endpoint_name: &str\s*\) -> Self/.test(endpoint)) {
    errors.push("endpoint constructor signature is not the frozen neutral two-string seam");
  }
  if (!endpoint.includes("&[workload_incarnation_key, endpoint_name]")) {
    errors.push("endpoint identity does not derive only from workload incarnation and endpoint name");
  }
  if (/address|host_port|guest_port|sandbox_id|provider|lease_epoch/i.test(endpoint)) {
    errors.push("endpoint identity gained address, port, sandbox, provider, or epoch input");
  }
}

const route = functionBody("for_workload_route");
if (route) {
  if (!/pub fn for_workload_route\(\s*workload_incarnation_key: &str,\s*service_name: &str,\s*route_name: &str,?\s*\) -> Self/.test(route)) {
    errors.push("route constructor signature is not the frozen neutral three-string seam");
  }
  if (!route.includes("&[workload_incarnation_key, service_name, route_name]")) {
    errors.push("route identity does not derive only from workload incarnation, service, and route names");
  }
  if (/address|host_port|guest_port|sandbox_id|provider|lease_epoch/i.test(route)) {
    errors.push("route identity gained address, port, sandbox, provider, or epoch input");
  }
}

process.stdout.write(errors.join("\n"));
NODE
  )"
  if [ -n "${identity_error}" ]; then
    while IFS= read -r error; do
      [ -n "${error}" ] && add_error "${error}"
    done <<<"${identity_error}"
  fi
}

verify_dependency_boundaries() {
  metadata_file="$(mktemp "${TMPDIR:-/tmp}/nnc62-metadata.XXXXXX")" || {
    add_error "could not create a bounded cargo-metadata capture"
    return
  }
  if ! cargo metadata --no-deps --format-version 1 >"${metadata_file}" 2>/dev/null; then
    add_error "cargo metadata failed while checking NNC6.2 dependency boundaries"
    rm -f "${metadata_file}"
    return
  fi
  dependency_error="$(node - "${metadata_file}" <<'NODE'
const fs = require("fs");
const metadata = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const errors = [];
function workspaceDependencies(name) {
  const pkg = metadata.packages.find(candidate => candidate.name === name);
  if (!pkg) {
    errors.push(`missing workspace package ${name}`);
    return [];
  }
  return pkg.dependencies.filter(dependency => dependency.path != null).map(dependency => dependency.name).sort();
}
const network = workspaceDependencies("nimbus-network");
if (network.join(",") !== "nimbus-core") {
  errors.push(`nimbus-network workspace dependencies are ${network.join(",") || "empty"}, expected exactly nimbus-core`);
}
const workloads = workspaceDependencies("nimbus-workloads");
for (const forbidden of ["nimbus-services", "nimbus-sandbox"]) {
  if (workloads.includes(forbidden)) errors.push(`nimbus-workloads gained forbidden ${forbidden} dependency`);
}
process.stdout.write(errors.join("\n"));
NODE
  )"
  rm -f "${metadata_file}"
  if [ -n "${dependency_error}" ]; then
    while IFS= read -r error; do
      [ -n "${error}" ] && add_error "${error}"
    done <<<"${dependency_error}"
  fi

  upper_imports="$({
    rg -n '^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?use[[:space:]]+nimbus_(tenant|services|sandbox|compute|machine|proxy|egress|server|system|cluster)\b|^[[:space:]]*extern[[:space:]]+crate[[:space:]]+nimbus_(tenant|services|sandbox|compute|machine|proxy|egress|server|system|cluster)\b' \
      crates/nimbus-network/src --glob '*.rs' || true
  })"
  if [ -n "${upper_imports}" ]; then
    add_error "nimbus-network imports upper policy/effect owners: ${upper_imports}"
  fi

  workload_imports="$({
    rg -n '^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?use[[:space:]]+nimbus_(services|sandbox)\b|^[[:space:]]*extern[[:space:]]+crate[[:space:]]+nimbus_(services|sandbox)\b' \
      crates/nimbus-workloads/src --glob '*.rs' || true
  })"
  if [ -n "${workload_imports}" ]; then
    add_error "portable workloads source imports service/sandbox owners: ${workload_imports}"
  fi

  cargo_changes="$(git diff --name-only HEAD -- Cargo.toml 'crates/*/Cargo.toml')"
  if [ -n "${cargo_changes}" ]; then
    add_error "NNC6.2 changed Cargo manifests even though all required edges pre-exist: ${cargo_changes}"
  fi
}

verify_oci_compiler_caller_baseline() {
  actual_callers=$(
    (
    rg -n 'oci_attachment_plan[[:space:]]*\(' crates/nimbus-sandbox/src --glob '*.rs' || true
    if [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "extra-oci-caller" ]; then
      printf '%s\n' 'crates/nimbus-sandbox/src/extra_nnc62_caller.rs:1:oci_attachment_plan('
    fi
    ) | while IFS=: read -r path _rest; do
    if [[ "${path}" == */tests/* || "${path}" == */tests.rs ||
      "${path}" == */test_support.rs ||
      "${path}" == */attachment_lifecycle/plan.rs ]]; then
      continue
    fi
    printf '%s\n' "${path}"
  done | sort | uniq -c | awk '{ print $2 " " $1 }'
  )

  expected_callers="$(printf '%s\n' \
    'crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs 2' \
    'crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/state.rs 1' \
    'crates/nimbus-sandbox/src/backends/oci/network/orphan_evidence/classifier.rs 1')"
  if [ -z "${actual_callers}" ]; then
    add_error "OCI attachment-plan production caller census is empty"
  elif [ "${actual_callers}" != "${expected_callers}" ]; then
    add_error "OCI attachment-plan caller baseline changed; expected [${expected_callers//$'\n'/; }], found [${actual_callers//$'\n'/; }]"
  fi
}

verify_later_owner_routing() {
  if ! require_nonempty_file "${OWNER_PLAN}" "canonical network owner plan"; then
    return
  fi
  if ! rg -q '^\| NNC6\.2a \| Persist the complete compiled network plan payload in workloads-owned saga intent\.' "${OWNER_PLAN}"; then
    add_error "NNC6.2a is not the canonical durable compiled-plan embedding owner"
  fi
  if ! rg -q '^\| NNC6\.1e1 \| Route lazy activation and explicit service/sandbox lifecycle requests through a compute-owned saga ingress after NNC6\.2a\.' "${OWNER_PLAN}"; then
    add_error "NNC6.1e1 is not the canonical lifecycle-ingress owner after NNC6.2a"
  fi
}

run_contract() {
  cd "${REPO_ROOT}" || return 1
  NNC62_ERRORS=()

  for tool in git rg node cargo awk; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      add_error "missing required verifier tool ${tool}"
    fi
  done
  if [ "${#NNC62_ERRORS[@]}" -ne 0 ]; then
    printf 'NNC6.2 contract failure: %s\n' "${NNC62_ERRORS[@]}" >&2
    return 1
  fi

  if [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "missing-compiler" ]; then
    COMPUTE_COMPILER="crates/nimbus-compute/src/missing_workload_network_plan.rs"
  fi
  if [ "${NIMBUS_NETWORK_NNC62_TEST_MUTATION:-}" = "missing-payload" ]; then
    PORTABLE_PAYLOAD="crates/nimbus-workloads/src/missing_network_plan.rs"
  fi

  verify_compiler_owner
  verify_portable_payload
  verify_deterministic_identity_contract
  verify_dependency_boundaries
  verify_oci_compiler_caller_baseline
  verify_later_owner_routing

  if [ "${#NNC62_ERRORS[@]}" -ne 0 ]; then
    for error in "${NNC62_ERRORS[@]}"; do
      printf 'NNC6.2 contract failure: %s\n' "${error}" >&2
    done
    return 1
  fi
  printf 'NNC6.2 workload-network plan compiler contract: 18 checks passed\n'
}

run_self_test() {
  self_test_root="$(mktemp -d "${TMPDIR:-/tmp}/nnc62-contract-self-test.XXXXXX")" || {
    printf 'NNC6.2 contract self-test: unable to create temporary directory\n' >&2
    return 1
  }
  trap 'rm -rf "${self_test_root}"' EXIT
  self_test_failures=0

  for mutation in \
    missing-compiler \
    missing-payload \
    extra-oci-caller \
    decision-bound-identity \
    uncorrelated-envelope \
    uncorrelated-resource-id \
    missing-item-commit; do
    output="${self_test_root}/${mutation}.out"
    if NIMBUS_NETWORK_NNC62_TEST_MUTATION="${mutation}" bash "${SCRIPT_PATH}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNCV028 %s unexpectedly passed\n' "${mutation}"
      self_test_failures=$((self_test_failures + 1))
      continue
    fi
    case "${mutation}" in
      missing-compiler) expected='missing or empty compute compiler target' ;;
      missing-payload) expected='missing or empty portable compiled-plan payload' ;;
      extra-oci-caller) expected='OCI attachment-plan caller baseline changed' ;;
      decision-bound-identity) expected='pure compiler gained effects, ambient input, epoch assignment, random identity, or provider-handle authority' ;;
      uncorrelated-envelope) expected='compiled payload lacks a content-derived envelope constructor' ;;
      uncorrelated-resource-id) expected='portable content does not rederive tenant-qualified resource identity through identity.attachment_id' ;;
      missing-item-commit) expected='NNC6.2 item commit is unavailable for ownership verification' ;;
    esac
    if ! rg -q -F "${expected}" "${output}"; then
      printf 'SELFTEST FAIL NNCV028 %s missed diagnostic %s\n' "${mutation}" "${expected}"
      self_test_failures=$((self_test_failures + 1))
    else
      printf 'SELFTEST PASS NNCV028 %s fails closed\n' "${mutation}"
    fi
  done

  if [ "${self_test_failures}" -ne 0 ]; then
    printf 'NNC6.2 contract self-test: %d failed\n' "${self_test_failures}"
    return 1
  fi
  printf 'NNC6.2 contract self-test: 7 passed, 0 failed\n'
}

case "${1:-}" in
  "" | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
