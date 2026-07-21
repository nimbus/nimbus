#!/usr/bin/env bash

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose_file="${NIMBUS_PROVIDER_FIXTURE_COMPOSE_FILE:-${repo_root}/compose.test-external-providers.yaml}"
project_name="${NIMBUS_PROVIDER_FIXTURE_PROJECT:-nimbus-external-provider-tests}"
container_runtime="${NIMBUS_CONTAINER_RUNTIME:-docker}"
test_runner="${NIMBUS_PROVIDER_TEST_RUNNER:-${repo_root}/scripts/test-external-providers.sh}"
wait_attempts="${NIMBUS_PROVIDER_FIXTURE_WAIT_ATTEMPTS:-60}"
wait_interval="${NIMBUS_PROVIDER_FIXTURE_WAIT_INTERVAL_SECONDS:-1}"
keep_fixtures="${NIMBUS_PROVIDER_FIXTURE_KEEP:-0}"
reuse_fixtures="${NIMBUS_PROVIDER_FIXTURE_REUSE:-0}"
fixture_owner="external-provider-tests"

started_providers=""
active_providers=""
cleanup_enabled=0

usage() {
  cat >&2 <<'EOF'
usage: scripts/external-provider-fixture.sh <run|up|down|logs> <postgres|mysql|libsql|all>

Environment:
  NIMBUS_PROVIDER_FIXTURE_KEEP=1   retain fixtures started by `run`
  NIMBUS_PROVIDER_FIXTURE_REUSE=1  reuse an existing healthy matching fixture
  NIMBUS_PROVIDER_FIXTURE_*_PORT   override a localhost published port
EOF
}

fail() {
  echo "external-provider fixture: $*" >&2
  return 1
}

validate_toggle() {
  local name="$1"
  local value="$2"
  if [[ "${value}" != "0" && "${value}" != "1" ]]; then
    fail "${name} must be 0 or 1 (got ${value})"
  fi
}

validate_positive_integer() {
  local name="$1"
  local value="$2"
  if [[ ! "${value}" =~ ^[1-9][0-9]*$ ]]; then
    fail "${name} must be a positive integer (got ${value})"
  fi
}

validate_port() {
  local name="$1"
  local value="$2"
  if [[ ! "${value}" =~ ^[0-9]+$ ]] || (( value < 1 || value > 65535 )); then
    fail "${name} must be an integer from 1 through 65535 (got ${value})"
  fi
}

provider_list() {
  case "$1" in
    postgres | mysql | libsql)
      printf '%s\n' "$1"
      ;;
    all)
      printf '%s\n' postgres mysql libsql
      ;;
    *)
      fail "unknown provider '$1' (expected postgres|mysql|libsql|all)"
      ;;
  esac
}

contains_provider() {
  local haystack="$1"
  local needle="$2"
  case " ${haystack} " in
    *" ${needle} "*) return 0 ;;
    *) return 1 ;;
  esac
}

append_provider() {
  local variable_name="$1"
  local provider="$2"
  local current="${!variable_name:-}"
  if ! contains_provider "${current}" "${provider}"; then
    printf -v "${variable_name}" '%s%s%s' "${current}" "${current:+ }" "${provider}"
  fi
}

compose() {
  "${container_runtime}" compose \
    --project-name "${project_name}" \
    --file "${compose_file}" \
    "$@"
}

