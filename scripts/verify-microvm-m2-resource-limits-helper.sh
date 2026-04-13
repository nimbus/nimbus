#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

required_env=(
  NEOVEX_KRUN_SMOKE_ROOTFS
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

log_root="${NEOVEX_KRUN_SMOKE_WORKDIR%/}/m2-resource-limit-verification"
mkdir -p "${log_root}"

direct_log="${log_root}/direct-rootfs.log"
image_log="${log_root}/image-backed.log"
summary_file="${log_root}/summary.txt"

cd "${repo_root}"

cargo fmt --all --check
cargo check -p neovex-sandbox -p neovex
cargo test -p neovex-sandbox

cargo test \
  -p neovex-sandbox \
  --test krun_linux_smoke \
  krun_backend_m2_direct_rootfs_resource_limits_lowering \
  -- \
  --ignored \
  --exact \
  --nocapture \
  --test-threads=1 \
  2>&1 | tee "${direct_log}"

cargo test \
  -p neovex-sandbox \
  --test krun_linux_smoke \
  krun_backend_m2_image_backed_resource_limits_lowering \
  -- \
  --ignored \
  --exact \
  --nocapture \
  --test-threads=1 \
  2>&1 | tee "${image_log}"

{
  printf 'm2.resource_limits.direct_log=%s\n' "${direct_log}"
  printf 'm2.resource_limits.image_log=%s\n' "${image_log}"
  printf 'm2.resource_limits.expected_memory_limit_bytes=268435456\n'
  printf 'm2.resource_limits.expected_vm_config={"cpus":2,"ram_mib":256}\n'
} > "${summary_file}"

printf 'wrote resource-limit verification logs to %s\n' "${log_root}"
printf 'summary file: %s\n' "${summary_file}"
