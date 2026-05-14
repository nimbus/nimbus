#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-machine-os-release-ref-contract.sh [--workflow <path>] [--machine-os-repo <path>]

Verify that the Nimbus release workflow builds the nimbus-machine-os bootc
artifact from one explicit source ref inside the release graph, then publishes
it only after the full CLI release target matrix has passed.

This prevents the default release path from drifting back to the older
cross-repo dispatch/watch handoff or to a separate release-workflow branch.

When --machine-os-repo is provided, also verify the selected source ref is
present and not ambiguous between a local branch and tag in that checkout.
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

job_section() {
  local job_name="$1"
  awk -v job_name="${job_name}" '
    $0 ~ "^  " job_name ":" {
      in_job = 1
      print
      next
    }
    in_job && $0 ~ "^  [A-Za-z0-9_-]+:" {
      exit
    }
    in_job {
      print
    }
  ' "${workflow_path}"
}

require_in_section() {
  local section_name="$1"
  local section="$2"
  local needle="$3"
  grep -F "${needle}" <<<"${section}" >/dev/null || \
    die "${section_name} must contain: ${needle}"
}

reject_in_section() {
  local section_name="$1"
  local section="$2"
  local needle="$3"
  if grep -F "${needle}" <<<"${section}" >/dev/null; then
    die "${section_name} must not contain: ${needle}"
  fi
}

workflow_path=".github/workflows/release.yml"
machine_os_repo=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workflow)
      workflow_path="${2:-}"
      shift 2
      ;;
    --machine-os-repo)
      machine_os_repo="${2:-}"
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

[[ -f "${workflow_path}" ]] || die "workflow not found: ${workflow_path}"

source_ref="$(
  awk '
    $1 == "MACHINE_OS_SOURCE_REF:" {
      print $2
      exit
    }
  ' "${workflow_path}"
)"

[[ -n "${source_ref}" ]] || die "MACHINE_OS_SOURCE_REF is missing from ${workflow_path}"
[[ "${source_ref}" =~ ^[A-Za-z0-9._/-]+$ ]] || die "machine-os source ref has unexpected characters: ${source_ref}"

if grep -F "MACHINE_OS_RELEASE_WORKFLOW_REF" "${workflow_path}" >/dev/null; then
  die "release workflow must not use MACHINE_OS_RELEASE_WORKFLOW_REF"
fi
if grep -F "uses: nimbus/nimbus-machine-os/.github/workflows/build.yml@" "${workflow_path}" >/dev/null; then
  die "release workflow must not call the nimbus-machine-os reusable build workflow for the default release path"
fi
if grep -F "repos/nimbus/nimbus-machine-os/actions/workflows/publish.yml/dispatches" "${workflow_path}" >/dev/null; then
  die "release workflow must not dispatch the nimbus-machine-os publish workflow"
fi
if grep -F -- "--workflow publish.yml" "${workflow_path}" >/dev/null; then
  die "release workflow must not poll nimbus-machine-os publish workflow runs"
fi
if grep -F "gh run watch" "${workflow_path}" >/dev/null; then
  die "release workflow must not watch a second asynchronous machine-os workflow"
fi

build_machine_os_section="$(job_section build-machine-os)"
publish_machine_os_section="$(job_section publish-machine-os)"
release_section="$(job_section release)"

[[ -n "${build_machine_os_section}" ]] || die "release workflow must define build-machine-os"
[[ -n "${publish_machine_os_section}" ]] || die "release workflow must define publish-machine-os"
[[ -n "${release_section}" ]] || die "release workflow must define release"

