#!/usr/bin/env bash
set -euo pipefail
set -o errtrace

PROOF_OUTPUT_DIR=""
PROOF_MODE=""
FAILED_COMMAND=""
TEMP_WORKDIR=""
STARTED_AT_UTC="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

usage() {
  cat <<'EOF' >&2
usage:
  bash scripts/verify-sqlcipher-proof.sh [--output-dir <dir>] cargo-lanes
  bash scripts/verify-sqlcipher-proof.sh [--output-dir <dir>] packaged-binary <path-to-neovex>
EOF
}

record_failed_command() {
  FAILED_COMMAND="${BASH_COMMAND}"
}

finalize_proof_bundle() {
  local exit_code=$?
  local status="passed"
  local finished_at_utc

  set +e
  if [[ ${exit_code} -ne 0 ]]; then
    status="failed"
  fi

  if [[ -n "${PROOF_OUTPUT_DIR}" ]]; then
    mkdir -p "${PROOF_OUTPUT_DIR}"
    finished_at_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    cat > "${PROOF_OUTPUT_DIR}/summary.txt" <<EOF
mode=${PROOF_MODE:-unknown}
status=${status}
started_at_utc=${STARTED_AT_UTC}
finished_at_utc=${finished_at_utc}
cwd=$(pwd)
runner_os=${RUNNER_OS:-unknown}
runner_arch=${RUNNER_ARCH:-unknown}
github_job=${GITHUB_JOB:-local}
github_run_id=${GITHUB_RUN_ID:-local}
github_run_attempt=${GITHUB_RUN_ATTEMPT:-local}
EOF
    if [[ -n "${FAILED_COMMAND}" ]]; then
      printf 'failed_command=%s\n' "${FAILED_COMMAND}" >> "${PROOF_OUTPUT_DIR}/summary.txt"
    fi
  fi

  if [[ -n "${TEMP_WORKDIR}" && -d "${TEMP_WORKDIR}" ]]; then
    rm -rf -- "${TEMP_WORKDIR}"
  fi
}

run_logged() {
  local log_name="$1"
  shift

  if [[ -z "${PROOF_OUTPUT_DIR}" ]]; then
    "$@"
    return
  fi

  local log_path="${PROOF_OUTPUT_DIR}/${log_name}.log"
  {
    printf '$'
    printf ' %q' "$@"
    printf '\n'
  } | tee "${log_path}" >/dev/null
  "$@" 2>&1 | tee -a "${log_path}"
}

capture_script_context() {
  if [[ -z "${PROOF_OUTPUT_DIR}" ]]; then
    return
  fi

  mkdir -p "${PROOF_OUTPUT_DIR}"
  {
    echo "script=scripts/verify-sqlcipher-proof.sh"
    echo "mode=${PROOF_MODE}"
    echo "cwd=$(pwd)"
    echo "runner_os=${RUNNER_OS:-unknown}"
    echo "runner_arch=${RUNNER_ARCH:-unknown}"
    echo "github_job=${GITHUB_JOB:-local}"
    echo "github_run_id=${GITHUB_RUN_ID:-local}"
    echo "github_run_attempt=${GITHUB_RUN_ATTEMPT:-local}"
  } > "${PROOF_OUTPUT_DIR}/context.txt"

  run_logged system-info bash -lc '
    uname -a
    if command -v cargo >/dev/null 2>&1; then
      cargo --version
    else
      echo "cargo: unavailable"
    fi
    if command -v rustc >/dev/null 2>&1; then
      rustc -Vv
    else
      echo "rustc: unavailable"
    fi
    if command -v python3 >/dev/null 2>&1; then
      python3 --version
    else
      echo "python3: unavailable"
    fi
  '
}

require_python() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required for SQLCipher package-consumer proof" >&2
    exit 127
  fi
}

create_plaintext_sqlite_fixture() {
  local path="$1"
  python3 - "$path" <<'PY'
import sqlite3
import sys

path = sys.argv[1]
conn = sqlite3.connect(path)
conn.execute("CREATE TABLE tasks (id INTEGER PRIMARY KEY, title TEXT NOT NULL)")
conn.execute("INSERT INTO tasks (title) VALUES (?)", ("sqlcipher package proof",))
conn.commit()
conn.close()
PY
}

