#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-conmon-drill-verify.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

bundle_dir="${tmp_dir}/bundle"
state_root="${tmp_dir}/state"
output_file="${tmp_dir}/helper-output.txt"

mkdir -p "${bundle_dir}"
printf '%s\n' '{}' > "${bundle_dir}/config.json"
mkdir -p "${state_root}"

bundle_dir="$(cd "${bundle_dir}" && pwd)"
state_root="$(cd "${state_root}" && pwd)"

bash "${repo_root}/scripts/prepare-conmon-krun-drill.sh" \
  --bundle-dir "${bundle_dir}" \
  --state-root "${state_root}" \
  --container-id nimbus-http \
  --name nimbus-http \
  --conmon /usr/bin/conmon \
  --runtime /usr/libexec/nimbus/crun \
  > "${output_file}"

container_state_dir="${state_root}/containers/nimbus-http"
command_file="${container_state_dir}/run-conmon.sh"
find_attach_sockets_script="${container_state_dir}/find-attach-sockets.sh"
capture_process_tree_script="${container_state_dir}/capture-process-tree.sh"
wait_for_exit_script="${container_state_dir}/wait-for-exit.sh"
show_exit_status_script="${container_state_dir}/show-exit-status.sh"
graceful_stop_script="${container_state_dir}/graceful-stop.sh"
force_stop_script="${container_state_dir}/force-stop.sh"
metadata_file="${container_state_dir}/drill.env"
ctr_log="${container_state_dir}/ctr.log"
oci_log="${container_state_dir}/oci.log"
pidfile="${container_state_dir}/pidfile"
conmon_pidfile="${container_state_dir}/conmon.pid"
exit_dir="${state_root}/exits"
exit_status_file="${exit_dir}/nimbus-http"
persist_dir="${state_root}/persist/nimbus-http"

for generated_file in \
  "${command_file}" \
  "${find_attach_sockets_script}" \
  "${capture_process_tree_script}" \
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
  "${find_attach_sockets_script}" \
  "${capture_process_tree_script}" \
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
grep -F "drill.ctr_log=${ctr_log}" "${output_file}" >/dev/null
grep -F "drill.oci_log=${oci_log}" "${output_file}" >/dev/null
grep -F "drill.pidfile=${pidfile}" "${output_file}" >/dev/null
grep -F "drill.conmon_pidfile=${conmon_pidfile}" "${output_file}" >/dev/null
grep -F "drill.exit_status_file=${exit_status_file}" "${output_file}" >/dev/null
grep -F "drill.persist_dir=${persist_dir}" "${output_file}" >/dev/null
grep -F "drill.attach_socket_search_cmd=bash ${find_attach_sockets_script}" "${output_file}" >/dev/null
grep -F "drill.process_tree_cmd=bash ${capture_process_tree_script}" "${output_file}" >/dev/null
grep -F "drill.graceful_stop_cmd=bash ${graceful_stop_script}" "${output_file}" >/dev/null

grep -F "exec /usr/bin/conmon --api-version 1 -c nimbus-http -u nimbus-http -r /usr/libexec/nimbus/crun -b ${bundle_dir} -p ${pidfile} -n nimbus-http --exit-dir ${exit_dir} --persist-dir ${persist_dir} --full-attach -l k8s-file:${ctr_log} --log-level debug --syslog --conmon-pidfile ${conmon_pidfile} --runtime-arg --log-format=json --runtime-arg --log --runtime-arg ${oci_log}" "${command_file}" >/dev/null
grep -F "find ${persist_dir} -type s -print | sort" "${find_attach_sockets_script}" >/dev/null
grep -F "conmon_pid=\"\$(cat ${conmon_pidfile})\"" "${capture_process_tree_script}" >/dev/null
grep -F "runtime_pid=\"\$(cat ${pidfile})\"" "${capture_process_tree_script}" >/dev/null
grep -F "cat ${exit_status_file}" "${show_exit_status_script}" >/dev/null
grep -F "kill -TERM \"\${runtime_pid}\"" "${graceful_stop_script}" >/dev/null
grep -F "kill -KILL \"\${runtime_pid}\"" "${force_stop_script}" >/dev/null
grep -F "EXIT_STATUS_FILE=${exit_status_file}" "${metadata_file}" >/dev/null

echo "verified: conmon krun drill helper generated ${command_file}"