ensure_runtime() {
  if [[ "${container_runtime}" == */* ]]; then
    [[ -x "${container_runtime}" ]] || fail "container runtime is not executable: ${container_runtime}"
  else
    command -v "${container_runtime}" >/dev/null 2>&1 \
      || fail "container runtime '${container_runtime}' is not installed or not on PATH"
  fi
  "${container_runtime}" info >/dev/null 2>&1 \
    || fail "container runtime '${container_runtime}' is installed but unavailable"
  "${container_runtime}" compose version >/dev/null 2>&1 \
    || fail "'${container_runtime} compose' is unavailable"
  [[ -f "${compose_file}" ]] || fail "fixture compose file does not exist: ${compose_file}"
  compose config --quiet >/dev/null \
    || fail "fixture compose configuration is invalid: ${compose_file}"
}

provider_image() {
  local provider="$1"
  local output
  if ! output="$(compose config --images "${provider}")"; then
    fail "could not resolve the compose image for '${provider}'"
    return 1
  fi
  if [[ -z "${output}" || "${output}" == *$'\n'* ]]; then
    fail "fixture service '${provider}' must resolve to exactly one image"
    return 1
  fi
  printf '%s\n' "${output}"
}

provider_config_hash() {
  local provider="$1"
  local output
  if ! output="$(compose config --hash "${provider}")"; then
    fail "could not resolve the compose config hash for '${provider}'"
    return 1
  fi
  if [[ "${output}" != "${provider} "* ]]; then
    fail "could not resolve the compose config hash for '${provider}'"
    return 1
  fi
  printf '%s\n' "${output#* }"
}

provider_ports() {
  case "$1" in
    postgres)
      printf '%s\n' "${NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT:-5432}"
      ;;
    mysql)
      printf '%s\n' "${NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT:-3306}"
      ;;
    libsql)
      printf '%s %s\n' \
        "${NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT:-18080}" \
        "${NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT:-18081}"
      ;;
  esac
}

validate_provider_ports() {
  local provider="$1"
  local ports
  local port
  ports="$(provider_ports "${provider}")"
  for port in ${ports}; do
    validate_port "${provider} fixture port" "${port}"
  done
  if [[ "${provider}" == "libsql" ]]; then
    local primary_port="${ports%% *}"
    local admin_port="${ports##* }"
    [[ "${primary_port}" != "${admin_port}" ]] \
      || fail "libSQL primary and admin fixture ports must differ"
  fi
}

container_id() {
  compose ps --all --quiet "$1" 2>/dev/null || true
}

container_metadata() {
  "${container_runtime}" inspect \
    --format '{{ index .Config.Labels "org.nimbus.fixture.owner" }}|{{ index .Config.Labels "org.nimbus.fixture.provider" }}|{{ index .Config.Labels "com.docker.compose.config-hash" }}|{{ .Config.Image }}|{{ if .State.Health }}{{ .State.Health.Status }}{{ else }}none{{ end }}' \
    "$1"
}

matching_container_metadata() {
  local provider="$1"
  local id="$2"
  local expected_image="$3"
  local expected_hash="$4"
  local metadata
  local owner
  local actual_provider
  local actual_hash
  local actual_image
  local health
  if ! metadata="$(container_metadata "${id}")"; then
    fail "cannot inspect existing '${provider}' fixture container ${id}"
    return 1
  fi
  IFS='|' read -r owner actual_provider actual_hash actual_image health <<<"${metadata}"
  if [[ "${owner}" != "${fixture_owner}" \
    || "${actual_provider}" != "${provider}" \
    || "${actual_hash}" != "${expected_hash}" \
    || "${actual_image}" != "${expected_image}" ]]; then
    fail "refusing foreign or mismatched '${provider}' container ${id}; owner/provider/image/config must match this fixture definition"
    return 1
  fi
  printf '%s\n' "${health}"
}

port_is_in_use() {
  local port="$1"
  if [[ -n "${NIMBUS_PROVIDER_FIXTURE_PORT_CHECK_BIN:-}" ]]; then
    "${NIMBUS_PROVIDER_FIXTURE_PORT_CHECK_BIN}" 127.0.0.1 "${port}"
    return
  fi
  if command -v nc >/dev/null 2>&1; then
    nc -z 127.0.0.1 "${port}" >/dev/null 2>&1
    return
  fi
  (exec 3<>"/dev/tcp/127.0.0.1/${port}") >/dev/null 2>&1
}

ensure_ports_free() {
  local provider="$1"
  local port
  local override_name
  case "${provider}" in
    postgres) override_name=NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT ;;
    mysql) override_name=NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT ;;
    libsql) override_name=NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT ;;
  esac
  for port in $(provider_ports "${provider}"); do
    if port_is_in_use "${port}"; then
      fail "localhost port ${port} is already in use; choose ${override_name} (and the libSQL admin override when applicable) or stop the owning service"
    fi
  done
}

print_logs() {
  local provider="$1"
  echo "--- ${provider} fixture logs ---" >&2
  compose logs --no-color --tail 200 "${provider}" >&2 || true
}

wait_for_health() {
  local provider="$1"
  local expected_image="$2"
  local expected_hash="$3"
  local attempt
  local id
  local health
  for ((attempt = 1; attempt <= wait_attempts; attempt++)); do
    id="$(container_id "${provider}")"
    if [[ -n "${id}" ]]; then
      health="$(matching_container_metadata "${provider}" "${id}" "${expected_image}" "${expected_hash}")" \
        || return 1
      case "${health}" in
        healthy) return 0 ;;
        unhealthy)
          print_logs "${provider}"
          fail "${provider} fixture became unhealthy"
          return 1
          ;;
      esac
    fi
    sleep "${wait_interval}"
  done
  print_logs "${provider}"
  fail "timed out after ${wait_attempts} readiness attempts for ${provider} fixture"
}

libsql_readiness_probe() {
  local admin_port="${NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT:-18081}"
  local http_bin="${NIMBUS_PROVIDER_FIXTURE_HTTP_BIN:-curl}"
  local probe="nimbus_fixture_readiness"
  if [[ "${http_bin}" == */* ]]; then
    if [[ ! -x "${http_bin}" ]]; then
      fail "libSQL readiness HTTP client is not executable: ${http_bin}"
      return 1
    fi
  else
    if ! command -v "${http_bin}" >/dev/null 2>&1; then
      fail "libSQL readiness requires '${http_bin}' on PATH"
      return 1
    fi
  fi
  "${http_bin}" -fsS -X POST \
    "http://localhost:${admin_port}/v1/namespaces/${probe}/create" \
    -H 'content-type: application/json' \
    --data '{}' >/dev/null \
    || {
      fail "libSQL admin API did not create the readiness namespace"
      return 1
    }
  "${http_bin}" -fsS -X DELETE \
    "http://localhost:${admin_port}/v1/namespaces/${probe}" >/dev/null \
    || {
      fail "libSQL admin API did not delete the readiness namespace"
      return 1
    }
}

