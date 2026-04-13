#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: prepare-linux-vmm-validation-bundle.sh --crun-source <path> [options]

Prepare a deterministic Linux-host execution bundle for the `LH1`-`LH6`
validation queue from the archived VMM foundation plan. The bundle does not
execute the VMM stack by itself; it stages numbered command scripts, artifact
directories, and a write-back checklist so another Linux host can run the
queue with minimal judgment.

options:
  --crun-source <path>         Upstream crun source checkout (required)
  --output-root <path>         Output root for the generated bundle
                               (default: ${TMPDIR:-/tmp}/neovex-linux-vmm-validation)
  --stage-dir <path>           Directory for the staged private runtime
                               (default: <output-root>/stage)
  --stage-binary <path>        Staged runtime binary path
                               (default: <stage-dir>/crun)
  --install-path <path>        Private runtime install path
                               (default: /usr/libexec/neovex/crun)
  --system-runtime <path>      System runtime path or command
                               (default: resolved `crun` or literal `crun`)
  --bundle-dir <path>          OCI bundle directory
                               (default: <output-root>/bundle)
  --image <ref>                Buildah image for the first guest probe
                               (default: docker.io/library/busybox:latest)
  --buildah-name <name>        Buildah working-container name
                               (default: neovex-vmm-busybox)
  --host-port <port>           Host port for the first probe
                               (default: 18080)
  --guest-port <port>          Guest port for the first probe
                               (default: 8080)
  --direct-state-root <path>   Direct-runtime drill state root
                               (default: <output-root>/direct-drill)
  --direct-container-id <id>   Direct-runtime container ID
                               (default: neovex-http)
  --conmon-state-root <path>   Conmon drill state root
                               (default: <output-root>/conmon-drill)
  --conmon <path>              conmon path or command
                               (default: resolved `conmon` or literal `conmon`)
  --conmon-name <name>         Human-readable name for the conmon drill
                               (default: same as --direct-container-id)
  --probe-host <host>          Probe host for curl commands
                               (default: 127.0.0.1)
  --probe-path <path>          Probe path for curl commands
                               (default: /)
  -h, --help                   Show this help

examples:
  bash scripts/prepare-linux-vmm-validation-bundle.sh \
    --crun-source ~/src/github.com/containers/crun

  bash scripts/prepare-linux-vmm-validation-bundle.sh \
    --crun-source ~/src/github.com/containers/crun \
    --output-root /tmp/neovex-linux-vmm-validation
EOF
}

resolve_dir_path() {
  local dir_path="$1"

  mkdir -p "${dir_path}"
  (
    cd "${dir_path}"
    pwd
  )
}

resolve_existing_dir() {
  local dir_path="$1"

  if [[ ! -d "${dir_path}" ]]; then
    echo "directory not found: ${dir_path}" >&2
    exit 66
  fi

  (
    cd "${dir_path}"
    pwd
  )
}

resolve_file_path() {
  local file_path="$1"
  local parent_dir=""
  local base_name=""

  parent_dir="$(dirname "${file_path}")"
  base_name="$(basename "${file_path}")"
  mkdir -p "${parent_dir}"

  if [[ "${file_path}" == /* ]]; then
    printf '%s\n' "${file_path}"
    return 0
  fi

  (
    cd "${parent_dir}"
    printf '%s/%s\n' "$(pwd)" "${base_name}"
  )
}

resolve_command_or_default() {
  local candidate="$1"

  if [[ "${candidate}" == */* ]]; then
    printf '%s\n' "${candidate}"
    return 0
  fi

  if command -v "${candidate}" >/dev/null 2>&1; then
    command -v "${candidate}"
    return 0
  fi

  printf '%s\n' "${candidate}"
}

