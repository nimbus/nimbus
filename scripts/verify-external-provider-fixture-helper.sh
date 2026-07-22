#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_script="${repo_root}/scripts/external-provider-fixture.sh"
test_script="${repo_root}/scripts/test-external-providers.sh"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-provider-fixture-test.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

fake_state="${tmp_dir}/state"
fake_log="${tmp_dir}/docker.log"
http_log="${tmp_dir}/http.log"
runner_log="${tmp_dir}/runner.log"
mkdir -p "${fake_state}"

fail() {
  echo "provider fixture helper test failed: $*" >&2
  exit 1
}

assert_contains() {
  local file="$1"
  local expected="$2"
  grep -F -- "${expected}" "${file}" >/dev/null \
    || fail "${file} did not contain: ${expected}"
}

assert_not_contains() {
  local file="$1"
  local unexpected="$2"
  if grep -F -- "${unexpected}" "${file}" >/dev/null 2>&1; then
    fail "${file} unexpectedly contained: ${unexpected}"
  fi
}

reset_fake() {
  rm -rf "${fake_state}"
  mkdir -p "${fake_state}"
  : >"${fake_log}"
  : >"${http_log}"
  : >"${runner_log}"
}

fake_runtime="${tmp_dir}/docker"
fake_port_check="${tmp_dir}/port-check"
fake_http="${tmp_dir}/http"
fake_runner="${tmp_dir}/runner"
fake_nextest="${tmp_dir}/cargo-nextest"

cat >"${fake_runtime}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_DOCKER_STATE:?}"
: "${FAKE_DOCKER_LOG:?}"
printf 'docker' >>"${FAKE_DOCKER_LOG}"
printf ' %q' "$@" >>"${FAKE_DOCKER_LOG}"
printf '\n' >>"${FAKE_DOCKER_LOG}"

provider_image() {
  case "$1" in
    postgres) printf '%s\n' postgres:16 ;;
    mysql) printf '%s\n' mysql:8.4 ;;
    libsql) printf '%s\n' ghcr.io/tursodatabase/libsql-server:v0.24.33 ;;
  esac
}

provider_hash() {
  printf '%s-hash\n' "$1"
}

if [[ "${1:-}" == "info" ]]; then
  exit "${FAKE_DOCKER_INFO_EXIT:-0}"
fi

if [[ "${1:-}" == "inspect" ]]; then
  id="${@: -1}"
  provider="${id#fake-}"
  owner="$(cat "${FAKE_DOCKER_STATE}/${provider}.owner" 2>/dev/null || printf '%s' external-provider-tests)"
  actual_provider="$(cat "${FAKE_DOCKER_STATE}/${provider}.provider" 2>/dev/null || printf '%s' "${provider}")"
  hash="$(cat "${FAKE_DOCKER_STATE}/${provider}.hash" 2>/dev/null || provider_hash "${provider}")"
  image="$(cat "${FAKE_DOCKER_STATE}/${provider}.image" 2>/dev/null || provider_image "${provider}")"
  health="$(cat "${FAKE_DOCKER_STATE}/${provider}.health" 2>/dev/null || printf '%s' healthy)"
  printf '%s|%s|%s|%s|%s\n' "${owner}" "${actual_provider}" "${hash}" "${image}" "${health}"
  exit 0
fi

[[ "${1:-}" == "compose" ]] || exit 97
shift
if [[ "${1:-}" == "version" ]]; then
  exit 0
fi
while [[ "${1:-}" == "--project-name" || "${1:-}" == "--file" ]]; do
  shift 2
done
action="${1:-}"
shift || true
case "${action}" in
  config)
    if [[ "${1:-}" == "--quiet" ]]; then
      exit 0
    fi
    mode="${1:-}"
    provider="${2:-}"
    case "${mode}" in
      --images) provider_image "${provider}" ;;
      --hash) printf '%s %s\n' "${provider}" "$(provider_hash "${provider}")" ;;
      *) exit 96 ;;
    esac
    ;;
  ps)
    provider="${@: -1}"
    if [[ -f "${FAKE_DOCKER_STATE}/${provider}.exists" ]]; then
      printf 'fake-%s\n' "${provider}"
    fi
    ;;
  up)
    provider="${@: -1}"
    printf 'up:%s:pg=%s:mysql=%s:libsql=%s:admin=%s\n' \
      "${provider}" \
      "${NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT:-5432}" \
      "${NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT:-3306}" \
      "${NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT:-18080}" \
      "${NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT:-18081}" >>"${FAKE_DOCKER_LOG}"
    touch "${FAKE_DOCKER_STATE}/${provider}.exists"
    provider_image "${provider}" >"${FAKE_DOCKER_STATE}/${provider}.image"
    provider_hash "${provider}" >"${FAKE_DOCKER_STATE}/${provider}.hash"
    printf '%s\n' external-provider-tests >"${FAKE_DOCKER_STATE}/${provider}.owner"
    printf '%s\n' "${provider}" >"${FAKE_DOCKER_STATE}/${provider}.provider"
    printf '%s\n' "${FAKE_DOCKER_HEALTH:-healthy}" >"${FAKE_DOCKER_STATE}/${provider}.health"
    if [[ "${FAKE_DOCKER_UP_EXIT:-0}" != "0" ]]; then
      exit "${FAKE_DOCKER_UP_EXIT}"
    fi
    ;;
  logs)
    provider="${@: -1}"
    printf 'fixture-log:%s\n' "${provider}"
    ;;
  rm)
    provider="${@: -1}"
    printf 'rm:%s\n' "${provider}" >>"${FAKE_DOCKER_LOG}"
    rm -f "${FAKE_DOCKER_STATE}/${provider}."*
    ;;
  *)
    exit 95
    ;;
