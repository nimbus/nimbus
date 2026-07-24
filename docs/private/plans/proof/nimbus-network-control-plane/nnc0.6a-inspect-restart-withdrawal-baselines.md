# NNC0.6a Inspect/Restart Withdrawal Baselines

Status: `expected-red predicates reproduced`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `f57bfbb3f`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, Rust test profile, local temporary
filesystem, intercepted provider-launch entry

## Result

Two explicitly ignored tests prove that both execute-mode observation paths
currently own workload restart side effects:

- `ContainerSandboxBackend::inspect_sync` calls
  `maybe_restart_after_exit`, which selects `RestartNow`, resets runtime
  artifacts, and enters `launch_manifest`; and
- `KrunSandboxBackend::inspect_sync` calls its restart-policy branch, resets
  runtime artifacts, and enters its own `launch_manifest`.

The shared `RestartLaunchTestProbe` is compiled only under `cfg(test)`. It
intercepts the first instruction of each real `launch_manifest`, acknowledges
that inspection entered launch authority, and parks on a predicate-guarded
condition variable. The coordinator then persists the same manifest as
withdrawn (`shutdown_requested = true`, `Stopped`, no endpoints) before
releasing the provider entry.

On release, stale inspection records one launch effect, restores `Starting`,
and overwrites the durable withdrawal. Both tests reach their final safety
assertion and fail with the backend-specific NNCF20 diagnostic:

```text
assertion `left == right` failed:
NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before
release must veto the stale container restart provider effect
  left: 1
 right: 0

assertion `left == right` failed:
NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before
release must veto the stale krun restart provider effect
  left: 1
 right: 0
```

This is not a readiness test. The terminal predicate counts entry into the
provider launch authority after a durable withdrawal. NNC5.6 owns making
inspection side-effect-free; NNC6.4a owns routing eligible restart through the
compute saga with current generation, attachment, and PEP evidence.

## Harness properties

`RestartLaunchTestProbe` uses one mutex-protected state record and
`Condvar::wait_timeout_while` for both directions:

1. inspection marks `entered` while holding the lock;
2. the parent waits on `!entered` with a one-second bound;
3. inspection waits on `!released` with the same bound;
4. the parent persists withdrawal, sets `released`, and notifies;
5. the intercepted provider entry increments `effects`; and
6. thread join establishes completion before the final assertion.

Predicate re-checking prevents lost wakeups and spurious wakeups from changing
the ordering. If a fixture fails before launch entry, the test joins the
inspection thread and prints its concrete result. If the parent fails after
entry but before release, the worker times out with a named error rather than
parking the test process forever.

The initial fixture used `/bin/true` for the reset command. The first
characterization run failed before the barrier on this host because that path
does not exist. It was corrected to the portable repository-host path
`/usr/bin/true`, and only the subsequent runs that reached the terminal NNCF20
assertion count as expected-red evidence.

## Commands and results

Both accepted fail-before commands exited `101` at the terminal zero-launch
assertion:

```text
timeout 180 cargo test -p nimbus-sandbox \
  nnc0_6a_container_inspect_must_not_restart_after_withdrawal \
  -- --ignored --nocapture
# 0 passed; 1 failed; 254 filtered out. inspect returned Starting and the
# intercepted launch effect count was 1 after withdrawal.

timeout 180 cargo test -p nimbus-sandbox \
  nnc0_6a_krun_inspect_must_not_restart_after_withdrawal \
  -- --ignored --nocapture
# 0 passed; 1 failed; 254 filtered out. inspect returned Starting and the
# intercepted launch effect count was 1 after withdrawal.
```

The ordinary suite and static gates remained green:

```text
timeout 300 cargo test -p nimbus-sandbox
# Library: 243 passed; 0 failed; 12 ignored.
# Guest-user-switch binary: 2 passed; 0 failed.
# Platform integration/doc targets: 0 applicable tests on this host.

timeout 300 cargo clippy -p nimbus-sandbox --all-targets -- -D warnings
# Exit 0; no warning from nimbus-sandbox. Existing vendored Brotli warnings
# remain outside the changed crate.

cargo fmt --all --check
git diff --check
# Exit 0.
```

No random seed, sleep, wall-clock ordering, live OCI runtime, KVM, privileged
provider, cloud service, cluster, cross-target, or sovereignty-denial lane
applies. The launch interceptor is the provider test double; the real
inspection, restart-policy, reset, durable-manifest, and `launch_manifest`
control flow executes unchanged up to that boundary.

## Independent closeout review

The seven-file test diff was reviewed with the repository autoreview skill and
independent Claude Opus 4.8 at maximum reasoning, with the fail-before phase
boundary supplied explicitly. The review exited `0`:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.72)
```

The reviewer independently confirmed that the interceptor is at the true
provider-launch entry, the withdrawal/stale-copy overwrite is deterministic,
the final predicate proves launch authority rather than readiness, the
condition-variable protocol is bounded and lost-wakeup-safe, no panic can
permanently strand the process, and every hook/field/type is absent from
production and disabled by default in ordinary test instances.
