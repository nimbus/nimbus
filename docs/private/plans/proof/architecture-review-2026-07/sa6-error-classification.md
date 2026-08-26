# SA6 Error Classification Proof

Date: 2026-08-26  
Implementation: PR #327, merge `619a0a4c8`  
Owner routing: Band SA6 / BLI-SA6

## Corrected scope

SA6 confirms two defects:

1. Local-pack scrub and rebuild converted every direct-verification error into
   a `HashMismatch` finding and quarantine. A transient I/O error could become
   false corruption evidence.
2. Erasure-manifest publish treated every rollback preimage read error as an
   absent replica. A later rollback could unlink a committed replica whose
   preimage read failed transiently.

The original heal clause is refuted. A manifest that names a shard whose local
pack is missing describes canonical repair loss. Heal must reconstruct that
shard. Nimbus autoreview found this during the first review pass, so the heal
change was removed and a missing-pack repair regression replaced it.

## Shipped contract

- `classify_verification` in `crates/nimbus-blob/src/scrub.rs` turns only
  `StorageErrorKind::Corruption` into a finding candidate. Success remains
  verified. Every other typed error propagates and cannot authorize quarantine.
- Both corrupt-index rebuild paths use the same classifier before they publish
  index or quarantine state.
- Erasure-manifest publish reads every non-identical replica preimage before the
  first write. `NotFound` alone records no prior replica. Any other read error
  returns typed I/O with `rollback_durable = true` and zero writes.
- The original write and rollback ordering remains unchanged after preflight.
- Heal still repairs missing indexed physical pack data.

## Regression evidence

| Contract | Test | Result |
| --- | --- | --- |
| Transient verification is not corruption | `direct_verification_preserves_transient_errors` | passed |
| Preimage failure makes zero manifest writes | `erasure_publish_preimage_read_failure_aborts_before_writes` | passed |
| Missing indexed pack remains repairable | `erasure_heal_repairs_missing_backing_pack` | passed |

The manifest test injects `PermissionDenied`, observes no sync events, and
compares every replica to its pre-call bytes. The heal test removes the named
physical pack, runs repair, verifies the generation advance and one rewritten
stripe/shard, then reads the original payload.

## Repository evidence

- Focused SA6 regressions: 3 passed, 0 failed.
- Strict `cargo clippy -p nimbus-blob --all-targets --all-features -- -D warnings`:
  passed. Existing vendored dependency warnings remained outside the crate
  warning gate.
- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Nimbus autoreview: the first pass found and corrected the heal error; the
  final pre-PR pass was clean.
- `env PATH="/opt/homebrew/opt/node@22/bin:$PATH" make ci`: passed end to end.
  The process-isolated Rust suite passed 7,662 tests with 111 skipped; runtime
  tests passed 517 with 134 ignored; the UI passed 95 files and 832 tests; the
  required verification harness, JavaScript build/typecheck/tests, and proof
  helpers passed.

## Closure

BLI-SA6 is terminal. This prerequisite did not promote the proposed BLI plan or
start BLI0 through BLI5. Band SA proceeds to the NKV-owned SA11 contract.