esac
EOF

cat >"${fake_port_check}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
port="${2:?}"
case " ${FAKE_PORTS_IN_USE:-} " in
  *" ${port} "*) exit 0 ;;
  *) exit 1 ;;
esac
EOF

cat >"${fake_http}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_HTTP_LOG:?}"
printf 'http' >>"${FAKE_HTTP_LOG}"
printf ' %q' "$@" >>"${FAKE_HTTP_LOG}"
printf '\n' >>"${FAKE_HTTP_LOG}"
exit "${FAKE_HTTP_EXIT:-0}"
EOF

cat >"${fake_runner}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_RUNNER_LOG:?}"
printf 'provider=%s require=%s pg=%s mysql=%s libsql=%s admin=%s\n' \
  "${NIMBUS_PROVIDER_FILTER:-}" \
  "${NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES:-}" \
  "${NIMBUS_TEST_POSTGRES_URL:-}" \
  "${NIMBUS_MYSQL_URL:-}" \
  "${NIMBUS_LIBSQL_URL:-}" \
  "${NIMBUS_LIBSQL_ADMIN_URL:-}" >>"${FAKE_RUNNER_LOG}"
if [[ "${FAKE_TEST_INTERRUPT:-0}" == "1" ]]; then
  kill -TERM "${PPID}"
  exit 0
fi
exit "${FAKE_TEST_EXIT:-0}"
EOF

cat >"${fake_nextest}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_RUNNER_LOG:?}"
printf 'nextest' >>"${FAKE_RUNNER_LOG}"
printf ' %q' "$@" >>"${FAKE_RUNNER_LOG}"
printf '\n' >>"${FAKE_RUNNER_LOG}"
exit "${FAKE_NEXTEST_EXIT:-0}"
EOF

chmod +x "${fake_runtime}" "${fake_port_check}" "${fake_http}" "${fake_runner}" "${fake_nextest}"

fixture_env=(
  "NIMBUS_CONTAINER_RUNTIME=${fake_runtime}"
  "NIMBUS_PROVIDER_FIXTURE_PORT_CHECK_BIN=${fake_port_check}"
  "NIMBUS_PROVIDER_FIXTURE_HTTP_BIN=${fake_http}"
  "NIMBUS_PROVIDER_TEST_RUNNER=${fake_runner}"
  "NIMBUS_PROVIDER_FIXTURE_COMPOSE_FILE=${repo_root}/compose.test-external-providers.yaml"
  "NIMBUS_PROVIDER_FIXTURE_WAIT_ATTEMPTS=2"
  "NIMBUS_PROVIDER_FIXTURE_WAIT_INTERVAL_SECONDS=0"
  "FAKE_DOCKER_STATE=${fake_state}"
  "FAKE_DOCKER_LOG=${fake_log}"
  "FAKE_HTTP_LOG=${http_log}"
  "FAKE_RUNNER_LOG=${runner_log}"
)

run_fixture() {
  local output="$1"
  local expected_status="$2"
  shift 2
  local status
  set +e
  env "${fixture_env[@]}" "$@" >"${output}" 2>&1
  status=$?
  set -e
  [[ "${status}" == "${expected_status}" ]] \
    || fail "expected exit ${expected_status}, got ${status}; output: $(cat "${output}")"
}

