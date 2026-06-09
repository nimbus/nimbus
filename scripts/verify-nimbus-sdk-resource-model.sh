#!/usr/bin/env bash
# Aggregate completion-gate verifier for
# docs/plans/nimbus-sdk-resource-model-plan.md.
#
# SRM0 ships this as a registration and model verifier. Later SRM phases add
# concrete route/SDK/resource checks before marking their phases done.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/plans/nimbus-sdk-resource-model-plan.md"
MODEL_DOC="docs/architecture/sandbox/service-sandbox-session-model.md"
PLANS_README="docs/plans/README.md"
AGENTS_MD="AGENTS.md"
SDK_PACKAGE="packages/nimbus/package.json"
SDK_ROOT="packages/nimbus/src/index.ts"
SDK_SELFTEST="packages/nimbus/src/selftest.mjs"
SDK_SURFACE_CONTRACT="packages/nimbus/src/capability_surface_contract.mjs"
ROOT_SDK_POLICY="scripts/nimbus-root-sdk-artifact-policy.mjs"
SDK_EXAMPLES_DOC="docs/examples/nimbus-sdk-resource-model.md"
DOCS_README="docs/README.md"
SDK_README="packages/nimbus/README.md"
SERVICES_MANAGER="crates/nimbus-services/src/manager.rs"
SERVICES_MANAGER_DEFINITION_TESTS="crates/nimbus-services/src/manager/tests/definitions.rs"
SERVER_ROUTER="crates/nimbus-server/src/router.rs"
SERVER_SERVICE_GRANTS="crates/nimbus-server/src/http/service_grants.rs"
SERVER_SERVICES="crates/nimbus-server/src/http/services.rs"
SERVER_SANDBOXES="crates/nimbus-server/src/http/sandboxes.rs"
SERVER_SANDBOX_SPEC="crates/nimbus-server/src/http/sandbox_spec.rs"
SERVER_SESSIONS="crates/nimbus-server/src/http/sessions.rs"
SERVER_SERVICE_MANAGER="crates/nimbus-server/src/service_manager.rs"
SERVER_SERVICE_MANAGER_TESTS="crates/nimbus-server/src/service_manager/tests.rs"
SERVER_SERVICE_MANAGER_DEFINITION_TESTS="crates/nimbus-server/src/service_manager/tests/definitions.rs"
SERVER_SERVICE_MANAGER_REDACTION_TESTS="crates/nimbus-server/src/service_manager/tests/redaction.rs"
SERVER_SERVICE_MANAGER_SANDBOX_TESTS="crates/nimbus-server/src/service_manager/tests/sandboxes.rs"
SERVER_SERVICE_MANAGER_SESSION_TESTS="crates/nimbus-server/src/service_manager/tests/sessions.rs"

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
  [ -f "${PLAN}" ] &&
    [ -f "${MODEL_DOC}" ] &&
    grep -q 'nimbus-sdk-resource-model-plan.md' "${PLANS_README}" &&
    grep -q 'nimbus-sdk-resource-model-plan.md' "${AGENTS_MD}" &&
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
const sdk = fs.readFileSync("packages/nimbus/src/index.ts", "utf8");

const hasCanonicalManifest =
  pkg.name === "@nimbus/nimbus" &&
  pkg.exports?.["."] === "./src/index.ts" &&
  pkg.exports?.["./transports/rest"] === "./src/transports/rest.ts" &&
  !pkg.exports?.["./rest"] &&
  !pkg.exports?.["./transports/host"];

const hasCanonicalMethods = [
  "export class Nimbus",
  "readonly services: NimbusServices",
  "start(input",
  "stop(input",
  "restart(input",
  "get(selector",
  "wait(input",
  "/api/tenants/",
  "/services/",
  "createDefaultRestClient",
].every((fragment) => sdk.includes(fragment));

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

process.exit(hasCanonicalManifest && hasCanonicalMethods && lacksFutureOrStaleRootSurface ? 0 : 1);
NODE
}

condition_srm1_lifecycle_wait_contract() {
  grep -q 'NimbusServiceActivationWaitCondition' "${SDK_ROOT}" &&
    grep -q 'NimbusServiceStopWaitCondition' "${SDK_ROOT}" &&
    grep -q 'assertLifecycleWaitUntil("start", input.waitUntil, \["ready", "healthy"\])' "${SDK_ROOT}" &&
    grep -q 'assertLifecycleWaitUntil("stop", input.waitUntil, \["stopped"\])' "${SDK_ROOT}" &&
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
    grep -q 'requires operator credentials or authenticated tenant/spawned workload identity' "${SERVER_SERVICES}" &&
    grep -q 'principal_has_exact_service_grant' "${SERVER_SERVICE_GRANTS}" &&
    grep -q 'service_grant_value_contains_wildcard' "${SERVER_SERVICE_GRANTS}" &&
    grep -q 'services:\*' "${SERVER_SERVICE_GRANTS}"
}

