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
# shellcheck disable=SC2088 # Match Bun's literal ~/ expansion rule.
case "${webkit_repo}" in
  "~") webkit_repo="${HOME}" ;;
  "~/"*) webkit_repo="${HOME}/${webkit_repo#\~/}" ;;
  "~\\"*) webkit_repo="${HOME}/${webkit_repo#\~}" ;;
  /*) ;;
  *) webkit_repo="${bun_repo}/${webkit_repo}" ;;
esac
[[ -d "${webkit_repo}" ]] || die "WebKit source checkout not found: ${webkit_repo:-missing}"
webkit_repo="$(cd "${webkit_repo}" && pwd -L)"
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

if [[ ! "${webkit_version}" =~ ^[0-9a-f]{40}$ ]]; then
  die "Bun release proof requires an immutable 40-character WEBKIT_VERSION, got ${webkit_version}"
fi
expected_revision="${webkit_version}"

git_webkit() {
  GIT_NO_REPLACE_OBJECTS=1 git -C "${webkit_repo}" "$@"
}

if ! git_webkit cat-file -e "${expected_revision}^{commit}" 2>/dev/null; then
  die "WebKit checkout does not contain Bun pin ${expected_revision}: ${webkit_repo}"
fi

replacement_refs="$(git_webkit for-each-ref --format='%(refname)' refs/replace/)"
if [[ -n "${replacement_refs}" ]]; then
  printf 'WebKit proof worktree must not use replacement refs:\n%s\n' \
    "${replacement_refs}" >&2
  exit 1
fi

hidden_index_entries="$(
  git_webkit ls-files -v |
    awk 'substr($0, 1, 1) == "S" || substr($0, 1, 1) ~ /[a-z]/ { print }'
)"
if [[ -n "${hidden_index_entries}" ]]; then
  printf 'WebKit proof worktree must not use assume-unchanged or skip-worktree index flags:\n%s\n' \
    "${hidden_index_entries}" >&2
  exit 1
fi

actual_revision="$(git_webkit rev-parse HEAD 2>/dev/null || true)"
if [[ "${actual_revision}" != "${expected_revision}" ]]; then
  die "unexpected WebKit revision: Bun pins ${expected_revision}, got ${actual_revision:-missing} in ${webkit_repo}"
fi

webkit_status="$(
  GIT_NO_REPLACE_OBJECTS=1 git -c status.showUntrackedFiles=all -C "${webkit_repo}" \
    status --short --untracked-files=all --ignore-submodules=none
)"
if [[ -n "${webkit_status}" ]]; then
  printf 'WebKit proof worktree must be clean:\n%s\n' "${webkit_status}" >&2
  exit 1
fi

printf 'verified: WebKit source %s is clean at Bun pin %s\n' \
  "${webkit_repo}" "${expected_revision}"