export_provider_environment() {
  local provider="$1"
  local ports
  ports="$(provider_ports "${provider}")"
  export NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1
  case "${provider}" in
    postgres)
      export NIMBUS_TEST_POSTGRES_URL="host=127.0.0.1 port=${ports} user=postgres password=fixture-postgres dbname=postgres"
      ;;
    mysql)
      export NIMBUS_MYSQL_URL="mysql://root:fixture-mysql-root@127.0.0.1:${ports}/test"
      ;;
    libsql)
      export NIMBUS_LIBSQL_URL="http://localhost:${ports%% *}"
      export NIMBUS_LIBSQL_ADMIN_URL="http://localhost:${ports##* }"
      ;;
  esac
}

remove_provider() {
  local provider="$1"
  local id
  local expected_image
  local expected_hash
  id="$(container_id "${provider}")"
  [[ -n "${id}" ]] || return 0
  expected_image="$(provider_image "${provider}")"
  expected_hash="$(provider_config_hash "${provider}")"
  matching_container_metadata \
    "${provider}" "${id}" "${expected_image}" "${expected_hash}" >/dev/null \
    || return 1
  compose rm --stop --force --volumes "${provider}" >/dev/null \
    || fail "failed to remove owned ${provider} fixture"
}

start_provider() {
  local provider="$1"
  local allow_reuse="$2"
  local expected_image
  local expected_hash
  local id
  local health
  validate_provider_ports "${provider}"
  expected_image="$(provider_image "${provider}")"
  expected_hash="$(provider_config_hash "${provider}")"
  id="$(container_id "${provider}")"
  if [[ -n "${id}" ]]; then
    health="$(matching_container_metadata \
      "${provider}" "${id}" "${expected_image}" "${expected_hash}")" \
      || return 1
    if [[ "${health}" == "healthy" ]]; then
      if [[ "${allow_reuse}" != "1" ]]; then
        fail "healthy owned ${provider} fixture already exists; set REUSE=1 or run provider-fixture-down explicitly"
        return 1
      fi
      echo "reusing healthy owned ${provider} fixture (${expected_image})"
      append_provider active_providers "${provider}"
      if [[ "${provider}" == "libsql" ]] && ! libsql_readiness_probe; then
        print_logs "${provider}"
        return 1
      fi
      export_provider_environment "${provider}"
      return 0
    fi
    echo "recreating owned ${provider} fixture with health=${health}" >&2
    print_logs "${provider}"
    remove_provider "${provider}"
  fi

  ensure_ports_free "${provider}"
  append_provider started_providers "${provider}"
  append_provider active_providers "${provider}"
  echo "starting ${provider} fixture (${expected_image})"
  if ! compose up --detach --no-deps "${provider}"; then
    print_logs "${provider}"
    fail "failed to start ${provider} fixture"
    return 1
  fi
  if ! wait_for_health "${provider}" "${expected_image}" "${expected_hash}"; then
    return 1
  fi
  if [[ "${provider}" == "libsql" ]] && ! libsql_readiness_probe; then
    print_logs "${provider}"
    return 1
  fi
  export_provider_environment "${provider}"
}

