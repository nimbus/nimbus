#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." && pwd)"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/runtime/node/oracle-run.sh --lane <node20|node22|node24> --fixture <test-relative-path> [--output-root <path>] [--node-bin <path>]

Examples:
  bash scripts/runtime/node/oracle-run.sh --lane node22 --fixture test/parallel/test-buffer-alloc.js
  bash scripts/runtime/node/oracle-run.sh --lane node22 --fixture test/parallel/test-buffer-alloc.js --node-bin /opt/homebrew/bin/node
  bash scripts/runtime/node/oracle-run.sh --lane node20 --fixture test/parallel/test-buffer-alloc.js --output-root target/node-compat/oracle
EOF
}

lane=""
fixture=""
output_root=""
node_bin=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lane)
      lane="${2:?missing lane}"
      shift 2
      ;;
    --fixture)
      fixture="${2:?missing fixture path}"
      shift 2
      ;;
    --output-root)
      output_root="${2:?missing output root}"
      shift 2
      ;;
    --node-bin)
      node_bin="${2:?missing node binary path}"
      shift 2
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

[[ -n "${lane}" ]] || {
  echo "--lane is required" >&2
  usage >&2
  exit 1
}
[[ -n "${fixture}" ]] || {
  echo "--fixture is required" >&2
  usage >&2
  exit 1
}

if [[ -z "${output_root}" ]]; then
  output_root="${REPO_ROOT}/target/node-compat/oracle"
elif [[ "${output_root}" != /* ]]; then
  output_root="${REPO_ROOT}/${output_root}"
fi

if [[ -n "${node_bin}" && "${node_bin}" != /* ]]; then
  node_bin="${REPO_ROOT}/${node_bin}"
fi

echo "emitting node-compat oracle artifact for ${lane}:${fixture} into ${output_root}"

NEOVEX_NODE_COMPAT_ORACLE_LANE="${lane}" \
NEOVEX_NODE_COMPAT_ORACLE_FIXTURE="${fixture}" \
NEOVEX_NODE_COMPAT_ORACLE_OUTPUT_ROOT="${output_root}" \
NEOVEX_NODE_COMPAT_ORACLE_NODE_BIN="${node_bin}" \
bash "${REPO_ROOT}/scripts/single-flight.sh" \
  --key "node-compat-oracle-${lane}-$(echo "${fixture}" | tr '/.' '--')" \
  -- cargo test -p neovex-runtime \
    node_compat_oracle_entrypoint_emits_fixture_artifact \
    -- --ignored --nocapture --test-threads=1
