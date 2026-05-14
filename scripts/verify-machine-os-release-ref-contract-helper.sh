#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-machine-os-release-ref-contract.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

source_ref="$(
  awk '
    $1 == "MACHINE_OS_SOURCE_REF:" {
      print $2
      exit
    }
  ' "${repo_root}/.github/workflows/release.yml"
)"
[[ -n "${source_ref}" ]] || {
  echo "MACHINE_OS_SOURCE_REF is missing from release workflow" >&2
  exit 1
}

machine_os_repo="${tmp_dir}/machine-os"
git init -q "${machine_os_repo}"
git -C "${machine_os_repo}" checkout -q -b "${source_ref}"
git -C "${machine_os_repo}" config user.name "Nimbus Test"
git -C "${machine_os_repo}" config user.email "nimbus@example.invalid"
git -C "${machine_os_repo}" config commit.gpgsign false
git -C "${machine_os_repo}" config tag.gpgSign false
touch "${machine_os_repo}/README.md"
git -C "${machine_os_repo}" add README.md
git -C "${machine_os_repo}" commit -q -m "seed"

bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --machine-os-repo "${machine_os_repo}" \
  >"${tmp_dir}/clean.out"
grep -F "nimbus/nimbus-machine-os@${source_ref}" \
  "${tmp_dir}/clean.out" >/dev/null

git -C "${machine_os_repo}" tag "${source_ref}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --machine-os-repo "${machine_os_repo}" \
  >"${tmp_dir}/ambiguous.out" 2>&1; then
  echo "expected release ref contract to reject branch/tag ambiguity" >&2
  exit 1
fi
grep -F "is ambiguous" "${tmp_dir}/ambiguous.out" >/dev/null

bad_workflow="${tmp_dir}/bad-release.yml"
cp "${repo_root}/.github/workflows/release.yml" "${bad_workflow}"
cat >>"${bad_workflow}" <<'EOF'
  MACHINE_OS_RELEASE_WORKFLOW_REF: release-workflow-v1
EOF
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_workflow}" \
  >"${tmp_dir}/bad-workflow.out" 2>&1; then
  echo "expected release ref contract to reject legacy workflow ref usage" >&2
  exit 1
fi
grep -F "MACHINE_OS_RELEASE_WORKFLOW_REF" "${tmp_dir}/bad-workflow.out" >/dev/null

printf 'verified: machine-os release source contract helper rejects ambiguous refs and legacy dispatch state\n'
