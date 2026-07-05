#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/ci/disk-guard.sh report [--target-dir DIR] [--report-file FILE] [--label LABEL]
  scripts/ci/disk-guard.sh enforce [--min-free-gib N] [--max-used-gib N] (at least one) [--target-dir DIR] [--report-file FILE] [--label LABEL]

Report mode prints df/du diagnostics and appends a sample to target/disk-report.json.
Enforce mode records the same report, then exits 28 if free space or target usage
breaches the supplied GiB thresholds.
USAGE
}

mode="${1:-report}"
if [[ "${mode}" == "report" || "${mode}" == "enforce" ]]; then
  shift || true
else
  mode="report"
fi

target_dir="target"
report_file=""
label=""
min_free_gib=""
max_used_gib=""

while (($#)); do
  case "$1" in
    --target-dir)
      target_dir="${2:?--target-dir requires a value}"
      shift 2
      ;;
    --report-file)
      report_file="${2:?--report-file requires a value}"
      shift 2
      ;;
    --label)
      label="${2:?--label requires a value}"
      shift 2
      ;;
    --min-free-gib)
      min_free_gib="${2:?--min-free-gib requires a value}"
      shift 2
      ;;
    --max-used-gib)
      max_used_gib="${2:?--max-used-gib requires a value}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

# At least one threshold must be set; each is enforced independently when given.
if [[ "${mode}" == "enforce" && -z "${min_free_gib}" && -z "${max_used_gib}" ]]; then
  printf 'enforce mode requires --min-free-gib and/or --max-used-gib\n' >&2
  exit 2
fi

mkdir -p "${target_dir}"
if [[ -z "${report_file}" ]]; then
  report_file="${target_dir}/disk-report.json"
fi
mkdir -p "$(dirname "${report_file}")"

if [[ -z "${label}" ]]; then
  label="${GITHUB_JOB:-local}-$(date -u +%Y%m%dT%H%M%SZ)"
fi

printf '== disk guard: %s (%s) ==\n' "${label}" "${mode}"
printf '== df -h %s ==\n' "${target_dir}"
df -h "${target_dir}"

du_tmp="$(mktemp "${TMPDIR:-/tmp}/nimbus-du.XXXXXX")"
du_sorted_tmp="$(mktemp "${TMPDIR:-/tmp}/nimbus-du-sorted.XXXXXX")"
if du -h -d2 "${target_dir}" >"${du_tmp}" 2>/dev/null; then
  printf '== du -h -d2 %s (top 30) ==\n' "${target_dir}"
  sort -hr "${du_tmp}" >"${du_sorted_tmp}"
  head -30 "${du_sorted_tmp}"
else
  printf 'warning: unable to collect du -h -d2 for %s\n' "${target_dir}" >&2
fi
rm -f "${du_tmp}" "${du_sorted_tmp}"

files_tmp="$(mktemp "${TMPDIR:-/tmp}/nimbus-files.XXXXXX")"
files_sorted_tmp="$(mktemp "${TMPDIR:-/tmp}/nimbus-files-sorted.XXXXXX")"
find "${target_dir}" -type f -printf '%s\t%p\n' >"${files_tmp}" 2>/dev/null || true
printf '== largest files under %s (top 30) ==\n' "${target_dir}"
sort -nr "${files_tmp}" >"${files_sorted_tmp}"
head -30 "${files_sorted_tmp}" | while IFS=$'\t' read -r bytes path; do
  [[ -n "${bytes}" ]] || continue
  if command -v numfmt >/dev/null 2>&1; then
    human="$(numfmt --to=iec --suffix=B "${bytes}")"
    printf '%8s\t%s\n' "${human}" "${path}"
  else
    printf '%s\t%s\n' "${bytes}" "${path}"
  fi
done
rm -f "${files_tmp}" "${files_sorted_tmp}"

TARGET_DIR="${target_dir}" REPORT_FILE="${report_file}" LABEL="${label}" MODE="${mode}" python3 - <<'PY'
import datetime as dt
import json
import os
import subprocess
from pathlib import Path


