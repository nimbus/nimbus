#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-machine-os-release-ref-contract.sh [--workflow <path>] [--machine-os-repo <path>]

Verify that the Nimbus release workflow builds the machine-os bootc artifact
from one explicit source ref inside the release graph, then dispatches the
machine-os repository to publish it only after the full CLI release target
matrix has passed.

This prevents the default release path from drifting back to the old
repository/package name, the reusable-workflow package ownership bug, broad
PAT publishing, a separate release-workflow branch, or title-matched run
polling.

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
  grep -F -- "${needle}" <<<"${section}" >/dev/null || \
    die "${section_name} must contain: ${needle}"
}

reject_in_section() {
  local section_name="$1"
  local section="$2"
  local needle="$3"
  if grep -F -- "${needle}" <<<"${section}" >/dev/null; then
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
if grep -F "uses: nimbus/machine-os/.github/workflows/build.yml@" "${workflow_path}" >/dev/null; then
  die "release workflow must not call the machine-os reusable build workflow for the default release path"
fi
if grep -F "gh run list" "${workflow_path}" >/dev/null; then
  die "release workflow must not discover machine-os publish runs by listing recent runs"
fi
if grep -F "gh run watch" "${workflow_path}" >/dev/null; then
  die "release workflow must not use loose gh run watch state for machine-os publication"
fi
grep -F "MACHINE_OS_REPOSITORY: nimbus/machine-os" "${workflow_path}" >/dev/null || \
  die "release workflow must set MACHINE_OS_REPOSITORY: nimbus/machine-os"
grep -F "MACHINE_OS_PACKAGE: ghcr.io/nimbus/machine-os" "${workflow_path}" >/dev/null || \
  die "release workflow must set MACHINE_OS_PACKAGE: ghcr.io/nimbus/machine-os"
grep -F "MACHINE_OS_RELEASE_APP_CLIENT_ID" "${workflow_path}" >/dev/null || \
  die "release workflow must use MACHINE_OS_RELEASE_APP_CLIENT_ID for GitHub App token creation"
if grep -F "MACHINE_OS_RELEASE_APP_ID" "${workflow_path}" >/dev/null; then
  die "release workflow must not use deprecated MACHINE_OS_RELEASE_APP_ID; use MACHINE_OS_RELEASE_APP_CLIENT_ID"
fi
if grep -F "HOMEBREW_TAP_APP_ID" "${workflow_path}" >/dev/null; then
  die "release workflow must not use deprecated HOMEBREW_TAP_APP_ID; use HOMEBREW_TAP_CLIENT_ID"
fi
if grep -F "app-id:" "${workflow_path}" >/dev/null; then
  die "release workflow must not use deprecated actions/create-github-app-token app-id input; use client-id"
fi
grep -F "actions/create-github-app-token@v3.2.0" "${workflow_path}" >/dev/null || \
  die "release workflow must pin actions/create-github-app-token@v3.2.0 so client-id metadata is stable for actionlint and runners"
grep -F "MACHINE_OS_FEDORA_BOOTC_IMAGE: quay.io/fedora/fedora-bootc@sha256:" "${workflow_path}" >/dev/null || \
  die "release workflow must set MACHINE_OS_FEDORA_BOOTC_IMAGE to a digest-pinned Fedora bootc image"
fedora_bootc_refs="$(
  grep -Eo 'quay\.io/fedora/fedora-bootc(@sha256:[0-9a-f]{64}|:[A-Za-z0-9._-]+)?' \
    "${workflow_path}" | sort -u
)"
fedora_bootc_ref_count="$(printf '%s\n' "${fedora_bootc_refs}" | sed '/^$/d' | wc -l | tr -d ' ')"
[[ "${fedora_bootc_ref_count}" == "1" ]] || \
  die "release workflow must use one Fedora bootc image reference; found: ${fedora_bootc_refs}"

build_machine_os_section="$(job_section build-machine-os)"
publish_machine_os_section="$(job_section publish-machine-os)"
release_section="$(job_section release)"

[[ -n "${build_machine_os_section}" ]] || die "release workflow must define build-machine-os"
[[ -n "${publish_machine_os_section}" ]] || die "release workflow must define publish-machine-os"
[[ -n "${release_section}" ]] || die "release workflow must define release"