assert_contains "${repo_root}/compose.test-external-providers.yaml" "image: postgres:16"
assert_contains "${repo_root}/compose.test-external-providers.yaml" "image: mysql:8.4"
assert_contains "${repo_root}/compose.test-external-providers.yaml" "--innodb-redo-log-capacity=536870912"
assert_contains "${repo_root}/compose.test-external-providers.yaml" "image: ghcr.io/tursodatabase/libsql-server:v0.24.33"
assert_contains "${repo_root}/compose.test-external-providers.yaml" "127.0.0.1:\${NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT:-3306}:3306"
assert_not_contains "${repo_root}/Cargo.toml" "testcontainers-modules"
assert_not_contains "${repo_root}/crates/nimbus-storage/Cargo.toml" "testcontainers-modules"
assert_not_contains "${repo_root}/crates/nimbus-engine/Cargo.toml" "testcontainers-modules"
if grep -R "testcontainers\|ContainerAsync\|GenericImage" \
  "${repo_root}/crates/nimbus-storage/src/tests/postgres_provider" \
  "${repo_root}/crates/nimbus-storage/src/tests/mysql_provider" \
  "${repo_root}/crates/nimbus-storage/src/tests/libsql_provider" \
  "${repo_root}/crates/nimbus-engine/src/tests/postgres_provider" \
  "${repo_root}/crates/nimbus-engine/src/tests/mysql_provider.rs" \
  "${repo_root}/crates/nimbus-engine/src/tests/libsql_replica_provider.rs" >/dev/null 2>&1; then
  fail "Rust provider helpers must not provision implicit per-test containers"
fi

for provider in postgres mysql libsql; do
  reset_fake
  output="${tmp_dir}/selection-${provider}.out"
  run_fixture "${output}" 0 \
    NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT=25432 \
    NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
    NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT=28080 \
    NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT=28081 \
    "${fixture_script}" run "${provider}"
  assert_contains "${fake_log}" "config --images ${provider}"
  assert_contains "${fake_log}" "up:${provider}:pg=25432:mysql=23306:libsql=28080:admin=28081"
  assert_contains "${runner_log}" "provider=${provider} require=1"
  assert_contains "${fake_log}" "rm:${provider}"
  case "${provider}" in
    postgres) assert_contains "${runner_log}" "port=25432 user=postgres" ;;
    mysql) assert_contains "${runner_log}" "@127.0.0.1:23306/test" ;;
    libsql) assert_contains "${runner_log}" "libsql=http://localhost:28080 admin=http://localhost:28081" ;;
  esac
done

reset_fake
timeout_output="${tmp_dir}/timeout.out"
run_fixture "${timeout_output}" 1 \
  FAKE_DOCKER_HEALTH=starting \
  NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT=25432 \
  "${fixture_script}" run postgres
assert_contains "${timeout_output}" "timed out after 2 readiness attempts"
assert_contains "${timeout_output}" "fixture-log:postgres"
assert_contains "${fake_log}" "rm:postgres"

reset_fake
failure_output="${tmp_dir}/test-failure.out"
run_fixture "${failure_output}" 42 \
  FAKE_TEST_EXIT=42 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
assert_contains "${failure_output}" "fixture-log:mysql"
assert_contains "${fake_log}" "rm:mysql"

reset_fake
startup_output="${tmp_dir}/startup-failure.out"
run_fixture "${startup_output}" 1 \
  FAKE_DOCKER_UP_EXIT=7 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
assert_contains "${startup_output}" "failed to start mysql fixture"
assert_contains "${startup_output}" "fixture-log:mysql"
assert_contains "${fake_log}" "rm:mysql"

reset_fake
interrupt_output="${tmp_dir}/interrupt.out"
run_fixture "${interrupt_output}" 130 \
  FAKE_TEST_INTERRUPT=1 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
assert_contains "${fake_log}" "rm:mysql"

reset_fake
keep_output="${tmp_dir}/keep.out"
run_fixture "${keep_output}" 0 \
  NIMBUS_PROVIDER_FIXTURE_KEEP=1 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
[[ -f "${fake_state}/mysql.exists" ]] || fail "KEEP=1 did not retain the fixture"
assert_not_contains "${fake_log}" "rm:mysql"

: >"${fake_log}"
reuse_output="${tmp_dir}/reuse.out"
run_fixture "${reuse_output}" 0 \
  NIMBUS_PROVIDER_FIXTURE_REUSE=1 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
assert_contains "${reuse_output}" "reusing healthy owned mysql fixture"
assert_not_contains "${fake_log}" "up:mysql"
assert_not_contains "${fake_log}" "rm:mysql"
run_fixture "${tmp_dir}/down.out" 0 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" down mysql
assert_contains "${fake_log}" "rm:mysql"

reset_fake
touch "${fake_state}/libsql.exists"
libsql_reuse_output="${tmp_dir}/libsql-reuse.out"
run_fixture "${libsql_reuse_output}" 1 \
  FAKE_HTTP_EXIT=22 \
  NIMBUS_PROVIDER_FIXTURE_REUSE=1 \
  NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT=28080 \
  NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT=28081 \
  "${fixture_script}" run libsql
assert_contains "${libsql_reuse_output}" "libSQL admin API did not create the readiness namespace"
assert_contains "${libsql_reuse_output}" "fixture-log:libsql"
assert_contains "${http_log}" "http -fsS -X POST http://localhost:28081/v1/namespaces/nimbus_fixture_readiness/create"
assert_not_contains "${runner_log}" "provider=libsql"
assert_not_contains "${fake_log}" "rm:libsql"

