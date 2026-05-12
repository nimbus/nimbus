#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/collect-encryption-benchmark-evidence.sh --output-dir <path>

Runs the reproducible encryption-at-rest benchmark capture flow:
  1. embedded providers in plaintext mode
  2. embedded providers with manifest-backed local encryption
  3. libsql replica provider local-cache reopen and refresh drills with
     encrypted local cache, when
     NIMBUS_LIBSQL_URL and NIMBUS_LIBSQL_ADMIN_URL are set

Outputs:
  - system-info.log
  - embedded-plaintext-report.md
  - embedded-encrypted-report.md
  - libsql-replica-encrypted-cache-report.md (when libsql env is configured)
  - per-command *.log files
  - summary.txt
EOF
}

output_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir)
      shift
      if [[ $# -eq 0 ]]; then
        echo "expected a path after --output-dir" >&2
        exit 1
      fi
      output_dir="$1"
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

if [[ -z "$output_dir" ]]; then
  echo "set --output-dir to the destination directory" >&2
  exit 1
fi

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

system_info_log="$output_dir/system-info.log"
summary_path="$output_dir/summary.txt"
embedded_plain_report="$output_dir/embedded-plaintext-report.md"
embedded_plain_log="$output_dir/embedded-plaintext.log"
embedded_encrypted_report="$output_dir/embedded-encrypted-report.md"
embedded_encrypted_log="$output_dir/embedded-encrypted.log"
libsql_report="$output_dir/libsql-replica-encrypted-cache-report.md"
libsql_log="$output_dir/libsql-replica-encrypted-cache.log"
libsql_workloads="point-read indexed-query composite-indexed-query barrier-refresh peer-catch-up"

collect_system_info() {
  {
    echo "generated_at_utc=$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    echo "pwd=$(pwd)"
    echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unavailable)"
    echo "git_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unavailable)"
    echo "uname=$(uname -a)"
    if command -v sw_vers >/dev/null 2>&1; then
      echo
      echo "[sw_vers]"
      sw_vers
    fi
    if command -v sysctl >/dev/null 2>&1; then
      echo
      echo "[sysctl]"
      sysctl -n machdep.cpu.brand_string 2>/dev/null || true
      sysctl -n hw.ncpu 2>/dev/null || true
      sysctl -n hw.memsize 2>/dev/null || true
    fi
    if command -v lscpu >/dev/null 2>&1; then
      echo
      echo "[lscpu]"
      lscpu
    fi
    if command -v free >/dev/null 2>&1; then
      echo
      echo "[free -h]"
      free -h
    fi
    if command -v rustc >/dev/null 2>&1; then
      echo
      echo "[rustc -Vv]"
      rustc -Vv
    fi
    if command -v cargo >/dev/null 2>&1; then
      echo
      echo "[cargo -V]"
      cargo -V
    fi
    echo
    echo "[benchmark env overrides]"
    env | sort | grep -E '^NIMBUS_(BENCH|LIBSQL_REPLICA_BENCH)_' || true
  } >"$system_info_log"
}

run_logged() {
  local label="$1"
  local log_path="$2"
  shift 2
  echo "running $label"
  "$@" >"$log_path" 2>&1
}

collect_system_info

run_logged \
  "embedded plaintext benchmark" \
  "$embedded_plain_log" \
  make bench-embedded-providers REPORT="$embedded_plain_report"

run_logged \
  "embedded encrypted benchmark" \
  "$embedded_encrypted_log" \
  make bench-embedded-providers REPORT="$embedded_encrypted_report" ENCRYPTION=temp-master-key-file

libsql_status="skipped: set NIMBUS_LIBSQL_URL and NIMBUS_LIBSQL_ADMIN_URL to capture encrypted libsql replica evidence"
if [[ -n "${NIMBUS_LIBSQL_URL:-}" && -n "${NIMBUS_LIBSQL_ADMIN_URL:-}" ]]; then
  run_logged \
    "libsql replica benchmark with encrypted local cache" \
    "$libsql_log" \
    make bench-libsql-replica-provider REPORT="$libsql_report" ENCRYPTION=temp-master-key-file WORKLOADS="$libsql_workloads"
  libsql_status="captured: $(basename "$libsql_report")"
fi

{
  echo "Encryption benchmark evidence"
  echo
  echo "output_dir=$output_dir"
  echo "system_info=$(basename "$system_info_log")"
  echo "embedded_plaintext_report=$(basename "$embedded_plain_report")"
  echo "embedded_plaintext_log=$(basename "$embedded_plain_log")"
  echo "embedded_encrypted_report=$(basename "$embedded_encrypted_report")"
  echo "embedded_encrypted_log=$(basename "$embedded_encrypted_log")"
  echo "libsql_replica_encrypted_cache=$libsql_status"
  if [[ -f "$libsql_log" ]]; then
    echo "libsql_replica_encrypted_cache_log=$(basename "$libsql_log")"
  fi
  echo
  echo "embedded_plaintext_command=make bench-embedded-providers REPORT=$embedded_plain_report"
  echo "embedded_encrypted_command=make bench-embedded-providers REPORT=$embedded_encrypted_report ENCRYPTION=temp-master-key-file"
  echo "libsql_replica_encrypted_cache_command=make bench-libsql-replica-provider REPORT=$libsql_report ENCRYPTION=temp-master-key-file WORKLOADS=\"$libsql_workloads\""
} >"$summary_path"

echo "wrote encryption benchmark evidence into $output_dir"