require_in_section build-machine-os "${build_machine_os_section}" "needs: [build-linux-arm64]"
require_in_section build-machine-os "${build_machine_os_section}" "contents: read"
require_in_section build-machine-os "${build_machine_os_section}" 'client-id: ${{ vars.MACHINE_OS_RELEASE_APP_CLIENT_ID }}'
require_in_section build-machine-os "${build_machine_os_section}" 'repository: ${{ env.MACHINE_OS_REPOSITORY }}'
require_in_section build-machine-os "${build_machine_os_section}" 'ref: ${{ env.MACHINE_OS_SOURCE_REF }}'
require_in_section build-machine-os "${build_machine_os_section}" "path: machine-os"
require_in_section build-machine-os "${build_machine_os_section}" "machine_os_source_revision:"
require_in_section build-machine-os "${build_machine_os_section}" "bash scripts/build.sh"
require_in_section build-machine-os "${build_machine_os_section}" "bash scripts/package-oci.sh"
require_in_section build-machine-os "${build_machine_os_section}" 'name: ${{ env.MACHINE_OS_STAGED_ARTIFACT }}'
require_in_section build-machine-os "${build_machine_os_section}" 'path: ${{ env.STAGE_DIR }}/**'
require_in_section build-machine-os "${build_machine_os_section}" "docker://\${{ env.MACHINE_OS_PACKAGE }}:"
require_in_section build-machine-os "${build_machine_os_section}" "https://github.com/\${{ env.MACHINE_OS_REPOSITORY }}"
require_in_section build-machine-os "${build_machine_os_section}" 'sudo podman pull "${MACHINE_OS_FEDORA_BOOTC_IMAGE}"'
require_in_section build-machine-os "${build_machine_os_section}" 'sudo podman save -o "${cache_dir}/fedora-bootc-base.tar" "${MACHINE_OS_FEDORA_BOOTC_IMAGE}"'
require_in_section build-machine-os "${build_machine_os_section}" "build-output"
require_in_section build-machine-os "${build_machine_os_section}" "layout"
reject_in_section build-machine-os "${build_machine_os_section}" "packages: write"
reject_in_section build-machine-os "${build_machine_os_section}" "id-token: write"
reject_in_section build-machine-os "${build_machine_os_section}" "attestations: write"
reject_in_section build-machine-os "${build_machine_os_section}" "permission-packages: write"
reject_in_section build-machine-os "${build_machine_os_section}" "bash scripts/publish.sh"
reject_in_section build-machine-os "${build_machine_os_section}" "bash scripts/verify-machine-os-release-default-gate.sh"
reject_in_section build-machine-os "${build_machine_os_section}" "gh release"
reject_in_section build-machine-os "${build_machine_os_section}" "actions/attest"

require_in_section publish-machine-os "${publish_machine_os_section}" "needs: [build-linux-arm64, build, build-machine-os]"
require_in_section publish-machine-os "${publish_machine_os_section}" 'client-id: ${{ vars.MACHINE_OS_RELEASE_APP_CLIENT_ID }}'
require_in_section publish-machine-os "${publish_machine_os_section}" "permission-actions: write"
require_in_section publish-machine-os "${publish_machine_os_section}" "permission-contents: read"
require_in_section publish-machine-os "${publish_machine_os_section}" "repos/\${MACHINE_OS_REPOSITORY}/actions/workflows/publish.yml/dispatches"
require_in_section publish-machine-os "${publish_machine_os_section}" "return_run_details=true"
require_in_section publish-machine-os "${publish_machine_os_section}" "inputs[source_repository]"
require_in_section publish-machine-os "${publish_machine_os_section}" "inputs[source_run_id]"
require_in_section publish-machine-os "${publish_machine_os_section}" "inputs[source_run_attempt]"
require_in_section publish-machine-os "${publish_machine_os_section}" "inputs[machine_os_source_revision]"
require_in_section publish-machine-os "${publish_machine_os_section}" "workflow_run_id"
require_in_section publish-machine-os "${publish_machine_os_section}" "gh run view"
require_in_section publish-machine-os "${publish_machine_os_section}" "actions/download-artifact@v8"
require_in_section publish-machine-os "${publish_machine_os_section}" 'name: ${{ env.MACHINE_OS_RELEASE_ARTIFACT }}'
require_in_section publish-machine-os "${publish_machine_os_section}" 'repository: ${{ env.MACHINE_OS_REPOSITORY }}'
require_in_section publish-machine-os "${publish_machine_os_section}" 'run-id: ${{ steps.machine_os_publish.outputs.run_id }}'
require_in_section publish-machine-os "${publish_machine_os_section}" "bash scripts/verify-machine-os-release-default-gate.sh"
require_in_section publish-machine-os "${publish_machine_os_section}" "--require-ghcr-public"
reject_in_section publish-machine-os "${publish_machine_os_section}" "packages: write"
reject_in_section publish-machine-os "${publish_machine_os_section}" "id-token: write"
reject_in_section publish-machine-os "${publish_machine_os_section}" "attestations: write"
reject_in_section publish-machine-os "${publish_machine_os_section}" "permission-packages: write"
reject_in_section publish-machine-os "${publish_machine_os_section}" "bash scripts/publish.sh"
reject_in_section publish-machine-os "${publish_machine_os_section}" "gh release"
reject_in_section publish-machine-os "${publish_machine_os_section}" "actions/attest@v4"
reject_in_section publish-machine-os "${publish_machine_os_section}" 'NIMBUS_MACHINE_OS_REGISTRY_PASSWORD: ${{ steps.machine_os_token.outputs.token }}'
reject_in_section publish-machine-os "${publish_machine_os_section}" 'NIMBUS_MACHINE_OS_REGISTRY_PASSWORD: ${{ secrets.GITHUB_TOKEN }}'

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

printf 'verified: machine-os release source contract stages nimbus/machine-os@%s and dispatches machine-os-owned publication only after release targets pass\n' "${source_ref}"
