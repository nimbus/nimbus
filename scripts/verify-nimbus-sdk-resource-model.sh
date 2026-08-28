#!/usr/bin/env bash
# Aggregate completion-gate verifier for
# docs/private/plans/nimbus-sdk-resource-model-plan.md.
#
# SRM0 ships this as a registration and model verifier. Later SRM phases add
# concrete route/SDK/resource checks before marking their phases done.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 1

PLAN="docs/private/plans/archive/nimbus-sdk-resource-model-plan.md"
MODEL_DOC="docs/private/architecture/sandbox/service-sandbox-session-model.md"
# packages/nimbus/src/index.ts is now a thin re-export shell (the SDK package
# reorg moved the Nimbus class/methods into control-plane/client.ts and the
# request/response types into control-plane/types.ts). Root-surface checks
# must span all of these files, since no single file carries the full
# canonical surface any more.
SDK_ROOT_FILES=(
  "packages/nimbus/src/index.ts"
  "packages/nimbus/src/control-plane/client.ts"
  "packages/nimbus/src/control-plane/types.ts"
  "packages/nimbus/src/control-plane/discovery.ts"
  "packages/nimbus/src/control-plane/index.ts"
)
SDK_CONTROL_ROUTES="packages/nimbus/src/control_plane_routes.ts"
SDK_SELFTEST="packages/nimbus/src/selftest.mjs"
SDK_SURFACE_CONTRACT="packages/nimbus/src/capability_surface_contract.mjs"
ROOT_SDK_POLICY="scripts/nimbus-root-sdk-artifact-policy.mjs"
SDK_EXAMPLES_DOC="docs/private/examples/nimbus-sdk-resource-model.md"
DOCS_README="docs/README.md"
SDK_README="packages/nimbus/README.md"
SERVICES_MANAGER="crates/nimbus-services/src/manager.rs"
SERVICES_MANAGER_DEFINITION_TESTS="crates/nimbus-services/src/manager/tests/definition_lifecycle.rs"
SERVICES_MANAGER_SOURCE_RETIREMENT="crates/nimbus-services/src/manager/source_retirement.rs"
SERVICES_MANAGER_SOURCE_RETIREMENT_TESTS="crates/nimbus-services/src/manager/tests/source_retirement.rs"
SERVICES_MANAGER_SANDBOX_RESOURCE_TESTS="crates/nimbus-services/src/manager/tests/sandbox_resources.rs"
SERVICES_MANAGER_SOURCE_PROJECTION_TESTS="crates/nimbus-services/src/manager/tests/source_projection.rs"
SERVICES_MANAGER_SESSION_TESTS="crates/nimbus-services/src/manager/tests/sessions.rs"
SERVICES_MANAGER_TENANT_RETIREMENT="crates/nimbus-services/src/manager/tenant_retirement.rs"
SERVICES_MANAGER_TENANT_RETIREMENT_TESTS="crates/nimbus-services/src/manager/tenant_retirement/tests.rs"
SERVER_ROUTER="crates/nimbus-server/src/router.rs"
SERVER_SERVICE_GRANTS="crates/nimbus-server/src/http/service_grants.rs"
SERVER_SERVICES="crates/nimbus-server/src/http/services.rs"
COMPUTE_SERVICES="crates/nimbus-compute/src/services.rs"
COMPUTE_SANDBOXES="crates/nimbus-compute/src/sandboxes.rs"
SERVER_RESOURCE_CONTROL_SANDBOXES="crates/nimbus-server/src/http/resource_control/sandboxes.rs"
SERVER_RESOURCE_CONTROL_SERVICES="crates/nimbus-server/src/http/resource_control/services.rs"
SERVER_RESOURCE_CONTROL_SESSIONS="crates/nimbus-server/src/http/resource_control/sessions.rs"
SERVER_SANDBOXES="crates/nimbus-server/src/http/sandboxes.rs"
SERVER_SANDBOX_SPEC="crates/nimbus-compute/src/sandbox_spec.rs"
SERVER_SESSIONS="crates/nimbus-server/src/http/sessions.rs"
SERVER_TESTS_ROOT="crates/nimbus-server/src/tests.rs"
SERVER_SERVICE_MANAGER_TESTS="crates/nimbus-server/src/tests/service_manager.rs"
SERVER_SERVICE_MANAGER_DEFINITION_RETIREMENT_TESTS="crates/nimbus-server/src/tests/service_manager/definition_retirement.rs"
SERVER_SERVICE_MANAGER_DEFINITION_TESTS="crates/nimbus-server/src/tests/service_manager/definitions.rs"
SERVER_SERVICE_MANAGER_REDACTION_TESTS="crates/nimbus-server/src/tests/service_manager/redaction.rs"
SERVER_SERVICE_MANAGER_SANDBOX_TESTS="crates/nimbus-server/src/tests/service_manager/sandboxes.rs"
SERVER_SERVICE_MANAGER_SESSION_TESTS="crates/nimbus-server/src/tests/service_manager/sessions.rs"

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
  local output
  output="$(mktemp "${TMPDIR:-/tmp}/nimbus-sdk-resource-model.XXXXXX")"
  if "$@" >"${output}" 2>&1; then
    rm -f "${output}"
    pass "${desc}"
    return 0
  fi
  fail "${desc}" "$(tail -20 "${output}" | tr '\n' ' ')"
  rm -f "${output}"
  return 1
}

