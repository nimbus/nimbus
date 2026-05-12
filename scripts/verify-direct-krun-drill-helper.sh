#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-direct-krun-drill-verify.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

bundle_dir="${tmp_dir}/bundle"
state_root="${tmp_dir}/state"
output_file="${tmp_dir}/helper-output.txt"

mkdir -p "${bundle_dir}" "${state_root}"

bundle_dir="$(cd "${bundle_dir}" && pwd)"
state_root="$(cd "${state_root}" && pwd)"

cat_fixture_path="${bundle_dir}/config.json"
python3 - "${cat_fixture_path}" <<'PY'
import json
import sys
from pathlib import Path

config = {
    "annotations": {
        "run.oci.handler": "krun",
        "krun.port_map": "18080:8080",
    }
}

path = Path(sys.argv[1])
with path.open("w", encoding="utf-8") as fh:
    json.dump(config, fh, indent=2)
    fh.write("\n")
PY

bash "${repo_root}/scripts/prepare-direct-krun-drill.sh" \
  --bundle-dir "${bundle_dir}" \
  --state-root "${state_root}" \
  --container-id nimbus-http \
  --runtime /usr/libexec/nimbus/crun \
  > "${output_file}"

container_state_dir="${state_root}/containers/nimbus-http"
command_file="${container_state_dir}/run-runtime.sh"
start_script="${container_state_dir}/start-runtime.sh"
probe_http_script="${container_state_dir}/probe-http.sh"
wait_for_http_script="${container_state_dir}/wait-for-http.sh"
wait_for_exit_script="${container_state_dir}/wait-for-exit.sh"
show_exit_status_script="${container_state_dir}/show-exit-status.sh"
graceful_stop_script="${container_state_dir}/graceful-stop.sh"
force_stop_script="${container_state_dir}/force-stop.sh"
metadata_file="${container_state_dir}/drill.env"
stdout_log="${container_state_dir}/runtime.stdout.log"
stderr_log="${container_state_dir}/runtime.stderr.log"
runtime_pidfile="${container_state_dir}/runtime.pid"
launcher_pidfile="${container_state_dir}/launcher.pid"
exit_status_file="${container_state_dir}/exit.status"
probe_url="http://127.0.0.1:18080/"

for generated_file in \
  "${command_file}" \
  "${start_script}" \
  "${probe_http_script}" \
  "${wait_for_http_script}" \
  "${wait_for_exit_script}" \
  "${show_exit_status_script}" \
  "${graceful_stop_script}" \
  "${force_stop_script}" \
  "${metadata_file}"
do
  if [[ ! -f "${generated_file}" ]]; then
    echo "expected generated file not found: ${generated_file}" >&2
    exit 70
  fi
done

for generated_script in \
  "${command_file}" \
  "${start_script}" \
  "${probe_http_script}" \
  "${wait_for_http_script}" \
  "${wait_for_exit_script}" \
  "${show_exit_status_script}" \
  "${graceful_stop_script}" \
  "${force_stop_script}"
do
  if [[ ! -x "${generated_script}" ]]; then
    echo "expected generated script to be executable: ${generated_script}" >&2
    exit 70
  fi

  bash -n "${generated_script}"
done

grep -F "drill.bundle_config=${bundle_dir}/config.json" "${output_file}" >/dev/null
grep -F "drill.command_file=${command_file}" "${output_file}" >/dev/null
grep -F "drill.start_script=${start_script}" "${output_file}" >/dev/null
grep -F "drill.stdout_log=${stdout_log}" "${output_file}" >/dev/null
grep -F "drill.stderr_log=${stderr_log}" "${output_file}" >/dev/null
grep -F "drill.runtime_pidfile=${runtime_pidfile}" "${output_file}" >/dev/null
grep -F "drill.launcher_pidfile=${launcher_pidfile}" "${output_file}" >/dev/null
grep -F "drill.exit_status_file=${exit_status_file}" "${output_file}" >/dev/null
grep -F "drill.host_port=18080" "${output_file}" >/dev/null
grep -F "drill.probe_url=${probe_url}" "${output_file}" >/dev/null
grep -F "drill.probe_http_cmd=bash ${probe_http_script}" "${output_file}" >/dev/null
grep -F "drill.wait_for_http_cmd=bash ${wait_for_http_script}" "${output_file}" >/dev/null

grep -F "/usr/libexec/nimbus/crun run --bundle ${bundle_dir} nimbus-http" "${command_file}" >/dev/null
grep -F "printf '%s\\n' \"\${runtime_pid}\" > ${runtime_pidfile}" "${command_file}" >/dev/null
grep -F "printf '%s\\n' \"\${status}\" > ${exit_status_file}" "${command_file}" >/dev/null
grep -F "bash ${command_file} &" "${start_script}" >/dev/null
grep -F "exec curl -fsS ${probe_url}" "${probe_http_script}" >/dev/null
grep -F "curl -fsS ${probe_url}" "${wait_for_http_script}" >/dev/null
grep -F "cat ${exit_status_file}" "${show_exit_status_script}" >/dev/null
grep -F "kill -TERM \"\${runtime_pid}\"" "${graceful_stop_script}" >/dev/null
grep -F "kill -KILL \"\${runtime_pid}\"" "${force_stop_script}" >/dev/null
grep -F "PROBE_URL=${probe_url}" "${metadata_file}" >/dev/null

echo "verified: direct krun drill helper generated ${command_file}"
