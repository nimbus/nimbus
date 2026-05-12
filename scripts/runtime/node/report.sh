#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." && pwd)"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/runtime/node/report.sh --family <family-id> --slice <slice-id> [--output-root <path>] [--observed-results <path>] [--capture-live]

Examples:
  bash scripts/runtime/node/report.sh --family networking --slice dns-net-foundation
  bash scripts/runtime/node/report.sh --family networking --slice dns-net-foundation --observed-results /tmp/networking-results.json
  bash scripts/runtime/node/report.sh --family networking --slice dns-net-foundation --capture-live
  bash scripts/runtime/node/report.sh --family loader-context --slice module-and-async-foundation --output-root target/node-compat
EOF
}

family=""
slice=""
output_root=""
observed_results=""
capture_mode="seeded"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --family)
      family="${2:?missing family id}"
      shift 2
      ;;
    --slice)
      slice="${2:?missing slice id}"
      shift 2
      ;;
    --output-root)
      output_root="${2:?missing output root}"
      shift 2
      ;;
    --observed-results)
      observed_results="${2:?missing observed results path}"
      shift 2
      ;;
    --capture-live)
      capture_mode="live"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

[[ -n "${family}" ]] || {
  echo "--family is required" >&2
  usage >&2
  exit 1
}
[[ -n "${slice}" ]] || {
  echo "--slice is required" >&2
  usage >&2
  exit 1
}

if [[ -z "${output_root}" ]]; then
  output_root="${REPO_ROOT}/target/node-compat"
elif [[ "${output_root}" != /* ]]; then
  output_root="${REPO_ROOT}/${output_root}"
fi

if [[ -n "${observed_results}" && "${observed_results}" != /* ]]; then
  observed_results="${REPO_ROOT}/${observed_results}"
fi

if [[ "${capture_mode}" == "live" && -n "${observed_results}" ]]; then
  echo "--capture-live cannot be combined with --observed-results" >&2
  exit 1
fi

echo "emitting node-compat report artifacts for ${family}:${slice} into ${output_root} (mode=${capture_mode})"

NIMBUS_NODE_COMPAT_REPORT_FAMILY="${family}" \
NIMBUS_NODE_COMPAT_REPORT_SLICE="${slice}" \
NIMBUS_NODE_COMPAT_REPORT_OUTPUT_ROOT="${output_root}" \
NIMBUS_NODE_COMPAT_REPORT_CAPTURE_MODE="${capture_mode}" \
NIMBUS_NODE_COMPAT_REPORT_OBSERVED_RESULTS="${observed_results}" \
bash "${REPO_ROOT}/scripts/single-flight.sh" \
  --key "node-compat-report-${family}-${slice}" \
  -- cargo test -p nimbus-runtime \
    node_compat_manifest_report_entrypoint_emits_slice_artifacts \
    -- --ignored --nocapture --test-threads=1
