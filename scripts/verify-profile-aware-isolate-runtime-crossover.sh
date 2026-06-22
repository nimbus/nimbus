#!/usr/bin/env bash
# Scheduled/manual CI smoke gate for PIR0/PIR2 runtime-pool crossover benches.
#
# This intentionally runs a tiny Criterion sample instead of the full PIR0
# matrix. The goal is to keep benchmark rows compiling and executing in hosted
# CI so snapshot-vs-warm and context-recycle crossover drift is visible without
# putting timing-sensitive Criterion work on every PR.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

SAMPLE_SIZE="${NIMBUS_PIR_CROSSOVER_SAMPLE_SIZE:-10}"
MEASUREMENT_TIME="${NIMBUS_PIR_CROSSOVER_MEASUREMENT_TIME:-1}"
WARM_UP_TIME="${NIMBUS_PIR_CROSSOVER_WARM_UP_TIME:-1}"

if [[ -n "${NIMBUS_PIR_CROSSOVER_TRACE_DIR:-}" ]]; then
  TRACE_DIR="${NIMBUS_PIR_CROSSOVER_TRACE_DIR}"
  mkdir -p "${TRACE_DIR}"
else
  TMP_ROOT="${TMPDIR:-/tmp}"
  TRACE_DIR="$(mktemp -d "${TMP_ROOT%/}/nimbus-pir-crossover.XXXXXX")"
fi

printf 'Profile-aware isolate runtime crossover smoke\n'
printf 'Nimbus repo: %s\n' "${REPO_ROOT}"
printf 'Trace directory: %s\n' "${TRACE_DIR}"
printf 'Criterion bounds: sample-size=%s measurement-time=%ss warm-up-time=%ss\n\n' \
  "${SAMPLE_SIZE}" "${MEASUREMENT_TIME}" "${WARM_UP_TIME}"

run_bench() {
  local label="$1"
  local filter="$2"
  local trace_path="$3"

  printf '[%s] %s\n' "${label}" "${filter}"
  NIMBUS_PIR0_TRACE_PATH="${trace_path}" \
    cargo bench -p nimbus-runtime --bench runtime_pool_modes -- \
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

PIR0_TRACE="${TRACE_DIR}/pir0-node22-hostless-crossover.jsonl"
PIR2_TRACE="${TRACE_DIR}/pir2-web-context-recycle-crossover.jsonl"

run_bench \
  "1/2 PIR0 Node22 snapshot-vs-warm crossover" \
  "runtime_pool_modes_pir0_profile_matrix/node22/hostless_trivial/cooperative_locker" \
  "${PIR0_TRACE}"

grep -F '"benchmark_group":"runtime_pool_modes_pir0_profile_matrix"' "${PIR0_TRACE}" >/dev/null
grep -F '"profile":"node22"' "${PIR0_TRACE}" >/dev/null
grep -F '"pool_kind":"startup_snapshot_cache"' "${PIR0_TRACE}" >/dev/null
grep -F '"pool_kind":"warm_pool"' "${PIR0_TRACE}" >/dev/null

run_bench \
  "2/2 PIR2 WebStandard context-recycle crossover" \
  "runtime_pool_modes_pir2_context_recycle_impact/web_standard/hostless_trivial/cooperative_locker" \
  "${PIR2_TRACE}"

grep -F '"benchmark_group":"runtime_pool_modes_pir2_context_recycle_impact"' "${PIR2_TRACE}" >/dev/null
grep -F '"profile":"web_standard"' "${PIR2_TRACE}" >/dev/null
grep -F '"pool_kind":"startup_snapshot_cache"' "${PIR2_TRACE}" >/dev/null
grep -F '"pool_kind":"warm_pool"' "${PIR2_TRACE}" >/dev/null
grep -F '"pool_kind":"warm_context_recycle"' "${PIR2_TRACE}" >/dev/null

printf 'Profile-aware isolate runtime crossover smoke: pass\n'