crun_source=""
output_root="${TMPDIR:-/tmp}/neovex-linux-vmm-validation"
stage_dir=""
stage_binary=""
install_path="/usr/libexec/neovex/crun"
system_runtime="crun"
bundle_dir=""
image_ref="docker.io/library/busybox:latest"
buildah_name="neovex-vmm-busybox"
host_port="18080"
guest_port="8080"
direct_state_root=""
direct_container_id="neovex-http"
conmon_state_root=""
conmon_path="conmon"
conmon_name=""
probe_host="127.0.0.1"
probe_path="/"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --crun-source)
      crun_source="${2:-}"
      shift 2
      ;;
    --output-root)
      output_root="${2:-}"
      shift 2
      ;;
    --stage-dir)
      stage_dir="${2:-}"
      shift 2
      ;;
    --stage-binary)
      stage_binary="${2:-}"
      shift 2
      ;;
    --install-path)
      install_path="${2:-}"
      shift 2
      ;;
    --system-runtime)
      system_runtime="${2:-}"
      shift 2
      ;;
    --bundle-dir)
      bundle_dir="${2:-}"
      shift 2
      ;;
    --image)
      image_ref="${2:-}"
      shift 2
      ;;
    --buildah-name)
      buildah_name="${2:-}"
      shift 2
      ;;
    --host-port)
      host_port="${2:-}"
      shift 2
      ;;
    --guest-port)
      guest_port="${2:-}"
      shift 2
      ;;
    --direct-state-root)
      direct_state_root="${2:-}"
      shift 2
      ;;
    --direct-container-id)
      direct_container_id="${2:-}"
      shift 2
      ;;
    --conmon-state-root)
      conmon_state_root="${2:-}"
      shift 2
      ;;
    --conmon)
      conmon_path="${2:-}"
      shift 2
      ;;
    --conmon-name)
      conmon_name="${2:-}"
      shift 2
      ;;
    --probe-host)
      probe_host="${2:-}"
      shift 2
      ;;
    --probe-path)
      probe_path="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ -z "${crun_source}" ]]; then
  usage >&2
  exit 64
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crun_source="$(resolve_existing_dir "${crun_source}")"
output_root="$(resolve_dir_path "${output_root}")"

if [[ -z "${stage_dir}" ]]; then
  stage_dir="${output_root}/stage"
fi
stage_dir="$(resolve_dir_path "${stage_dir}")"

if [[ -z "${stage_binary}" ]]; then
  stage_binary="${stage_dir}/crun"
fi
stage_binary="$(resolve_file_path "${stage_binary}")"

if [[ -z "${bundle_dir}" ]]; then
  bundle_dir="${output_root}/bundle"
fi
bundle_dir="$(resolve_dir_path "${bundle_dir}")"

if [[ -z "${direct_state_root}" ]]; then
  direct_state_root="${output_root}/direct-drill"
fi
direct_state_root="$(resolve_dir_path "${direct_state_root}")"

if [[ -z "${conmon_state_root}" ]]; then
  conmon_state_root="${output_root}/conmon-drill"
fi
conmon_state_root="$(resolve_dir_path "${conmon_state_root}")"

if [[ -z "${conmon_name}" ]]; then
  conmon_name="${direct_container_id}"
fi