condition_srm1_adapter_surfaces_stay_clean() {
  ! grep -RqE 'ctx\.(services|sandboxes|sessions|browser)|@nimbus/nimbus/transports/(rest|host)' \
    packages/convex/src packages/firebase/src packages/mongodb/src packages/dynamodb/src
}

condition_srm2_sdk_definition_surface() {
  grep -q 'create(input: NimbusServiceCreateRequest)' "${SDK_ROOT}" &&
    grep -q 'update(input: NimbusServiceUpdateRequest)' "${SDK_ROOT}" &&
    grep -q 'delete(input: NimbusServiceDeleteRequest)' "${SDK_ROOT}" &&
    grep -q 'list(input: NimbusServiceListRequest' "${SDK_ROOT}" &&
    grep -q 'kind: "builtIn"' "${SDK_ROOT}" &&
    grep -q 'kind: "external"' "${SDK_ROOT}" &&
    grep -q 'ifMatchGeneration' "${SDK_ROOT}" &&
    grep -q 'NimbusServiceDefinitionCollection' "${SDK_ROOT}" &&
    grep -q 'NimbusSandboxOwnerSpec' "${SDK_ROOT}" &&
    grep -q 'NimbusSandboxRootSpec' "${SDK_ROOT}" &&
    grep -q 'NimbusSandboxRootResponse' "${SDK_ROOT}" &&
    grep -q 'NimbusSandboxOciImageReferenceSource' "${SDK_ROOT}" &&
    ! grep -q 'kind: "rootfs"' "${SDK_ROOT}" &&
    ! grep -q 'dockerfilePath' "${SDK_ROOT}" &&
    grep -q 'NimbusSandboxProcessSpec' "${SDK_ROOT}" &&
    grep -q 'NimbusSandboxProcessResponse' "${SDK_ROOT}" &&
    grep -q 'NimbusRedactedValues' "${SDK_ROOT}" &&
    grep -q 'export interface NimbusSandboxSpec' "${SDK_ROOT}" &&
    grep -q 'export interface NimbusSandboxSpecResponse' "${SDK_ROOT}" &&
    grep -q 'NimbusServiceBackendResponse' "${SDK_ROOT}" &&
    grep -q 'tenantId?: string' "${SDK_ROOT}" &&
    grep -q '_sdk.services.create' "${SDK_SELFTEST}" &&
    grep -q '_sdk.services.update' "${SDK_SELFTEST}" &&
    grep -q '_sdk.services.delete' "${SDK_SELFTEST}" &&
    grep -q '_sdk.services.list' "${SDK_SELFTEST}"
}

condition_srm2_server_definition_routes() {
  grep -q '/api/tenants/{tenant_id}/services' "${SERVER_ROUTER}" &&
    grep -q 'get(http::list_service_definitions).post(http::create_service_definition)' "${SERVER_ROUTER}" &&
    grep -q 'put(http::update_service_definition)' "${SERVER_ROUTER}" &&
    grep -q 'delete(http::delete_service_definition)' "${SERVER_ROUTER}" &&
    grep -q 'ServiceDefinitionResourceResponse' "${SERVER_SERVICES}" &&
    grep -q 'ServiceDefinitionCollectionResponse' "${SERVER_SERVICES}" &&
    grep -q 'ServiceBackendInput::Sandbox' "${SERVER_SERVICES}" &&
    grep -q 'into_spec(tenant_id, Some(service_name))' "${SERVER_SERVICES}" &&
    grep -q 'SandboxSpecResponse::from_spec' "${SERVER_SERVICES}" &&
    grep -q 'pub(crate) struct SandboxSpecInput' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'RedactedValuesResponse' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'operatorOnlyLaunchInput' "${SERVER_SANDBOX_SPEC}" &&
    ! grep -q 'env: process.env' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'sandbox spec tenantId.*must match route tenant' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'host rootfs path.*operator-only internal input' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'local build context paths.*operator-only internal inputs' "${SERVER_SANDBOX_SPEC}" &&
    grep -q 'ExternalEndpointPolicyResponse' "${SERVER_SERVICES}" &&
    grep -q 'ExternalAuthPolicyResponse' "${SERVER_SERVICES}" &&
    grep -q 'HealthCheckPolicyResponse' "${SERVER_SERVICES}" &&
    grep -q 'ServiceDefinitionProjection::List' "${SERVER_SERVICES}" &&
    grep -q 'ServiceDefinitionProjection::Inspect' "${SERVER_SERVICES}" &&
    grep -q 'requiresInspectPermission' "${SERVER_SERVICES}" &&
    grep -q 'rename = "builtIn"' "${SERVER_SERVICES}" &&
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
    grep -q 'delete_service_definition_async' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'service_volume_policy_for_tenant' crates/nimbus-services/src/catalog.rs &&
    grep -q 'service_launch_for_tenant' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'volume_policy' crates/nimbus-services/src/manager/launch.rs &&
    grep -q 'ensure_sandbox_mounts_match' crates/nimbus-services/src/manager/launch.rs &&
    grep -q 'Url::parse' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'SUPPORTED_BUILT_IN_PROVIDERS' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'endpoint must not embed credentials' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'health.path must start with `/`' crates/nimbus-services/src/manager/definitions.rs
}

