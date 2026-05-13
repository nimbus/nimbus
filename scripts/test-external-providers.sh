#!/usr/bin/env bash

set -euo pipefail

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "set ${name} to run external provider integration tests" >&2
    exit 1
  fi
}

require_env NIMBUS_TEST_POSTGRES_URL
require_env NIMBUS_MYSQL_URL
require_env NIMBUS_LIBSQL_URL
require_env NIMBUS_LIBSQL_ADMIN_URL

export NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES="${NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES:-1}"

cargo test -p nimbus-storage postgres_provider -- --nocapture
cargo test -p nimbus-storage mysql_provider -- --nocapture
cargo test -p nimbus-storage libsql_provider -- --nocapture
cargo test -p nimbus-engine postgres_provider -- --nocapture
cargo test -p nimbus-engine mysql_provider -- --nocapture
cargo test -p nimbus-engine libsql_replica_provider -- --nocapture