if [[ "${probe_path}" != /* ]]; then
  probe_path="/${probe_path}"
fi

system_runtime="$(resolve_command_or_default "${system_runtime}")"
conmon_path="$(resolve_command_or_default "${conmon_path}")"

commands_dir="${output_root}/commands"
artifacts_dir="${output_root}/artifacts"
commands_dir="$(resolve_dir_path "${commands_dir}")"
artifacts_dir="$(resolve_dir_path "${artifacts_dir}")"

lh1_dir="$(resolve_dir_path "${artifacts_dir}/lh1")"
lh2_dir="$(resolve_dir_path "${artifacts_dir}/lh2")"
lh3_dir="$(resolve_dir_path "${artifacts_dir}/lh3")"
lh4_dir="$(resolve_dir_path "${artifacts_dir}/lh4")"
lh5_dir="$(resolve_dir_path "${artifacts_dir}/lh5")"
lh6_dir="$(resolve_dir_path "${artifacts_dir}/lh6")"

session_env="${output_root}/session.env"
readme_file="${output_root}/README.md"
queue_runner="${commands_dir}/00-run-through-lh6.sh"
lh1_script="${commands_dir}/01-lh1-host-preflight.sh"
lh2_script="${commands_dir}/02-lh2-verify-crun-patch.sh"
lh3_stage_script="${commands_dir}/03-lh3-build-stage-runtime.sh"
lh3_install_script="${commands_dir}/04-lh3-install-private-runtime.sh"
lh4_script="${commands_dir}/05-lh4-verify-runtime-separation.sh"
lh5_rootfs_script="${commands_dir}/06-lh5-buildah-rootfs.sh"
lh5_bundle_script="${commands_dir}/07-lh5-prepare-krun-bundle.sh"
lh5_direct_prepare_script="${commands_dir}/08-lh5-prepare-direct-drill.sh"
lh5_direct_run_script="${commands_dir}/09-lh5-run-direct-drill.sh"
lh6_prepare_script="${commands_dir}/10-lh6-prepare-conmon-drill.sh"
lh6_run_script="${commands_dir}/11-lh6-run-conmon-drill.sh"
cleanup_script="${commands_dir}/12-cleanup-buildah-rootfs.sh"
checklist_file="${output_root}/99-writeback-checklist.txt"

rootfs_file="${lh5_dir}/rootfs-path.txt"
buildah_inspect_file="${lh5_dir}/buildah-inspect.txt"
direct_drill_env="${direct_state_root}/containers/${direct_container_id}/drill.env"
conmon_drill_env="${conmon_state_root}/containers/${direct_container_id}/drill.env"
probe_url="http://${probe_host}:${host_port}${probe_path}"

cat > "${session_env}" <<EOF
SESSION_ROOT=${output_root}
REPO_ROOT=${repo_root}
COMMANDS_DIR=${commands_dir}
ARTIFACTS_DIR=${artifacts_dir}
CRUN_SOURCE=${crun_source}
STAGE_DIR=${stage_dir}
STAGE_BINARY=${stage_binary}
INSTALL_PATH=${install_path}
SYSTEM_RUNTIME=${system_runtime}
BUNDLE_DIR=${bundle_dir}
IMAGE_REF=${image_ref}
BUILDAH_NAME=${buildah_name}
ROOTFS_FILE=${rootfs_file}
BUILDAH_INSPECT_FILE=${buildah_inspect_file}
HOST_PORT=${host_port}
GUEST_PORT=${guest_port}
PROBE_HOST=${probe_host}
PROBE_PATH=${probe_path}
PROBE_URL=${probe_url}
DIRECT_STATE_ROOT=${direct_state_root}
DIRECT_CONTAINER_ID=${direct_container_id}
DIRECT_DRILL_ENV=${direct_drill_env}
CONMON_STATE_ROOT=${conmon_state_root}
CONMON_PATH=${conmon_path}
CONMON_NAME=${conmon_name}
CONMON_DRILL_ENV=${conmon_drill_env}
LH1_DIR=${lh1_dir}
LH2_DIR=${lh2_dir}
LH3_DIR=${lh3_dir}
LH4_DIR=${lh4_dir}
LH5_DIR=${lh5_dir}
LH6_DIR=${lh6_dir}
EOF

cat > "${readme_file}" <<EOF
# Linux VMM Validation Bundle

This bundle was generated by \`scripts/prepare-linux-vmm-validation-bundle.sh\`.

Run order:

1. \`bash ${lh1_script}\`
2. \`bash ${lh2_script}\`
3. \`bash ${lh3_stage_script}\`
4. \`bash ${lh3_install_script}\`
5. \`bash ${lh4_script}\`
6. \`bash ${lh5_rootfs_script}\`
7. \`bash ${lh5_bundle_script}\`
8. \`bash ${lh5_direct_prepare_script}\`
9. \`bash ${lh5_direct_run_script}\`
10. \`bash ${lh6_prepare_script}\`
11. \`bash ${lh6_run_script}\`
12. After evidence is recorded, clean up with \`bash ${cleanup_script}\`

Or run the prepared sequence through:

\`\`\`bash
bash ${queue_runner}
\`\`\`

Important files:

- session env: \`${session_env}\`
- plan write-back checklist: \`${checklist_file}\`
- direct-runtime drill env: \`${direct_drill_env}\`
- conmon drill env: \`${conmon_drill_env}\`
EOF

cat > "${lh1_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh1.host_preflight.output=\${LH1_DIR}/check-vmm-host.txt"
bash "\${REPO_ROOT}/scripts/check-vmm-host.sh" | tee "\${LH1_DIR}/check-vmm-host.txt"

echo "lh1.package_versions.output=\${LH1_DIR}/collect-vmm-package-versions.txt"
bash "\${REPO_ROOT}/scripts/collect-vmm-package-versions.sh" | tee "\${LH1_DIR}/collect-vmm-package-versions.txt"
EOF
chmod 0755 "${lh1_script}"

cat > "${lh2_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh2.patch_verify.output=\${LH2_DIR}/verify-crun-patch.txt"
bash "\${REPO_ROOT}/scripts/verify-crun-patch.sh" "\${CRUN_SOURCE}" | tee "\${LH2_DIR}/verify-crun-patch.txt"
EOF
chmod 0755 "${lh2_script}"

cat > "${lh3_stage_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh3.build_stage.output=\${LH3_DIR}/build-stage-runtime.txt"
bash "\${REPO_ROOT}/scripts/build-neovex-crun.sh" \\
  --source "\${CRUN_SOURCE}" \\
  --output "\${STAGE_BINARY}" | tee "\${LH3_DIR}/build-stage-runtime.txt"

echo "lh3.stage_binary=\${STAGE_BINARY}" | tee -a "\${LH3_DIR}/build-stage-runtime.txt"
"\${STAGE_BINARY}" --version | tee "\${LH3_DIR}/stage-runtime-version.txt"
EOF
chmod 0755 "${lh3_stage_script}"

cat > "${lh3_install_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh3.install.output=\${LH3_DIR}/install-private-runtime.txt"
bash "\${REPO_ROOT}/scripts/build-neovex-crun.sh" \\
  --source "\${CRUN_SOURCE}" \\
  --output "\${STAGE_BINARY}" \\
  --install-path "\${INSTALL_PATH}" \\
  --sudo-install | tee "\${LH3_DIR}/install-private-runtime.txt"

"\${INSTALL_PATH}" --version | tee "\${LH3_DIR}/install-runtime-version.txt"
EOF
chmod 0755 "${lh3_install_script}"

cat > "${lh4_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh4.runtime_separation.output=\${LH4_DIR}/verify-runtime-separation.txt"
bash "\${REPO_ROOT}/scripts/verify-runtime-separation.sh" \\
  --system-runtime "\${SYSTEM_RUNTIME}" \\
  --private-runtime "\${INSTALL_PATH}" | tee "\${LH4_DIR}/verify-runtime-separation.txt"
EOF
chmod 0755 "${lh4_script}"

cat > "${lh5_rootfs_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

existing_name=""
if buildah containers --format '{{.ContainerName}}' 2>/dev/null | grep -Fx "\${BUILDAH_NAME}" >/dev/null 2>&1; then
  existing_name="\${BUILDAH_NAME}"
fi

if [[ -n "\${existing_name}" ]]; then
  buildah umount "\${existing_name}" >/dev/null 2>&1 || true
  buildah rm "\${existing_name}" >/dev/null 2>&1 || true
fi

echo "lh5.buildah_from.output=\${LH5_DIR}/buildah-from.txt"
buildah from --name "\${BUILDAH_NAME}" "\${IMAGE_REF}" | tee "\${LH5_DIR}/buildah-from.txt"

echo "lh5.rootfs_file=\${ROOTFS_FILE}" | tee "\${LH5_DIR}/buildah-mount.txt"
buildah mount "\${BUILDAH_NAME}" | tee "\${ROOTFS_FILE}" | tee -a "\${LH5_DIR}/buildah-mount.txt" >/dev/null

buildah inspect "\${BUILDAH_NAME}" | tee "\${BUILDAH_INSPECT_FILE}" >/dev/null
EOF
chmod 0755 "${lh5_rootfs_script}"

cat > "${lh5_bundle_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

rootfs_path="\$(cat "\${ROOTFS_FILE}")"
echo "lh5.prepare_bundle.output=\${LH5_DIR}/prepare-krun-bundle.txt"
bash "\${REPO_ROOT}/scripts/prepare-krun-bundle.sh" \\
  --bundle-dir "\${BUNDLE_DIR}" \\
  --rootfs "\${rootfs_path}" \\
  --host-port "\${HOST_PORT}" \\
  --guest-port "\${GUEST_PORT}" | tee "\${LH5_DIR}/prepare-krun-bundle.txt"
EOF
chmod 0755 "${lh5_bundle_script}"

cat > "${lh5_direct_prepare_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh5.prepare_direct.output=\${LH5_DIR}/prepare-direct-drill.txt"
bash "\${REPO_ROOT}/scripts/prepare-direct-krun-drill.sh" \\
  --bundle-dir "\${BUNDLE_DIR}" \\
  --state-root "\${DIRECT_STATE_ROOT}" \\
  --container-id "\${DIRECT_CONTAINER_ID}" \\
  --runtime "\${INSTALL_PATH}" | tee "\${LH5_DIR}/prepare-direct-drill.txt"

cp "\${DIRECT_DRILL_ENV}" "\${LH5_DIR}/direct-drill.env"
EOF
chmod 0755 "${lh5_direct_prepare_script}"

cat > "${lh5_direct_run_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}
source "\${DIRECT_DRILL_ENV}"

echo "lh5.direct_start.output=\${LH5_DIR}/direct-start.txt"
bash "\${START_SCRIPT}" | tee "\${LH5_DIR}/direct-start.txt"

echo "lh5.direct_wait_for_http.output=\${LH5_DIR}/direct-wait-for-http.txt"
bash "\${WAIT_FOR_HTTP}" 60 | tee "\${LH5_DIR}/direct-wait-for-http.txt"

echo "lh5.direct_probe.output=\${LH5_DIR}/direct-probe-http.txt"
bash "\${PROBE_HTTP}" | tee "\${LH5_DIR}/direct-probe-http.txt"

echo "lh5.direct_graceful_stop.output=\${LH5_DIR}/direct-graceful-stop.txt"
bash "\${GRACEFUL_STOP}" 60 | tee "\${LH5_DIR}/direct-graceful-stop.txt"

echo "lh5.direct_exit_status.output=\${LH5_DIR}/direct-exit-status.txt"
bash "\${SHOW_EXIT_STATUS}" | tee "\${LH5_DIR}/direct-exit-status.txt"
EOF
chmod 0755 "${lh5_direct_run_script}"

cat > "${lh6_prepare_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

echo "lh6.prepare_conmon.output=\${LH6_DIR}/prepare-conmon-drill.txt"
bash "\${REPO_ROOT}/scripts/prepare-conmon-krun-drill.sh" \\
  --bundle-dir "\${BUNDLE_DIR}" \\
  --state-root "\${CONMON_STATE_ROOT}" \\
  --container-id "\${DIRECT_CONTAINER_ID}" \\
  --name "\${CONMON_NAME}" \\
  --conmon "\${CONMON_PATH}" \\
  --runtime "\${INSTALL_PATH}" | tee "\${LH6_DIR}/prepare-conmon-drill.txt"

cp "\${CONMON_DRILL_ENV}" "\${LH6_DIR}/conmon-drill.env"
EOF
chmod 0755 "${lh6_prepare_script}"

cat > "${lh6_run_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}
source "\${CONMON_DRILL_ENV}"

run_stdout="\${LH6_DIR}/run-conmon.stdout.txt"
run_stderr="\${LH6_DIR}/run-conmon.stderr.txt"

echo "lh6.run_conmon.stdout=\${run_stdout}"
echo "lh6.run_conmon.stderr=\${run_stderr}"
bash "\${COMMAND_FILE}" >"\${run_stdout}" 2>"\${run_stderr}" &
run_wrapper_pid=\$!
printf '%s\n' "\${run_wrapper_pid}" | tee "\${LH6_DIR}/run-conmon-wrapper.pid" >/dev/null

deadline=\$((SECONDS + 60))
while (( SECONDS <= deadline )); do
  if curl -fsS "\${PROBE_URL}" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "lh6.attach_sockets.output=\${LH6_DIR}/attach-sockets.txt"
bash "\${FIND_ATTACH_SOCKETS}" | tee "\${LH6_DIR}/attach-sockets.txt"

echo "lh6.process_tree.output=\${LH6_DIR}/process-tree.txt"
bash "\${CAPTURE_PROCESS_TREE}" | tee "\${LH6_DIR}/process-tree.txt"

echo "lh6.probe.output=\${LH6_DIR}/conmon-probe-http.txt"
curl -fsS "\${PROBE_URL}" | tee "\${LH6_DIR}/conmon-probe-http.txt"

echo "lh6.graceful_stop.output=\${LH6_DIR}/conmon-graceful-stop.txt"
bash "\${GRACEFUL_STOP}" 60 | tee "\${LH6_DIR}/conmon-graceful-stop.txt"

echo "lh6.exit_status.output=\${LH6_DIR}/conmon-exit-status.txt"
bash "\${SHOW_EXIT_STATUS}" | tee "\${LH6_DIR}/conmon-exit-status.txt"

wait "\${run_wrapper_pid}"
EOF
chmod 0755 "${lh6_run_script}"

cat > "${cleanup_script}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source ${session_env}

buildah umount "\${BUILDAH_NAME}" >/dev/null 2>&1 || true
buildah rm "\${BUILDAH_NAME}" >/dev/null 2>&1 || true
echo "cleanup.buildah_name=\${BUILDAH_NAME}"
EOF
chmod 0755 "${cleanup_script}"

cat > "${queue_runner}" <<EOF
#!/usr/bin/env bash
set -euo pipefail

bash ${lh1_script}
bash ${lh2_script}
bash ${lh3_stage_script}
bash ${lh3_install_script}
bash ${lh4_script}
bash ${lh5_rootfs_script}
bash ${lh5_bundle_script}
bash ${lh5_direct_prepare_script}
bash ${lh5_direct_run_script}
bash ${lh6_prepare_script}
bash ${lh6_run_script}

echo "queue.complete=yes"
echo "cleanup.next=bash ${cleanup_script}"
EOF
chmod 0755 "${queue_runner}"

cat > "${checklist_file}" <<EOF
Record these paths and outcomes alongside the current task and compare against
docs/plans/archive/vmm-infrastructure-plan.md:

LH1:
- ${lh1_dir}/check-vmm-host.txt
- ${lh1_dir}/collect-vmm-package-versions.txt

LH2:
- ${lh2_dir}/verify-crun-patch.txt

LH3:
- ${lh3_dir}/build-stage-runtime.txt
- ${lh3_dir}/stage-runtime-version.txt
- ${lh3_dir}/install-private-runtime.txt
- ${lh3_dir}/install-runtime-version.txt
- staged binary: ${stage_binary}
- install path: ${install_path}

LH4:
- ${lh4_dir}/verify-runtime-separation.txt

LH5:
- ${lh5_dir}/buildah-from.txt
- ${rootfs_file}
- ${buildah_inspect_file}
- ${lh5_dir}/prepare-krun-bundle.txt
- ${lh5_dir}/prepare-direct-drill.txt
- ${lh5_dir}/direct-drill.env
- ${lh5_dir}/direct-start.txt
- ${lh5_dir}/direct-wait-for-http.txt
- ${lh5_dir}/direct-probe-http.txt
- ${lh5_dir}/direct-graceful-stop.txt
- ${lh5_dir}/direct-exit-status.txt

LH6:
- ${lh6_dir}/prepare-conmon-drill.txt
- ${lh6_dir}/conmon-drill.env
- ${lh6_dir}/run-conmon.stdout.txt
- ${lh6_dir}/run-conmon.stderr.txt
- ${lh6_dir}/run-conmon-wrapper.pid
- ${lh6_dir}/attach-sockets.txt
- ${lh6_dir}/process-tree.txt
- ${lh6_dir}/conmon-probe-http.txt
- ${lh6_dir}/conmon-graceful-stop.txt
- ${lh6_dir}/conmon-exit-status.txt

Record the exact command files used:
- ${queue_runner}
- ${lh1_script}
- ${lh2_script}
- ${lh3_stage_script}
- ${lh3_install_script}
- ${lh4_script}
- ${lh5_rootfs_script}
- ${lh5_bundle_script}
- ${lh5_direct_prepare_script}
- ${lh5_direct_run_script}
- ${lh6_prepare_script}
- ${lh6_run_script}
- ${cleanup_script}
EOF

echo "bundle.output_root=${output_root}"
echo "bundle.session_env=${session_env}"
echo "bundle.readme=${readme_file}"
echo "bundle.queue_runner=${queue_runner}"
echo "bundle.checklist=${checklist_file}"
echo "bundle.crun_source=${crun_source}"
echo "bundle.stage_binary=${stage_binary}"
echo "bundle.install_path=${install_path}"
echo "bundle.bundle_dir=${bundle_dir}"
echo "bundle.image_ref=${image_ref}"
echo "bundle.host_port=${host_port}"
echo "bundle.guest_port=${guest_port}"
echo "bundle.probe_url=${probe_url}"