condition_srm0_registration() {
  # This plan is done and archived; repo policy removes archived plans from
  # docs/private/plans/README.md and AGENTS.md (they are deliberately
  # unlisted once folded into their successor/history), so registration is
  # checked against the archived plan file and the architecture model doc
  # content only, not README/AGENTS.md listings.
  [ -f "${PLAN}" ] &&
    [ -f "${MODEL_DOC}" ] &&
    grep -q 'Service:' "${PLAN}" &&
    grep -q 'Sandbox:' "${PLAN}" &&
    grep -q 'Session:' "${PLAN}" &&
    grep -q 'Runtime isolate:' "${PLAN}" &&
    grep -q 'Service' "${MODEL_DOC}" &&
    grep -q 'Sandbox' "${MODEL_DOC}" &&
    grep -q 'Session' "${MODEL_DOC}"
}

condition_srm1_package_surface() {
  node --input-type=module - <<'NODE'
import fs from "node:fs";

const pkg = JSON.parse(fs.readFileSync("packages/nimbus/package.json", "utf8"));
// index.ts is a thin re-export shell; the class/methods live in
// control-plane/client.ts and the request/response types in
// control-plane/types.ts. Concatenate the root surface so fragment checks
// below see the full canonical surface regardless of which file carries it.
const sdk = [
  "packages/nimbus/src/index.ts",
  "packages/nimbus/src/control-plane/client.ts",
  "packages/nimbus/src/control-plane/types.ts",
  "packages/nimbus/src/control-plane/discovery.ts",
  "packages/nimbus/src/control-plane/index.ts",
].map((file) => fs.readFileSync(file, "utf8")).join("\n");
const routes = fs.readFileSync("packages/nimbus/src/control_plane_routes.ts", "utf8");

const hasCanonicalManifest =
  pkg.name === "@nimbus/nimbus" &&
  pkg.exports?.["."] === "./src/index.ts" &&
  pkg.exports?.["./transports/rest"] === "./src/transports/rest.ts" &&
  !pkg.exports?.["./rest"] &&
  !pkg.exports?.["./transports/host"];

const canonicalMethods = [
  ["Nimbus class", /export class Nimbus/],
  ["services namespace", /readonly services: NimbusServices/],
  ["service start", /start\(input: NimbusServiceStartRequest/],
  ["service stop", /stop\(input: NimbusServiceStopRequest/],
  ["service restart", /restart\(\s*input: NimbusServiceRestartRequest/],
  ["service get", /get\(selector: NimbusServiceSelector/],
  ["service wait", /wait\(input: NimbusServiceWaitRequest/],
  ["route path", /controlPlaneRoutePath/],
  ["route verb", /controlPlaneRouteVerb/],
  ["default REST client", /createDefaultRestClient/],
];
const missingCanonicalMethods = canonicalMethods
  .filter(([, pattern]) => !pattern.test(sdk))
  .map(([label]) => label);
const hasCanonicalMethods = missingCanonicalMethods.length === 0;

const hasCanonicalControlPlaneRoutes = [
  "NIMBUS_CONTROL_PLANE_ROUTES",
  "services.start",
  "services.stop",
  "services.restart",
  "services.create",
  "sandboxes.create",
  "sandboxes.stop",
  "sessions.open",
  "sessions.close",
  "/api/tenants/{tenant_id}/services/{service_name}",
  "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}",
  "/api/sessions/{session_id}/close",
].every((fragment) => routes.includes(fragment));

const rootAvoidsEmbeddedControlPlaneRoutes = [
  "/api/tenants/",
  "/api/sessions",
].every((fragment) => !sdk.includes(fragment));

const lacksFutureOrStaleRootSurface = [
  "ensureRunning",
  "NimbusSessionCreateRequest",
  "sessions.create",
  "sessions.renew",
  "sessions.extend",
  "/api/services/",
  "async request(path",
  "async resolveRestClient",
].every((fragment) => !sdk.includes(fragment));

const failedContracts = [
  ["canonical package manifest", hasCanonicalManifest],
  ["canonical methods", hasCanonicalMethods],
  ["canonical control-plane routes", hasCanonicalControlPlaneRoutes],
  ["root avoids embedded routes", rootAvoidsEmbeddedControlPlaneRoutes],
  ["root lacks future or stale surface", lacksFutureOrStaleRootSurface],
].filter(([, passed]) => !passed).map(([label]) => label);
if (failedContracts.length > 0) {
  console.error(`failed SDK root contracts: ${failedContracts.join(", ")}`);
}
if (missingCanonicalMethods.length > 0) {
  console.error(`missing SDK methods: ${missingCanonicalMethods.join(", ")}`);
}

process.exit(
  hasCanonicalManifest &&
  hasCanonicalMethods &&
  hasCanonicalControlPlaneRoutes &&
  rootAvoidsEmbeddedControlPlaneRoutes &&
  lacksFutureOrStaleRootSurface
    ? 0
    : 1,
);
NODE
}

condition_srm1_lifecycle_wait_contract() {
  grep -q 'NimbusServiceActivationWaitCondition' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusServiceStopWaitCondition' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'assertLifecycleWaitUntil("start", input.waitUntil, \["ready", "healthy"\])' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'assertLifecycleWaitUntil("stop", input.waitUntil, \["stopped"\])' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'assertLifecycleWaitValidation' "${SDK_SELFTEST}" &&
    grep -q 'service stop waits for stopped, not readiness' "${SDK_SELFTEST}" &&
    grep -q 'service start waits for activation conditions, not stopped' "${SDK_SELFTEST}"
}

condition_srm1_future_surface_guards() {
  grep -q 'ensureRunning' "${ROOT_SDK_POLICY}" &&
    grep -q '/api/services/' "${ROOT_SDK_POLICY}" &&
    grep -q 'sessions.create' "${ROOT_SDK_POLICY}" &&
    grep -q 'sessions.renew' "${ROOT_SDK_POLICY}" &&
    grep -q 'sessions.extend' "${ROOT_SDK_POLICY}" &&
    grep -q '_sdk.services.ensureRunning' "${SDK_SELFTEST}" &&
    grep -q '_sdk.sandboxes.get({ name: "worker" })' "${SDK_SELFTEST}" &&
    grep -q '_sdk.sessions.create' "${SDK_SELFTEST}" &&
    grep -q '_sdk.sessions.renew' "${SDK_SELFTEST}" &&
    grep -q '_sdk.sessions.extend' "${SDK_SELFTEST}" &&
    grep -q 'NimbusServiceActivationWaitCondition' "${SDK_SURFACE_CONTRACT}" &&
    grep -q 'NimbusSandboxResource' "${SDK_SURFACE_CONTRACT}"
}

condition_srm1_server_routes() {
  grep -q '/api/tenants/{tenant_id}/services/{service_name}' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/services/{service_name}/start' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/services/{service_name}/stop' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/services/{service_name}/restart' "${SERVER_ROUTER}" &&
    grep -q 'pub(crate) async fn get_service' "${SERVER_SERVICES}" &&
    grep -q 'pub(crate) async fn start_service' "${SERVER_SERVICES}" &&
    grep -q 'pub(crate) async fn stop_service' "${SERVER_SERVICES}" &&
    grep -q 'pub(crate) async fn restart_service' "${SERVER_SERVICES}" &&
    grep -q 'requires operator credentials or authenticated tenant/spawned workload identity' "${SERVER_RESOURCE_CONTROL_SERVICES}" &&
    grep -q 'principal_has_exact_service_grant' "${SERVER_SERVICE_GRANTS}" &&
    grep -q 'service_grant_value_contains_wildcard' "${SERVER_SERVICE_GRANTS}" &&
    grep -q 'services:\*' "${SERVER_SERVICE_GRANTS}"
}

condition_srm1_adapter_surfaces_stay_clean() {
  ! grep -RqE 'ctx\.(services|sandboxes|sessions|browser)|@nimbus/nimbus/transports/(rest|host)' \
    packages/convex/src packages/firebase/src packages/mongodb/src packages/dynamodb/src
}

condition_srm2_sdk_definition_surface() {
  grep -q 'create(input: NimbusServiceCreateRequest)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'update(input: NimbusServiceUpdateRequest)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'delete(input: NimbusServiceDeleteRequest)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'input: NimbusServiceListRequest = {}' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'kind: "builtIn"' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'kind: "external"' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'ifMatchGeneration' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusServiceDefinitionCollection' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusSandboxOwnerSpec' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusSandboxRootSpec' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusSandboxRootResponse' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusSandboxOciImageReferenceSource' "${SDK_ROOT_FILES[@]}" &&
    ! grep -q 'kind: "rootfs"' "${SDK_ROOT_FILES[@]}" &&
    ! grep -q 'dockerfilePath' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusSandboxProcessSpec' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusSandboxProcessResponse' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusRedactedValues' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'export interface NimbusSandboxSpec' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'export interface NimbusSandboxSpecResponse' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'NimbusServiceBackendResponse' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'tenantId?: string' "${SDK_ROOT_FILES[@]}" &&
    grep -q '_sdk.services.create' "${SDK_SELFTEST}" &&
    grep -q '_sdk.services.update' "${SDK_SELFTEST}" &&
    grep -q '_sdk.services.delete' "${SDK_SELFTEST}" &&
    grep -q '_sdk.services.list' "${SDK_SELFTEST}"
}

condition_srm2_server_definition_routes() {
  # CP3 (crates/nimbus-compute/src/services.rs) moved the response DTOs and
  # backend-projection business logic into nimbus-compute; nimbus-server's
  # http/services.rs kept the route-level parsing/validation that depends on
  # the request TenantId/HeaderMap.
  grep -q '/api/tenants/{tenant_id}/services' "${SERVER_ROUTER}" &&
    grep -q 'get(http::list_service_definitions).post(http::create_service_definition)' "${SERVER_ROUTER}" &&
    grep -q 'put(http::update_service_definition)' "${SERVER_ROUTER}" &&
    grep -q 'delete(http::delete_service_definition)' "${SERVER_ROUTER}" &&
    grep -q 'ServiceDefinitionResourceResponse' "${SERVER_SERVICES}" &&
    grep -q 'ServiceDefinitionCollectionResponse' "${SERVER_SERVICES}" &&
    grep -q 'ServiceBackendInput::Sandbox' "${COMPUTE_SERVICES}" &&
    grep -q 'into_spec(tenant_id, Some(service_name))' "${COMPUTE_SERVICES}" &&
    grep -q 'SandboxSpecResponse::from_spec' "${COMPUTE_SERVICES}" &&
    grep -q 'pub struct SandboxSpecInput' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'RedactedValuesResponse' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'operatorOnlyLaunchInput' "${SERVER_SANDBOX_SPEC}" &&
    ! grep -q 'env: process.env' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'sandbox spec tenantId.*must match route tenant' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'host rootfs path.*operator-only internal input' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'local build context paths.*operator-only internal inputs' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'ExternalEndpointPolicyResponse' "${COMPUTE_SERVICES}" &&
    grep -q 'ExternalAuthPolicyResponse' "${COMPUTE_SERVICES}" &&
    grep -q 'HealthCheckPolicyResponse' "${COMPUTE_SERVICES}" &&
    grep -q 'ServiceDefinitionProjection::List' "${COMPUTE_SERVICES}" &&
    grep -q 'ServiceDefinitionProjection::Inspect' "${COMPUTE_SERVICES}" &&
    grep -q 'requiresInspectPermission' "${COMPUTE_SERVICES}" &&
    grep -q 'rename = "builtIn"' "${COMPUTE_SERVICES}" &&
    grep -q 'ifMatchGeneration query precondition' "${SERVER_SERVICES}" &&
    grep -q 'validate_body_tenant' "${SERVER_SERVICES}" &&
    grep -q 'validate_body_service_name' "${SERVER_SERVICES}"
}

condition_srm2_manager_definition_state() {
  grep -q 'pub struct ServiceDefinition' crates/nimbus-services/src/catalog.rs &&
    grep -q 'ServiceDefinitionSource' crates/nimbus-services/src/catalog.rs &&
    grep -q 'pub enum ExternalAuthPolicy' crates/nimbus-services/src/catalog.rs &&
    grep -q 'pub enum HealthCheckPolicy' crates/nimbus-services/src/catalog.rs &&
    grep -q 'definitions: BTreeMap<TenantServiceKey, ServiceDefinition>' crates/nimbus-services/src/manager/types.rs &&
    grep -q 'create_service_definition' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'update_service_definition' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'finalize_unmanaged_service_definition_deletion' "${SERVICES_MANAGER_SOURCE_RETIREMENT}" &&
    grep -q 'claim_service_definition_retirement' "${SERVICES_MANAGER_SOURCE_RETIREMENT}" &&
    grep -q 'finalize_service_definition_after_recorded' "${SERVICES_MANAGER_SOURCE_RETIREMENT}" &&
    grep -q 'service_volume_policy_for_tenant' crates/nimbus-services/src/catalog.rs &&
    grep -q 'prepare_sandbox_service_provision_source' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'validate_sandbox_service_provision_decision' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'volume_policy' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'ensure_sandbox_mounts_match' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'Url::parse' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'SUPPORTED_BUILT_IN_PROVIDERS' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'endpoint must not embed credentials' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q "health.path must start with \`/\`" crates/nimbus-services/src/manager/definitions.rs
}

condition_srm2_authorization_split_tests() {
  grep -q 'service_definition_permissions_do_not_imply_service_lifecycle_grants' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_force_delete_requires_separate_policy_and_exact_service_grant' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_list_only_permission_redacts_inspect_details' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_update_rejects_active_backend_until_stopped' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'definition_delete_fences_and_joins_inflight_provision_before_removing_source' "${SERVER_SERVICE_MANAGER_DEFINITION_RETIREMENT_TESTS}" &&
    grep -q 'definition_delete_keeps_source_and_sessions_until_recorded_teardown' "${SERVER_SERVICE_MANAGER_DEFINITION_RETIREMENT_TESTS}" &&
    grep -q 'force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects' "${SERVER_SERVICE_MANAGER_DEFINITION_RETIREMENT_TESTS}" &&
    grep -q 'service_source_validation_rejects_volume_without_catalog_policy_before_provider_io' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'service_source_validation_accepts_declared_volume_without_starting_provider' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'create_service_definition_rejects_malformed_external_endpoint' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'retirement_claim_fences_source_update_and_start_reservation' "${SERVICES_MANAGER_SOURCE_RETIREMENT_TESTS}" &&
    grep -q 'source_retirement_claim_exists' crates/nimbus-services/src/manager/sessions.rs &&
    ! grep -q 'yield_now' "${SERVICES_MANAGER_SOURCE_RETIREMENT_TESTS}" &&
    grep -q 'ServiceDefinitionAction::ForceDelete' "${SERVER_RESOURCE_CONTROL_SERVICES}" &&
    grep -q 'ServiceDefinitionAction::Inspect' "${SERVER_SERVICES}" &&
    grep -q 'has an active backend; stop the service before updating its definition' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'retirement claim in progress' "${SERVICES_MANAGER_SOURCE_RETIREMENT}" &&
    grep -q 'claim_service_definition_retirement' "${SERVICES_MANAGER_SOURCE_RETIREMENT}" &&
    grep -q 'public service definitions must reject host rootfs paths' "${SERVER_SERVICE_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'public service definitions must reject local build context paths' "${SERVER_SERVICE_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'operator_service_definition_routes_are_resource_shaped_and_preconditioned' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_responses_redact_sandbox_launch_details' "${SERVER_SERVICE_MANAGER_REDACTION_TESTS}" &&
    grep -q 'sandbox resource response must not expose raw env values' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'launch-secret' "${SERVER_SERVICE_MANAGER_REDACTION_TESTS}" &&
    grep -q 'StatusCode::PRECONDITION_FAILED' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'Error::PreconditionFailed' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'StatusCode::PRECONDITION_FAILED' crates/nimbus-server/src/error_envelope.rs &&
    grep -q 'service_definition_routes_reject_body_conflicts_and_inline_credentials' "${SERVER_SERVICE_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'hostless external create should send' "${SERVER_SERVICE_MANAGER_DEFINITION_TESTS}" &&
    grep -Fq 'external_body["spec"]["backend"]["endpoint"]["url"]' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'principal_has_service_definition_permission' "${SERVER_RESOURCE_CONTROL_SERVICES}" &&
    grep -q 'principal_has_exact_service_grant' "${SERVER_SERVICE_GRANTS}"
}

condition_srm3_sdk_sandbox_surface() {
  grep -q 'readonly sandboxes: NimbusSandboxes' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'class NimbusSandboxes' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'create(input: NimbusSandboxCreateRequest)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'get(input: NimbusSandboxSelector)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'list(input: NimbusSandboxListRequest' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'stop(input: NimbusSandboxSelector)' "${SDK_ROOT_FILES[@]}" &&
    grep -q '/api/tenants/{tenant_id}/sandboxes' "${SDK_CONTROL_ROUTES}" &&
    grep -q '/api/tenants/{tenant_id}/sandboxes/{sandbox_id}' "${SDK_CONTROL_ROUTES}" &&
    grep -q 'sandbox resources are id-addressed, not name-addressed' "${SDK_SELFTEST}" &&
    ! grep -q 'resolveSandboxByName' "${SDK_ROOT_FILES[@]}"
}

condition_srm3_server_sandbox_routes() {
  # CP3 moved the SandboxSpecInput/SandboxSpecResponse request handling into
  # nimbus-compute/src/sandboxes.rs; the route mounting and response DTOs
  # stayed in nimbus-server/src/http/sandboxes.rs.
  grep -q '/api/tenants/{tenant_id}/sandboxes' "${SERVER_ROUTER}" &&
    grep -q 'get(http::list_sandboxes).post(http::create_sandbox)' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/sandboxes/{sandbox_id}' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop' "${SERVER_ROUTER}" &&
    grep -q 'SandboxResourceResponse' "${SERVER_SANDBOXES}" &&
    grep -q 'SandboxCollectionResponse' "${SERVER_SANDBOXES}" &&
    grep -q 'spec: SandboxSpecInput' "${COMPUTE_SANDBOXES}" &&
    grep -q 'SandboxSpecResponse::from_spec' "${COMPUTE_SANDBOXES}" &&
    grep -q 'principal_has_sandbox_permission' "${SERVER_RESOURCE_CONTROL_SANDBOXES}" &&
    grep -q 'principal_has_sandbox_list_permission' "${SERVER_RESOURCE_CONTROL_SANDBOXES}" &&
    grep -q 'sandbox_permission_scope_is_listable' "${SERVER_RESOURCE_CONTROL_SANDBOXES}" &&
    grep -q 'label_key' "${SERVER_SANDBOXES}"
}

condition_srm3_manager_sandbox_state() {
  # Native workload cutover split immutable desired source from provider
  # observation. The manager must key both by tenant plus opaque resource id,
  # reserve source before effects, and accept only exact execution projection.
  grep -q 'pub struct SandboxResource' crates/nimbus-services/src/catalog.rs &&
    grep -q 'pub(super) struct TenantSandboxResourceKey' crates/nimbus-services/src/manager/types.rs &&
    grep -q 'sandbox_resource_sources: BTreeMap<TenantSandboxResourceKey, SandboxResourceSource>' crates/nimbus-services/src/manager/types.rs &&
    grep -q 'BTreeMap<TenantSandboxResourceKey, SandboxResourceObservation>' crates/nimbus-services/src/manager/types.rs &&
    grep -q 'prepare_standalone_sandbox_provision_source' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'reserve_standalone_sandbox_provision_source' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'validate_standalone_sandbox_provision_decision' crates/nimbus-services/src/manager/source.rs &&
    grep -q 'project_sandbox_resource_execution_observation' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'sandbox_resource_snapshot_for_tenant' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'list_sandbox_resource_snapshots_for_tenant' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'standalone_source_owns_initial_generation_and_exact_replay_version' "${SERVICES_MANAGER_SANDBOX_RESOURCE_TESTS}" &&
    grep -q 'crossed_standalone_decision_rejects_before_source_mutation' "${SERVICES_MANAGER_SANDBOX_RESOURCE_TESTS}" &&
    grep -q 'source_only_sandbox_reads_are_truthful_repeatable_and_effect_free' "${SERVICES_MANAGER_SOURCE_PROJECTION_TESTS}" &&
    grep -q 'compute_projection_authenticates_source_version_and_execution_id_before_first_write' "${SERVICES_MANAGER_SOURCE_PROJECTION_TESTS}" &&
    grep -q 'requires standalone sandbox owner metadata' crates/nimbus-services/src/manager/sandboxes.rs &&
    ! grep -q 'belongs to tenant' crates/nimbus-services/src/manager/sandboxes.rs
}

condition_srm3_sandbox_boundary_tests() {
  grep -q 'sandbox_resource_routes_are_id_addressed_and_do_not_publish_services' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'sandbox_routes_enforce_owner_authority_and_backend_admission' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'sandbox_routes_reject_public_host_path_roots_before_launch' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'sandbox_routes_mask_cross_tenant_sandbox_ids_as_not_found' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'cross-tenant sandbox probes must not stop the probed sandbox' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'backend-admission rejection must happen before backend start' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'exact-scoped sandbox list should send' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'prefix-scoped sandbox list should send' "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}" &&
    grep -q 'tenant-a-sandbox' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'tenant-b-sandbox' "${SERVER_SERVICE_MANAGER_TESTS}"
}

condition_srm4_sdk_session_surface() {
  grep -q 'readonly sessions: NimbusSessions' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'class NimbusSessions' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'open(input: NimbusSessionOpenRequest)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'get(input: NimbusSessionSelector)' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'list(input: NimbusSessionListRequest' "${SDK_ROOT_FILES[@]}" &&
    grep -q 'close(input: NimbusSessionCloseRequest)' "${SDK_ROOT_FILES[@]}" &&
    grep -q '/api/sessions' "${SDK_CONTROL_ROUTES}" &&
    grep -q 'sessions use open, not create' "${SDK_SELFTEST}" &&
    grep -q 'client-managed renewal is not part of the session lifecycle' "${SDK_SELFTEST}" &&
    grep -q 'unsupported channels are not part of the public session channel set' "${SDK_SELFTEST}" &&
    ! grep -q 'sessions.create' "${SDK_ROOT_FILES[@]}" &&
    ! grep -q 'sessions.renew' "${SDK_ROOT_FILES[@]}" &&
    ! grep -q 'sessions.extend' "${SDK_ROOT_FILES[@]}"
}

condition_srm4_server_session_routes() {
  grep -q '/api/sessions' "${SERVER_ROUTER}" &&
    grep -q 'get(http::list_sessions).post(http::open_session)' "${SERVER_ROUTER}" &&
    grep -q '/api/sessions/{session_id}' "${SERVER_ROUTER}" &&
    grep -q '/api/sessions/{session_id}/close' "${SERVER_ROUTER}" &&
    grep -q 'SessionResourceResponse' "${SERVER_SESSIONS}" &&
    grep -q 'SessionCollectionResponse' "${SERVER_SESSIONS}" &&
    grep -q 'authorize_session_resource_lookup' "${SERVER_SESSIONS}" &&
    grep -q 'authorize_session_resource_target' "${SERVER_SESSIONS}" &&
    grep -q 'session_target_reachable' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q 'principal_can_list_session' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q 'principal_has_session_permission' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q 'principal_has_session_list_permission' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q 'session_permission_scope_is_listable' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q "session open target requires exactly one of \`service\` or \`sandbox\`" "${SERVER_SESSIONS}" &&
    grep -q 'principal_has_exact_service_grant' "${SERVER_SERVICE_GRANTS}" &&
    grep -q 'super::super::service_grants::principal_has_exact_service_grant' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q 'principal_has_sandbox_reach' "${SERVER_RESOURCE_CONTROL_SESSIONS}" &&
    grep -q 'session_permission_channels_allow' "${SERVER_RESOURCE_CONTROL_SESSIONS}"
}

condition_srm4_manager_session_state() {
  grep -q 'pub struct SessionResource' crates/nimbus-services/src/catalog.rs &&
    grep -q 'pub enum SessionTarget' crates/nimbus-services/src/catalog.rs &&
    grep -q 'pub enum SessionLifecycleState' crates/nimbus-services/src/catalog.rs &&
    grep -q 'sessions: BTreeMap<String, SessionResource>' crates/nimbus-services/src/manager/types.rs &&
    grep -q 'open_session_async' crates/nimbus-services/src/manager/sessions.rs &&
    grep -q 'session open requires a ready sandbox target' crates/nimbus-services/src/manager/sessions.rs &&
    grep -q 'close_session' crates/nimbus-services/src/manager/sessions.rs &&
    grep -q 'fn next_session_id' crates/nimbus-services/src/manager/sessions.rs &&
    grep -q 'Ulid::new' crates/nimbus-services/src/manager/sessions.rs &&
    ! grep -q 'format!("session-{}", state.next_session_version)' crates/nimbus-services/src/manager/sessions.rs &&
    grep -q 'DEFAULT_SESSION_TTL_MILLIS' crates/nimbus-services/src/manager/sessions.rs &&
    grep -q 'built-in browser service' crates/nimbus-services/src/manager/sessions.rs
}

condition_srm4_session_boundary_tests() {
  grep -q 'session_routes_open_service_sessions_with_target_snapshot_and_audit' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'session_routes_reject_service_sessions_without_exact_grants_and_unsupported_channels' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'session_routes_open_sandbox_sessions_by_id_and_expire_fail_closed' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'session ids must be opaque rather than sequence-shaped' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'wrong-tenant existing-session get should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'same-tenant no-session-permission get should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'same-tenant no-target-grant get should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'service-scoped session get should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'service-scoped session list should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'sandbox-scoped session list should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'service_scoped_close' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'open_session_rejects_not_ready_sandbox_targets' "${SERVICES_MANAGER_SESSION_TESTS}" &&
    grep -q 'tenant-a-browser-session-service-scope' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'session list must filter sessions whose service target is not reachable by exact grant' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'wildcard-grant session open should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'ambiguous session target open should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'wait_for_session_lifecycle_state' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'service_definition_delete_refuses_live_sessions_unless_force_closes_them' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'unauthenticated missing-session close should send' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'late_client_close' "${SERVER_SERVICE_MANAGER_SESSION_TESTS}" &&
    grep -q 'tenant-a-browser-session-no-grant' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'tenant-a-sandbox-session' "${SERVER_SERVICE_MANAGER_TESTS}"
}

condition_srm5_examples_are_documented() {
  [ -f "${SDK_EXAMPLES_DOC}" ] &&
    grep -q 'examples/nimbus-sdk-resource-model.md' "${DOCS_README}" &&
    grep -q 'Start A Compose Service' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'Register A Built-In Load Balancer Service' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'Create A Task Sandbox And Open A Session' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'Open A Built-In Browser Service Session' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'Register A Temporary Sandbox-Backed Service' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'Use Nimbus From An Adapter Action' "${SDK_EXAMPLES_DOC}"
}

condition_srm5_examples_use_current_sdk_surface() {
  grep -q 'nimbus.services.start({ name: "db", waitUntil: "ready" })' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'nimbus.sandboxes.create' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'target: { sandbox: { id: sandbox.metadata.id } }' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'target: { service: { name: "browser" } }' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'target: { service: { name: "mcp-tools" } }' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'channels: \["cdp", "page"\]' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'channels: \["stdio", "files"\]' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'sessions.create' "${SDK_README}" &&
    ! grep -q 'ensureRunning' "${SDK_EXAMPLES_DOC}" &&
    ! grep -q 'ctx\\.services' "${SDK_EXAMPLES_DOC}" &&
    ! grep -q 'nimbus\\.services\\.resolve' "${SDK_EXAMPLES_DOC}"
}

condition_srm5_type_fixtures_cover_examples() {
  grep -q 'const _serviceStartReady = _sdk.services.start' "${SDK_SELFTEST}" &&
    grep -q 'const _serviceCreateBuiltIn = _sdk.services.create' "${SDK_SELFTEST}" &&
    grep -q 'const _sandboxCreate = _sdk.sandboxes.create' "${SDK_SELFTEST}" &&
    grep -q 'const _serviceSession = _sdk.sessions.open' "${SDK_SELFTEST}" &&
    grep -q 'const _sandboxSession = _sdk.sessions.open' "${SDK_SELFTEST}" &&
    grep -q 'export const warmSearch' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'import { action } from "./_generated/server"' "${SDK_EXAMPLES_DOC}" &&
    grep -q 'import { Nimbus } from "@nimbus/nimbus"' "${SDK_EXAMPLES_DOC}"
}

condition_srm6_closeout_recorded() {
    grep -q "| SRM6 | \`done\`" "${PLAN}" &&
    grep -q 'SRM6 final closeout' "${PLAN}" &&
    grep -q 'mod definitions;' "${SERVICES_MANAGER}" &&
    [ "$(wc -l < "${SERVICES_MANAGER}")" -lt 1500 ] &&
    [ "$(wc -l < "${SERVICES_MANAGER_DEFINITION_TESTS}")" -lt 2000 ] &&
    # The service manager remains concept-owned in nimbus-services. Durable
    # definition retirement now spans its source-retirement seam and the
    # server composition test seam; both must stay below their documented
    # decomposition thresholds.
    grep -q 'mod service_manager;' "${SERVER_TESTS_ROOT}" &&
    [ "$(wc -l < "${SERVICES_MANAGER_SOURCE_RETIREMENT}")" -lt 1500 ] &&
    [ "$(wc -l < "${SERVICES_MANAGER_SOURCE_RETIREMENT_TESTS}")" -lt 2000 ] &&
    grep -q 'mod definitions;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod definition_retirement;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod redaction;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod sandboxes;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod sessions;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service-manager route fixture root' "${PLAN}" &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_DEFINITION_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_DEFINITION_RETIREMENT_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_REDACTION_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_SESSION_TESTS}")" -lt 2000 ] &&
    grep -q 'finalize_tenant_sources_after_recorded' "${SERVICES_MANAGER_TENANT_RETIREMENT}" &&
    grep -q 'tenant_retirement_finalizer_removes_complete_sources_and_sessions_without_effects' "${SERVICES_MANAGER_TENANT_RETIREMENT_TESTS}" &&
    grep -q 'service_definition_observations' "${SERVICES_MANAGER_TENANT_RETIREMENT}" &&
    grep -q 'sandbox_resource_sources' "${SERVICES_MANAGER_TENANT_RETIREMENT}" &&
    grep -q 'sandbox_resource_observations' "${SERVICES_MANAGER_TENANT_RETIREMENT}" &&
    grep -q 'retain(|_, session| &session.tenant_id != tenant_id)' "${SERVICES_MANAGER_TENANT_RETIREMENT}" &&
    grep -q 'bash scripts/verify-nimbus-sdk-resource-model.sh` pass' "${PLAN}" &&
    grep -q 'bash scripts/verify-nimbus-capability-segregation.sh` pass' "${PLAN}" &&
    grep -q 'git diff --check` pass' "${PLAN}"
}

step "SRM0" "plan registration and architecture model"
check "resource-model plan is registered and linked to architecture model" condition_srm0_registration

step "SRM1" "top-level SDK service lifecycle/status baseline"
check "package root exposes Nimbus service APIs and no future/stale root surface" condition_srm1_package_surface
check "service lifecycle wait contract is verb-aware" condition_srm1_lifecycle_wait_contract
check "selftests and artifact policy reject stale/future SDK methods" condition_srm1_future_surface_guards
check "server exposes canonical tenant service status/lifecycle routes" condition_srm1_server_routes
check "adapter package sources expose no Nimbus resource/transport shortcuts" condition_srm1_adapter_surfaces_stay_clean
require_command_passes "package typecheck proves SDK surface contract" npm run typecheck --workspace @nimbus/nimbus

step "SRM2" "service definition resource API"
check "SDK exposes service definition CRUD without sandbox/session namespaces" condition_srm2_sdk_definition_surface
check "server exposes canonical service definition collection/update/delete routes" condition_srm2_server_definition_routes
check "service manager owns dynamic definition state and backend validation" condition_srm2_manager_definition_state
check "tests and helpers enforce service-definition permission split" condition_srm2_authorization_split_tests

step "SRM3" "id-addressed sandbox resource API"
check "SDK exposes id-addressed sandbox resource APIs" condition_srm3_sdk_sandbox_surface
check "server exposes tenant sandbox collection/get/stop routes" condition_srm3_server_sandbox_routes
check "service manager tracks standalone sandbox resources by opaque id" condition_srm3_manager_sandbox_state
check "tests enforce id-addressing, standalone owner, and label non-authority" condition_srm3_sandbox_boundary_tests

step "SRM4" "scoped session resource API"
check "SDK exposes session open/get/list/close and rejects stale verbs" condition_srm4_sdk_session_surface
check "server exposes session collection/get/close routes with target reach checks" condition_srm4_server_session_routes
check "service manager tracks sessions, target snapshots, TTL, and channel support" condition_srm4_manager_session_state
check "tests enforce service grants, sandbox ids, channel gates, expiration, and audit" condition_srm4_session_boundary_tests

step "SRM5" "agent/app examples and adapter boundary"
check "SDK resource-model examples are documented and linked" condition_srm5_examples_are_documented
check "examples use the current service/sandbox/session SDK surface" condition_srm5_examples_use_current_sdk_surface
check "owned package type fixtures cover the documented usage families" condition_srm5_type_fixtures_cover_examples

step "SRM6" "final verifier and closeout evidence"
check "plan ledger records final verifier and required gate evidence" condition_srm6_closeout_recorded

printf '\nSummary: %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf ' - %s\n' "${detail}"
  done
  exit 1
fi
