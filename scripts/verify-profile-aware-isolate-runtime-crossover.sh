#!/usr/bin/env bash
# Scheduled/manual CI smoke gate for the selected runtime-pool crossover benches.
#
# This intentionally runs a tiny Criterion sample instead of the full PIR0
# matrix. The goal is to keep the selected Node and Web rows compiling and
# executing in hosted CI without putting timing-sensitive Criterion work on
# every PR.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

SAMPLE_SIZE="${NIMBUS_PIR_CROSSOVER_SAMPLE_SIZE:-10}"
MEASUREMENT_TIME="${NIMBUS_PIR_CROSSOVER_MEASUREMENT_TIME:-1}"
WARM_UP_TIME="${NIMBUS_PIR_CROSSOVER_WARM_UP_TIME:-1}"
PROOF_DIR="${NIMBUS_PIR_CROSSOVER_PROOF_DIR:-}"
CARGO_CONFIG_ARGS=()
if [[ -n "${NIMBUS_PIR_CROSSOVER_CARGO_CONFIG:-}" ]]; then
  if [[ ! -f "${NIMBUS_PIR_CROSSOVER_CARGO_CONFIG}" ]]; then
    printf 'Cargo config does not exist: %s\n' \
      "${NIMBUS_PIR_CROSSOVER_CARGO_CONFIG}" >&2
    exit 1
  fi
  CARGO_CONFIG_ARGS=(--config "${NIMBUS_PIR_CROSSOVER_CARGO_CONFIG}")
fi

if [[ -n "${NIMBUS_PIR_CROSSOVER_TRACE_DIR:-}" ]]; then
  TRACE_ROOT="${NIMBUS_PIR_CROSSOVER_TRACE_DIR}"
  mkdir -p "${TRACE_ROOT}"
else
  TRACE_ROOT="${TMPDIR:-/tmp}"
fi
TRACE_DIR="$(mktemp -d "${TRACE_ROOT%/}/nimbus-pir-crossover.XXXXXX")"
TRACE_RUN_ID="${TRACE_DIR##*/}"

printf 'Profile-aware isolate runtime crossover smoke\n'
printf 'Nimbus repo: %s\n' "${REPO_ROOT}"
printf 'Trace directory: %s\n' "${TRACE_DIR}"
printf 'Trace run ID: %s\n' "${TRACE_RUN_ID}"
printf 'Cargo config: %s\n' "${NIMBUS_PIR_CROSSOVER_CARGO_CONFIG:-repository default}"
printf 'Criterion bounds: sample-size=%s measurement-time=%ss warm-up-time=%ss\n\n' \
  "${SAMPLE_SIZE}" "${MEASUREMENT_TIME}" "${WARM_UP_TIME}"

run_bench() {
  local label="$1"
  local filter="$2"
  local trace_path="$3"

  printf '[%s] %s\n' "${label}" "${filter}"
  NIMBUS_PIR0_TRACE_PATH="${trace_path}" \
  NIMBUS_PIR0_TRACE_RUN_ID="${TRACE_RUN_ID}" \
    cargo bench "${CARGO_CONFIG_ARGS[@]}" \
      -p nimbus-runtime --bench runtime_pool_modes -- \
      "${filter}" \
      --sample-size "${SAMPLE_SIZE}" \
      --measurement-time "${MEASUREMENT_TIME}" \
      --warm-up-time "${WARM_UP_TIME}"

  if [[ ! -s "${trace_path}" ]]; then
    printf 'expected non-empty PIR trace at %s\n' "${trace_path}" >&2
    exit 1
  fi
  printf 'trace: %s\n\n' "${trace_path}"
}

validate_trace() {
  local trace_path="$1"
  local benchmark_group="$2"
  local profile="$3"
  local actual_construction_mode="$4"
  local startup_strategy_label="$5"
  local expected_run_id="$6"

  python3 scripts/verify_profile_aware_isolate_runtime_crossover_trace.py \
    --trace "${trace_path}" \
    --benchmark-group "${benchmark_group}" \
    --profile "${profile}" \
    --workload hostless_trivial \
    --execution-model cooperative_locker \
    --actual-construction-mode "${actual_construction_mode}" \
    --startup-strategy-label "${startup_strategy_label}" \
    --expected-run-id "${expected_run_id}"
}

NODE_TRACE="${TRACE_DIR}/node22-hostless-crossover.jsonl"
WEB_TRACE="${TRACE_DIR}/web-standard-hostless-crossover.jsonl"

run_bench \
  "1/2 PIR0 Node22 snapshot-vs-warm crossover" \
  "runtime_pool_modes_pir0_profile_matrix/node22/hostless_trivial/cooperative_locker" \
  "${NODE_TRACE}"

validate_trace \
  "${NODE_TRACE}" \
  runtime_pool_modes_pir0_profile_matrix \
  node22 \
  startup_snapshot \
  startup_snapshot_cache \
  "${TRACE_RUN_ID}"

run_bench \
  "2/2 WebStandard unsnapshotted-cache-vs-warm crossover" \
  "runtime_pool_modes_web_selected/web_standard/hostless_trivial/cooperative_locker" \
  "${WEB_TRACE}"

validate_trace \
  "${WEB_TRACE}" \
  runtime_pool_modes_web_selected \
  web_standard \
  unsnapshotted \
  unsnapshotted_runtime_cache \
  "${TRACE_RUN_ID}"

if [[ -n "${PROOF_DIR}" ]]; then
  mkdir -p "${PROOF_DIR}"
  install -m 0644 \
    "${NODE_TRACE}" \
    "${PROOF_DIR}/rrc8-u3-node22-hostless-crossover.jsonl"
  install -m 0644 \
    "${WEB_TRACE}" \
    "${PROOF_DIR}/rrc8-u3-web-standard-hostless-crossover.jsonl"
  printf 'proof traces: %s\n' "${PROOF_DIR}"
fi

printf 'Profile-aware isolate runtime crossover smoke: pass\n'
