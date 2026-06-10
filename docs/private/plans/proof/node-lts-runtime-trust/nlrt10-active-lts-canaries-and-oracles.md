# NLRT10 Active LTS Canaries And Oracles

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Expand package/framework canary evidence so every supported LTS lane has its
own lane-local checks, add a version-matched Node24 oracle sample, and add a
verifier that prevents the product-default Node22 lane from standing in for
Node24 support evidence.

## Files Changed

- `crates/nimbus-runtime/src/limits/resources.rs`
- `crates/nimbus-runtime/src/limits/tests.rs`
- `crates/nimbus-runtime/src/runtime/tests/basic_invocation/package_resolution.rs`
- `crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs`
- `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`
- `tests/runtime/node/canary-registry.json`
- `tests/runtime/node/networking-canaries/bundles/platform.mjs`
- `tests/runtime/node/README.md`
- `scripts/runtime/node/canary_registry.py`
- `scripts/runtime/node/dashboard.py`
- `scripts/verify-node-lts-canaries-and-oracles.sh`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `docs/architecture/runtime/node-compat-evidence/latest/*`
- `docs/runtimes/nodejs/evidence/latest.md`
- `docs/runtimes/nodejs/evidence/node22.md`
- `docs/runtimes/nodejs/evidence/node24.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt10-active-lts-canaries-and-oracles.md`

## Decisions

- Added a `node-platform-builtins` application canary that exercises ESM/CJS
  loading, process metadata, fs/path, streams, timers, crypto, and fetch/http
  under the selected Node lane.
- Added Node24 application and tooling canary batches instead of reporting the
  Node22 batch as shared evidence.
- Kept Node20 Express/Fastify canaries as legacy-grace regression checks, but
  removed Node20 from active public canary claim coverage.
- Added Convex `"use node"` action package metadata canaries in
  `nimbus-convex` and taught the canary runner to execute canary lane runs from
  crates other than `nimbus-runtime`.
- Replaced the stale tooling invariant that forced `RuntimePreset::Tooling` to
  Node22. Tooling profiles now require an explicit Node target and support
  Node20, Node22, and Node24 constructors.
- Added `scripts/verify-node-lts-canaries-and-oracles.sh` to enforce supported
  LTS lane coverage, required NLRT10 surfaces, non-borrowed cargo tests,
  passing published canary claims, version-matched oracle majors, and zero
  required canary gaps.

## Evidence Summary

Published dashboard:

```text
canary_report_count: 2
canary_claim_count: 12
canary_check_count: 26
oracle_report_count: 2
required_canary_gaps: 0
```

Application canary report:

```text
target/node-compat/canaries/preset-application.json
canary_count: 16
passed: 16
failed: 0
node20 legacy: 2 passed
node22 supported/default: 7 passed
node24 supported: 7 passed
```

Tooling canary report:

```text
target/node-compat/canaries/preset-tooling.json
canary_count: 10
passed: 10
failed: 0
node22 supported/default: 5 passed
node24 supported: 5 passed
```

Oracle reports:

```text
node22 test/parallel/test-buffer-alloc.js: recorded Node oracle v22.22.2, agreement_pass
node24 test/parallel/test-buffer-alloc.js: Node oracle v24.16.0, agreement_pass
```

## Verification

The first sandboxed application canary run failed with loopback `EACCES`; the
canaries intentionally bind local sockets. The rerun with approved escalation
passed.

```text
make node-compat-canaries PRESET=application
application_node22_networking_package_canary_batch: 1 passed
application_node24_networking_package_canary_batch: 1 passed
application_node20_networking_legacy_canary_batch: 1 passed
convex_use_node_action_package_canary_node22: 1 passed
convex_use_node_action_package_canary_node24: 1 passed
report: 16 passed, 0 failed
```

```text
make node-compat-canaries PRESET=tooling
tooling_node22_package_canary_batch: 1 passed
tooling_node24_package_canary_batch: 1 passed
report: 10 passed, 0 failed
```

```text
make node-compat-oracle LANE=node24 SAMPLE=test/parallel/test-buffer-alloc.js NODE_BIN=/Users/jack/.local/share/mise/installs/node/24.16.0/bin/node
node_compat_oracle_entrypoint_emits_fixture_artifact: 1 passed
oracle artifact: target/node-compat/oracle/node24/test-parallel-test-buffer-alloc-js/oracle-node24-test-parallel-test-buffer-alloc-js.json
```

```text
bash scripts/verify-node-lts-canaries-and-oracles.sh
Summary: 12 passed, 0 failed
```

```text
bash scripts/runtime/node/validate-claims.sh
validated 12 active claim mappings against 12 registered canaries
```

```text
cargo test -p nimbus-runtime package_resolution -- --nocapture
6 passed; 0 failed; 5 ignored
```

```text
cargo test -p nimbus-runtime tooling_preset_requires_node_target -- --nocapture
1 passed
```

```text
cargo test -p nimbus-runtime runtime_self_exec_run_grant_requires_node_target -- --nocapture
1 passed
```

```text
cargo test -p nimbus-convex runtime_access -- --nocapture
2 passed; 0 failed; 2 ignored
```

```text
bash scripts/verify-node-lts-docs.sh
Node LTS docs guard passed
```

```text
python3 scripts/runtime/node/publish_docs.py --check
Node.js runtime evidence docs are current
```

```text
cargo fmt --all --check
pass
```

```text
npm run docs:validate-refs:strict
docs reference validation: pass (222 working-tree Markdown files)
```

```text
git diff --check
pass
```

## Remaining Risks

- The Node22 oracle is a recorded dashboard artifact from the existing evidence
  set because this machine currently has a local Node24 binary through `mise`
  but no local Node22 binary in the shell environment.
- Tooling canary bundles still use the repo's host-node shim for package CLI
  subprocesses; the lane-local claim is enforced at the Nimbus runtime target
  and canary runner level. A future stricter lane can make the shim itself
  version-managed per target.