require_in_section build-machine-os "${build_machine_os_section}" "needs: [build-linux-arm64]"
require_in_section build-machine-os "${build_machine_os_section}" "contents: read"
require_in_section build-machine-os "${build_machine_os_section}" "repository: nimbus/nimbus-machine-os"
require_in_section build-machine-os "${build_machine_os_section}" 'ref: ${{ env.MACHINE_OS_SOURCE_REF }}'
require_in_section build-machine-os "${build_machine_os_section}" "path: nimbus-machine-os"
require_in_section build-machine-os "${build_machine_os_section}" "machine_os_source_revision:"
require_in_section build-machine-os "${build_machine_os_section}" "bash scripts/build.sh"
require_in_section build-machine-os "${build_machine_os_section}" "bash scripts/package-oci.sh"
require_in_section build-machine-os "${build_machine_os_section}" "name: nimbus-machine-os-arm64-staged"
require_in_section build-machine-os "${build_machine_os_section}" '${{ env.LAYOUT_DIR }}/**'
reject_in_section build-machine-os "${build_machine_os_section}" "packages: write"
reject_in_section build-machine-os "${build_machine_os_section}" "id-token: write"
reject_in_section build-machine-os "${build_machine_os_section}" "attestations: write"
reject_in_section build-machine-os "${build_machine_os_section}" "permission-packages: write"
reject_in_section build-machine-os "${build_machine_os_section}" "bash scripts/publish.sh"
reject_in_section build-machine-os "${build_machine_os_section}" "bash scripts/verify-machine-os-release-default-gate.sh"
reject_in_section build-machine-os "${build_machine_os_section}" "gh release"
reject_in_section build-machine-os "${build_machine_os_section}" "actions/attest"

require_in_section publish-machine-os "${publish_machine_os_section}" "needs: [build-linux-arm64, build, build-machine-os]"
require_in_section publish-machine-os "${publish_machine_os_section}" "packages: write"
require_in_section publish-machine-os "${publish_machine_os_section}" "id-token: write"
require_in_section publish-machine-os "${publish_machine_os_section}" "attestations: write"
require_in_section publish-machine-os "${publish_machine_os_section}" "repository: nimbus/nimbus-machine-os"
require_in_section publish-machine-os "${publish_machine_os_section}" 'ref: ${{ needs.build-machine-os.outputs.machine_os_source_revision }}'
require_in_section publish-machine-os "${publish_machine_os_section}" "name: nimbus-machine-os-arm64-staged"
require_in_section publish-machine-os "${publish_machine_os_section}" "bash scripts/publish.sh"
require_in_section publish-machine-os "${publish_machine_os_section}" "bash scripts/verify-machine-os-release-default-gate.sh"
require_in_section publish-machine-os "${publish_machine_os_section}" "gh release create"
require_in_section publish-machine-os "${publish_machine_os_section}" "actions/attest@v4"
require_in_section publish-machine-os "${publish_machine_os_section}" "name: nimbus-machine-os-arm64-release"

require_in_section release "${release_section}" "needs: [build-linux-arm64, build, publish-machine-os]"
reject_in_section release "${release_section}" "needs: [build-linux-arm64, build, build-machine-os]"

if [[ -n "${machine_os_repo}" ]]; then
  [[ -d "${machine_os_repo}" ]] || die "machine-os repo not found: ${machine_os_repo}"
  git -C "${machine_os_repo}" rev-parse --git-dir >/dev/null 2>&1 || \
    die "machine-os repo is not a git checkout: ${machine_os_repo}"

  branch_ref="refs/heads/${source_ref}"
  remote_branch_ref="refs/remotes/origin/${source_ref}"
  tag_ref="refs/tags/${source_ref}"

  has_branch=0
  has_remote_branch=0
  has_tag=0
  git -C "${machine_os_repo}" show-ref --verify --quiet "${branch_ref}" && has_branch=1
  git -C "${machine_os_repo}" show-ref --verify --quiet "${remote_branch_ref}" && has_remote_branch=1
  git -C "${machine_os_repo}" show-ref --verify --quiet "${tag_ref}" && has_tag=1

  if [[ "${has_tag}" -eq 1 && $((has_branch + has_remote_branch)) -gt 0 ]]; then
    die "machine-os source ref ${source_ref} is ambiguous in ${machine_os_repo}: branch and tag refs both exist"
  fi
  if [[ $((has_branch + has_remote_branch + has_tag)) -eq 0 ]]; then
    die "machine-os source ref ${source_ref} was not found in ${machine_os_repo}"
  fi
fi

printf 'verified: machine-os release source contract builds nimbus/nimbus-machine-os@%s in a staged job and publishes only after release targets pass\n' "${source_ref}"
