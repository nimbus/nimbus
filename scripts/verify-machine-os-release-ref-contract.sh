#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-machine-os-release-ref-contract.sh [--workflow <path>] [--machine-os-repo <path>]

Verify that the Nimbus release workflow builds the nimbus-machine-os bootc
artifact directly from one explicit source ref inside the release graph.

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

grep -F "build-machine-os:" "${workflow_path}" >/dev/null || \
  die "release workflow must define build-machine-os"
grep -F "repository: nimbus/nimbus-machine-os" "${workflow_path}" >/dev/null || \
  die "release workflow must check out nimbus/nimbus-machine-os"
grep -F 'ref: ${{ env.MACHINE_OS_SOURCE_REF }}' "${workflow_path}" >/dev/null || \
  die "machine-os checkout must use MACHINE_OS_SOURCE_REF"
grep -F "path: nimbus-machine-os" "${workflow_path}" >/dev/null || \
  die "machine-os checkout must use a stable nimbus-machine-os path"
grep -F "bash scripts/build.sh" "${workflow_path}" >/dev/null || \
  die "release workflow must run the machine-os build script in the release graph"
grep -F "bash scripts/package-oci.sh" "${workflow_path}" >/dev/null || \
  die "release workflow must package the machine-os OCI artifact in the release graph"
grep -F "bash scripts/publish.sh" "${workflow_path}" >/dev/null || \
  die "release workflow must publish the machine-os OCI artifact in the release graph"
grep -F "bash scripts/verify-machine-os-release-default-gate.sh" "${workflow_path}" >/dev/null || \
  die "release workflow must run the machine-os release default gate before creating releases"
grep -F "needs: [build-linux-arm64, build, build-machine-os]" "${workflow_path}" >/dev/null || \
  die "Nimbus release job must depend on build-machine-os"

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

printf 'verified: machine-os release source contract builds nimbus/nimbus-machine-os@%s inside the release graph\n' "${source_ref}"