reset_fake
touch "${fake_state}/mysql.exists"
printf '%s\n' foreign-owner >"${fake_state}/mysql.owner"
foreign_output="${tmp_dir}/foreign.out"
run_fixture "${foreign_output}" 1 \
  NIMBUS_PROVIDER_FIXTURE_REUSE=1 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
assert_contains "${foreign_output}" "refusing foreign or mismatched"
assert_not_contains "${fake_log}" "rm:mysql"

reset_fake
touch "${fake_state}/mysql.exists"
printf '%s\n' external-provider-tests >"${fake_state}/mysql.owner"
printf '%s\n' mysql >"${fake_state}/mysql.provider"
printf '%s\n' mysql:8.0 >"${fake_state}/mysql.image"
printf '%s\n' mysql-hash >"${fake_state}/mysql.hash"
mismatch_output="${tmp_dir}/mismatch.out"
run_fixture "${mismatch_output}" 1 \
  NIMBUS_PROVIDER_FIXTURE_REUSE=1 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" down mysql
assert_contains "${mismatch_output}" "refusing foreign or mismatched"
assert_not_contains "${fake_log}" "rm:mysql"

reset_fake
collision_output="${tmp_dir}/collision.out"
run_fixture "${collision_output}" 1 \
  FAKE_PORTS_IN_USE=23306 \
  NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
  "${fixture_script}" run mysql
assert_contains "${collision_output}" "localhost port 23306 is already in use"
assert_not_contains "${fake_log}" "up:mysql"

reset_fake
unknown_output="${tmp_dir}/unknown.out"
run_fixture "${unknown_output}" 1 "${fixture_script}" run oracle
assert_contains "${unknown_output}" "unknown provider 'oracle'"

missing_runtime_output="${tmp_dir}/missing-runtime.out"
set +e
NIMBUS_CONTAINER_RUNTIME="${tmp_dir}/missing-docker" \
  "${fixture_script}" run mysql >"${missing_runtime_output}" 2>&1
missing_runtime_status=$?
set -e
[[ "${missing_runtime_status}" == "1" ]] || fail "missing runtime returned ${missing_runtime_status}"
assert_contains "${missing_runtime_output}" "container runtime is not executable"

missing_url_output="${tmp_dir}/missing-url.out"
set +e
env -u NIMBUS_MYSQL_URL \
  NIMBUS_PROVIDER_FILTER=mysql \
  NIMBUS_CARGO_NEXTEST_BIN="${fake_nextest}" \
  FAKE_RUNNER_LOG="${runner_log}" \
  "${test_script}" >"${missing_url_output}" 2>&1
missing_url_status=$?
set -e
[[ "${missing_url_status}" == "1" ]] || fail "missing URL returned ${missing_url_status}"
assert_contains "${missing_url_output}" "set NIMBUS_MYSQL_URL"

: >"${runner_log}"
zero_test_output="${tmp_dir}/zero-test.out"
set +e
NIMBUS_PROVIDER_FILTER=mysql \
  NIMBUS_MYSQL_URL=mysql://fixture.invalid/test \
  NIMBUS_CARGO_NEXTEST_BIN="${fake_nextest}" \
  FAKE_NEXTEST_EXIT=4 \
  FAKE_RUNNER_LOG="${runner_log}" \
  "${test_script}" >"${zero_test_output}" 2>&1
zero_test_status=$?
set -e
[[ "${zero_test_status}" == "4" ]] || fail "nextest zero-test exit returned ${zero_test_status}"
assert_contains "${runner_log}" "--no-tests fail"
assert_contains "${runner_log}" "package\\(nimbus-system\\)"

: >"${runner_log}"
focused_filter_output="${tmp_dir}/focused-filter.out"
run_fixture "${focused_filter_output}" 0 \
  NIMBUS_PROVIDER_FILTER=mysql \
  NIMBUS_MYSQL_URL=mysql://fixture.invalid/test \
  'NIMBUS_EXTERNAL_PROVIDER_TEST_FILTER=test(mysql_committer_lease_concurrent_acquire_has_exactly_one_winner)' \
  NIMBUS_CARGO_NEXTEST_BIN="${fake_nextest}" \
  FAKE_RUNNER_LOG="${runner_log}" \
  "${test_script}"
assert_contains "${runner_log}" "mysql_committer_lease_concurrent_acquire_has_exactly_one_winner"
assert_contains "${runner_log}" "and"

echo "external provider fixture helper tests: PASS (selection/readiness/exit/logs/cleanup/signal/keep/reuse/ownership/runtime/ports/URLs/zero-tests/focused-filter)"
