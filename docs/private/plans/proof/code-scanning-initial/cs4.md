# Security closeout batches

Baseline: `b527e1742ab3c86f8e896765b7d4e63b964116ab`.
The second batch started with 106 security alerts and 162 quality alerts.
The user authorized parallel implementation and publication without waiting for CI or Actions.

## Alert dispositions

The GitHub API confirmed 97 more dismissals. The batch covers 87 test diagnostics and 10 false positives.
The first batch closed 41 alerts. Both batches closed 138 of the original 147 security alerts.
The main-branch API snapshot after these dismissals contains nine security alerts and 162 quality alerts.

`cs4-triage.json` retains the source location, guard or data flow, reason, comment, and API response for each new dismissal.
The test diagnostics are assertion or panic messages inside test modules. They do not emit production credentials or accept production log input.
The other findings cover configuration field names, recovery-status booleans, a framed encryption nonce, the caller-configured WebSocket client, and the docs anchor sanitizer.

The framed cipher combines a fixed 32-bit prefix with a 64-bit frame counter under each blob subkey.
The key seed derives from a content hash or a random stream salt. Identical content deliberately repeats under AES-GCM-SIV for deduplication.

The recovery booleans report file-publication status. They have no key-material flow.
The vendor option labels select separate runtime values. They are not credentials.

## Repair queue

The queue has six independent branches from the same main commit. PR #345 contains the Firebase repair and owns this audit.
All five new draft PRs are open. GitHub reports the same head commits that received local verification and required review.

| Repair | PR | Security alerts | Focused proof |
| --- | --- | --- | --- |
| Firebase own-field access | [#345](https://github.com/nimbus/nimbus/pull/345) | #3, #4 | 14 regression cases |
| Codegen reference trees | [#346](https://github.com/nimbus/nimbus/pull/346) | #1, #2, #6919 | Full codegen selftest, typecheck, and build |
| Bootc option values | [#347](https://github.com/nimbus/nimbus/pull/347) | #7244 | 4 bootc tests |
| Runtime filesystem canary | [#348](https://github.com/nimbus/nimbus/pull/348) | #1942 | 2 verifier tests and 1 Node 24 canary batch |
| Brotli allocator ownership | [#349](https://github.com/nimbus/nimbus/pull/349) | #7100 | 8 FFI tests |
| Object-store TLS verification | [#350](https://github.com/nimbus/nimbus/pull/350) | #7141 | 2 rejected-key tests and 1 configuration roundtrip |

All five new branches passed workspace formatting and Clippy. Both vendor patches passed third-party attribution checks.
Nimbus pre-PR review passed for each final code commit.
The reviewer used Claude Opus 5 high with the configured P0 threshold.

`cs4-verification.json` records the exact commits, commands, source hashes, and log hashes. No accepted finding remains.

## Manual audit

Codegen uses own-property traversal and data-property writes. Computed property syntax keeps generated `__proto__` names as ordinary fields.
Bootc uses an option terminator for the image and binds transport and tag values to their option names.
The runtime filesystem canary uses exclusive creation with owner-only permissions. Its verifier requires a capability denial from every probe.

The Brotli paths read allocator context from the moved local. The free callback and destruction order remain unchanged.
The TLS patch removes the certificate-verification bypass while keeping system and explicit-root verification.

These changes preserve all three engine mutation paths: queued journal, direct, and execution-unit.
They change no storage transaction, document index, runtime bundle verification, or crate dependency direction.

## Verification limits

The earlier full local CI run encountered an active development server. That run remains unverified.
The new batches passed focused checks and required workspace Clippy runs. They do not repeat the unsafe discovery path.
The external object-store SSE-C integration fixture needs a trusted endpoint and is not run here.

The current Nimbus dependency graph excludes Brotli `ffi-api`. Its focused tests need an isolated vendor-crate manifest with that feature enabled.

No hosted CI or Actions wait is part of this batch. GitHub CodeQL must confirm repaired alert closure after merge.

Raw command logs, source hashes, recovery records, and complete API snapshots remain in `/Users/jack/nimbus-cleanup-2026-09-11`.

## Writing and documentation checks

The plan, new proof, PR descriptions, and changed index entry pass the writing linter.
The full plan index reports 74 pre-existing diagnostics. The unchanged baseline reports the same 74 diagnostics.
The update preserves those other owners' entries.

The docs honesty gate passed: 109 pages, valid source paths, and an intact private fence.
