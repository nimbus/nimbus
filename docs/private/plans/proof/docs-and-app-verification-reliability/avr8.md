# AVR8 Structured Evidence

Date: 2026-08-18

## Result

AVR8 replaces console-only evidence with a versioned JSON report and a
deterministic JUnit projection. The report seam is
`scripts/examples-verify-report.mjs`. It has no product crate or package
dependency.

Each report separates desired case behavior, observed results, provenance, and
cleanup. Per-case records have one writer. Final case order comes from the
validated manifest. This design lets AVR9 add workers without a shared-file
write race.

The final live run used report
`nimbus-examples-verify.wza1cz-633b582dff25`. It passed nine applications and
37 ordered smoke anchors in 83,725 milliseconds. JUnit recorded 12 tests, zero
failures, and zero skips.

## Fail-Before Evidence

Before AVR8, both task conditions were red:

| Command | Result |
| --- | --- |
| `bash scripts/examples-verify-contract-test.sh --task AVR8` | Fail. AVRC21-AVRC22 were 0/2. |
| `bash scripts/verify-docs-app-verification.sh --task AVR8` | Fail. AVRC21-AVRC22 were 0/2. |

The runner printed case output only. It did not record a binary digest,
manifest digest, source digests, observed ports, durations, exits, or cleanup
state.

## Acceptance Ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR8.1 Define the report schema. | Pass. | Schema version 1 allows only named run, provenance, case, source, and cleanup fields. Nine manifest cases declare 37 expected anchors. |
| AVR8.2 Write reports atomically. | Pass. | Temporary files are synced and renamed. Single-writer state uses an atomic no-replace link. An injected pre-rename interruption preserved the prior canonical report. |
| AVR8.3 Redact credentials. | Pass. | The schema rejects credential fields. Defensive redaction removes named values, bearer values, and URL user information. Smoke output records only validated anchor names. |
| AVR8.4 Validate case and cleanup outcomes. | Pass. | Passed cases require every declared anchor, a loopback endpoint, exit zero, and cleanup success. Failed and not-run cases remain explicit. Source and cleanup contradictions fail. |
| AVR8.5 Derive JUnit only from valid reports. | Pass. | JUnit generation first validates JSON. It projects every case plus run, source, and cleanup outcomes in manifest order. |

## Behavioral Evidence

| Command or proof | Result |
| --- | --- |
| `node scripts/examples-verify-report-test.mjs` | Pass. 8/8 covered success and rejection goldens, redaction, interrupted writes, ordering, both JUnit outcomes, and anchor parsing. |
| `node scripts/examples-verify-supervisor-test.mjs` | Pass. 2/2 proved owner-only smoke output and refusal before duplicate output spawn. |
| `node scripts/examples-verify-workspace-test.mjs` | Pass. Nine groups covered all preparation fixtures, source preservation, manifest validation, and expected-anchor ownership. |
| `node scripts/examples-verify-lifetime-test.mjs` | Pass. 12/12. Lifetime results now separate workload exit from cleanup status. |
| `node scripts/examples-verify-runner-fault-test.mjs --bin target/debug/nimbus` | Pass. 7/7 fault and retry cases produced failed JSON and JUnit evidence, retained diagnostics, released ports, and removed credentials. |
| `bash scripts/examples-verify-contract-test.sh --task AVR8` | Pass. AVRC21-AVRC22 were 2/2. The same run repeated 8/8 report and 2/2 supervisor behaviors. |
| `bash scripts/verify-docs-app-verification.sh --task AVR8` | Pass. AVRC21-AVRC22 were 2/2. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 mutations failed closed. |
| Full Node.js 24 live run | Pass. Nine applications, 37 anchors, nine distinct observed ports, matching source digests, and clean resource removal. |
| Bash syntax and ShellCheck | Pass. No diagnostics. |
| Node syntax | Pass for every changed module and test. |
| `actionlint .github/workflows/ci.yml` | Pass. |
| `git diff --check` | Pass. |

## Live Report Evidence

| Field | Value |
| --- | --- |
| Binary | `nimbus 0.1.45`; SHA-256 `4e7f6939a130190609bae2ddb4a847add566f609e1f27d4dbd7057bf8ae0ea37` |
| Manifest | Schema 1; SHA-256 `9fd462b34d03d8af214f98aff26335636d7e89ee9af0221aa413bfac3c1c4a77` |
| Source | Before and after SHA-256 `937ecf3d34ab918651147f27fe823fdefbe29cf5948066b4642c147184e55598`; status `matched` |
| Cases | 9 passed in manifest order |
| Anchors | 37 observed in declared order |
| Ports | 9 distinct loopback ports recorded as observations, not identities |
| Cleanup | `passed`; exit 0; no artifact retained |
| JUnit | 12 tests; 0 failures; 0 skips |

A credential scan over the final JSON and JUnit found no bearer value, launch
token, password, secret, authorization value, cookie, or API key. CI uploads
only final `report.json` and `junit.xml` files. Internal working records stay
outside the uploaded glob.

## Cleanup Race Correction

The first final fault run exposed a real immediate-spawn race. TERM could stop
Nimbus after durable listener reservation but before its shutdown owner was
ready. The retained state showed `reserved` or `active` instead of `released`.

Cleanup now waits for bounded case-local discovery. It reads the case-local
admin token and uses the existing graceful shutdown path when startup finishes.
TERM and KILL remain bounded fallbacks. The repeated seven-case fault suite
proved only terminal lease phases.

## CI Contract

The Examples Verify job uploads JSON and JUnit with `if: always()`. Missing
reports fail the upload step. The artifact has a 14-day retention period.

## Residual Boundary

The runner remains serial. AVR9 owns bounded scheduling, serial diagnostics,
stable concurrent report order, and measured time budgets. AVR8 adds no port,
network, process, or product-effect authority.
