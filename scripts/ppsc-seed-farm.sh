#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_name="tests::ppsc::seed_farm::ppsc_seed_farm_executes_selected_redb_scenarios"
filter_expr="package(nimbus-engine) and test(/^${test_name}$/)"
artifact_dir="${NIMBUS_PPSC_FAILURE_DIR:?NIMBUS_PPSC_FAILURE_DIR must identify the seed-farm artifact directory}"
if [[ "${artifact_dir}" != /* ]]; then
  artifact_dir="${repo_root}/${artifact_dir}"
fi
export NIMBUS_PPSC_FAILURE_DIR="${artifact_dir}"
cargo_bin="${NIMBUS_PPSC_CARGO_BIN:-cargo}"
mkdir -p "${artifact_dir}"

if [[ -n "${NIMBUS_PPSC_ARCHIVE_FILE:-}" ]]; then
  nextest_bin="${NIMBUS_CARGO_NEXTEST_BIN:-cargo-nextest}"
  list_output="$(
    "${nextest_bin}" nextest list \
      --archive-file "${NIMBUS_PPSC_ARCHIVE_FILE}" \
      --workspace-remap "${GITHUB_WORKSPACE:-${PWD}}" \
      --profile ci-ppsc-seed-farm \
      --run-ignored only \
      -E "${filter_expr}" \
      --message-format json
  )"
  selected="$(
    python3 -c '
import json
import re
import sys

text = sys.stdin.read()
match = re.search(r"^\{", text, re.MULTILINE)
if not match:
    raise SystemExit("PPSC seed-farm nextest list did not emit JSON")
data = json.loads(text[match.start():])
count = 0
for suite in data.get("rust-suites", {}).values():
    for case in suite.get("testcases", {}).values():
        status = case.get("filter-match", {}).get("status")
        if status in (None, "matches"):
            count += 1
print(count)
' <<<"${list_output}"
  )"
  if [[ "${selected}" -ne 1 ]]; then
    echo "PPSC seed-farm archive filter must select exactly one ignored driver (selected ${selected})" >&2
    exit 1
  fi
  exec "${nextest_bin}" nextest run \
    --archive-file "${NIMBUS_PPSC_ARCHIVE_FILE}" \
    --workspace-remap "${GITHUB_WORKSPACE:-${PWD}}" \
    --profile ci-ppsc-seed-farm \
    --run-ignored only \
    --no-tests fail \
    -E "${filter_expr}"
fi

list_output="$("${cargo_bin}" test -p nimbus-engine "${test_name}" -- --ignored --exact --list)"
selected="$(awk '/: test$/{count++} END{print count+0}' <<<"${list_output}")"
if [[ "${selected}" -ne 1 ]]; then
  echo "PPSC seed-farm local filter must select exactly one ignored driver (selected ${selected})" >&2
  exit 1
fi
exec "${cargo_bin}" test -p nimbus-engine "${test_name}" -- \
  --ignored \
  --exact \
  --nocapture \
  --test-threads=1
