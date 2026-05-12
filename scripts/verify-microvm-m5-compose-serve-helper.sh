#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

required_env=(
  NIMBUS_KRUN_SMOKE_WORKDIR
  NIMBUS_KRUN_SMOKE_RUNTIME
  NIMBUS_KRUN_SMOKE_CONMON
  NIMBUS_KRUN_SMOKE_BUILDAH
)

for env_key in "${required_env[@]}"; do
  if [[ -z "${!env_key:-}" ]]; then
    echo "missing required environment variable: ${env_key}" >&2
    exit 64
  fi
done

host_port="${NIMBUS_KRUN_SMOKE_M5_HOST_PORT:-18091}"
guest_port="${NIMBUS_KRUN_SMOKE_M5_GUEST_PORT:-8091}"
export NIMBUS_KRUN_SMOKE_M5_HOST_PORT="${host_port}"
export NIMBUS_KRUN_SMOKE_M5_GUEST_PORT="${guest_port}"

control_root="${NIMBUS_KRUN_SMOKE_WORKDIR%/}/m5-compose-control"
log_root="${NIMBUS_KRUN_SMOKE_WORKDIR%/}/m5-compose-serve-verification"
rm -rf "${control_root}" "${log_root}"
mkdir -p "${log_root}"

smoke_log="${log_root}/compose-serve.log"
summary_file="${log_root}/summary.txt"
metadata_file="${log_root}/metadata.json"

cd "${repo_root}"

cargo fmt --all --check
cargo check -p nimbus-sandbox -p nimbus-server -p nimbus-bin -p nimbus
cargo test -p nimbus-bin

export NIMBUS_KRUN_SMOKE_M5_METADATA_FILE="${metadata_file}"
cargo test \
  -p nimbus-bin \
  tests::convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down \
  -- \
  --ignored \
  --exact \
  --nocapture \
  --test-threads=1 \
  2>&1 | tee "${smoke_log}"

if [[ ! -f "${metadata_file}" ]]; then
  echo "compose smoke did not write metadata file: ${metadata_file}" >&2
  exit 1
fi

project_root="$(python3 -c "import json; print(json.load(open('${metadata_file}'))['project_root'])")"
project_key="$(python3 -c "import json; print(json.load(open('${metadata_file}'))['project_key'])")"

if [[ -z "${project_root}" || -z "${project_key}" ]]; then
  echo "failed to read M5 project identity from ${metadata_file}" >&2
  exit 1
fi

{
  printf 'm5.compose_serve.log=%s\n' "${smoke_log}"
  printf 'm5.compose_serve.metadata=%s\n' "${metadata_file}"
  printf 'm5.compose_serve.host_port=%s\n' "${host_port}"
  printf 'm5.compose_serve.guest_port=%s\n' "${guest_port}"
  printf 'm5.compose_serve.control_root=%s\n' "${control_root}"
  printf 'm5.compose_serve.project_root=%s\n' "${project_root}"
  printf 'm5.compose_serve.project_key=%s\n' "${project_key}"
  printf 'm5.compose_serve.exact_test=tests::convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down\n'
} > "${summary_file}"

printf 'wrote compose-serve verification log to %s\n' "${smoke_log}"
printf 'summary file: %s\n' "${summary_file}"