condition_srm2_authorization_split_tests() {
  grep -q 'service_definition_permissions_do_not_imply_service_lifecycle_grants' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_force_delete_requires_separate_policy_and_exact_service_grant' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_list_only_permission_redacts_inspect_details' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service_definition_update_rejects_active_backend_until_stopped' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'delete_service_definition_serializes_with_in_flight_activation' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'force delete must record a stopped handle with endpoints cleared' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'start_service_for_decision_rejects_service_volume_without_catalog_policy' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'start_service_for_decision_accepts_declared_service_tenant_volume' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'create_service_definition_rejects_malformed_external_endpoint' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'open_service_session_rejects_in_flight_definition_delete' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    ! grep -q 'yield_now' "${SERVICES_MANAGER_DEFINITION_TESTS}" &&
    grep -q 'ServiceDefinitionAction::ForceDelete' "${SERVER_SERVICES}" &&
    grep -q 'ServiceDefinitionAction::Inspect' "${SERVER_SERVICES}" &&
    grep -q 'has an active backend; stop the service before updating its definition' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'activation in progress' crates/nimbus-services/src/manager/definitions.rs &&
    grep -q 'claim_service_definition_delete' crates/nimbus-services/src/manager/definitions.rs &&
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
    grep -q 'principal_has_service_definition_permission' "${SERVER_SERVICES}" &&
    grep -q 'principal_has_exact_service_grant' "${SERVER_SERVICE_GRANTS}"
}

condition_srm3_sdk_sandbox_surface() {
  grep -q 'readonly sandboxes: NimbusSandboxes' "${SDK_ROOT}" &&
    grep -q 'class NimbusSandboxes' "${SDK_ROOT}" &&
    grep -q 'create(input: NimbusSandboxCreateRequest)' "${SDK_ROOT}" &&
    grep -q 'get(input: NimbusSandboxSelector)' "${SDK_ROOT}" &&
    grep -q 'list(input: NimbusSandboxListRequest' "${SDK_ROOT}" &&
    grep -q 'stop(input: NimbusSandboxSelector)' "${SDK_ROOT}" &&
    grep -q '/api/tenants/.*/sandboxes' "${SDK_ROOT}" &&
    grep -q 'sandbox resources are id-addressed, not name-addressed' "${SDK_SELFTEST}" &&
    ! grep -q 'resolveSandboxByName' "${SDK_ROOT}"
}

condition_srm3_server_sandbox_routes() {
  grep -q '/api/tenants/{tenant_id}/sandboxes' "${SERVER_ROUTER}" &&
    grep -q 'get(http::list_sandboxes).post(http::create_sandbox)' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/sandboxes/{sandbox_id}' "${SERVER_ROUTER}" &&
    grep -q '/api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop' "${SERVER_ROUTER}" &&
    grep -q 'SandboxResourceResponse' "${SERVER_SANDBOXES}" &&
    grep -q 'SandboxCollectionResponse' "${SERVER_SANDBOXES}" &&
    grep -q 'spec: SandboxSpecInput' "${SERVER_SANDBOXES}" &&
    grep -q 'SandboxSpecResponse::from_spec' "${SERVER_SANDBOXES}" &&
    grep -q 'principal_has_sandbox_permission' "${SERVER_SANDBOXES}" &&
    grep -q 'principal_has_sandbox_list_permission' "${SERVER_SANDBOXES}" &&
    grep -q 'sandbox_permission_scope_is_listable' "${SERVER_SANDBOXES}" &&
    grep -q 'label_key' "${SERVER_SANDBOXES}"
}

