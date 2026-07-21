#!/usr/bin/env bash

set -euo pipefail

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "set ${name} to run external provider integration tests" >&2
    exit 1
  fi
}

export NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES="${NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES:-1}"
nextest_bin="${NIMBUS_CARGO_NEXTEST_BIN:-cargo-nextest}"

run_archive_filter() {
  local filter="$1"
  "${nextest_bin}" nextest run \
    --archive-file "${NIMBUS_EXTERNAL_PROVIDER_ARCHIVE_FILE}" \
    --workspace-remap "${GITHUB_WORKSPACE:-${PWD}}" \
    --profile ci-pr \
    --no-tests fail \
    -E "${filter}"
}

run_local_filter() {
  local filter="$1"
  "${nextest_bin}" nextest run \
    --profile ci-pr \
    --no-tests fail \
    -E "${filter}"
}

run_provider_filter() {
  local provider_filter="$1"
  local effective_filter="${provider_filter}"
  if [[ -n "${NIMBUS_EXTERNAL_PROVIDER_TEST_FILTER:-}" ]]; then
    effective_filter="(${provider_filter}) and (${NIMBUS_EXTERNAL_PROVIDER_TEST_FILTER})"
  fi
  if [[ -n "${NIMBUS_EXTERNAL_PROVIDER_ARCHIVE_FILE:-}" ]]; then
    run_archive_filter "${effective_filter}"
    return
  fi
  run_local_filter "${effective_filter}"
}

run_postgres() {
  require_env NIMBUS_TEST_POSTGRES_URL
  run_provider_filter '((package(nimbus-storage) or package(nimbus-engine)) and (test(/^(tests::)?postgres_/) or test(/^(tests::)?postgres_provider::/))) or (package(nimbus-system) and test(projection_provider_restart_reconciles_cancelled_scope))'
}

run_mysql() {
  require_env NIMBUS_MYSQL_URL
  run_provider_filter '((package(nimbus-storage) or package(nimbus-engine)) and (test(/^(tests::)?mysql_/) or test(/^(tests::)?mysql_provider::/))) or (package(nimbus-system) and test(projection_mysql_two_engine_takeover_rejects_late_old_document_schema_and_delete))'
}

run_libsql() {
  require_env NIMBUS_LIBSQL_URL
  require_env NIMBUS_LIBSQL_ADMIN_URL
  run_provider_filter '((package(nimbus-storage) or package(nimbus-engine)) and (test(/^(tests::)?libsql_/) or test(/^(tests::)?libsql_replica_provider::/))) or (package(nimbus-system) and test(projection_libsql_two_engine_takeover_rejects_late_old_document_schema_and_delete))'
}

case "${NIMBUS_PROVIDER_FILTER:-all}" in
  postgres)
    run_postgres
    ;;
  mysql)
    run_mysql
    ;;
  libsql)
    run_libsql
    ;;
  all)
    run_postgres
    run_mysql
    run_libsql
    ;;
  *)
    echo "unknown NIMBUS_PROVIDER_FILTER=${NIMBUS_PROVIDER_FILTER} (expected postgres|mysql|libsql|all)" >&2
    exit 1
    ;;
esac