def run(command):
    completed = subprocess.run(command, check=False, text=True, capture_output=True)
    return {
        "command": command,
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def parse_df_kib(output):
    lines = [line.split() for line in output.splitlines() if line.strip()]
    if len(lines) < 2 or len(lines[1]) < 6:
        return {}
    row = lines[1]
    return {
        "filesystem": row[0],
        "size_bytes": int(row[1]) * 1024,
        "used_bytes": int(row[2]) * 1024,
        "available_bytes": int(row[3]) * 1024,
        "capacity": row[4],
        "mounted_on": row[5],
    }


def parse_du_kib(output):
    entries = []
    for line in output.splitlines():
        if not line.strip():
            continue
        size, _, path = line.partition("\t")
        if not path:
            parts = line.split(maxsplit=1)
            if len(parts) != 2:
                continue
            size, path = parts
        try:
            bytes_used = int(size) * 1024
        except ValueError:
            continue
        entries.append({"path": path, "bytes": bytes_used})
    return sorted(entries, key=lambda entry: (-entry["bytes"], entry["path"]))


def parse_file_sizes(output):
    entries = []
    for line in output.splitlines():
        if not line.strip():
            continue
        size, _, path = line.partition("\t")
        try:
            bytes_used = int(size)
        except ValueError:
            continue
        entries.append({"path": path, "bytes": bytes_used})
    return sorted(entries, key=lambda entry: (-entry["bytes"], entry["path"]))[:30]


target_dir = Path(os.environ["TARGET_DIR"])
report_file = Path(os.environ["REPORT_FILE"])
df_h = run(["df", "-h", str(target_dir)])
df_k = run(["df", "-Pk", str(target_dir)])
du_h = run(["du", "-h", "-d2", str(target_dir)])
du_k = run(["du", "-k", "-d", "2", str(target_dir)])
files = run(["find", str(target_dir), "-type", "f", "-printf", "%s\t%p\n"])

sample = {
    "label": os.environ["LABEL"],
    "mode": os.environ["MODE"],
    "created_at": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
    "target_dir": str(target_dir),
    "filesystem": parse_df_kib(df_k["stdout"]),
    "target_depth2": parse_du_kib(du_k["stdout"]),
    "largest_files": parse_file_sizes(files["stdout"]),
    "commands": {
        "df_h": df_h,
        "du_h_d2": du_h,
    },
}

if report_file.exists():
    try:
        report = json.loads(report_file.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        report = {"schema_version": 1, "samples": []}
else:
    report = {"schema_version": 1, "samples": []}

samples = report.setdefault("samples", [])
if not isinstance(samples, list):
    report["samples"] = samples = []
samples.append(sample)
report_file.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"wrote {report_file}")
PY

if [[ "${mode}" == "enforce" ]]; then
  free_kib="$(df -Pk "${target_dir}" | awk 'NR == 2 { print $4 }')"
  used_kib="$(du -sk "${target_dir}" | awk '{ print $1 }')"
  if [[ -n "${min_free_gib}" ]] && ! awk -v free_kib="${free_kib}" -v min_gib="${min_free_gib}" 'BEGIN { exit (free_kib >= min_gib * 1024 * 1024) ? 0 : 1 }'; then
    printf '::error::disk guard breach: free space %.2f GiB is below required %.2f GiB\n' \
      "$(awk -v kib="${free_kib}" 'BEGIN { printf "%.2f", kib / 1024 / 1024 }')" \
      "${min_free_gib}" >&2
    exit 28
  fi
  if [[ -n "${max_used_gib}" ]] && ! awk -v used_kib="${used_kib}" -v max_gib="${max_used_gib}" 'BEGIN { exit (used_kib <= max_gib * 1024 * 1024) ? 0 : 1 }'; then
    printf '::error::disk guard breach: target usage %.2f GiB exceeds allowed %.2f GiB\n' \
      "$(awk -v kib="${used_kib}" 'BEGIN { printf "%.2f", kib / 1024 / 1024 }')" \
      "${max_used_gib}" >&2
    exit 28
  fi
fi
