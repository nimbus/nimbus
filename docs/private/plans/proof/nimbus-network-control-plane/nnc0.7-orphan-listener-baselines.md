# NNC0.7 Orphan And Listener-Group Baselines

Status: `expected-red predicates reproduced`

Source branch: `codex/nimbus-network-architecture-audit`

Starting HEAD: `011692cf0`

Execution base: `9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Environment: `aarch64-apple-darwin`, Rust test profile, local temporary
filesystem and loopback TCP

## Result

Three explicitly ignored tests preserve the two NNC0.7 failure families.

### Provider effects precede durable ownership

Both OCI-family implementations create the persistent namespace and run
Netavark before taking the allocator hold:

- container: `configure_network` creates the namespace at
  `container/runtime.rs:797`, starts provider setup at line 798, performs
  egress pin and optional machine forwarding, and does not call
  `segment_allocator().acquire` until lines 870-871; and
- krun: `configure_network` creates the namespace at
  `krun/vm/lifecycle.rs:240`, starts provider setup at line 241, performs the
  egress pin, and does not acquire the hold until lines 279-280.

`nnc0_7_effect_before_hold_crash_must_not_leave_an_unowned_provider_effect`
materializes the exact durable recovery image at that cut: namespace,
manifest, and provider-effect evidence exist, while no allocator hold or
desired attachment/provider attempt exists. NNC0.1b already proves
exact-boundary process kill and same-root recovery; reproducing that process
protocol inside `nimbus-sandbox` would require the forbidden
`nimbus-sandbox -> nimbus-testing -> nimbus-tenant -> nimbus-sandbox` cycle.

The current startup reaper enumerates only netns filenames and reconciles
allocator holds (`reaper.rs:67-76`). With no hold to iterate, it reports zero
reclaimed segments and leaves the namespace, manifest, and provider-effect
evidence unowned:

```text
assertion `left == right` failed:
NNCF8: recovery must remove or durably quarantine the provider effect and
netns when no desired attachment/provider attempt owns them
  left: "unowned-evidence-left-behind"
 right: "fully-removed"
```

NNC5.2a owns persisting exact desired attachment association plus the sandbox
provider attempt before effects. NNC5.2b owns replacing filename-only startup
classification with durable evidence. NNC8.3 owns repeated cleanup/release
convergence.

### Complete orphan evidence matrix

`nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix` executes
all eight required rows against separate durable roots:

| Evidence row | Safe disposition | Current observation |
| --- | --- | --- |
| hold + desired + effect | adopt | retained by netns filename |
| hold + no desired + effect | remove or quarantine | retained by netns filename |
| hold + no netns | remove or quarantine | hold released; desired/manifest left |
| effect + no hold | remove or quarantine | unowned netns/manifest/effect left |
| manifest + no hold | remove or quarantine | unowned manifest left |
| hold + netns + no manifest | adopt from canonical desired evidence | retained by netns filename |
| stale generation | remove or quarantine | retained by netns filename |
| unknown inspection | cleanup-pending | retained by netns filename |

All rows run before one terminal assertion prints the complete mismatch map and
full post-reconcile observations. The two valid adoption rows deliberately
show that retention happens for the wrong reason: current code cannot
distinguish matching desired/provider generations from stale or unknown
evidence.

### Partial sibling-listener startup

`nnc0_7_kth_adapter_failure_must_not_leave_prior_listener_live` drives the
real `serve` loop through two `WireProtocolAdapter` implementations. The first
binds an OS-assigned loopback port, records through the real engine path, and
spawns a task that serves `still-live`. A separately owned loopback socket
forces the second adapter bind to fail specifically with `AddrInUse`.

Current `serve` returns through `?` before reaching its only abort loop.
Dropping the first `JoinHandle` detaches the task, so the test connects after
startup has returned failure and receives all ten bytes. It captures and
aborts the detached task before the terminal assertion:

```text
NNCF17: startup returned Address already in use (os error 48), but the earlier
sibling listener still accepted and served bytes after the listener-group
failure
```

The connect/read is bounded to one second and does not use sleeps.
NNC7.1a owns the server-local structured listener group and full unwind/join.

## Commands and results

Each accepted fail-before command exited `101` at its named terminal safety
assertion:

```text
timeout 300 cargo test -p nimbus-sandbox --lib \
  nnc0_7_effect_before_hold_crash_must_not_leave_an_unowned_provider_effect \
  -- --ignored --nocapture
