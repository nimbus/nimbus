#!/usr/bin/env bash
# Verifies that a local WebKit source build uses the revision pinned by Bun.

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: verify-bun-webkit-source.sh --bun-repo PATH --webkit-repo PATH
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

bun_repo=""
webkit_repo=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bun-repo)
      bun_repo="${2:-}"
      shift 2
      ;;
    --webkit-repo)
      webkit_repo="${2:-}"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -d "${bun_repo}" ]] || die "Bun source checkout not found: ${bun_repo:-missing}"
[[ -d "${webkit_repo}" ]] || die "WebKit source checkout not found: ${webkit_repo:-missing}"
command -v git >/dev/null 2>&1 || die "git is required"

pin_file="${bun_repo}/scripts/build/deps/webkit.ts"
[[ -f "${pin_file}" ]] || die "Bun WebKit pin file not found: ${pin_file}"

webkit_versions="$({
  sed -nE 's/^export const WEBKIT_VERSION = "([^"]+)";$/\1/p' "${pin_file}"
} || true)"
webkit_version_count="$(printf '%s\n' "${webkit_versions}" | awk 'NF { count++ } END { print count + 0 }')"
if [[ "${webkit_version_count}" -ne 1 ]]; then
  die "expected exactly one WEBKIT_VERSION in ${pin_file}, found ${webkit_version_count}"
fi
webkit_version="${webkit_versions}"

if [[ "${webkit_version}" =~ ^[0-9a-f]{40}$ ]]; then
  expected_revision="${webkit_version}"
else
  expected_revision="$(git -C "${webkit_repo}" rev-parse "${webkit_version}^{commit}" 2>/dev/null || true)"
  [[ -n "${expected_revision}" ]] ||
    die "WebKit checkout cannot resolve Bun pin ${webkit_version}: ${webkit_repo}"
fi

if ! git -C "${webkit_repo}" cat-file -e "${expected_revision}^{commit}" 2>/dev/null; then
  die "WebKit checkout does not contain Bun pin ${expected_revision}: ${webkit_repo}"
fi

actual_revision="$(git -C "${webkit_repo}" rev-parse HEAD 2>/dev/null || true)"
if [[ "${actual_revision}" != "${expected_revision}" ]]; then
  die "unexpected WebKit revision: Bun pins ${expected_revision}, got ${actual_revision:-missing} in ${webkit_repo}"
fi

webkit_status="$(git -C "${webkit_repo}" status --short)"
if [[ -n "${webkit_status}" ]]; then
  printf 'WebKit proof worktree must be clean:\n%s\n' "${webkit_status}" >&2
  exit 1
fi

printf 'verified: WebKit source is clean at Bun pin %s\n' "${expected_revision}"
