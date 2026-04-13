#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

required_env=(
  NEOVEX_KRUN_SMOKE_WORKDIR
  NEOVEX_KRUN_SMOKE_RUNTIME
  NEOVEX_KRUN_SMOKE_CONMON
  NEOVEX_KRUN_SMOKE_BUILDAH
)

for env_key in "${required_env[@]}"; do
  if [[ -z "${!env_key:-}" ]]; then
    echo "missing required environment variable: ${env_key}" >&2
    exit 64
  fi
done

host_port="${NEOVEX_KRUN_SMOKE_M5_HOST_PORT:-18091}"
guest_port="${NEOVEX_KRUN_SMOKE_M5_GUEST_PORT:-8091}"
export NEOVEX_KRUN_SMOKE_M5_HOST_PORT="${host_port}"
export NEOVEX_KRUN_SMOKE_M5_GUEST_PORT="${guest_port}"

control_root="${NEOVEX_KRUN_SMOKE_WORKDIR%/}/m5-compose-control"
log_root="${NEOVEX_KRUN_SMOKE_WORKDIR%/}/m5-compose-serve-verification"
rm -rf "${control_root}" "${log_root}"
mkdir -p "${log_root}"

smoke_log="${log_root}/compose-serve.log"
summary_file="${log_root}/summary.txt"

cd "${repo_root}"

cargo fmt --all --check
cargo check -p neovex-sandbox -p neovex-server -p neovex-bin -p neovex
cargo test -p neovex-bin

cargo test \
  -p neovex-bin \
  tests::convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down \
  -- \
  --ignored \
  --exact \
  --nocapture \
  --test-threads=1 \
  2>&1 | tee "${smoke_log}"

project_root="$(grep '^M5_PROJECT_ROOT=' "${smoke_log}" | tail -1 | cut -d= -f2-)"
project_key="$(grep '^M5_PROJECT_KEY=' "${smoke_log}" | tail -1 | cut -d= -f2-)"

if [[ -z "${project_root}" || -z "${project_key}" ]]; then
  echo "failed to extract M5 project identity from ${smoke_log}" >&2
  exit 1
fi

{
  printf 'm5.compose_serve.log=%s\n' "${smoke_log}"
  printf 'm5.compose_serve.host_port=%s\n' "${host_port}"
  printf 'm5.compose_serve.guest_port=%s\n' "${guest_port}"
  printf 'm5.compose_serve.control_root=%s\n' "${control_root}"
  printf 'm5.compose_serve.project_root=%s\n' "${project_root}"
  printf 'm5.compose_serve.project_key=%s\n' "${project_key}"
  printf 'm5.compose_serve.exact_test=tests::convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down\n'
} > "${summary_file}"

printf 'wrote compose-serve verification log to %s\n' "${smoke_log}"
printf 'summary file: %s\n' "${summary_file}"
