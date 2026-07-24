# NNC0.6 Withdrawal And Attachment-Readiness Baselines

Status: `expected-red predicates reproduced`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `d68b548ed`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, Rust test profile, local temporary
filesystem and loopback

## Result

Three explicitly ignored tests preserve the two distinct NNC0.6 failure
families.

### Service resolution remains published during stop

`nnc0_6_service_binding_is_withdrawn_before_backend_stop_waits` starts a ready
sandbox-backed service, then parks the stub backend's stop future at a
semantic semaphore boundary. The parent test receives the `entered`
acknowledgement before performing the cache-only
`resolve_service_binding` lookup.

The current service manager awaits backend stop before removing the ready
handle from `ServiceManagerState`. The concurrent lookup therefore returns a
routable binding after stop has begun. The test always releases and joins the
parked task before asserting the safe invariant, so an expected panic cannot
strand the Tokio runtime:

```text
NNCF11: service resolution must be withdrawn before awaiting backend stop;
the cache still returned a routable binding after stop had begun
```

NNC6.6 owns fencing or withdrawing logical resolution before the awaited stop.

### OCI readiness lacks complete attachment evidence

`nnc0_6_container_is_not_ready_at_partial_attachment_boundary` builds a real
container manifest, persists only its network-namespace path, and proves the
Netavark status artifact is absent. With no published endpoint probe to
override the result, current `running_status` reports `Ready` solely from
workload liveness:

```text
assertion `left != right` failed:
NNCF6: workload liveness without complete same-generation attachment evidence
must not publish container readiness
  left: Ready
 right: Ready
```

`nnc0_6_krun_rejects_netns_path_without_complete_attachment_evidence` reaches
the same namespace-created/Netavark-status-absent boundary for krun. A real
loopback PEP with an active deny-all policy is registered to isolate the
remaining condition. Current `ensure_execute_egress_preconditions` accepts the
namespace path plus ready PEP even though neither the Netavark result nor an
egress-pin phase is represented:

```text
NNCF6: a netns path plus ready PEP cannot prove Netavark setup or egress pin;
partial same-generation attachment must deny launch
```

NNC5.2 owns a durable provider handle and explicit attachment phase/generation;
later attachment contract work must make both proofs green without moving
Netavark, nftables, PEP, or socket effects into `nimbus-network`.

## Commands and results

Each fail-before command exited `101` at its terminal safety assertion:

```text
timeout 180 cargo test -p nimbus-services \
  nnc0_6_service_binding_is_withdrawn_before_backend_stop_waits \
  -- --ignored --nocapture
# 0 passed; 1 failed; 93 filtered out. The semantic stop barrier was entered,
# released, and joined before the stale routable binding assertion failed.

timeout 180 cargo test -p nimbus-sandbox \
  nnc0_6_container_is_not_ready_at_partial_attachment_boundary \
  -- --ignored --nocapture
# 0 passed; 1 failed; 252 filtered out. Partial attachment reported Ready.

timeout 180 cargo test -p nimbus-sandbox \
  nnc0_6_krun_rejects_netns_path_without_complete_attachment_evidence \
  -- --ignored --nocapture
# 0 passed; 1 failed; 252 filtered out. Namespace path plus ready PEP was
# accepted without complete attachment evidence.
```

The ordinary suites and static gates remained green:

```text
timeout 300 cargo test -p nimbus-services
# 93 passed; 0 failed; 1 expected-red ignored.

timeout 300 cargo test -p nimbus-sandbox
# Library: 243 passed; 0 failed; 10 ignored.
# Guest-user-switch binary: 2 passed; 0 failed.
# Platform integration/doc targets: 0 applicable tests on this host.

timeout 300 cargo clippy -p nimbus-services -p nimbus-sandbox \
  --all-targets -- -D warnings
# Exit 0; no warning from either changed crate. Existing vendored Brotli
# warnings remain outside the changed crates.

cargo fmt --all --check
git diff --check
# Exit 0.
```

The service wait uses one-second bounded semantic acknowledgements, not sleeps.
No random seed, wall-clock ordering, cloud service, KVM, privileged provider,
live cluster, cross-target, or sovereignty-denial lane applies. The krun
fixture uses a real loopback PEP only to establish the positive control that
policy readiness is not the missing condition.

## Independent closeout review

The four-file test diff was reviewed with the repository autoreview skill and
independent Claude Opus 4.8 at maximum reasoning, with the fail-before phase
boundary supplied explicitly. The review exited `0`:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.78)
```

The reviewer independently confirmed that the semaphore handoff is
deterministic and bounded, release and join precede the expected-red assertion,
the optional stub seam leaves ordinary behavior unchanged, the two backend
fixtures isolate incomplete attachment evidence at their actual current
decision points, and the pass-after owners match NNC5.2 and NNC6.6.
