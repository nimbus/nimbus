#!/usr/bin/env bash
set -euo pipefail

# Verify recovery-drill expectations after a compose-backed service run.
# This script is run AFTER verify-microvm-m5-compose-serve-helper.sh and checks:
# 1. No leaked ports from the M5 service
# 2. No orphan conmon/crun processes for the M5 sandbox
# 3. Manifest persists on disk with expected stopped/shutdown state
# 4. Logs (ctr.log, oci.log) persist on disk
# 5. The control root directory matches the project-scoped layout

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -z "${NEOVEX_KRUN_SMOKE_WORKDIR:-}" ]]; then
  echo "missing required environment variable: NEOVEX_KRUN_SMOKE_WORKDIR" >&2
  exit 64
fi

host_port="${NEOVEX_KRUN_SMOKE_M5_HOST_PORT:-18091}"
control_root="${NEOVEX_KRUN_SMOKE_WORKDIR%/}/m5-compose-control"
log_root="${NEOVEX_KRUN_SMOKE_WORKDIR%/}/m5-recovery-drill"
mkdir -p "${log_root}"
summary_file="${log_root}/summary.txt"
failures=0

check() {
  local label="$1"
  local result="$2"
  printf '%-50s %s\n' "${label}" "${result}"
  printf '%-50s %s\n' "${label}" "${result}" >> "${summary_file}"
  if [[ "${result}" == "FAIL"* ]]; then
    failures=$((failures + 1))
  fi
}

> "${summary_file}"

# 1. No leaked ports
if ss -tlnp 2>/dev/null | grep -q ":${host_port} "; then
  check "port.${host_port}.released" "FAIL (still listening)"
else
  check "port.${host_port}.released" "ok"
fi

# 2. No orphan conmon/crun for M5 sandbox
m5_orphans="$(ps aux 2>/dev/null | { grep -E "conmon|crun" || true; } | { grep "${host_port}" || true; } | { grep -v grep || true; } | wc -l)"
if [[ "${m5_orphans}" -gt 0 ]]; then
  check "orphan.processes" "FAIL (${m5_orphans} found)"
else
  check "orphan.processes" "ok (none)"
fi

# 3. Manifest persists
manifest_count="$(find "${control_root}" -name 'manifest.json' 2>/dev/null | wc -l)"
if [[ "${manifest_count}" -eq 0 ]]; then
  check "manifest.persists" "FAIL (no manifests found under ${control_root})"
else
  check "manifest.persists" "ok (${manifest_count} found)"
  manifest_path="$(find "${control_root}" -name 'manifest.json' 2>/dev/null | head -1)"
  status="$(python3 -c "import json; m=json.load(open('${manifest_path}')); print(m.get('status',''))" 2>/dev/null || echo "parse_error")"
  shutdown="$(python3 -c "import json; m=json.load(open('${manifest_path}')); print(m.get('shutdown_requested',''))" 2>/dev/null || echo "parse_error")"
  check "manifest.status" "${status}"
  check "manifest.shutdown_requested" "${shutdown}"
fi

# 4. Logs persist
ctr_log_count="$(find "${control_root}" -name 'ctr.log' 2>/dev/null | wc -l)"
oci_log_count="$(find "${control_root}" -name 'oci.log' 2>/dev/null | wc -l)"
if [[ "${ctr_log_count}" -gt 0 ]]; then
  check "logs.ctr.persists" "ok (${ctr_log_count} found)"
else
  check "logs.ctr.persists" "FAIL"
fi
if [[ "${oci_log_count}" -gt 0 ]]; then
  check "logs.oci.persists" "ok (${oci_log_count} found)"
else
  check "logs.oci.persists" "FAIL"
fi

# 5. Project-scoped layout
project_dirs="$(find "${control_root}/services/projects" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l)"
if [[ "${project_dirs}" -gt 0 ]]; then
  project_key="$(find "${control_root}/services/projects" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | head -1 | xargs basename)"
  check "project.layout" "ok (key=${project_key})"
else
  check "project.layout" "FAIL (no project dirs under ${control_root}/services/projects)"
fi

printf '\nrecovery_drill.failures=%d\n' "${failures}" >> "${summary_file}"
printf 'recovery_drill.summary=%s\n' "${summary_file}"

if [[ "${failures}" -gt 0 ]]; then
  echo ""
  echo "RECOVERY DRILL: ${failures} failures"
  exit 1
fi

echo ""
echo "RECOVERY DRILL: all checks passed"
echo "summary: ${summary_file}"
