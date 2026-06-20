#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Nimbus crypto extraction plan
# (`docs/private/plans/nimbus-crypto-extraction-plan.md`, NC0..NC4).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in NC0 so /goal is verifiable from day one; NC1..NC4 progressively
# flip conditions from FAIL to PASS.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/nimbus-crypto-extraction-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/nimbus-crypto-extraction-plan.md"
AGENTS_MD="AGENTS.md"
CLAUDE_MD="CLAUDE.md"
PLANS_README="docs/private/plans/README.md"
PROOF_DIR="docs/private/plans/proof/nimbus-crypto-extraction"
PROOF_NC0="${PROOF_DIR}/nc0-baseline.md"
PROOF_EXEMPLAR="${PROOF_DIR}/nc0-exemplar-comparison.md"
PROOF_NC4="${PROOF_DIR}/nc4-closeout.md"

ROOT_CARGO="Cargo.toml"
CRYPTO_CARGO="crates/nimbus-crypto/Cargo.toml"
CRYPTO_SRC="crates/nimbus-crypto/src"
STORAGE_ENCRYPTION_DIR="crates/nimbus-storage/src/encryption"
STORAGE_SRC="crates/nimbus-storage/src"
BLOB_SRC="crates/nimbus-blob/src"
ARCH_DOC="docs/private/architecture/storage/encryption-at-rest.md"
ARCHITECTURE_MD="ARCHITECTURE.md"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

plan_file() {
  if [ -f "${PLAN_ACTIVE}" ]; then
    printf '%s\n' "${PLAN_ACTIVE}"
  elif [ -f "${PLAN_ARCHIVED}" ]; then
    printf '%s\n' "${PLAN_ARCHIVED}"
  else
    printf ''
  fi
}

grep_dir() {
  [ -d "$2" ] || return 1
  grep -rqE --include='*.rs' "$1" "$2" 2>/dev/null
}

grep_file() {
  [ -f "$2" ] || return 1
  grep -qE "$1" "$2" 2>/dev/null
}

printf '\033[1mNC verification gate - nimbus-crypto-extraction\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan file exists.
step 1 "Plan checked in or present in local private state"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Routing entries exist.
step 2 "Routing entries exist"
has_agents_route=0
has_claude_route=0
has_plans_route=0
if [ -f "${AGENTS_MD}" ] && grep -q 'nimbus-crypto-extraction-plan' "${AGENTS_MD}"; then
  has_agents_route=1
fi
if [ -f "${CLAUDE_MD}" ] || [ -L "${CLAUDE_MD}" ]; then
  if grep -q 'nimbus-crypto-extraction-plan' "${CLAUDE_MD}" 2>/dev/null; then
    has_claude_route=1
  fi
fi
if [ -f "${PLANS_README}" ] && grep -q 'nimbus-crypto-extraction-plan.md' "${PLANS_README}"; then
  has_plans_route=1
fi
if [ "${has_agents_route}" = "1" ] && [ "${has_claude_route}" = "1" ] && [ "${has_plans_route}" = "1" ]; then
  pass "AGENTS.md, CLAUDE.md, and ${PLANS_README} reference the NC plan"
else
  fail "Routing entries incomplete" "agents=${has_agents_route} claude=${has_claude_route} plans_readme=${has_plans_route}"
fi

# 3. NC0 proof files exist and contain the required anchors.
step 3 "NC0 baseline and exemplar proofs"
baseline_ok=0
exemplar_ok=0
if [ -f "${PROOF_NC0}" ] \
  && grep -q '4324 total' "${PROOF_NC0}" \
  && grep -q 'codex/nimbus-crypto-extraction' "${PROOF_NC0}" \
  && grep -q 'nimbus-blob scaffold' "${PROOF_NC0}"; then
  baseline_ok=1
fi
if [ -f "${PROOF_EXEMPLAR}" ] \
  && grep -q 'AWS Encryption SDK' "${PROOF_EXEMPLAR}" \
  && grep -q 'Tink' "${PROOF_EXEMPLAR}" \
  && grep -q 'restic' "${PROOF_EXEMPLAR}" \
  && grep -q 'kopia' "${PROOF_EXEMPLAR}" \
  && grep -q 'mongo-rust-driver' "${PROOF_EXEMPLAR}" \
  && grep -q 'iroh-blobs' "${PROOF_EXEMPLAR}"; then
  exemplar_ok=1
fi
if [ "${baseline_ok}" = "1" ] && [ "${exemplar_ok}" = "1" ]; then
  pass "NC0 proof bundle exists with baseline and exemplar anchors"
else
  fail "NC0 proof bundle incomplete" "baseline=${baseline_ok} exemplar=${exemplar_ok}"
fi

