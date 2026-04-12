#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-neovex-crun-fedora-userspace.sh --crun-source <path> [options]

Run the neovex crun patch + build helpers inside a disposable Fedora container
through Docker Desktop. This is a macOS-friendly Linux userspace validation
lane: it proves the patch applies and the checked-in build helper can produce a
Linux crun binary, but it does NOT prove /dev/kvm, libkrun VM boot, or host
runtime integration.

options:
  --crun-source <path>   Upstream crun checkout on the host
  --image <ref>          Fedora container image to use (default: fedora:43)
  --output-dir <path>    Host directory for the staged crun binary
  --work-dir <path>      Host directory for the disposable build tree
  -h, --help             Show this help

examples:
  bash scripts/verify-neovex-crun-fedora-userspace.sh \
    --crun-source ~/src/github.com/containers/crun

  bash scripts/verify-neovex-crun-fedora-userspace.sh \
    --crun-source ~/src/github.com/containers/crun \
    --output-dir /tmp/neovex-crun-fedora-output
EOF
}

require_command() {
  local command_name="$1"

  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
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

resolve_dir_path() {
  local dir_path="$1"

  mkdir -p "${dir_path}"
  (
    cd "${dir_path}"
    pwd
  )
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crun_source=""
image_ref="fedora:43"
output_dir=""
work_dir=""
cleanup_output_dir=0
cleanup_work_dir=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --crun-source)
      crun_source="${2:-}"
      shift 2
      ;;
    --image)
      image_ref="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --work-dir)
      work_dir="${2:-}"
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

require_command docker
crun_source="$(resolve_existing_dir "${crun_source}")"

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-crun-fedora-output.XXXXXX")"
  cleanup_output_dir=1
fi
output_dir="$(resolve_dir_path "${output_dir}")"

if [[ -z "${work_dir}" ]]; then
  work_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-crun-fedora-build.XXXXXX")"
  cleanup_work_dir=1
fi
work_dir="$(resolve_dir_path "${work_dir}")"

cleanup() {
  if [[ "${cleanup_work_dir}" -eq 1 ]]; then
    rm -rf "${work_dir}"
  fi
  if [[ "${cleanup_output_dir}" -eq 1 ]]; then
    rm -rf "${output_dir}"
  fi
}
trap cleanup EXIT

container_output_dir="/work/output"
container_work_dir="/work/build"
staged_binary="${output_dir}/crun"

echo "userspace.image=${image_ref}"
echo "userspace.repo=${repo_root}"
echo "userspace.crun_source=${crun_source}"
echo "userspace.output_dir=${output_dir}"
echo "userspace.work_dir=${work_dir}"

docker run --rm \
  -v "${repo_root}:/work/repo:ro" \
  -v "${crun_source}:/work/crun-src:ro" \
  -v "${output_dir}:${container_output_dir}" \
  -v "${work_dir}:${container_work_dir}" \
  --workdir /work/repo \
  "${image_ref}" \
  bash -lc '
    set -euo pipefail
    dnf install -y \
      autoconf \
      automake \
      bash \
      coreutils \
      findutils \
      gcc \
      git \
      libcap-devel \
      libkrun-devel \
      libseccomp-devel \
      libtool \
      patch \
      pkgconf-pkg-config \
      python3 \
      systemd-devel \
      which \
      yajl-devel
    bash scripts/verify-crun-patch.sh /work/crun-src
    bash scripts/build-neovex-crun.sh \
      --source /work/crun-src \
      --output /work/output/crun \
      --build-dir /work/build
    /work/output/crun --version
  '

if [[ ! -f "${staged_binary}" ]]; then
  echo "expected staged binary not found after userspace validation: ${staged_binary}" >&2
  exit 70
fi

echo "userspace.staged_binary=${staged_binary}"
