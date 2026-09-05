#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bun-webkit-source-helper.XXXXXX")"
tmp_root="$(cd "${tmp_root}" && pwd -L)"
trap 'rm -rf "${tmp_root}"' EXIT

bun_repo="${tmp_root}/bun"
webkit_repo="${tmp_root}/WebKit"
mkdir -p "${bun_repo}/scripts/build/deps" "${webkit_repo}"

git -C "${webkit_repo}" init -q
git -C "${webkit_repo}" config user.name "Nimbus Test"
git -C "${webkit_repo}" config user.email "nimbus-test@example.invalid"
printf 'first\n' >"${webkit_repo}/source.txt"
git -C "${webkit_repo}" add source.txt
git -C "${webkit_repo}" commit -q --no-gpg-sign -m "first"
pinned_revision="$(git -C "${webkit_repo}" rev-parse HEAD)"

printf 'export const WEBKIT_VERSION = "%s";\n' "${pinned_revision}" \
  >"${bun_repo}/scripts/build/deps/webkit.ts"

bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/pass.out"
grep -F "verified: WebKit source ${webkit_repo} is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/pass.out" >/dev/null

bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "../WebKit" \
  >"${tmp_root}/relative.out"
grep -F "WebKit source ${webkit_repo} is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/relative.out" >/dev/null

# shellcheck disable=SC2088 # Exercise a literal ~/ value from BUN_WEBKIT_PATH.
HOME="${tmp_root}" bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "~/WebKit" \
  >"${tmp_root}/tilde.out"
grep -F "WebKit source ${webkit_repo} is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/tilde.out" >/dev/null

# shellcheck disable=SC2088 # Exercise Bun's exact literal ~ rule.
HOME="${webkit_repo}" bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "~" \
  >"${tmp_root}/home.out"
grep -F "WebKit source ${webkit_repo} is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/home.out" >/dev/null

ln -s "WebKit" "${tmp_root}/\\WebKit"
# shellcheck disable=SC2088 # Exercise Bun's literal ~\ path rule.
HOME="${tmp_root}" bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "~\\WebKit" \
  >"${tmp_root}/backslash-home.out"
grep -F "is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/backslash-home.out" >/dev/null

mkdir -p "${bun_repo}/vendor" "${tmp_root}/src/github.com/oven-sh/WebKit"
ln -s "../../WebKit" "${bun_repo}/vendor/WebKit"
env -u BUN_WEBKIT_PATH \
  HOME="${tmp_root}" \
  NIMBUS_BUN_REPO="${bun_repo}" \
  make -s -C "${repo_root}" verify-bun-webkit-source \
  >"${tmp_root}/make-unset.out"
grep -F "is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/make-unset.out" >/dev/null

HOME="${tmp_root}" \
  NIMBUS_BUN_REPO="${bun_repo}" \
  BUN_WEBKIT_PATH="" \
  make -s -C "${repo_root}" verify-bun-webkit-source \
  >"${tmp_root}/make-empty.out"
grep -F "is clean at Bun pin ${pinned_revision}" \
  "${tmp_root}/make-empty.out" >/dev/null

printf 'dirty\n' >"${webkit_repo}/dirty.txt"
git -C "${webkit_repo}" config status.showUntrackedFiles no
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/dirty.out" 2>"${tmp_root}/dirty.err"; then
  printf 'dirty WebKit checkout unexpectedly passed\n' >&2
  exit 1
fi
grep -F "WebKit proof worktree must be clean" "${tmp_root}/dirty.err" >/dev/null
git -C "${webkit_repo}" config --unset status.showUntrackedFiles
rm "${webkit_repo}/dirty.txt"

printf 'second\n' >>"${webkit_repo}/source.txt"
git -C "${webkit_repo}" add source.txt
git -C "${webkit_repo}" commit -q --no-gpg-sign -m "second"
second_revision="$(git -C "${webkit_repo}" rev-parse HEAD)"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/stale.out" 2>"${tmp_root}/stale.err"; then
  printf 'stale WebKit checkout unexpectedly passed\n' >&2
  exit 1
