#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: prepare-nimbus-machine-local-bootc-image.sh --nimbus-binary <path> --output-dir <path> [options]

Prepare a local Nimbus machine image for macOS development by invoking the
nimbus/machine-os-owned bootc build. The raw disk remains a machine-os artifact:
this helper only orchestrates the same-host or SSH-builder call and fetches the
result into the host proof/recreate output directory.

options:
  --nimbus-binary <path>           Local Linux arm64 nimbus binary to bake
  --machine-os-repo <path>         Local machine-os checkout for same-host builds
                                   (required unless --builder is set)
  --output-dir <path>              Local output directory
  --nimbus-version <version>       Version recorded in image metadata
                                   (default: dev-local)
  --source-revision <revision>     Source revision recorded in image metadata
                                   (default: unknown)
  --builder <ssh-target>           Optional SSH target for Linux arm64 builder
                                   (or $NIMBUS_MACHINE_OS_BUILDER)
  --builder-machine-os-repo <path> machine-os checkout path on the SSH builder
                                   (or $NIMBUS_MACHINE_OS_BUILDER_REPO)
  --builder-work-dir <path>        Scratch/output root on the SSH builder
                                   (or $NIMBUS_MACHINE_OS_BUILDER_WORK_DIR)
  --ssh <path>                     SSH command (default: $NIMBUS_MACHINE_OS_BUILDER_SSH or ssh)
  --scp <path>                     SCP command (default: $NIMBUS_MACHINE_OS_BUILDER_SCP or scp)
  --builder-sudo <command>         Remote sudo command (default: $NIMBUS_MACHINE_OS_BUILDER_SUDO or sudo)
  -h, --help                       Show this help
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 64
}

write_command_file() {
  local output_path="$1"
  shift

  local -a rendered=()
  local arg=""

  for arg in "$@"; do
    rendered+=( "$(printf '%q' "${arg}")" )
  done

  printf '%s\n' "${rendered[*]}" > "${output_path}"
}

shell_join() {
  local -a rendered=()
  local arg=""

  for arg in "$@"; do
    rendered+=( "$(printf '%q' "${arg}")" )
  done

  printf '%s' "${rendered[*]}"
}

nimbus_binary=""
machine_os_repo=""
output_dir=""
nimbus_version="${NIMBUS_MACHINE_OS_LOCAL_VERSION:-dev-local}"
source_revision="${NIMBUS_MACHINE_OS_LOCAL_SOURCE_REVISION:-unknown}"
builder="${NIMBUS_MACHINE_OS_BUILDER:-}"
builder_machine_os_repo="${NIMBUS_MACHINE_OS_BUILDER_REPO:-}"
builder_work_dir="${NIMBUS_MACHINE_OS_BUILDER_WORK_DIR:-}"
ssh_cmd="${NIMBUS_MACHINE_OS_BUILDER_SSH:-ssh}"
scp_cmd="${NIMBUS_MACHINE_OS_BUILDER_SCP:-scp}"
builder_sudo="${NIMBUS_MACHINE_OS_BUILDER_SUDO:-sudo}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --nimbus-binary)
      nimbus_binary="${2:?missing nimbus binary}"
      shift 2
      ;;
    --machine-os-repo)
      machine_os_repo="${2:?missing machine-os repo}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    --nimbus-version)
      nimbus_version="${2:?missing nimbus version}"
      shift 2
      ;;
    --source-revision)
      source_revision="${2:?missing source revision}"
      shift 2
      ;;
    --builder)
      builder="${2:?missing builder}"
      shift 2
      ;;
    --builder-machine-os-repo)
      builder_machine_os_repo="${2:?missing builder machine-os repo}"
      shift 2
      ;;
    --builder-work-dir)
      builder_work_dir="${2:?missing builder work dir}"
      shift 2
      ;;
    --ssh)
      ssh_cmd="${2:?missing ssh command}"
      shift 2
      ;;
    --scp)
      scp_cmd="${2:?missing scp command}"
      shift 2
      ;;
    --builder-sudo)
      builder_sudo="${2:?missing builder sudo command}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "${nimbus_binary}" ]] || die "--nimbus-binary is required"
[[ -n "${output_dir}" ]] || die "--output-dir is required"
[[ -x "${nimbus_binary}" ]] || die "nimbus binary is not executable at ${nimbus_binary}"

mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd)"

