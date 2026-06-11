#!/usr/bin/env bash
# Aggregate completion-gate verifier for
# docs/private/plans/nimbus-capability-segregation-plan.md.
#
# CB0 ships this as a failing control gate. Each later CB phase flips its own
# numbered condition to PASS without weakening previous conditions.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/nimbus-capability-segregation-plan.md"
PLANS_README="docs/private/plans/README.md"
AGENTS_MD="AGENTS.md"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 -- $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

check() {
  local desc="$1"
  shift
  if "$@"; then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

require_contains() {
  local desc="$1"
  local pattern="$2"
  shift 2
  if grep -RqE "${pattern}" "$@" 2>/dev/null; then
    pass "${desc}"
    return 0
  fi
  fail "${desc}" "missing pattern: ${pattern}"
  return 1
}

require_absent() {
  local desc="$1"
  local pattern="$2"
  shift 2
  if grep -RqE "${pattern}" "$@" 2>/dev/null; then
    fail "${desc}" "unexpected pattern: ${pattern}"
    return 1
  fi
  pass "${desc}"
  return 0
}

require_command_passes() {
  local desc="$1"
  shift
  if "$@" >/tmp/nimbus-capability-segregation-command.out 2>&1; then
    pass "${desc}"
    return 0
  fi
  fail "${desc}" "$(tail -20 /tmp/nimbus-capability-segregation-command.out | tr '\n' ' ')"
  return 1
}

condition_1_engine_rename() {
  printf '    evidence: require engine module, reject old nimbus-engine service module and public coordinator names\n'
  local ok=0
  [ -d crates/nimbus-engine/src/engine ] || ok=1
  [ ! -d crates/nimbus-engine/src/service ] || ok=1
  if grep -RqE 'nimbus_engine::Service|ServiceBootstrapPlan|ServicePersistenceConfig|crates/nimbus-engine/src/service' \
    crates docs/private/architecture docs/private/staging/operating docs/private/plans/*.md 2>/dev/null; then
    ok=1
  fi
  return "${ok}"
}

condition_2_no_core_package() {
  printf '    evidence: no packages/core or @nimbus/core root in live package/codegen inputs\n'
  [ ! -d packages/core ] &&
    [ ! -f packages/core/package.json ] &&
    ! grep -RqE '@nimbus/core|packages/core' \
      package.json packages Makefile crates/nimbus-bin crates/nimbus-assets 2>/dev/null
}

condition_3_js_sdk_boundary() {
  printf '    evidence: scoped SDK package, root Nimbus export, internal default transport selection, no public host transport, transport-namespace compat lint, managed codegen name, and frozen compat surfaces\n'
  node --input-type=module - <<'NODE' || return 1
import fs from "node:fs";
import {
  NIMBUS_ROOT_SDK_ARTIFACT_PATHS,
  NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS,
  NIMBUS_ROOT_SDK_METHOD_FRAGMENTS,
  assertNimbusRootSdkArtifactText,
} from "./scripts/nimbus-root-sdk-artifact-policy.mjs";

function readJson(path) {
  return JSON.parse(fs.readFileSync(path, "utf8"));
}
function read(path) {
  return fs.readFileSync(path, "utf8");
}
function includesAll(text, fragments) {
  return fragments.every((fragment) => text.includes(fragment));
}
function excludesAll(text, fragments) {
  return fragments.every((fragment) => !text.includes(fragment));
}
const pkg = readJson("packages/nimbus/package.json");
const distPkg = readJson("packages/nimbus/dist/package.json");
const embeddedPkg = readJson("crates/nimbus-assets/embedded/packages/@nimbus/nimbus/package.json");
const sdk = read("packages/nimbus/src/index.ts");
const provisionedExportsOk = (manifest) =>
  manifest.name === "@nimbus/nimbus" &&
  manifest.exports &&
  manifest.exports["."] &&
  manifest.exports["."].types === "./index.d.ts" &&
  manifest.exports["."].default === "./index.js" &&
  manifest.exports["./transports/rest"] &&
  manifest.exports["./transports/rest"].types === "./transports/rest.d.ts" &&
  manifest.exports["./transports/rest"].default === "./transports/rest.js" &&
  !manifest.exports["./rest"] &&
  !manifest.exports["./transports/host"];
const rootSdkArtifactsOk = NIMBUS_ROOT_SDK_ARTIFACT_PATHS.every((artifactPath) => {
  try {
    assertNimbusRootSdkArtifactText(artifactPath, read(artifactPath));
    return true;
  } catch {
    return false;
  }
});
const ok =
  pkg.name === "@nimbus/nimbus" &&
  pkg.exports &&
  pkg.exports["."] === "./src/index.ts" &&
  pkg.exports["./transports/rest"] === "./src/transports/rest.ts" &&
  !pkg.exports["./rest"] &&
  !pkg.exports["./transports/host"] &&
  fs.existsSync("packages/nimbus/src/index.ts") &&
  fs.existsSync("packages/nimbus/src/transports/rest.ts") &&
  !fs.existsSync("packages/nimbus/src/transports/host.ts") &&
  provisionedExportsOk(distPkg) &&
  provisionedExportsOk(embeddedPkg) &&
  sdk.includes("export class Nimbus") &&
  sdk.includes("new NimbusRestClient") &&
  sdk.includes("createDefaultRestClient") &&
  sdk.includes("async #controlPlaneRequest") &&
  sdk.includes("async #resolveRestClient") &&
  includesAll(sdk, NIMBUS_ROOT_SDK_METHOD_FRAGMENTS) &&
  sdk.includes("/api/tenants/") &&
  excludesAll(sdk, NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS) &&
  !sdk.includes("fromWorkloadIdentity") &&
  rootSdkArtifactsOk;
process.exit(ok ? 0 : 1);
NODE
  (
    probe_dir="packages/convex/src/.capability-boundary-probe"
    rest_probe="${probe_dir}/rest.ts"
    host_probe="${probe_dir}/host.ts"
    root_probe="${probe_dir}/root.ts"
    probe_output="$(mktemp "${TMPDIR:-/tmp}/nimbus-capability-boundary-lint.XXXXXX")"
    trap 'rm -rf "${probe_dir}"; rm -f "${probe_output}"' EXIT INT TERM
    rm -rf "${probe_dir}"
    mkdir -p "${probe_dir}" || exit 1
    printf 'import "@nimbus/nimbus/transports/rest";\n' >"${rest_probe}"
    printf 'import "@nimbus/nimbus/transports/host";\n' >"${host_probe}"
    printf 'import { Nimbus } from "@nimbus/nimbus";\n' >"${root_probe}"
    if npx biome lint --config-path ./biome.json "${rest_probe}" >"${probe_output}" 2>&1; then
      exit 1
    fi
    if ! grep -q 'noRestrictedImports' "${probe_output}"; then
      exit 1
    fi
    if npx biome lint --config-path ./biome.json "${host_probe}" >"${probe_output}" 2>&1; then
      exit 1
    fi
    if ! grep -q 'noRestrictedImports' "${probe_output}"; then
      exit 1
    fi
    if ! npx biome lint --config-path ./biome.json "${root_probe}" >"${probe_output}" 2>&1; then
      exit 1
    fi
  )
}

condition_4_service_capability_host() {
  printf '    evidence: RuntimeServiceCapabilityHost exists and Cloud Functions remains refusal-only\n'
  grep -Rq 'RuntimeServiceCapabilityHost' crates/nimbus-server crates/nimbus-cloud-functions &&
    grep -RqE 'RuntimeServiceCapabilityHost[[:space:]]*::|service_capabilities|service_capability' \
      crates/nimbus-server &&
    grep -RqE 'RuntimeServiceCapabilityHost|CtxServiceLookup' crates/nimbus-cloud-functions
}

condition_5_v8_service_ext_partition() {
  printf '    evidence: nimbus_service_ext is separate, exact grants plus explicit capability gate it, snapshot excludes service ops, and RuntimePoolPartitionKey carries service-op state\n'
  grep -Rq 'nimbus_service_ext' crates/nimbus-runtime &&
    grep -RqE 'service_capability_enabled.*has_service_grants|has_service_grants.*service_capability_enabled' \
      crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs &&
    grep -Rq 'RuntimePoolPartitionKey' crates/nimbus-runtime &&
    grep -RqE 'service_op|service_grant|service_capabilit' crates/nimbus-runtime &&
    ! grep -RqE 'op_nimbus_ctx_service_lookup' crates/nimbus-runtime/src/runtime/bootstrap/snapshot* 2>/dev/null
}

condition_6_no_ungranted_indirect_path() {
  printf '    evidence: tests cover no adapter Nimbus shortcuts, no ungranted V8 op, and no bun_jsc indirect privileged path\n'
  grep -RqE 'ungranted.*(service|CtxServiceLookup)|service.*ungranted' crates/nimbus-runtime crates/nimbus-server &&
    grep -RqE 'bun_jsc.*(service|CtxServiceLookup)|CtxServiceLookup.*bun_jsc' \
      crates/nimbus-runtime crates/nimbus-server &&
    grep -RqE 'no.*ctx\.services|ctx\.services.*absent|adapter.*ctx\.services|service/sandbox/session/control-plane shortcut' \
      crates/nimbus-runtime crates/nimbus-server packages &&
    ! grep -RqE 'ctx\.(services|sandboxes|sessions|browser)' \
      packages/convex/src packages/firebase/src packages/mongodb/src packages/dynamodb/src \
      crates/nimbus-cloud-functions 2>/dev/null
}

condition_7_permission_profile_partition() {
  printf '    evidence: per-tier permission profile participates in isolate construction and warm-pool reuse keys\n'
  grep -RqE 'RuntimePermissionProfile|PermissionProfile' crates/nimbus-runtime &&
    grep -Rq 'RuntimePoolPartitionKey' crates/nimbus-runtime &&
    grep -RqE 'query.*(net|fs|ffi).*deny|mutation.*(net|fs|ffi).*deny|action.*permission' \
      crates/nimbus-runtime crates/nimbus-server
}

condition_8_tenant_bundle_admission() {
  printf '    evidence: tenant bundle admission rejects low-level/operator transport namespace and packaged operator credentials while admitting high-level SDK identity auth\n'
  grep -RqE '@nimbus/nimbus/transports/rest' packages/codegen crates/nimbus-server packages/nimbus &&
    grep -RqE '@nimbus/nimbus/transports/host|startsWith\("@nimbus/nimbus/transports/"\)|starts_with\("@nimbus/nimbus/transports/"\)' \
      packages/codegen crates/nimbus-runtime &&
    grep -RqE 'operator-only|operator credential|LocalAdminTokenRecord' \
      packages/codegen crates/nimbus-server packages/nimbus &&
    grep -RqE 'tenant bundle admission|bundle admission|realm separation|realm-separation' \
      packages/codegen crates/nimbus-server packages/nimbus
}

condition_9_principal_route_policy() {
  printf '    evidence: route tests cover operator, tenant, spawned service routes, spawned admin rejection, exact grants, and retired identity alias absence\n'
  grep -RqE 'principal.*class|PrincipalClass' crates/nimbus-server/src &&
    grep -RqE 'operator.*cross-tenant|cross-tenant.*operator' crates/nimbus-server/src &&
    grep -RqE 'tenant.*cross-tenant|cross-tenant.*tenant' crates/nimbus-server/src &&
    grep -RqE 'spawned.*service.*route|service.*route.*spawned|tenant-a-spawned-db.*/api/tenants/.*/services' crates/nimbus-server/src &&
    grep -RqE 'spawned.*admin|admin.*spawned' crates/nimbus-server/src &&
    grep -RqE 'exact.*service|service.*exact' crates/nimbus-server/src &&
    ! grep -RqE 'TenantWorkloadStableIdentity|TenantWorkloadIdentity|TenantWorkloadLocation' \
      crates docs/private/architecture docs/private/staging/operating docs/private/plans/*.md 2>/dev/null
}

condition_10_debrand_and_routing() {
  printf '    evidence: plan routing exists and live surfaces have no retired brand leakage\n'
  grep -q 'nimbus-capability-segregation-plan.md' "${PLANS_README}" &&
    grep -q 'nimbus-capability-segregation-plan.md' "${AGENTS_MD}" &&
    grep -q 'verify-nimbus-capability-segregation.sh' "${PLAN}" &&
    ! grep -RqE 'Neovex|neovex' \
      README.md ARCHITECTURE.md AGENTS.md crates packages docs --exclude-dir=target 2>/dev/null
}

run_condition() {
  local number="$1"
  local desc="$2"
  local fn="$3"
  step "${number}" "${desc}"
  check "${desc}" "${fn}"
}

printf '\033[1mNimbus Capability Segregation -- completion gate\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

run_condition 1 "CB1 engine coordinator rename is complete" condition_1_engine_rename
run_condition 2 "CB2 retired @nimbus/core extraction remains absent" condition_2_no_core_package
run_condition 3 "CB3 scoped SDK and JS boundary are complete" condition_3_js_sdk_boundary
run_condition 4 "CB4 service capability host/refusal boundary is complete" condition_4_service_capability_host
run_condition 5 "CB5 V8 service extension and pool partitioning are complete" condition_5_v8_service_ext_partition
run_condition 6 "CB6 indirect ungranted service paths are covered" condition_6_no_ungranted_indirect_path
run_condition 7 "CB7 runtime permission profile partitioning is complete" condition_7_permission_profile_partition
run_condition 8 "CB7a tenant bundle admission is complete" condition_8_tenant_bundle_admission
run_condition 9 "CB8 principal-class route policy is complete" condition_9_principal_route_policy
run_condition 10 "CB9 de-brand/routing guards are complete" condition_10_debrand_and_routing

printf '\n\033[1mSummary:\033[0m %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
