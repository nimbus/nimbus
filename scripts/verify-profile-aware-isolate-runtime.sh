#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

printf 'Nimbus runtime-strategy completion gate\n\n'

if rg -n \
  'WarmContextRecycle|RuntimeNodeFullRealmReusePolicy|realm_lease|realm_lifecycle|extension_replay_(js|esm)_sources' \
  crates/nimbus-runtime crates/nimbus; then
  printf '\nFAIL rejected fresh-realm product symbols remain in the Nimbus release graph\n' >&2
  exit 1
fi
printf 'PASS rejected fresh-realm product symbols are absent\n'

bash scripts/verify-runtime-execution-classification.sh
bash scripts/verify-runtime-tenant-isolation.sh
bash scripts/verify-tenant-function-autoscaling.sh
python3 -m unittest scripts.test_verify_profile_aware_isolate_runtime_crossover_trace
TRACE_RUN_ID="$(python3 scripts/verify_profile_aware_isolate_runtime_crossover_trace.py \
  --trace docs/private/plans/proof/release-readiness-2026-08/artifacts/rrc8-u3-node22-hostless-crossover.jsonl \
  --benchmark-group runtime_pool_modes_pir0_profile_matrix \
  --profile node22 \
  --workload hostless_trivial \
  --execution-model cooperative_locker \
  --actual-construction-mode startup_snapshot \
  --startup-strategy-label startup_snapshot_cache \
  --print-run-id)"
printf 'PASS Node crossover trace uses run ID %s\n' "${TRACE_RUN_ID}"
python3 scripts/verify_profile_aware_isolate_runtime_crossover_trace.py \
  --trace docs/private/plans/proof/release-readiness-2026-08/artifacts/rrc8-u3-web-standard-hostless-crossover.jsonl \
  --benchmark-group runtime_pool_modes_web_selected \
  --profile web_standard \
  --workload hostless_trivial \
  --execution-model cooperative_locker \
  --actual-construction-mode unsnapshotted \
  --startup-strategy-label unsnapshotted_runtime_cache \
  --expected-run-id "${TRACE_RUN_ID}"

printf '\nPASS current runtime strategy, authority isolation, autoscaling, and crossover trace contracts\n'