fi
grep -F "unexpected WebKit revision: Bun pins ${pinned_revision}" \
  "${tmp_root}/stale.err" >/dev/null

git -C "${webkit_repo}" checkout -q "${pinned_revision}"
git -C "${webkit_repo}" update-index --assume-unchanged source.txt
printf 'hidden by assume-unchanged\n' >"${webkit_repo}/source.txt"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/assume.out" 2>"${tmp_root}/assume.err"; then
  printf 'assume-unchanged WebKit source unexpectedly passed\n' >&2
  exit 1
fi
grep -F "must not use assume-unchanged or skip-worktree index flags" \
  "${tmp_root}/assume.err" >/dev/null
git -C "${webkit_repo}" update-index --no-assume-unchanged source.txt
git -C "${webkit_repo}" checkout -q -- source.txt

git -C "${webkit_repo}" update-index --skip-worktree source.txt
printf 'hidden by skip-worktree\n' >"${webkit_repo}/source.txt"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/skip.out" 2>"${tmp_root}/skip.err"; then
  printf 'skip-worktree WebKit source unexpectedly passed\n' >&2
  exit 1
fi
grep -F "must not use assume-unchanged or skip-worktree index flags" \
  "${tmp_root}/skip.err" >/dev/null
git -C "${webkit_repo}" update-index --no-skip-worktree source.txt
git -C "${webkit_repo}" checkout -q -- source.txt

git -C "${webkit_repo}" replace "${pinned_revision}" "${second_revision}"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/replace.out" 2>"${tmp_root}/replace.err"; then
  printf 'replacement-ref WebKit source unexpectedly passed\n' >&2
  exit 1
fi
grep -F "WebKit proof worktree must not use replacement refs" \
  "${tmp_root}/replace.err" >/dev/null
git -C "${webkit_repo}" replace -d "${pinned_revision}" >/dev/null

git -C "${webkit_repo}" tag autobuild-test "${pinned_revision}"
printf 'export const WEBKIT_VERSION = "autobuild-test";\n' \
  >"${bun_repo}/scripts/build/deps/webkit.ts"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/symbolic.out" 2>"${tmp_root}/symbolic.err"; then
  printf 'symbolic WebKit pin unexpectedly passed\n' >&2
  exit 1
fi
grep -F "requires an immutable 40-character WEBKIT_VERSION" \
  "${tmp_root}/symbolic.err" >/dev/null

missing_revision="ffffffffffffffffffffffffffffffffffffffff"
printf 'export const WEBKIT_VERSION = "%s";\n' "${missing_revision}" \
  >"${bun_repo}/scripts/build/deps/webkit.ts"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/missing.out" 2>"${tmp_root}/missing.err"; then
  printf 'missing WebKit revision unexpectedly passed\n' >&2
  exit 1
fi
grep -F "WebKit checkout does not contain Bun pin ${missing_revision}" \
  "${tmp_root}/missing.err" >/dev/null

printf 'export const WRONG_WEBKIT_VERSION = "%s";\n' "${pinned_revision}" \
  >"${bun_repo}/scripts/build/deps/webkit.ts"
if bash "${repo_root}/scripts/verify-bun-webkit-source.sh" \
  --bun-repo "${bun_repo}" \
  --webkit-repo "${webkit_repo}" \
  >"${tmp_root}/malformed.out" 2>"${tmp_root}/malformed.err"; then
  printf 'missing WebKit pin unexpectedly passed\n' >&2
  exit 1
fi
grep -F "expected exactly one WEBKIT_VERSION" "${tmp_root}/malformed.err" >/dev/null

printf 'verified: Bun WebKit source proof rejects dirty, stale, hidden, replaced, symbolic, missing, and malformed inputs\n'
