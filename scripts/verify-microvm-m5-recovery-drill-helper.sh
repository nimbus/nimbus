#!/usr/bin/env bash
set -euo pipefail

# Verify recovery-drill expectations after a compose-backed service run.
# This script is run AFTER verify-microvm-m5-compose-serve-helper.sh and checks
# the exact project root recorded by that helper.

if [[ -z "${NIMBUS_KRUN_SMOKE_WORKDIR:-}" ]]; then
  echo "missing required environment variable: NIMBUS_KRUN_SMOKE_WORKDIR" >&2
  exit 64
fi

host_port="${NIMBUS_KRUN_SMOKE_M5_HOST_PORT:-18091}"
compose_summary="${NIMBUS_KRUN_SMOKE_WORKDIR%/}/m5-compose-serve-verification/summary.txt"
log_root="${NIMBUS_KRUN_SMOKE_WORKDIR%/}/m5-recovery-drill"
mkdir -p "${log_root}"
summary_file="${log_root}/summary.txt"
failures=0

if [[ ! -f "${compose_summary}" ]]; then
  echo "missing compose-serve summary file: ${compose_summary}" >&2
  exit 64
fi

summary_value() {
  local key="$1"
  grep "^${key}=" "${compose_summary}" | tail -1 | cut -d= -f2-
}

project_root="$(summary_value 'm5.compose_serve.project_root')"
project_key="$(summary_value 'm5.compose_serve.project_key')"

if [[ -z "${project_root}" || -z "${project_key}" ]]; then
  echo "compose-serve summary did not include project identity" >&2
  exit 1
fi

manifest_field() {
  local manifest_path="$1"
  local field_path="$2"
  python3 - "${manifest_path}" "${field_path}" <<'PY'
import json
import sys

manifest_path, field_path = sys.argv[1], sys.argv[2]
value = json.load(open(manifest_path))
for part in field_path.split("."):
    value = value[part]
if value is None:
    print("")
else:
    print(value)
PY
}

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

printf 'recovery_drill.compose_summary=%s\n' "${compose_summary}" >> "${summary_file}"
printf 'recovery_drill.project_root=%s\n' "${project_root}" >> "${summary_file}"
printf 'recovery_drill.project_key=%s\n' "${project_key}" >> "${summary_file}"

# 1. No leaked ports
if ss -tlnp 2>/dev/null | grep -q ":${host_port} "; then
  check "port.${host_port}.released" "FAIL (still listening)"
else
  check "port.${host_port}.released" "ok"
fi

# 2. Exact project root exists
if [[ -d "${project_root}" ]]; then
  check "project.root.exists" "ok"
else
  check "project.root.exists" "FAIL (${project_root} missing)"
fi

# 3. Manifest persists for this exact project root
mapfile -t manifests < <(find "${project_root}" -name 'manifest.json' -print 2>/dev/null | sort)
manifest_count="${#manifests[@]}"
if [[ "${manifest_count}" -eq 0 ]]; then
  check "manifest.persists" "FAIL (no manifests found under ${project_root})"
else
  check "manifest.persists" "ok (${manifest_count} found)"
fi

total_orphans=0
ctr_logs_present=0
oci_logs_present=0

for manifest_path in "${manifests[@]}"; do
  sandbox_id="$(manifest_field "${manifest_path}" 'handle.id')"
  status="$(manifest_field "${manifest_path}" 'status')"
  shutdown="$(manifest_field "${manifest_path}" 'shutdown_requested')"
  exit_code="$(manifest_field "${manifest_path}" 'last_exit_code')"
  ctr_log="$(manifest_field "${manifest_path}" 'conmon_layout.ctr_log')"
  oci_log="$(manifest_field "${manifest_path}" 'conmon_layout.oci_log')"

  check "manifest.${sandbox_id}.status" "${status}"
  check "manifest.${sandbox_id}.shutdown_requested" "${shutdown}"
  check "manifest.${sandbox_id}.last_exit_code" "${exit_code}"

  if [[ "${status}" != "stopped" ]]; then
    check "manifest.${sandbox_id}.status.expected" "FAIL (expected stopped)"
  fi
  if [[ "${shutdown}" != "True" && "${shutdown}" != "true" ]]; then
    check "manifest.${sandbox_id}.shutdown_requested.expected" "FAIL (expected true)"
  fi
  if [[ "${exit_code}" != "137" ]]; then
    check "manifest.${sandbox_id}.last_exit_code.expected" "FAIL (expected 137)"
  fi

  if [[ -f "${ctr_log}" ]]; then
    ctr_logs_present=$((ctr_logs_present + 1))
  fi
  if [[ -f "${oci_log}" ]]; then
    oci_logs_present=$((oci_logs_present + 1))
  fi

  sandbox_orphans="$(ps -ax -o pid=,command= 2>/dev/null | { grep -F "${sandbox_id}" || true; } | { grep -E 'conmon|crun' || true; } | wc -l)"
  total_orphans=$((total_orphans + sandbox_orphans))
done

if [[ "${ctr_logs_present}" -eq "${manifest_count}" ]]; then
  check "logs.ctr.persists" "ok (${ctr_logs_present}/${manifest_count})"
else
  check "logs.ctr.persists" "FAIL (${ctr_logs_present}/${manifest_count})"
fi

if [[ "${oci_logs_present}" -eq "${manifest_count}" ]]; then
  check "logs.oci.persists" "ok (${oci_logs_present}/${manifest_count})"
else
  check "logs.oci.persists" "FAIL (${oci_logs_present}/${manifest_count})"
fi

if [[ "${total_orphans}" -gt 0 ]]; then
  check "orphan.processes" "FAIL (${total_orphans} found)"
else
  check "orphan.processes" "ok (none)"
fi

# 4. Project-scoped layout
if [[ "${project_root}" == */services/projects/"${project_key}" ]]; then
  check "project.layout" "ok (key=${project_key})"
else
  check "project.layout" "FAIL (root=${project_root})"
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
