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
grep -F "nimbus/machine-os@${source_ref}" \
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

bad_repository="${tmp_dir}/bad-repository.yml"
sed 's|MACHINE_OS_REPOSITORY: nimbus/machine-os|MACHINE_OS_REPOSITORY: nimbus/nimbus-machine-os|' \
  "${repo_root}/.github/workflows/release.yml" >"${bad_repository}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_repository}" \
  >"${tmp_dir}/bad-repository.out" 2>&1; then
  echo "expected release ref contract to reject the legacy machine-os repository name" >&2
  exit 1
fi
grep -F "MACHINE_OS_REPOSITORY: nimbus/machine-os" \
  "${tmp_dir}/bad-repository.out" >/dev/null

bad_build_publish="${tmp_dir}/bad-build-publish.yml"
awk '
  $0 == "  publish-machine-os:" && inserted == 0 {
    print "      - name: Forbidden publish in build stage"
    print "        run: bash scripts/publish.sh"
    inserted = 1
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_build_publish}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_build_publish}" \
  >"${tmp_dir}/bad-build-publish.out" 2>&1; then
  echo "expected release ref contract to reject publishing from build-machine-os" >&2
  exit 1
fi
grep -F "build-machine-os must not contain: bash scripts/publish.sh" \
  "${tmp_dir}/bad-build-publish.out" >/dev/null

bad_publish_gate="${tmp_dir}/bad-publish-gate.yml"
awk '
  $0 ~ /bash scripts\/verify-machine-os-release-default-gate\.sh/ {
    next
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_publish_gate}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_publish_gate}" \
  >"${tmp_dir}/bad-publish-gate.out" 2>&1; then
  echo "expected release ref contract to reject publish-machine-os without the default gate" >&2
  exit 1
fi
grep -F "publish-machine-os must contain: bash scripts/verify-machine-os-release-default-gate.sh" \
  "${tmp_dir}/bad-publish-gate.out" >/dev/null

bad_public_gate="${tmp_dir}/bad-public-gate.yml"
awk '
  $0 ~ /--require-ghcr-public/ {
    next
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_public_gate}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_public_gate}" \
  >"${tmp_dir}/bad-public-gate.out" 2>&1; then
  echo "expected release ref contract to reject publish-machine-os without GHCR public-read gate" >&2
  exit 1
fi
grep -F "publish-machine-os must contain: --require-ghcr-public" \
  "${tmp_dir}/bad-public-gate.out" >/dev/null

bad_dispatch_return="${tmp_dir}/bad-dispatch-return.yml"
awk '
  $0 ~ /return_run_details=true/ {
    next
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_dispatch_return}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_dispatch_return}" \
  >"${tmp_dir}/bad-dispatch-return.out" 2>&1; then
  echo "expected release ref contract to reject dispatch without returned run details" >&2
  exit 1
fi
grep -F "publish-machine-os must contain: return_run_details=true" \
  "${tmp_dir}/bad-dispatch-return.out" >/dev/null

bad_run_discovery="${tmp_dir}/bad-run-discovery.yml"
awk '
  $0 == "      - name: Wait for machine-os publish workflow" && inserted == 0 {
    print "      - name: Forbidden loose run discovery"
    print "        run: gh run list --repo \"${MACHINE_OS_REPOSITORY}\" --workflow publish.yml"
    inserted = 1
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_run_discovery}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_run_discovery}" \
  >"${tmp_dir}/bad-run-discovery.out" 2>&1; then
  echo "expected release ref contract to reject loose machine-os run discovery" >&2
  exit 1
fi
grep -F "must not discover machine-os publish runs by listing recent runs" \
  "${tmp_dir}/bad-run-discovery.out" >/dev/null

bad_release_artifact="${tmp_dir}/bad-release-artifact.yml"
awk '
  $0 ~ /MACHINE_OS_RELEASE_ARTIFACT/ {
    next
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_release_artifact}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_release_artifact}" \
  >"${tmp_dir}/bad-release-artifact.out" 2>&1; then
  echo "expected release ref contract to reject missing machine-os release artifact handoff" >&2
  exit 1
fi
grep -F 'publish-machine-os must contain: name: ${{ env.MACHINE_OS_RELEASE_ARTIFACT }}' \
  "${tmp_dir}/bad-release-artifact.out" >/dev/null

bad_release_needs="${tmp_dir}/bad-release-needs.yml"
awk '
  $0 == "    needs: [build-linux-arm64, build, publish-machine-os]" && replaced == 0 {
    print "    needs: [build-linux-arm64, build, build-machine-os]"
    replaced = 1
    next
  }
  { print }
' "${repo_root}/.github/workflows/release.yml" >"${bad_release_needs}"
if bash "${repo_root}/scripts/verify-machine-os-release-ref-contract.sh" \
  --workflow "${bad_release_needs}" \
  >"${tmp_dir}/bad-release-needs.out" 2>&1; then
  echo "expected release ref contract to reject final release bypassing publish-machine-os" >&2
  exit 1
fi
grep -F "release must contain: needs: [build-linux-arm64, build, publish-machine-os]" \
  "${tmp_dir}/bad-release-needs.out" >/dev/null

printf 'verified: machine-os release source contract helper rejects ambiguous refs, legacy repository names, loose run discovery, and build/publish DAG regressions\n'