# 4. NC1: nimbus-crypto crate exists and keeps the dependency boundary.
step 4 "NC1: nimbus-crypto crate and dependency boundary"
member_ok=0
crypto_cargo_ok=0
dep_boundary_ok=0
if grep_file '"crates/nimbus-crypto"' "${ROOT_CARGO}"; then
  member_ok=1
fi
if [ -f "${CRYPTO_CARGO}" ] && grep -q 'name = "nimbus-crypto"' "${CRYPTO_CARGO}"; then
  crypto_cargo_ok=1
fi
if [ -f "${CRYPTO_CARGO}" ]; then
  path_deps="$(grep -En 'path = "../nimbus-' "${CRYPTO_CARGO}" 2>/dev/null | grep -v 'nimbus-core' || true)"
  if grep -q 'nimbus-core' "${CRYPTO_CARGO}" && [ -z "${path_deps}" ]; then
    dep_boundary_ok=1
  fi
fi
if [ "${member_ok}" = "1" ] && [ "${crypto_cargo_ok}" = "1" ] && [ "${dep_boundary_ok}" = "1" ]; then
  pass "crates/nimbus-crypto is a workspace member with only nimbus-core as a workspace dependency"
else
  fail "NC1 crate boundary incomplete" "member=${member_ok} cargo=${crypto_cargo_ok} boundary=${dep_boundary_ok}"
fi

# 5. NC1: storage encryption module deleted with no shim.
step 5 "NC1: storage encryption module removed without shim"
dir_removed=0
no_storage_mod=0
if [ ! -d "${STORAGE_ENCRYPTION_DIR}" ]; then
  dir_removed=1
fi
if [ -d "${STORAGE_SRC}" ] \
  && ! grep -qsE '^[[:space:]]*pub mod encryption;|^[[:space:]]*mod encryption;' "${STORAGE_SRC}/lib.rs" \
  && ! grep -RqsE 'crate::encryption' "${STORAGE_SRC}"; then
  no_storage_mod=1
fi
if [ "${dir_removed}" = "1" ] && [ "${no_storage_mod}" = "1" ]; then
  pass "nimbus-storage no longer owns or shims the encryption module"
else
  fail "storage encryption module still present" "dir_removed=${dir_removed} no_storage_mod=${no_storage_mod}"
fi

# 6. NC1: public consumers import nimbus-crypto directly.
step 6 "NC1: public crypto imports moved"
storage_uses_crypto=0
engine_uses_crypto=0
facade_uses_crypto=0
bin_uses_crypto=0
no_public_storage_imports=0
grep_dir 'nimbus_crypto' crates/nimbus-storage/src && storage_uses_crypto=1
grep_dir 'nimbus_crypto' crates/nimbus-engine/src && engine_uses_crypto=1
grep_dir 'nimbus_crypto' crates/nimbus/src && facade_uses_crypto=1
grep_dir 'nimbus_crypto' crates/nimbus-bin/src && bin_uses_crypto=1
if ! grep -RqsE 'nimbus_storage::(AwsKmsKeyProvider|KeyDirectoryProvider|LocalKeyProvider|MasterKeyFileProvider|KeyManifest|ManifestCipher|DataEncryptionKey|WrappedDatabaseKey|GeneratedDatabaseKey)' crates/nimbus-engine crates/nimbus crates/nimbus-bin 2>/dev/null; then
  no_public_storage_imports=1
fi
if [ "${storage_uses_crypto}" = "1" ] && [ "${engine_uses_crypto}" = "1" ] && [ "${facade_uses_crypto}" = "1" ] && [ "${bin_uses_crypto}" = "1" ] && [ "${no_public_storage_imports}" = "1" ]; then
  pass "storage, engine, facade, and CLI import nimbus-crypto directly"
else
  fail "public crypto import movement incomplete" "storage=${storage_uses_crypto} engine=${engine_uses_crypto} facade=${facade_uses_crypto} bin=${bin_uses_crypto} no_storage_imports=${no_public_storage_imports}"
fi

# 7. NC2: materials/keyring API and provider error surface.
step 7 "NC2: materials/keyring API and provider failure mapping"
has_materials=0
has_suite=0
has_trace=0
has_provider_failures=0
grep_dir 'CryptoMaterials' "${CRYPTO_SRC}" && has_materials=1
grep_dir 'AlgorithmSuite|DekTemplate|Commitment' "${CRYPTO_SRC}" && has_suite=1
grep_dir 'KeyringTrace|ProviderIdentity|redacted' "${CRYPTO_SRC}" && has_trace=1
grep_dir 'wrong provider|bad key|denied decrypt|invalid credentials|network failure|endpoint|tls' "${CRYPTO_SRC}" && has_provider_failures=1
if [ "${has_materials}" = "1" ] && [ "${has_suite}" = "1" ] && [ "${has_trace}" = "1" ] && [ "${has_provider_failures}" = "1" ]; then
  pass "materials/keyring API, redacted trace, and provider-negative tests are present"
