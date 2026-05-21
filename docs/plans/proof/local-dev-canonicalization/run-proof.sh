#!/usr/bin/env bash
# LD5 fresh-clone proof runner. Executed inside the dedicated worktree
# /tmp/nimbus-ld5-proof, which is on a detached HEAD at the same commit as
# main and has no UI artifacts, no node_modules, and no target/. Logs land
# in the *parent* main tree so they can be committed there.

set -u

PROOF_DIR="/Users/jack/src/github.com/nimbus/nimbus/docs/plans/proof/local-dev-canonicalization"
CARGO_LOG="${PROOF_DIR}/cargo-direct-fresh-clone.log"
CI_LOG="${PROOF_DIR}/clean-tree-make-ci-required.log"
RUN_LOG="${PROOF_DIR}/run-proof.meta.log"

mkdir -p "${PROOF_DIR}"

stamp() { date -u +"%Y-%m-%dT%H:%M:%SZ"; }

{
  echo "=== LD5 fresh-clone proof — meta log ==="
  echo "Started: $(stamp)"
  echo "Worktree: $(pwd)"
  echo "HEAD: $(git rev-parse HEAD)"
  echo "git status (should be clean):"
  git status --short
  echo
  echo "Verifying fresh-clone state:"
  echo "  packages/nimbus-ui/dist/index.html exists?       $(test -f packages/nimbus-ui/dist/index.html && echo yes || echo no)"
  echo "  packages/nimbus-ui/.nimbus/convex/* exists?      $(test -d packages/nimbus-ui/.nimbus/convex && echo yes || echo no)"
  echo "  node_modules exists?                              $(test -d node_modules && echo yes || echo no)"
  echo "  target/ exists?                                   $(test -d target && echo yes || echo no)"
  echo
} > "${RUN_LOG}"

# --- Phase 1: npm ci so the JS toolchain is available ----------------------
echo "=== Phase 1: npm ci  (started $(stamp)) ===" | tee -a "${RUN_LOG}"
NPM_CI_START=$(date +%s)
npm ci >> "${RUN_LOG}" 2>&1
NPM_CI_RC=$?
NPM_CI_END=$(date +%s)
echo "Phase 1: npm ci exit=${NPM_CI_RC} duration=$((NPM_CI_END - NPM_CI_START))s" | tee -a "${RUN_LOG}"
if [ "${NPM_CI_RC}" -ne 0 ]; then
  echo "ABORT: npm ci failed" | tee -a "${RUN_LOG}"
  exit 1
fi

# --- Phase 2: cargo-direct proof — expect the actionable build.rs error ----
echo "=== Phase 2: cargo check -p nimbus-server (expect actionable error)  (started $(stamp)) ===" | tee -a "${RUN_LOG}"
{
  echo "=== LD5: cargo-direct fresh-clone proof ==="
  echo "Date: $(stamp)"
  echo "Worktree: $(pwd) (HEAD $(git rev-parse HEAD))"
  echo "Pre-state:"
  echo "  packages/nimbus-ui/dist/index.html exists?  $(test -f packages/nimbus-ui/dist/index.html && echo yes || echo no)"
  echo
  echo "Command: cargo check -p nimbus-server"
  echo
  echo "=== Output (stdout+stderr) ==="
} > "${CARGO_LOG}"

CARGO_START=$(date +%s)
cargo check -p nimbus-server >> "${CARGO_LOG}" 2>&1
CARGO_RC=$?
CARGO_END=$(date +%s)

{
  echo
  echo "=== Exit code: ${CARGO_RC} (expected: non-zero) ==="
  echo "Duration: $((CARGO_END - CARGO_START))s"
} >> "${CARGO_LOG}"

echo "Phase 2: cargo check exit=${CARGO_RC} duration=$((CARGO_END - CARGO_START))s" | tee -a "${RUN_LOG}"
if [ "${CARGO_RC}" -eq 0 ]; then
  echo "WARN: cargo check unexpectedly succeeded — LD2's build.rs guard may have regressed" | tee -a "${RUN_LOG}"
fi

# --- Phase 3: ci-required — expect Make to self-heal UI artifacts and pass --
echo "=== Phase 3: make ci-required (expect green; UI graph self-heals)  (started $(stamp)) ===" | tee -a "${RUN_LOG}"
{
  echo "=== LD5: clean-tree make ci-required proof ==="
  echo "Date: $(stamp)"
  echo "Worktree: $(pwd) (HEAD $(git rev-parse HEAD))"
  echo "Pre-state:"
  echo "  packages/nimbus-ui/dist/index.html exists?  $(test -f packages/nimbus-ui/dist/index.html && echo yes || echo no)"
  echo
  echo "Command: make ci-required"
  echo
  echo "=== Output (stdout+stderr) ==="
} > "${CI_LOG}"

CI_START=$(date +%s)
make ci-required >> "${CI_LOG}" 2>&1
CI_RC=$?
CI_END=$(date +%s)

{
  echo
  echo "=== Exit code: ${CI_RC} (expected: 0) ==="
  echo "Duration: $((CI_END - CI_START))s"
  echo "Post-state:"
  echo "  packages/nimbus-ui/dist/index.html exists?       $(test -f packages/nimbus-ui/dist/index.html && echo yes || echo no)"
  echo "  packages/nimbus-ui/.nimbus/convex/bundle.sha256? $(test -f packages/nimbus-ui/.nimbus/convex/bundle.sha256 && echo yes || echo no)"
} >> "${CI_LOG}"

echo "Phase 3: make ci-required exit=${CI_RC} duration=$((CI_END - CI_START))s" | tee -a "${RUN_LOG}"

echo "Finished: $(stamp)" | tee -a "${RUN_LOG}"

# Exit non-zero iff either expected-success step failed (npm ci already
# checked; cargo-direct is *expected* to fail so its non-zero is correct).
if [ "${CARGO_RC}" -eq 0 ] || [ "${CI_RC}" -ne 0 ]; then
  exit 1
fi
exit 0
