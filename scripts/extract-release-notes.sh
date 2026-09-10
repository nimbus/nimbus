#!/usr/bin/env bash
# Prints one release's section from CHANGELOG.md.
#
# CHANGELOG.md is the reviewed source of truth for what a release changed. It
# is written in the release preparation pull request, reviewed there, and
# frozen by the tag. The release workflow publishes the section this prints,
# so the notes on the GitHub release are the same words the pull request
# reviewed, and no bot rewrites them afterwards.
#
# The section runs from the `## [X.Y.Z] - DATE` heading to the next `## [`
# heading, with blank lines trimmed from both ends. An absent heading, or a
# heading with no content under it, is an error: the release must fail closed
# rather than publish empty notes.
#
# examples:
#   bash scripts/extract-release-notes.sh v0.1.9
#   bash scripts/extract-release-notes.sh 0.1.9
set -euo pipefail

usage() {
  cat <<'EOF'
usage: extract-release-notes.sh <tag-or-version> [--changelog <path>]

Prints the CHANGELOG.md section for the given release to stdout.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ $# -lt 1 ]]; then
  usage >&2
  exit 2
fi

expected_input="$1"
shift

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
changelog="${repo_root}/CHANGELOG.md"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --changelog)
      if [[ $# -lt 2 ]]; then
        printf 'error: --changelog needs a path\n' >&2
        exit 2
      fi
      changelog="$2"
      shift 2
      ;;
    *)
      printf 'error: unexpected argument %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

# Accept either the tag (v0.1.9) or the bare version (0.1.9).
expected_version="${expected_input#v}"
if [[ ! "${expected_version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  printf 'error: %s is not a release version\n' "${expected_input}" >&2
  exit 2
fi

if [[ ! -f "${changelog}" ]]; then
  printf 'error: %s does not exist\n' "${changelog}" >&2
  exit 1
fi

# `index($0, heading) == 1` matches the literal heading prefix, so the version
# needs no regular-expression escaping for its dots or brackets.
heading="## [${expected_version}] - "
if ! awk -v heading="${heading}" 'index($0, heading) == 1 { found = 1 } END { exit found ? 0 : 1 }' \
  "${changelog}"; then
  printf 'error: %s has no "%s..." heading for %s\n' \
    "${changelog}" "${heading}" "${expected_input}" >&2
  exit 1
fi

section="$(awk -v heading="${heading}" '
  index($0, heading) == 1 { collecting = 1; next }
  collecting && index($0, "## [") == 1 { exit }
  collecting { lines[++count] = $0 }
  END {
    first = 0
    last = 0
    for (i = 1; i <= count; i++) {
      if (lines[i] ~ /[^[:space:]]/) {
        if (first == 0) {
          first = i
        }
        last = i
      }
    }
    for (i = first; i <= last && first > 0; i++) {
      print lines[i]
    }
  }
' "${changelog}")"

if [[ -z "${section}" ]]; then
  printf 'error: %s has an empty section for %s\n' "${changelog}" "${expected_input}" >&2
  exit 1
fi

printf '%s\n' "${section}"