write_master_key_file() {
  local path="$1"
  python3 - "$path" <<'PY'
from pathlib import Path
import sys

Path(sys.argv[1]).write_bytes(bytes(range(32)))
PY
}

verify_plaintext_roundtrip() {
  local path="$1"
  python3 - "$path" <<'PY'
import sqlite3
import sys

path = sys.argv[1]
conn = sqlite3.connect(path)
title = conn.execute("SELECT title FROM tasks").fetchone()
conn.close()

if title is None or title[0] != "sqlcipher package proof":
    raise SystemExit(f"unexpected roundtrip payload: {title!r}")
PY
}

write_sha256_digest() {
  local path="$1"
  local output_path="$2"
  python3 - "$path" > "${output_path}" <<'PY'
from hashlib import sha256
from pathlib import Path
import sys

path = Path(sys.argv[1])
print(f"{sha256(path.read_bytes()).hexdigest()}  {path}")
PY
}

run_cargo_lanes() {
  run_logged sqlite-encryption-tests \
    cargo test -p neovex-storage sqlite::encryption -- --nocapture
  run_logged sqlite-foundation-encryption-tests \
    cargo test -p neovex-storage sqlite_foundation::encryption -- --nocapture
}

run_packaged_binary_proof() {
  local binary="${1:-}"
  if [[ -z "${binary}" ]]; then
    usage
    exit 64
  fi
  if [[ ! -x "${binary}" ]]; then
    echo "expected packaged Neovex binary to be executable: ${binary}" >&2
    exit 64
  fi

  require_python

  if [[ -n "${PROOF_OUTPUT_DIR}" ]]; then
    printf '%s\n' "${binary}" > "${PROOF_OUTPUT_DIR}/binary-path.txt"
    write_sha256_digest "${binary}" "${PROOF_OUTPUT_DIR}/binary-sha256.txt"
    run_logged binary-version "${binary}" --version
  fi

  TEMP_WORKDIR="$(mktemp -d)"

  local plaintext_db="${TEMP_WORKDIR}/tenant.sqlite3"
  local encrypted_db="${TEMP_WORKDIR}/tenant.encrypted.sqlite3"
  local roundtrip_db="${TEMP_WORKDIR}/tenant.roundtrip.sqlite3"
  local master_key="${TEMP_WORKDIR}/master.key"

  create_plaintext_sqlite_fixture "${plaintext_db}"
  write_master_key_file "${master_key}"

  export NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file
  export NEOVEX_ENCRYPTION_MASTER_KEY_FILE="${master_key}"

  run_logged migrate-proof \
    "${binary}" encryption migrate \
      --source "${plaintext_db}" \
      --target "${encrypted_db}" \
      --provider sqlite \
      --tenant-id package-proof

  test -f "${encrypted_db}"
  test -f "${encrypted_db}.neovex-enc"

  run_logged export-proof \
    "${binary}" encryption export \
      --source "${encrypted_db}" \
      --target "${roundtrip_db}" \
      --provider sqlite \
      --tenant-id package-proof

  verify_plaintext_roundtrip "${roundtrip_db}"

  if [[ -n "${PROOF_OUTPUT_DIR}" ]]; then
    ls -l "${TEMP_WORKDIR}" > "${PROOF_OUTPUT_DIR}/generated-files.txt"
  fi
}

trap record_failed_command ERR
trap finalize_proof_bundle EXIT

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir)
      if [[ $# -lt 2 ]]; then
        usage
        exit 64
      fi
      PROOF_OUTPUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      break
      ;;
  esac
done

PROOF_MODE="${1:-}"
capture_script_context

case "${PROOF_MODE}" in
  cargo-lanes)
    run_cargo_lanes
    ;;
  packaged-binary)
    shift
    run_packaged_binary_proof "${1:-}"
    ;;
  *)
    usage
    exit 64
    ;;
esac
