#!/usr/bin/env bash
# Mutation tests for the IMV7 performance proof parser.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"
cd "$ROOT" || { echo "cannot cd to repository root"; exit 2; }

PROOF="docs/private/plans/proof/incremental-materialized-verification"
VERIFIER="$PROOF/verify-imv7-performance.py"
FULL="$PROOF/imv7-raw.json"
CANDIDATE="$PROOF/imv7-candidate-raw.json"
TEMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEMP_ROOT"' EXIT

pass=0
fail=0

ok() { printf '  PASS  %s\n' "$1"; pass=$((pass + 1)); }
no() { printf '  FAIL  %s  [%s]\n' "$1" "$2"; fail=$((fail + 1)); }

expect_failure() {
  local label="$1" full="$2" candidate="$3" output status
  if output="$(python3 "$VERIFIER" "$full" "$candidate" 2>&1)"; then
    status=0
  else
    status=$?
  fi
  if [ "$status" -ne 0 ] \
    && [[ "$output" == IMV7\ performance\ proof\ invalid:* ]] \
    && [[ "$output" != *Traceback* ]]; then
    ok "$label fails closed without a traceback"
  else
    no "$label" "status=$status output=$output"
  fi
}

echo "IMV7 performance proof mutation tests"
echo "====================================="

if python3 "$VERIFIER" "$FULL" "$CANDIDATE" >/dev/null; then
  ok "accepted proof passes"
else
  no "accepted proof" "the retained artifact is invalid"
fi

python3 - "$CANDIDATE" "$TEMP_ROOT" <<'PY'
import copy
import json
import pathlib
import sys

source = json.load(open(sys.argv[1]))
root = pathlib.Path(sys.argv[2])

(root / "malformed.json").write_text("{", encoding="utf-8")

empty = copy.deepcopy(source)
empty["rungs"][0]["samples_ns"] = []
(root / "empty.json").write_text(json.dumps(empty), encoding="utf-8")

censored = copy.deepcopy(source)
censored["rungs"][0]["status"] = "resource_limited"
(root / "censored.json").write_text(json.dumps(censored), encoding="utf-8")

slow = copy.deepcopy(source)
million = next(rung for rung in slow["rungs"] if rung["documents"] == 1_000_000)
million["samples_ns"] = [60_000_000_001] * 21
million["summary"] = {
    "sample_count": 21,
    "p50_ns": 60_000_000_001,
    "p95_ns": 60_000_000_001,
    "p99_ns": 60_000_000_001,
}
(root / "slow.json").write_text(json.dumps(slow), encoding="utf-8")

large = copy.deepcopy(source)
million = next(rung for rung in large["rungs"] if rung["documents"] == 1_000_000)
million["resident_bytes"] = 192_000_001
million["resident_bytes_per_leaf"] = 193
(root / "large.json").write_text(json.dumps(large), encoding="utf-8")
PY

python3 - "$FULL" "$TEMP_ROOT" <<'PY'
import copy
import json
import pathlib
import sys

source = json.load(open(sys.argv[1]))
root = pathlib.Path(sys.argv[2])

truncated = copy.deepcopy(source)
decisive = next(
    row
    for row in truncated["matrix"]
    if (row["documents"], row["payload_bytes"], row["churn_basis_points"])
    == (100_000, 1_024, 10)
)
decisive["full"]["samples"] = decisive["full"]["samples"][:1]
sample = decisive["full"]["samples"][0]["elapsed_ns"]
decisive["full"]["summary"] = {
    "sample_count": 1,
    "p50_ns": sample,
    "p95_ns": sample,
    "p99_ns": sample,
}
(root / "truncated-full.json").write_text(json.dumps(truncated), encoding="utf-8")

wrong_host = copy.deepcopy(source)
wrong_host["target_arch"] = "x86_64"
(root / "wrong-full-host.json").write_text(json.dumps(wrong_host), encoding="utf-8")
PY

python3 - "$CANDIDATE" "$TEMP_ROOT" <<'PY'
import copy
import json
import pathlib
import sys

source = json.load(open(sys.argv[1]))
root = pathlib.Path(sys.argv[2])
wrong_host = copy.deepcopy(source)
wrong_host.pop("target_os")
(root / "missing-candidate-host.json").write_text(json.dumps(wrong_host), encoding="utf-8")
PY

expect_failure "malformed JSON" "$FULL" "$TEMP_ROOT/malformed.json"
expect_failure "empty candidate" "$FULL" "$TEMP_ROOT/empty.json"
expect_failure "censored candidate" "$FULL" "$TEMP_ROOT/censored.json"
expect_failure "slow candidate" "$FULL" "$TEMP_ROOT/slow.json"
expect_failure "high-memory candidate" "$FULL" "$TEMP_ROOT/large.json"
expect_failure "truncated decisive full samples" "$TEMP_ROOT/truncated-full.json" "$CANDIDATE"
expect_failure "wrong full-matrix host class" "$TEMP_ROOT/wrong-full-host.json" "$CANDIDATE"
expect_failure "missing candidate host class" "$FULL" "$TEMP_ROOT/missing-candidate-host.json"

echo "====================================="
echo "Summary: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
