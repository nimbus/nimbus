# FCE0: Baseline And Verifier Skeleton

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-005, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved: none. FCE0 is the control-plane baseline.
- Files/modules intentionally left in `nimbus-server`:
  - route mounting and `RouterBuildConfig`
  - listener lifecycle and process startup
  - `AppState` construction and global composition
  - shutdown signaling
  - process-backed verifier execution
  - server-owned artifact verifier process runner effects
  - server-owned adapter transport shells
  - exact verifier phrases: route mounting; listener lifecycle; AppState construction; global composition; shutdown signaling; process-backed verifier execution; server-owned adapter transport shells.
  - server-owned `_nimbus` writer implementations that call `nimbus-system`
- Crates created or updated: none.
- Target crates for this completion wave:
  - `nimbus-artifacts`
  - `nimbus-provenance`
  - `nimbus-services`
  - `nimbus-operator`
  - `nimbus-mongodb`
  - `nimbus-firebase`
  - `nimbus-cloud-functions`
  - `nimbus-convex`
  - optional final facade: `nimbus-adapters`

## Ownership Decisions

- Authority owner:
  - tenant authority: `nimbus-tenant`
  - application auth: `nimbus-auth`
  - operator authority: future `nimbus-operator`
  - runtime capabilities: `nimbus-bridge`
  - `_nimbus` system evidence: `nimbus-system` plus server-owned writer implementations
- Effect owner:
  - `nimbus-server` owns route mounting, listener lifecycle, global process startup, shutdown, process-backed verifier execution, and composition.
  - Pure extracted crates must not own process execution, `_nimbus` persistence, route mounting, or listener lifecycle.
- Server composition shell:
  - `nimbus-server` remains the shell around adapters, service lifecycle HTTP routes, deploy/local admin routes, runtime invocation wiring, and process-backed artifact verification.
- Explicit keep decisions:
  - `nimbus-adapters` is not created in FCE0 and remains allowed only as a final thin re-export facade.
  - FCE1-FCE8 must end in actual extracted crates, not only readiness.

## Seam Fix Attempts

- Messy seam found: FCE0 found no implementation seam to repair; the missing seam was the extraction control plane itself.
- Right-sized ownership-correct repair attempted: added the extraction verifier skeleton and this proof so future phases cannot stop at "ready to extract."
- Files changed or spike/proof performed:
  - `docs/plans/server-crate-extraction-completion-plan.md`
  - `docs/plans/proof/server-crate-extraction-completion/fce0-baseline.md`
  - `scripts/verify-server-crate-extraction-completion.sh`
- Result: completed. The verifier is executable and passed with predecessor SSE evidence.
- If blocked, exact architectural reason: n/a.
- Next implementation move: run the verifier, record its exact result, then advance FCE1.

## Dependency Evidence

Command:

```text
bash scripts/verify-server-crate-extraction-completion.sh
```

Relevant output:

```text
[2] Predecessor SSE verifier remains authoritative
  PASS  Predecessor server seam extraction readiness verifier passed

[5] Known architecture crates are present and server-free
  PASS  Existing authority/system/bridge/license crates are present and do not depend on nimbus-server

Summary: 8 passed, 0 failed
```

## Denied-Import Evidence

FCE0 establishes reusable denied-import checks. Extraction-specific denied
import output starts in FCE1 when the first target crate exists.

Baseline denied imports for extracted crates:

- `nimbus_server::`
- `use .*nimbus_server`
- `nimbus-server[[:space:]]*=`
- `crate::state`
- `crate::router`
- `crate::local_server`
- `crate::system_tenant`
- `crate::runtime_host`
- `crate::tenant`
- `AppState`
- `RouterBuildConfig`

## Tests

Command:

```text
bash scripts/verify-server-crate-extraction-completion.sh
```

Output:

```text
[1] Control-plane files exist
  PASS  Plan, proof directory, FCE0 proof, executable verifier, and predecessor verifier exist

[2] Predecessor SSE verifier remains authoritative
  PASS  Predecessor server seam extraction readiness verifier passed

[3] Phase ledger has exactly one active phase
  PASS  Exactly one phase is in_progress: FCE0 Baseline and verifier skeleton

[4] FCE0 proof records target crates and server-only shells
  PASS  FCE0 proof records target crates, server-only shells, and requirement coverage

[5] Known architecture crates are present and server-free
  PASS  Existing authority/system/bridge/license crates are present and do not depend on nimbus-server

[6] Reusable target-crate helper semantics are available
  PASS  Verifier includes reusable crate, dependency, denied-import, and proof helpers

[7] No premature optional aggregate adapter facade exists
  PASS  Optional nimbus-adapters facade has not been created before per-adapter crates are clean

[8] Extraction target crates follow the ledger state
  PASS  No completed extraction phase lacks its target crate/no-server dependency proof

Summary: 8 passed, 0 failed
```

Ignored tests:

- none recorded for FCE0.

## Verifier Update

- Conditions added or updated:
  - control-plane plan/proof/script presence
  - predecessor SSE verifier execution
  - exactly one `in_progress` phase in the FCE ledger
  - reusable `crate_exists`, `crate_has_no_server_dependency`, and denied-import scan helpers
  - FCE0 proof target-crate and server-only-shell evidence
- Current verifier result: `8 passed, 0 failed`.

## Residual Risk And Resume Notes

- Remaining risk: no target crate has been extracted yet; FCE0 only establishes the control plane.
- Next action: start FCE1 and extract `nimbus-artifacts`.
