#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-machine-os-release-ref-contract.sh [--workflow <path>] [--machine-os-repo <path>]

Verify that the Nimbus release workflow uses one explicit nimbus-machine-os
workflow ref for staging, workflow_dispatch, and run lookup.

This prevents the host release from staging a machine image with one workflow
revision and publishing or watching another.

When --machine-os-repo is provided, also verify the selected ref is not
ambiguous between a local branch and tag in that checkout.
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
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

release_ref="$(
  awk '
    $1 == "MACHINE_OS_RELEASE_WORKFLOW_REF:" {
      print $2
      exit
    }
  ' "${workflow_path}"
)"

[[ -n "${release_ref}" ]] || die "MACHINE_OS_RELEASE_WORKFLOW_REF is missing from ${workflow_path}"
[[ "${release_ref}" =~ ^[A-Za-z0-9._/-]+$ ]] || die "machine-os release workflow ref has unexpected characters: ${release_ref}"

stage_uses_count="$(
  grep -F "uses: nimbus/nimbus-machine-os/.github/workflows/build.yml@${release_ref}" \
    "${workflow_path}" | wc -l | tr -d '[:space:]'
)"
[[ "${stage_uses_count}" == "1" ]] || die "stage-machine-os must call build.yml@${release_ref} exactly once"

grep -F '"ref": "${MACHINE_OS_RELEASE_WORKFLOW_REF}"' "${workflow_path}" >/dev/null || \
  die "machine-os dispatch payload must use MACHINE_OS_RELEASE_WORKFLOW_REF"
grep -F -- '--branch "${MACHINE_OS_RELEASE_WORKFLOW_REF}"' "${workflow_path}" >/dev/null || \
  die "machine-os run lookup must use MACHINE_OS_RELEASE_WORKFLOW_REF"

if grep -F '"ref": "release-workflow-' "${workflow_path}" >/dev/null; then
  die "machine-os dispatch ref is hard-coded instead of MACHINE_OS_RELEASE_WORKFLOW_REF"
fi
if grep -F -- '--branch release-workflow-' "${workflow_path}" >/dev/null; then
  die "machine-os run lookup branch is hard-coded instead of MACHINE_OS_RELEASE_WORKFLOW_REF"
fi

if [[ -n "${machine_os_repo}" ]]; then
  [[ -d "${machine_os_repo}" ]] || die "machine-os repo not found: ${machine_os_repo}"
  git -C "${machine_os_repo}" rev-parse --git-dir >/dev/null 2>&1 || \
    die "machine-os repo is not a git checkout: ${machine_os_repo}"

  branch_ref="refs/heads/${release_ref}"
  remote_branch_ref="refs/remotes/origin/${release_ref}"
  tag_ref="refs/tags/${release_ref}"

  has_branch=0
  has_remote_branch=0
  has_tag=0
  git -C "${machine_os_repo}" show-ref --verify --quiet "${branch_ref}" && has_branch=1
  git -C "${machine_os_repo}" show-ref --verify --quiet "${remote_branch_ref}" && has_remote_branch=1
  git -C "${machine_os_repo}" show-ref --verify --quiet "${tag_ref}" && has_tag=1

  if [[ "${has_tag}" -eq 1 && $((has_branch + has_remote_branch)) -gt 0 ]]; then
    die "machine-os release ref ${release_ref} is ambiguous in ${machine_os_repo}: branch and tag refs both exist"
  fi
  if [[ $((has_branch + has_remote_branch + has_tag)) -eq 0 ]]; then
    die "machine-os release ref ${release_ref} was not found in ${machine_os_repo}"
  fi
fi

printf 'verified: machine-os release workflow ref contract uses %s consistently\n' "${release_ref}"
