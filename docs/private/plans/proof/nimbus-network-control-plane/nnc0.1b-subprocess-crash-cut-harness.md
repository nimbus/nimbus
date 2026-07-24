# NNC0.1b Persistence-Oriented Subprocess Crash-Cut Harness

Status: `passed`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `53ea4986a1e65eebce8504b113943311acdcd52d`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, macOS, Rust test and nextest profiles

## Result

`nimbus-testing::SubprocessCrashCutHarness` now provides the reusable
upper-layer coordinator for exact-boundary crash and fresh-process recovery
proofs. It shares the process capture, timeout, diagnostic, and cleanup
machinery introduced by NNC0.1a while keeping the crash protocol in the
concept-owned `process_harness/crash.rs` child module.

The protocol is:

```text
crash child -> ready
parent      -> run
crash child -> boundary:<caller-owned-token>
parent      -> kill and reap at the acknowledged boundary

fresh recovery child -> ready
parent               -> inspect
fresh recovery child -> recovered:<caller-owned-observation>
fresh recovery child -> exit
```

The parent rejects an empty, malformed, or different boundary and never treats
it as the requested crash cut. At the exact boundary it verifies the child is
still live, kills it, waits for its status, and records
`killed-at-boundary-and-reaped`. Only then does it spawn a different named role
over the same canonical state root. The recovery observation must exactly match
the parent's expected durable state/effect token and the fresh process must
exit successfully.

## Dependency and fault-point ownership

The coordinator accepts semantic tokens but does not define Nimbus network
product fault points. Future `nimbus-network` tests keep their typed boundary
vocabulary and child-role wiring in a dependency-safe local test-support
module, then use this upper-layer coordinator from an integration-test owner.
Provider-local boundaries stay with their existing sandbox/server/proxy
adapters.

No Cargo manifest changed. The future low-level crate therefore gains no normal
or dev edge to `nimbus-testing`, and this item introduces no policy, provider,
socket, cluster-transport, or persistence authority.

The self-test uses the network-shaped boundary
`network.store.after-state-and-effect-sync`. The crash child creates and
`sync_all`s separate state/effect evidence plus the containing directory on the
verified Unix host, acknowledges that exact boundary, and parks. A fresh child
reads the same root and must report
`state-committed:effect-created`. This proves the harness mechanics; it does not
claim that current segment/IPAM direct JSON replacement is crash safe.

## Behavioral proof

The six crash-harness parent tests prove:

| Condition | Asserted result |
| --- | --- |
| Exact boundary and recovery | Parent kills only at the expected boundary; fresh process sees exact state/effect; both phase diagnostics are retained. |
| Wrong boundary | Expected and actual names are reported; the recovery role never starts; cleanup kills and reaps the parked child. |
| Crash-child early exit | Error names the expected boundary and retains role, stdout/stderr, failing status, and last `ready`. |
| Crash-child boundary timeout | Bounded timeout names the missing boundary; cleanup kills and reaps the child. |
| Wrong recovery observation | Error names expected and actual observations and retains both crash and recovery diagnostics. |
| Recovery early exit/timeout | Both modes name the failed recovery checkpoint; an exited child is reaped and a parked child is killed and reaped. |

Together with the seven NNC0.1a parent tests, the focused module contains 13
passing parent tests and two ignored child-role entrypoints. Those ignored tests
are invoked directly in real subprocesses; they are not skipped evidence lanes.
No test uses polling sleep or an unbounded semantic wait.

## Commands and results

Final verification:

```text
timeout 180 cargo check -p nimbus-testing --all-targets
# Exit 0; finished dev profile.

timeout 180 cargo test -p nimbus-testing process_harness -- --nocapture
# 13 passed; 0 failed; 2 ignored child entrypoints; 49 filtered out.
# Test execution finished in 2.11s.

timeout 180 cargo nextest run -p nimbus-testing -E 'test(process_harness)'
# 13 passed; 51 skipped by the focused expression.
# Nextest execution finished in 2.108s.

timeout 180 cargo clippy -p nimbus-testing --all-targets -- -D warnings
# Exit 0; no nimbus-testing warning.

cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
```

The first clippy invocation correctly failed on
`clippy::items-after-test-module`; the two test-only protocol helpers were moved
above the test module, and the final clippy command above passed. Existing
vendored Brotli `unexpected_cfgs`, dead-code, and lifetime-syntax warnings
remain dependency output and do not originate in the changed crate.

No KVM, Netavark, external provider, cross-target, or sovereignty-denial lane
applies to this generic process utility. No random seed is used.

## Independent closeout review

The frozen six-file checkpoint was reviewed with:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local \
  --engine claude \
  --model claude-opus-4-8 \
  --thinking max
```

The command exited `0` with:

```text
autoreview clean: no accepted/actionable findings reported
```

No finding was accepted or rejected. The reviewer explicitly audited the
non-`Copy` checkpoint refactor, semantic-token framing, exact-boundary
kill/reap paths, bounded waits, same-root recovery, contention-harness
regression risk, and dependency ownership.
