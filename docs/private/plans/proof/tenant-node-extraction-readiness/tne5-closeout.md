# TNE5 Closeout

- **Phase ID:** TNE5
- **Status:** `done`
- **Git base:** `199c4c97` on `main`
- **Files touched:**
  - `scripts/verify-tenant-node-extraction-readiness.sh`
  - `docs/plans/tenant-and-node-crate-extraction-readiness-plan.md`
  - `docs/plans/proof/tenant-node-extraction-readiness/tne5-closeout.md`
- **Requirement IDs touched:** REQ-EFFECTS, REQ-VERIFIER,
  REQ-TENANT-CRATE, REQ-ADMIT, REQ-RAW, REQ-SYSTEM, REQ-STATUS,
  REQ-CREDS, REQ-HOST, REQ-TRUST, REQ-NODE-CRATE, REQ-DOCS

## Closeout Verifier

Added `scripts/verify-tenant-node-extraction-readiness.sh`.

The verifier checks:

- plan and proof presence
- workspace membership for `nimbus-tenant` and `nimbus-node`
- server `local_enforcement` shim shape
- forbidden tenant-domain side-effect imports
- `nimbus-node` normal dependency tree
- forbidden node crate imports for server, adapters, storage providers,
  `_nimbus` persistence, process launch, runtime executors, and HostBridge
- production reconciler calls to `validate`, `inspect`, `start`, and `stop`
- server-owned `SystemTenantStatusEvidenceWriter`
- `_nimbus` system/operator authority and projection/status match checks
- focused tenant, node, artifact verifier, system tenant, and tenant isolation
  tests
- workspace check, relevant clippy lanes, formatting, diff check, and docs
  reference validation

## Requirement Matrix

| ID | Evidence |
| --- | --- |
| REQ-EFFECTS | Verifier checks `nimbus-tenant` source and manifest for process launch, filesystem trusted-root probing, host lifecycle, runtime executor, server, storage, transport, and adapter imports. |
| REQ-VERIFIER | `cargo test -p nimbus-server artifact_verifier_effects -- --nocapture` passed: 37 passed, 0 failed, 745 filtered out. |
| REQ-TENANT-CRATE | `cargo test -p nimbus-tenant -- --nocapture` passed: 79 passed, 0 failed. Verifier confirms tenant crate side-effect/import audit. |
| REQ-ADMIT | Tenant isolation conformance passed: 3 selected tests passed, including 21 harness scenarios, 12 allowed and 9 denied. |
| REQ-RAW | `nimbus-node` tests cover systemd unit sanitization, property allowlists, trusted `ExecStart`, redaction, and high-cardinality metric exclusion. |
| REQ-SYSTEM | `cargo test -p nimbus-server system_tenant -- --nocapture` passed in TNE4: 14 passed, 0 failed. Verifier confirms `_nimbus` writer remains server-owned and requires system/operator authority. |
| REQ-STATUS | `nimbus-node` tests cover assigned node, workload UID, generation, decision ID, writer node, cleanup target misuse, and desired-state mutation denial. |
| REQ-CREDS | `nimbus-node` credential tests cover missing grant, wrong audience, wrong node, stale generation, wrong invocation, subject echo-back, and missing redaction metadata. |
| REQ-HOST | `nimbus-node` tests cover direct-process and systemd transient lifecycle paths. Verifier confirms production reconciler calls `validate`, `inspect`, `start`, and `stop`. |
| REQ-TRUST | `nimbus-node` runtime-pool trust test proves monotonic exposure and teardown on downgrade reuse. |
| REQ-NODE-CRATE | `cargo tree -p nimbus-node -e normal --depth 1` shows only `nimbus-core`, `nimbus-tenant`, `serde`, and `sha2` normal deps. Verifier checks no server/adapters/storage/persistence imports. |
| REQ-DOCS | `npm run docs:validate-refs:strict` passed: 213 working-tree Markdown files checked. |

## Final Verification Log

- `bash scripts/verify-tenant-node-extraction-readiness.sh`
  - First sandboxed run: 16 passed, 2 failed because server tests that bind
    localhost listeners hit sandbox `Operation not permitted`.
  - Final escalated rerun after adding the TNE5 proof check: 18 passed,
    0 failed.
- `cargo test -p nimbus-tenant -- --nocapture`
  - Result: 79 passed, 0 failed, 0 filtered out.
- `cargo test -p nimbus-node -- --nocapture`
  - Result: 27 passed, 0 failed, 0 filtered out.
- `cargo test -p nimbus-server artifact_verifier_effects -- --nocapture`
  - Result: 37 passed, 0 failed, 745 filtered out; integration targets
    selected 0 tests.
- `cargo test -p nimbus-server system_tenant -- --nocapture`
  - Result: 14 passed, 0 failed, 768 filtered out; integration targets
    selected 0 tests.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture --test-threads=1`
  - Result: 3 passed, 0 failed, 779 filtered out; integration targets selected
    0 tests.
  - Harness: 21 scenarios, 12 allowed, 9 denied.
- `cargo check --workspace`
  - Result: pass; finished dev profile in 14.69s.
- `cargo clippy -p nimbus-tenant --all-targets --no-deps`
  - Result: pass.
- `cargo clippy -p nimbus-node --all-targets --no-deps`
  - Result: pass.
- `cargo clippy -p nimbus-server --all-targets --no-deps`
  - Result: pass.
- `cargo fmt --all --check`
  - Result: pass.
- `git diff --check`
  - Result: pass with no whitespace errors.
- `npm run docs:validate-refs:strict`
  - Result: pass; 213 working-tree Markdown files checked.

## Closeout Decision

The plan is complete. The crate boundaries are now earned:

- `nimbus-tenant` owns pure tenant authority and evidence.
- `nimbus-node` owns node-local enforcement and observed status.
- `nimbus-server` owns transport, adapters, composition, and `_nimbus`
  persistence wiring.

No remaining blocker prevents using these crates as the tenant/node
control-plane boundary.
