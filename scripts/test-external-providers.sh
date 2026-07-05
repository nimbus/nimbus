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

run_archive_filter() {
  local filter="$1"
  cargo-nextest nextest run \
    --archive-file "${NIMBUS_EXTERNAL_PROVIDER_ARCHIVE_FILE}" \
    --workspace-remap "${GITHUB_WORKSPACE:-${PWD}}" \
    --profile ci-pr \
    -E "${filter}"
}

run_postgres() {
  require_env NIMBUS_TEST_POSTGRES_URL
  if [[ -n "${NIMBUS_EXTERNAL_PROVIDER_ARCHIVE_FILE:-}" ]]; then
    run_archive_filter '(package(nimbus-storage) or package(nimbus-engine)) and (test(/^(tests::)?postgres_/) or test(/^(tests::)?postgres_provider::/))'
    return
  fi
  cargo test -p nimbus-storage postgres_provider -- --nocapture
  cargo test -p nimbus-engine postgres_provider -- --nocapture
}

run_mysql() {
  require_env NIMBUS_MYSQL_URL
  if [[ -n "${NIMBUS_EXTERNAL_PROVIDER_ARCHIVE_FILE:-}" ]]; then
    run_archive_filter '(package(nimbus-storage) or package(nimbus-engine)) and (test(/^(tests::)?mysql_/) or test(/^(tests::)?mysql_provider::/))'
    return
  fi
  cargo test -p nimbus-storage mysql_provider -- --nocapture
  cargo test -p nimbus-engine mysql_provider -- --nocapture
}

run_libsql() {
  require_env NIMBUS_LIBSQL_URL
  require_env NIMBUS_LIBSQL_ADMIN_URL
  if [[ -n "${NIMBUS_EXTERNAL_PROVIDER_ARCHIVE_FILE:-}" ]]; then
    run_archive_filter '(package(nimbus-storage) or package(nimbus-engine)) and (test(/^(tests::)?libsql_/) or test(/^(tests::)?libsql_replica_provider::/))'
    return
  fi
  cargo test -p nimbus-storage libsql_provider -- --nocapture
  cargo test -p nimbus-engine libsql_replica_provider -- --nocapture
}

case "${NIMBUS_PROVIDER_FILTER:-}" in
  postgres)
    run_postgres
    ;;
  mysql)
    run_mysql
    ;;
  libsql)
    run_libsql
    ;;
  "")
    run_postgres
    run_mysql
    run_libsql
    ;;
  *)
    echo "unknown NIMBUS_PROVIDER_FILTER=${NIMBUS_PROVIDER_FILTER} (expected postgres|mysql|libsql)" >&2
    exit 1
    ;;
esac