if [[ -z "${builder}" ]]; then
  [[ -n "${machine_os_repo}" ]] || die "--machine-os-repo is required without --builder"
  [[ -f "${machine_os_repo}/scripts/build.sh" ]] || die "machine-os build script not found at ${machine_os_repo}/scripts/build.sh"
  build_cmd=(
    bash
    "${machine_os_repo}/scripts/build.sh"
    --nimbus-binary "${nimbus_binary}"
    --nimbus-version "${nimbus_version}"
    --source-revision "${source_revision}"
    --output-dir "${output_dir}"
  )
  {
    printf 'artifact.producer=nimbus/machine-os\n'
    printf 'artifact.producer_script=%s\n' "${machine_os_repo}/scripts/build.sh"
    printf 'artifact.output=%s\n' "${output_dir}/nimbus-machine-os.raw"
  } > "${output_dir}/nimbus-machine-local-bootc-provenance.txt"
  write_command_file "${output_dir}/nimbus-machine-local-bootc-build-command.txt" "${build_cmd[@]}"
  "${build_cmd[@]}"
else
  [[ -n "${builder_machine_os_repo}" ]] || die "--builder-machine-os-repo or NIMBUS_MACHINE_OS_BUILDER_REPO is required with --builder"
  [[ -n "${builder_work_dir}" ]] || die "--builder-work-dir or NIMBUS_MACHINE_OS_BUILDER_WORK_DIR is required with --builder"
  command -v "${ssh_cmd}" >/dev/null 2>&1 || die "ssh command not found: ${ssh_cmd}"
  command -v "${scp_cmd}" >/dev/null 2>&1 || die "scp command not found: ${scp_cmd}"

  remote_root="${builder_work_dir%/}/nimbus-machine-os-dev-${nimbus_version}-$$"
  remote_binary="${remote_root}/input/nimbus"
  remote_output="${remote_root}/output"

  mkdir_remote_cmd="$(shell_join mkdir -p "${remote_root}/input" "${remote_output}")"
  copy_to_builder_cmd=( "${scp_cmd}" "${nimbus_binary}" "${builder}:${remote_binary}" )
  chmod_remote_cmd="$(shell_join chmod +x "${remote_binary}")"
  remote_build_cmd="$(
    printf 'cd %s && %s\n' \
      "$(printf '%q' "${builder_machine_os_repo}")" \
      "$(shell_join "${builder_sudo}" bash scripts/build.sh \
        --nimbus-binary "${remote_binary}" \
        --nimbus-version "${nimbus_version}" \
        --source-revision "${source_revision}" \
        --output-dir "${remote_output}")"
  )"
  copy_from_builder_cmd=( "${scp_cmd}" -r "${builder}:${remote_output}/." "${output_dir}/" )

  {
    printf 'artifact.producer=nimbus/machine-os\n'
    printf 'artifact.producer_script=%s\n' "${builder_machine_os_repo}/scripts/build.sh"
    printf 'artifact.output=%s\n' "${output_dir}/nimbus-machine-os.raw"
    printf 'builder=%s\n' "${builder}"
    printf 'local_machine_os_repo=%s\n' "${machine_os_repo:-<unspecified>}"
    printf 'builder_machine_os_repo=%s\n' "${builder_machine_os_repo}"
    printf 'builder_work_dir=%s\n' "${builder_work_dir}"
    printf 'remote_output=%s\n' "${remote_output}"
  } > "${output_dir}/nimbus-machine-local-bootc-builder.txt"

  write_command_file "${output_dir}/nimbus-machine-local-bootc-build-command.txt" \
    "${ssh_cmd}" "${builder}" "${remote_build_cmd}"
  write_command_file "${output_dir}/nimbus-machine-local-bootc-copy-to-builder-command.txt" \
    "${copy_to_builder_cmd[@]}"
  write_command_file "${output_dir}/nimbus-machine-local-bootc-copy-from-builder-command.txt" \
    "${copy_from_builder_cmd[@]}"

  "${ssh_cmd}" "${builder}" "${mkdir_remote_cmd}"
  "${copy_to_builder_cmd[@]}"
  "${ssh_cmd}" "${builder}" "${chmod_remote_cmd}"
  "${ssh_cmd}" "${builder}" "${remote_build_cmd}"
  "${copy_from_builder_cmd[@]}"
fi

[[ -f "${output_dir}/nimbus-machine-os.raw" ]] || die "bootc build completed but did not produce ${output_dir}/nimbus-machine-os.raw"

printf 'local bootc image built at %s\n' "${output_dir}/nimbus-machine-os.raw"