# 0 passed; 1 failed; 256 filtered out. Exit 101.

timeout 300 cargo test -p nimbus-sandbox --lib \
  nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix \
  -- --ignored --nocapture
# 0 passed; 1 failed; 256 filtered out. All eight rows printed. Exit 101.

timeout 300 cargo test -p nimbus-server --lib \
  nnc0_7_kth_adapter_failure_must_not_leave_prior_listener_live \
  -- --ignored --nocapture
# 0 passed; 1 failed; 512 filtered out. AddrInUse preceded a successful
# post-failure connection/read from the first adapter. Exit 101.
```

The changed sandbox seam and focused server seams remain green:

```text
timeout 900 cargo test -p nimbus-sandbox --lib
# 243 passed; 0 failed; 14 ignored.

timeout 300 cargo test -p nimbus-sandbox --lib \
  backends::oci::network::reaper::tests
# 1 passed; 0 failed; 3 expected-red ignored.

timeout 600 cargo test -p nimbus-server --lib adapters::wire::tests
# 6 passed; 0 failed.

timeout 600 cargo test -p nimbus-server --lib construction::tests
# 0 passed; 0 failed; 1 expected-red ignored.

timeout 1200 cargo clippy -p nimbus-sandbox -p nimbus-server \
  --all-targets -- -D warnings
# Exit 0; no warning from either changed crate. Existing vendored Brotli
# warnings remain outside the changed crates.

cargo fmt --all --check
git diff --check
# Exit 0.

bash scripts/check-docs.sh
# PASS — 108 pages link-clean, source map resolves, private fence intact,
# titles unique.

npm --prefix website ci
npm --prefix website run build
bash scripts/verify-nimbus-docs-site.sh
# Lockfile install completed; 109 pages built; 17/17 conditions green.
```

The full server library suite was also run and is not represented as green:

```text
timeout 1200 cargo test -p nimbus-server --lib
# 485 passed; 2 failed; 26 ignored.
```

Both failures reproduce alone on this merged-base branch and are outside this
test-only diff:

- `deploy_admin_requires_local_admin_header_even_with_deploy_bearer` expects
  `200` but receives `400`; and
- `cloud_functions_passes_runtime_owner_lifecycle_conformance` expects `200`
  but its isolated child receives `409`.

They are post-merge Cloud Functions trust-fixture regressions, not hidden
passes and not NNC0.7 behavior. This item does not modify those tests or any
production path; their evidence is retained for the owning trust-boundary
work rather than weakening assertions or expanding the network item.

The first docs-site verifier run reported its missing-build precondition at
condition 6 (16/17), as designed. The isolated worktree initially had no Astro
binary, so the lockfile-pinned `npm ci` install preceded the successful build
and verifier rerun. Generated dependencies/build output are ignored and do
not alter the tracked diff.

No random seed, wall-clock ordering, live OCI runtime, KVM, privileged
provider, cloud service, cluster, cross-target, or sovereignty-denial lane
applies. The orphan tests use separate temporary durable roots. The listener
test uses real loopback sockets and the real engine-backed listener projection
path.

## Independent closeout review

The three-file test/evidence diff was reviewed with the repository autoreview
skill and independent Claude Opus 4.8 at maximum reasoning. The review exited
`0`:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.72)
```

The reviewer independently traced both OCI effect-before-hold orderings,
confirmed the no-`nimbus-testing` dependency/cycle constraint, recomputed all
eight isolated matrix outcomes, and verified that none collapse to the safe
result. It also confirmed the real kth bind reaches `AddrInUse` only after the
first listener task owns its socket, the exact ten-byte read is bounded, the
abort runs before the terminal assertion, all additions are test-only, and no
production dependency or behavior changes. The two Cloud Functions failures
were confirmed unreachable from this diff.