cleanup_started() {
  local status=0
  local provider
  for provider in ${started_providers}; do
    remove_provider "${provider}" || status=1
  done
  return "${status}"
}

cleanup_on_exit() {
  local status=$?
  local cleanup_status=0
  trap - EXIT INT TERM HUP
  if [[ "${cleanup_enabled}" == "1" && "${keep_fixtures}" != "1" ]]; then
    cleanup_started || cleanup_status=$?
  fi
  if [[ "${status}" == "0" && "${cleanup_status}" != "0" ]]; then
    status="${cleanup_status}"
  fi
  exit "${status}"
}

run_tests() {
  local selection="$1"
  local status
  export NIMBUS_PROVIDER_FILTER="${selection}"
  set +e
  "${test_runner}"
  status=$?
  set -e
  if [[ "${status}" != "0" ]]; then
    local provider
    for provider in ${active_providers}; do
      print_logs "${provider}"
    done
  fi
  return "${status}"
}

command_name="${1:-}"
selection="${2:-}"
if [[ -z "${command_name}" || -z "${selection}" ]]; then
  usage
  exit 2
fi
case "${command_name}" in
  run | up | down | logs) ;;
  *)
    usage
    fail "unknown command '${command_name}'"
    exit 2
    ;;
esac

validate_toggle NIMBUS_PROVIDER_FIXTURE_KEEP "${keep_fixtures}"
validate_toggle NIMBUS_PROVIDER_FIXTURE_REUSE "${reuse_fixtures}"
validate_positive_integer NIMBUS_PROVIDER_FIXTURE_WAIT_ATTEMPTS "${wait_attempts}"
provider_list "${selection}" >/dev/null
ensure_runtime

case "${command_name}" in
  run)
    cleanup_enabled=1
    trap cleanup_on_exit EXIT
    trap 'exit 130' INT TERM HUP
    for provider in $(provider_list "${selection}"); do
      start_provider "${provider}" "${reuse_fixtures}"
    done
    run_tests "${selection}"
    ;;
  up)
    for provider in $(provider_list "${selection}"); do
      start_provider "${provider}" 1
    done
    ;;
  down)
    for provider in $(provider_list "${selection}"); do
      remove_provider "${provider}"
    done
    ;;
  logs)
    for provider in $(provider_list "${selection}"); do
      print_logs "${provider}"
    done
    ;;
esac
