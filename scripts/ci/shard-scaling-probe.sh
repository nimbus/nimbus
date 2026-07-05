#!/usr/bin/env bash

set -euo pipefail

if [[ "$#" -ne 2 ]]; then
  echo "usage: shard-scaling-probe.sh <k> <output-json>" >&2
  exit 2
fi

k="$1"
output_json="$2"

if ! [[ "${k}" =~ ^[1-9][0-9]*$ ]]; then
  echo "K must be a positive integer, got ${k}" >&2
  exit 2
fi

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "set ${name} before running shard-scaling-probe.sh" >&2
    exit 2
  fi
}

now_ms() {
  date +%s%3N
}

seconds_from_ms() {
  local millis="$1"
  python3 - "${millis}" <<'PY'
import sys
print(f"{int(sys.argv[1]) / 1000:.3f}")
PY
}

require_env NIMBUS_TESTS_ARCHIVE
require_env GITHUB_WORKSPACE

mkdir -p "$(dirname "${output_json}")" "${RUNNER_TEMP:-/tmp}/shard-scaling-probe"
rows_tsv="${RUNNER_TEMP:-/tmp}/shard-scaling-probe/k-${k}.tsv"
: > "${rows_tsv}"

download_extract_seconds="${NIMBUS_PROBE_DOWNLOAD_EXTRACT_SECONDS:-PENDING(not-measured)}"
filter_args=()
if [[ -n "${NODE_DEPENDENT_FILTER:-}" ]]; then
  filter_args=(-E "not (${NODE_DEPENDENT_FILTER})")
fi

any_failed=0
total_partition_ms=0
max_partition_ms=0

for i in $(seq 1 "${k}"); do
  partition="${i}/${k}"
  log_file="${RUNNER_TEMP:-/tmp}/shard-scaling-probe/k-${k}-partition-${i}.log"
  start_ms="$(now_ms)"
  set +e
  cargo-nextest nextest run \
    --archive-file "${NIMBUS_TESTS_ARCHIVE}" \
    --workspace-remap "${GITHUB_WORKSPACE}" \
    --profile ci-pr \
    --partition "hash:${partition}" \
    "${filter_args[@]}" 2>&1 | tee "${log_file}"
  status="${PIPESTATUS[0]}"
  set -e
  end_ms="$(now_ms)"
  wall_ms=$((end_ms - start_ms))
  total_partition_ms=$((total_partition_ms + wall_ms))
  if (( wall_ms > max_partition_ms )); then
    max_partition_ms="${wall_ms}"
  fi
  if (( status != 0 )); then
    any_failed=1
  fi
  printf '%s\t%s\t%s\t%s\n' "${partition}" "$(seconds_from_ms "${wall_ms}")" "${status}" "${log_file}" >> "${rows_tsv}"
done

python3 - "${k}" "${download_extract_seconds}" "${rows_tsv}" "${output_json}" <<'PY'
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

k = int(sys.argv[1])
download_extract = sys.argv[2]
rows_path = Path(sys.argv[3])
output_path = Path(sys.argv[4])

partitions = []
for line in rows_path.read_text(encoding="utf-8").splitlines():
    partition, wall, status, log_file = line.split("\t", 3)
    partitions.append(
        {
            "partition": partition,
            "wall_seconds": float(wall),
            "status": int(status),
            "log_file": log_file,
        }
    )

max_wall = max((row["wall_seconds"] for row in partitions), default=0.0)
partition_total = sum(row["wall_seconds"] for row in partitions)
try:
    restore = float(download_extract)
except ValueError:
    restore = None
total = partition_total + (restore if restore is not None else 0.0)

payload = {
    "schema_version": 1,
    "workflow": "shard-scaling-probe",
    "k": k,
    "run_id": os.environ.get("GITHUB_RUN_ID", ""),
    "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", ""),
    "git_sha": os.environ.get("GITHUB_SHA", ""),
    "started_at": datetime.now(timezone.utc).isoformat(),
    "download_extract_seconds": restore,
    "download_extract_raw": download_extract,
    "partitions": partitions,
    "max_partition_seconds": max_wall,
    "partition_total_seconds": partition_total,
    "total_seconds": total,
}
output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

per_partition="$(
  awk -F '\t' '{
    if (NR > 1) printf "; ";
    printf "%s=%ss", $1, $2
  }' "${rows_tsv}"
)"
max_partition="$(seconds_from_ms "${max_partition_ms}")"
partition_total="$(seconds_from_ms "${total_partition_ms}")"
if [[ "${download_extract_seconds}" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  total="$(python3 - "${partition_total}" "${download_extract_seconds}" <<'PY'
import sys
print(f"{float(sys.argv[1]) + float(sys.argv[2]):.3f}")
PY
)"
else
  total="${partition_total}+${download_extract_seconds}"
fi

{
  echo "## Shard scaling probe K=${k}"
  echo
  echo "| K | per-partition wall | download+extract | max-partition wall | total |"
  echo "|---|---|---|---|---|"
  echo "| ${k} | ${per_partition} | ${download_extract_seconds}s | ${max_partition}s | ${total}s |"
} >> "${GITHUB_STEP_SUMMARY:-/dev/stdout}"

if (( any_failed != 0 )); then
  echo "one or more shard scaling probe partitions failed for K=${k}" >&2
  exit 1
fi