condition_srm3_manager_sandbox_state() {
  grep -q 'pub struct SandboxResource' crates/nimbus-services/src/catalog.rs &&
    grep -q 'sandbox_resources: BTreeMap<String, SandboxResource>' crates/nimbus-services/src/manager/types.rs &&
    grep -q 'create_sandbox_resource_async' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'create_sandbox_resource_for_context_async' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'WorkloadAttributes::sandbox' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'ensure_sandbox_spec_matches' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'ensure_sandbox_mounts_match' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'stop_started_sandbox_resource_after_create_error' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'create_sandbox_resource_stops_backend_after_post_start_validation_errors' crates/nimbus-services/src/manager.rs &&
    grep -q 'create_sandbox_resource_preserves_existing_backend_after_duplicate_started_id' crates/nimbus-services/src/manager.rs &&
    grep -q 'duplicate-id failure must not stop a tracked sandbox through the create path' crates/nimbus-services/src/manager.rs &&
    grep -q 'get_sandbox_resource_async' crates/nimbus-services/src/manager/sandboxes.rs &&
    grep -q 'stop_sandbox_resource_async' crates/nimbus-services/src/manager/sandboxes.rs &&
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
  grep -q 'readonly sessions: NimbusSessions' "${SDK_ROOT}" &&
    grep -q 'class NimbusSessions' "${SDK_ROOT}" &&
    grep -q 'open(input: NimbusSessionOpenRequest)' "${SDK_ROOT}" &&
    grep -q 'get(input: NimbusSessionSelector)' "${SDK_ROOT}" &&
    grep -q 'list(input: NimbusSessionListRequest' "${SDK_ROOT}" &&
    grep -q 'close(input: NimbusSessionCloseRequest)' "${SDK_ROOT}" &&
    grep -q '/api/sessions' "${SDK_ROOT}" &&
    grep -q 'sessions use open, not create' "${SDK_SELFTEST}" &&
    grep -q 'client-managed renewal is not part of the session lifecycle' "${SDK_SELFTEST}" &&
    grep -q 'unsupported channels are not part of the public session channel set' "${SDK_SELFTEST}" &&
    ! grep -q 'sessions.create' "${SDK_ROOT}" &&
    ! grep -q 'sessions.renew' "${SDK_ROOT}" &&
    ! grep -q 'sessions.extend' "${SDK_ROOT}"
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
    grep -q 'session_target_reachable' "${SERVER_SESSIONS}" &&
    grep -q 'principal_can_list_session' "${SERVER_SESSIONS}" &&
    grep -q 'principal_has_session_permission' "${SERVER_SESSIONS}" &&
    grep -q 'principal_has_session_list_permission' "${SERVER_SESSIONS}" &&
    grep -q 'session_permission_scope_is_listable' "${SERVER_SESSIONS}" &&
    grep -q 'session open target requires exactly one of `service` or `sandbox`' "${SERVER_SESSIONS}" &&
    grep -q 'principal_has_exact_service_grant' "${SERVER_SERVICE_GRANTS}" &&
    grep -q 'super::service_grants::principal_has_exact_service_grant' "${SERVER_SESSIONS}" &&
    grep -q 'principal_has_sandbox_reach' "${SERVER_SESSIONS}" &&
    grep -q 'session_permission_channels_allow' "${SERVER_SESSIONS}"
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
    grep -q 'open_session_rejects_not_ready_sandbox_targets' crates/nimbus-services/src/manager.rs &&
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
    grep -q '| SRM6 | `done`' "${PLAN}" &&
    grep -q 'SRM6 final closeout' "${PLAN}" &&
    grep -q 'mod definitions;' "${SERVICES_MANAGER}" &&
    [ "$(wc -l < "${SERVICES_MANAGER}")" -lt 1500 ] &&
    [ "$(wc -l < "${SERVICES_MANAGER_DEFINITION_TESTS}")" -lt 2000 ] &&
    grep -q 'mod tests;' "${SERVER_SERVICE_MANAGER}" &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER}")" -lt 2000 ] &&
    grep -q 'mod definitions;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod redaction;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod sandboxes;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'mod sessions;' "${SERVER_SERVICE_MANAGER_TESTS}" &&
    grep -q 'service-manager route fixture root' "${PLAN}" &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_DEFINITION_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_REDACTION_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_SANDBOX_TESTS}")" -lt 2000 ] &&
    [ "$(wc -l < "${SERVER_SERVICE_MANAGER_SESSION_TESTS}")" -lt 2000 ] &&
    grep -q 'teardown_tenant_stops_tracked_sandboxes_and_clears_tenant_resources' crates/nimbus-services/src/manager.rs &&
    grep -q 'retain(|key, _| &key.tenant_id != tenant_id)' crates/nimbus-services/src/manager/registry.rs &&
    grep -q 'retain(|_, resource| &resource.tenant_id != tenant_id)' crates/nimbus-services/src/manager/registry.rs &&
    grep -q 'retain(|_, session| &session.tenant_id != tenant_id)' crates/nimbus-services/src/manager/registry.rs &&
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
