#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Function Source Visibility plan
# (`docs/private/plans/nimbus-function-source-visibility-plan.md`).
#
# Ships in FSV0 so the plan can be audited from day one. Most conditions are
# expected to FAIL until FSV1-FSV7 land.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/nimbus-function-source-visibility-plan.md"
SYS_SCHEMA="crates/nimbus-system/src/schema.rs"
SYS_RECORDS="crates/nimbus-system/src/records.rs"
UI_SCHEMA="packages/nimbus-ui/convex/schema.ts"
UI_GEN_API="packages/nimbus-ui/convex/_generated/api.ts"
DEPLOY_CLIENT="crates/nimbus-bin/src/deploy.rs"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() { PASS=$((PASS + 1)); printf '  \033[32mPASS\033[0m  %s\n' "$1"; }
fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  [ $# -ge 2 ] && printf '        %s\n' "$2"
  FAIL_DETAIL+=("$1")
}
step() { printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"; }

printf '\033[1mFSV verification gate - function source visibility\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step FSV0 "Foundations + honest cleanup"
[ -f "${PLAN}" ] && pass "plan doc present" || fail "plan doc missing" "${PLAN}"
if grep -q "sandboxes: defineTable" "${UI_SCHEMA}" 2>/dev/null; then
  fail "invented _nimbus.sandboxes table still in UI schema"
else
  pass "invented sandboxes table removed from UI schema"
fi
[ -f "packages/nimbus-ui/convex/sandboxes.ts" ] \
  && fail "invented convex/sandboxes.ts still present" \
  || pass "invented convex/sandboxes.ts removed"
if grep -q "sandboxes" "${UI_GEN_API}" 2>/dev/null; then
  fail "api.sandboxes still in generated client (re-run codegen)"
else
  pass "no api.sandboxes in generated client"
fi

step FSV1 "System schema: source_packages + modules"
grep -q "SourcePackages" "${SYS_SCHEMA}" 2>/dev/null \
  && pass "source_packages in nimbus-system schema" \
  || fail "source_packages missing from nimbus-system schema"
grep -qE "Modules\b" "${SYS_SCHEMA}" 2>/dev/null \
  && pass "modules in nimbus-system schema" \
  || fail "modules missing from nimbus-system schema"
grep -q "source_packages: defineTable" "${UI_SCHEMA}" 2>/dev/null \
  && pass "source_packages in UI schema mirror" \
  || fail "source_packages missing from UI schema mirror"
grep -q "modules: defineTable" "${UI_SCHEMA}" 2>/dev/null \
  && pass "modules in UI schema mirror" \
  || fail "modules missing from UI schema mirror"

step FSV2 "Content-addressed source store"
grep -rq "SourcePackageStore" crates/ 2>/dev/null \
  && pass "SourcePackageStore seam present" \
  || fail "SourcePackageStore seam missing"

step FSV3 "Deploy capture (client + server)"
grep -q "source_package" "${DEPLOY_CLIENT}" 2>/dev/null \
  && pass "deploy client uploads a source package" \
  || fail "deploy client does not upload a source package"
grep -qE "SystemTable::Modules|SystemTable::SourcePackages" "${SYS_RECORDS}" 2>/dev/null \
  && pass "records.rs writes modules/source_packages" \
  || fail "records.rs does not write modules/source_packages"

step FSV4 "Source read path"
grep -rq "console/source\|fn .*source_handler\|read_module_source" crates/nimbus-server/src 2>/dev/null \
  && pass "source read endpoint present" \
  || fail "source read endpoint missing"

step FSV5 "Console wiring"
if grep -rq "/api/console/source" packages/nimbus-ui/src 2>/dev/null; then
  pass "console Source tab fetches the source endpoint"
else
  fail "console Source tab does not fetch the source endpoint"
fi

step FSV6 "Closeout: integration test + docs"
grep -rq "source_package_build_store_record_and_read_round_trip" crates/nimbus-system/src 2>/dev/null \
  && pass "store->record->read integration test present" \
  || fail "integration test missing"
grep -q "api/console/source" DESIGN.md 2>/dev/null \
  && pass "DESIGN.md documents the Compute source view" \
  || fail "DESIGN.md not updated"

step FSV9 "Deploy-time typecheck gate"
grep -q "TypeCheckMode" "${DEPLOY_CLIENT}" 2>/dev/null \
  && pass "deploy client has a typecheck gate (enable/try/disable)" \
  || fail "deploy client has no typecheck gate"

step FSV7 "Code navigation (oxc structural index)"
if [ -f crates/nimbus-code-index/src/lib.rs ] \
  && grep -q "analyze_module" crates/nimbus-code-index/src/lib.rs 2>/dev/null; then
  pass "oxc analysis foundation present (analyze_module: exports + imports)"
else
  fail "oxc analysis foundation missing"
fi

step FSV8 "Type intelligence (TS-compiler hover extraction)"
if [ -f crates/nimbus-bin/src/typeinfo.mjs ] \
  && grep -q "getQuickInfoAtPosition" crates/nimbus-bin/src/typeinfo.mjs 2>/dev/null; then
  pass "type-info extraction foundation present (TS Compiler API hover)"
else
  fail "type-info extraction foundation missing"
fi

printf '\n\033[1mSummary:\033[0m %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf 'Outstanding:\n'
  for d in "${FAIL_DETAIL[@]}"; do printf '  - %s\n' "$d"; done
  exit 1
fi
exit 0
