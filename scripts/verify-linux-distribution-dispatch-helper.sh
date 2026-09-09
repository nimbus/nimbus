#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
workflow="${root}/.github/workflows/release.yml"
dispatches=0

# Execute the dispatch body with a recorder in place of the GitHub client.
gh() {
  dispatches=$((dispatches + 1))
  local expected=(workflow run linux-distribution-release.yml
    --repo nimbus/nimbus --ref main
    -f release_tag=v0.1.47 -f publish_apt_repo=true
    -f submit_to_copr=false -f include_bun_jsc_adapter=false)
  [[ $# -eq ${#expected[@]} ]] || return 1
  local argument
  for argument in "${expected[@]}"; do
    if [[ "$1" != "$argument" ]]; then
      printf 'FAIL: expected %s, got %s\n' "$argument" "$1" >&2
      return 1
    fi
    shift
  done
  printf '%s\n' 'PASS: main workflow ref with exact release tag and publication policy'
}

body=$(awk '
  /^  trigger-linux-distribution:/ {job = 1; next}
  job && /^  [^ ]/ {exit}
  job && /^        run: \|/ {body = 1; next}
  body {sub(/^          /, ""); print}
' "$workflow")
[[ -n "$body" ]] || { printf '%s\n' 'FAIL: missing dispatch body' >&2; exit 1; }
body=${body//\$\{\{ github.repository \}\}/nimbus\/nimbus}
body=${body//\$\{\{ github.ref_name \}\}/v0.1.47}
eval "$body"
[[ "$dispatches" -eq 1 ]] || { printf '%s\n' 'FAIL: expected one dispatch' >&2; exit 1; }
