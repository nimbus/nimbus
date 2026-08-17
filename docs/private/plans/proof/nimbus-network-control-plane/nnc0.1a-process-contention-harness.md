# NNC0.1a Deterministic Two-Process Contention Harness

Status: `passed`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `929cf8955098fb8da91e454dd1aea558e88b8342`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, macOS, Rust test and nextest profiles

## Result

`nimbus-testing` now owns a reusable, dependency-safe real-process contention
harness. It spawns exactly two named child roles against one canonical state
root and coordinates them through the following flushed pipe protocol:

```text
child -> ready
parent -> enter
child -> entered
parent -> release
child -> released
child -> complete:won | complete:lost
```

Each phase has one shared deadline. The implementation uses semantic pipe
events and `recv_timeout`; it contains no polling sleep. Completion is accepted
only when exactly one child reports `won`, the other reports `lost`, both stdout
pipes close within the bound, and both child processes exit successfully.

The child operation receives its stable role and the same canonical state root.
The proof fixture performs a real cross-process `create_new` against one winner
file, syncs the winning role, and asserts the durable file agrees with the
parent's result.

## Failure diagnostics and cleanup

Every process-protocol failure cleans up and reaps all started children before
returning. The error retains, per role:

- complete captured stdout;
- complete captured stderr;
- exit status and portable success classification;
- last valid semantic checkpoint; and
- cleanup outcome such as `exited-and-reaped` or `killed-and-reaped`.

Named self-tests prove:

| Condition | Asserted evidence |
| --- | --- |
| Missing participant | Timeout names the missing role and `ready`; no checkpoint is fabricated. |
| Wrong checkpoint | Error names expected `ready`, actual `complete:won`, and retains that last checkpoint. |
| Early exit | Error reports status, stderr marker, and no fabricated checkpoint. |
| Timeout after release | Error names `complete` and retains `released` as the last boundary. |
| Cleanup | Two blocked children are both killed, waited, and retain reaped statuses. |
| Invalid bound | Zero timeout is rejected before either child starts or mutates the state root. |

The ignored `contention_protocol_child` test is not a skipped proof lane. It is
the child-role entrypoint and is invoked explicitly by the seven parent
self-tests' real subprocesses.

## Dependency direction

The harness lives in `nimbus-testing`, which is already an upper-layer test
fixture crate. No Cargo manifest changed. The future low-level
`nimbus-network` crate therefore gains neither a normal nor a dev dependency on
`nimbus-testing`; upper-layer integration tests may depend downward on
`nimbus-network` and reuse this harness without a cycle. Network-specific fault
point vocabulary remains outside this generic process coordinator.

## Commands and results

All final verification commands exited `0`:

```text
cargo check -p nimbus-testing --all-targets
# Finished dev profile in 1m 44s.

timeout 300 cargo test -p nimbus-testing process_harness -- --nocapture
# 7 passed; 0 failed; 1 ignored child entrypoint; 49 filtered out.
# Test execution finished in 2.01s.

timeout 120 cargo nextest run -p nimbus-testing -E 'test(process_harness)'
# 7 passed; 50 skipped by the focused expression.
# Nextest execution finished in 2.029s.

timeout 180 cargo clippy -p nimbus-testing --all-targets -- -D warnings
# Finished dev profile; no nimbus-testing warning.

cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
```

The first cold `timeout 120 cargo test ...` attempt exited `124` while compiling
dependencies; no test binary had started. It was not counted as test evidence.
After the shared test-profile cache was populated, the bounded 300-second rerun
above passed. Clippy emitted existing vendored Brotli `unexpected_cfgs`,
dead-code, and lifetime-syntax warnings while still exiting `0`; no warning
originated in the changed crate.

No KVM, Netavark, external provider, cross-target, or network-denial lane
applies to this process-test utility item. No random seed is used.

## Independent closeout review

The structured closeout used the repository autoreview helper with the
independent Claude engine required for a Codex-authored change:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local \
  --engine claude \
  --model claude-opus-4-8 \
  --thinking max
```

The first pass reported one actionable P3: the `silent` fixture returned after
cleanup closed stdin, so it could race the test's strict
`killed-and-reaped` assertion by exiting voluntarily. The finding was accepted
and fixed by parking that fixture after stdin EOF; only the harness kill/wait
path can now terminate it. Cargo test, nextest, and clippy were rerun and
passed with the counts above.

The required second review pass exited `0` with:

```text
autoreview clean: no accepted/actionable findings reported
```

No finding was rejected. The reviewer noted unbounded `wait()` after stdout EOF
as non-actionable sibling hardening: EOF for these owned test binaries means
process teardown is already in progress, while every semantic wait and the
outer verification commands remain explicitly bounded.
