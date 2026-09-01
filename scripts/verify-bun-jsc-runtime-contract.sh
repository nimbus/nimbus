#!/usr/bin/env bash
# Verifies the Nimbus-side Bun/JSC optional backend contract without building
# Bun itself. This is the fast CI lane for lazy execution, fail-closed adapter
# state, lane separation, memory semantics, and operator UI diagnostics.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "${REPO_ROOT}"

printf 'Bun/JSC runtime contract gate\n'
printf 'Nimbus repo: %s\n\n' "${REPO_ROOT}"

printf '[1/7] UI build prerequisites\n'
if [[ ! -f node_modules/.package-lock.json ]]; then
  npm ci
fi
make build-ui

printf '\n[2/7] Runtime policy and memory semantics\n'
cargo test -p nimbus-runtime limits::tests --lib

printf '\n[3/7] Bun/JSC pool scaffold contract\n'
cargo test -p nimbus-runtime backends::bun_jsc --lib

printf '\n[4/7] Convex runtime lane registry contract\n'
cargo test -p nimbus-server registry_and_license::registry --lib

printf '\n[5/7] Runtime diagnostics API contract\n'
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib

printf '\n[6/7] Tenant admission permits only the proven Bun/JSC profile\n'
tenant_admission_test="tests::production_untrusted_runtime_admission_allows_bun_jsc_fresh_discard_policy"
tenant_admission_inventory="$(cargo test -p nimbus-tenant --lib -- --list)"
grep -Fqx "${tenant_admission_test}: test" <<<"${tenant_admission_inventory}"
cargo test -p nimbus-tenant \
  --lib "${tenant_admission_test}" -- --exact

printf '\n[7/7] Operator UI runtime diagnostics contract\n'
npm run test --workspace nimbus-ui -- \
  src/test/msw.spec.ts \
  src/routes/operator/settings/configuration.spec.tsx

printf '\nBun/JSC runtime contract gate: pass\n'