else
  fail "NC2 materials/keyring API incomplete" "materials=${has_materials} suite=${has_suite} trace=${has_trace} provider_failures=${has_provider_failures}"
fi

# 8. NC2: shred, stale-handle revocation, rotation, and AAD tamper.
step 8 "NC2: crypto-shred and rotation behavior"
has_shred=0
has_stale=0
has_rotation=0
has_tamper=0
grep_dir 'crypto.?shred|tombstone|destroy.*wrapped|delete.*wrapped' "${CRYPTO_SRC}" && has_shred=1
grep_dir 'stale|generation|revocable|pre.?shred' "${CRYPTO_SRC}" && has_stale=1
grep_dir 'rewrap|rotate|rotation' "${CRYPTO_SRC}" && has_rotation=1
grep_dir 'AAD|associated data|tamper|authentication' "${CRYPTO_SRC}" && has_tamper=1
if [ "${has_shred}" = "1" ] && [ "${has_stale}" = "1" ] && [ "${has_rotation}" = "1" ] && [ "${has_tamper}" = "1" ]; then
  pass "crypto-shred, stale-handle, rotation, and AAD-tamper coverage are present"
else
  fail "NC2 shred behavior incomplete" "shred=${has_shred} stale=${has_stale} rotation=${has_rotation} tamper=${has_tamper}"
fi

# 9. NC3: typed framed AEAD seam and blob consumption.
step 9 "NC3: typed framed AEAD seam and nimbus-blob consumption"
has_framed=0
has_session=0
has_range=0
blob_uses_crypto=0
blob_no_raw_dek=0
grep_dir 'Framed.*Aead|Framed.*seal|FrameSealer|Aead' "${CRYPTO_SRC}" && has_framed=1
grep_dir 'FramedSealSession|FramedOpenSession|AlgorithmSuite|DekTemplate|frame_size|plaintext_bound|MAX_WRAPPED' "${CRYPTO_SRC}" && has_session=1
grep_dir 'overlapping frames|range.*frame|decrypt.*frame' "${CRYPTO_SRC}" && has_range=1
grep_dir 'nimbus_crypto' "${BLOB_SRC}" && blob_uses_crypto=1
if [ -d "${BLOB_SRC}" ] && ! grep -RqsE 'pub struct TenantDek|pub struct XorFrameSealer|Arc<Vec<u8>>' "${BLOB_SRC}"; then
  blob_no_raw_dek=1
fi
if [ "${has_framed}" = "1" ] && [ "${has_session}" = "1" ] && [ "${has_range}" = "1" ] && [ "${blob_uses_crypto}" = "1" ] && [ "${blob_no_raw_dek}" = "1" ]; then
  pass "typed framed AEAD seam exists and nimbus-blob consumes it without raw DEK production API"
else
  fail "NC3 framed AEAD incomplete" "framed=${has_framed} session=${has_session} range=${has_range} blob_crypto=${blob_uses_crypto} blob_no_raw_dek=${blob_no_raw_dek}"
fi

# 10. NC4: docs, ledger, and CI closeout.
step 10 "NC4: architecture doc, ledger, and CI proof"
doc_ok=0
crate_table_ok=0
ledger_clean=0
ci_proof=0
if [ -f "${ARCH_DOC}" ] && grep -q 'nimbus-crypto' "${ARCH_DOC}"; then
  doc_ok=1
fi
if [ -f "${ARCHITECTURE_MD}" ] && grep -q '`nimbus-crypto`' "${ARCHITECTURE_MD}"; then
  crate_table_ok=1
fi
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  ledger_rows="$(awk '
    /^\| NC \| Description \| Status \|/ {in_ledger=1; next}
    in_ledger && /^$/ {in_ledger=0}
    in_ledger && /^\| NC[0-9]/ {print}
  ' "${PLAN_FILE}")"
  if [ -n "${ledger_rows}" ] && ! printf '%s\n' "${ledger_rows}" | grep -vE '\| done \|' | grep -qE '^\| NC[0-9]'; then
    ledger_clean=1
  fi
fi
if [ -f "${PROOF_NC4}" ] && grep -qE 'CI.*(green|success)' "${PROOF_NC4}" && grep -q '10 passed, 0 failed' "${PROOF_NC4}"; then
  ci_proof=1
fi
if [ "${doc_ok}" = "1" ] && [ "${crate_table_ok}" = "1" ] && [ "${ledger_clean}" = "1" ] && [ "${ci_proof}" = "1" ]; then
  pass "architecture doc, crate table, ledger, and CI proof are complete"
else
  fail "NC4 closeout incomplete" "doc=${doc_ok} crate_table=${crate_table_ok} ledger=${ledger_clean} ci_proof=${ci_proof}"
fi

printf '\n\033[1m%d passed, %d failed\033[0m\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${d}"
  done
  exit 1
fi

exit 0
