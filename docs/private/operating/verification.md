# Verification

This runbook defines how contributors select repository checks, preserve real
failure status, diagnose stalls, and record evidence. Tests and source own
behavior. Active plans can add narrower acceptance gates, but they cannot
weaken these contracts.

## Select the smallest truthful gate

| Change | During iteration | Before handoff or pull request |
| --- | --- | --- |
| One Rust concept | Focused `cargo test -p <crate> <test>` | Full affected crate tests, `cargo fmt --all --check`, and relevant static verifier |
| Cross-crate Rust behavior | Focused affected tests | `make clippy` and the plan's affected repository gates |
| JavaScript package | Workspace test/typecheck command | `npm run typecheck`, `npm run test`, and `npm run build` as applicable |
| Public documentation | Link and technical-writing checks | Both docs gates and the website build |
| Repository-wide or PR-ready behavior | Focused fail-before and regression tests | `make ci` when feasible, then hosted CI |

Do not mock the contract that a test must prove. Test success, boundaries,
failures, recovery, and concurrency when those behaviors apply.

## Canonical commands

```bash
cargo fmt --all --check
make check
make test
make clippy
make deny
make verify-harness
make verify-harness-nightly
make ci
npm run typecheck
npm run test
npm run build
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Use `make verify-harness SURFACE=<surface>` for a required focused harness and
`make verify-harness-repro SURFACE=<surface> MODE=pr CASE=<case-id>` to replay
one case. Nightly harnesses and live external-provider lanes are additional
evidence, not substitutes for required checks.

`make test` composes the isolated runtime lane, the Nextest non-runtime
workspace lane, and workspace doctests. Nextest gives each non-runtime test a
separate process. Tests can therefore prove that Nimbus rejects a second
in-process network composition without competing with unrelated tests in the
same binary.

## Fresh-checkout and single-flight rules

Make targets own generated UI and embedded-package prerequisites. Use them for
a clean checkout and for advertised one-command gates. Focused Cargo commands
are valid after those prerequisites exist when they do not compile a consumer
that needs missing generated input.

Long repository targets use `scripts/single-flight.sh`. If a duplicate exits,
read the reported owner and inspect host activity. Clear a lock only after the
owner is absent or proven stale. Do not start a second Cargo build in another
target directory as the default recovery method.

## Preserve the exit status

Capture output with a construct that returns the command's status. With Bash:

```bash
set -o pipefail
command_under_test 2>&1 | tee verification.log
status=${PIPESTATUS[0]}
exit "${status}"
```

Bound waits and health checks. A timeout must report the child process,
recent logs, expected readiness signal, and retained artifact path. Do not
classify a slow but active compile as a hang without host activity evidence.

### Host activity evidence

Prove host activity before you call a check idle or hung. Two probes report a
false negative on macOS hosts here:

- The `find` on this machine is `bfs`. It rejects a relative timestamp, so
  `find . -newermt "-5 minutes"` prints an error and no paths. That output is
  identical to "no file changed", which makes any activity check built on it a
  permanent false zero. Use `find . -newer <reference-file>`, or compare
  `stat -f %m` values.
- A long process log is not a progress signal. Read file modification times in
  the work tree instead.

Diagnose a suspected hang with `sample <pid>` before you stop the process. Long
elapsed time with near-zero accumulated processor time is a block, not slow
work, and the stack names the blocked frame. `.config/nextest.toml` stops a
slow test after 45 seconds and three strikes, so Nextest lanes and hosted CI
are protected. A bare `cargo test` has no timeout. Wrap a long check in
`timeout <seconds> ...` and require the same of a delegated job.

## Host ports in tests

Do not discover a host port with a probe. A probe binds port zero, reads the
assigned number, closes the socket, and gives the bare number to code that
binds it later. The port belongs to nobody between the close and the bind.
Every test is its own process under nextest, so a probe races the rest of the
suite. This reached CI as `failed to bind egress proxy on 127.0.0.1:38373:
address in use`.

Claim a window instead. `PortWindow` binds the first port of the window and
keeps that socket for the life of the window. The kernel refuses the same
address to a second process, so the claim excludes other processes without a
lock file, and process exit releases it:

```rust
use nimbus_process_harness::PortWindow;

let window = PortWindow::claim();   // keep alive while the ports matter
let pep_port = window.port(0);
let published = window.ports(8, 21);
```

The window region is below the host ephemeral range. An unrelated `bind(0)`
cannot receive a claimed port. Partition the window explicitly. Do not give
one offset to two consumers.

A server started by hand is a different matter. It binds a fixed port, and the
claim holds only the window's first one. `PortWindow` therefore withholds every
window that spans a conventional Nimbus port, so a local MongoDB adapter on
27017 costs the suite nothing. Any other long-lived listener inside
17000-31999 still owns the port it sits on: stop it, or move it out of the
region.

A wider port range is not a fix. The sandbox port coordinator walks its range
lowest-first and compares each candidate only against its own durable state,
which each test roots in its own temporary directory. The coordinator never
asks the kernel. Two test processes that share one range select the same first
port, so a wider shared range makes collision certain instead of rare.

This rule also covers the inverse assertion. A test that re-binds an address to
prove the product released it must source that address from a claimed window.
If it does not, a foreign process that takes the port first turns a correct
release into a false failure against the product.

The gate below fails on any new probe-and-release site. It holds no allowlist,
because the count is zero:

```bash
cargo test -p nimbus-process-harness --test host_ports_are_claimed_not_probed
```

## A receipt is published when it parses

A file that carries a value has two publication steps. The writer opens the
path, and then it writes the value. Between those steps the file exists and is
empty. Code that waits for existence and then reads the value gets nothing
whenever it lands in that window, and it reports a parse failure against a
writer that did nothing wrong. A loaded host makes the window wider, because
the writer can lose the processor between the open and the write.

Wait for the value instead:

```rust
let pid = wait_for_receipt(&receipt, timeout, read_pid)
    .expect("receipt should carry a pid");
```

`wait_for_path` waits for existence only. Keep it for a marker whose presence
is the whole message. Do not use it for a pidfile, an exit-status file, or any
other receipt that carries content.

Two rules follow for a test fixture that publishes a receipt:

- Read the receipt before you stop the writer. A stop that lands inside the
  window leaves the file empty for good, and no amount of waiting recovers it.
- Do not make the fixture publish atomically to hide the window. Nimbus does
  not own how conmon and crun publish their receipts, so the reader must
  tolerate a split that the writer is free to make.

This reached CI as
`descendant receipt should contain a PID: ParseIntError { kind: Empty }`.

## External providers and host capabilities

Use repository-owned provider fixtures instead of recreating image, port,
readiness, URL, or cleanup policy:

```bash
make test-external-provider PROVIDER=postgres
make test-external-providers
```

Provider and host-capability tests must distinguish unsupported, skipped, and
failed states. If a required provider is absent, the test must fail closed.
Record the host, provider mode, feature flags, selection, counts, and cleanup
result.

A missing cargo feature removes tests instead of failing them.
`cargo nextest run -p nimbus-storage` with no feature flag runs only the
embedded-provider tests, about 297, and reports green. The crate has no default
features, so every libSQL, MySQL, and PostgreSQL test is absent. The full set
of about 439 needs `--features libsql,mysql,postgres` plus live fixtures for
the provider lanes. `make` entrypoints and `--workspace` runs are unaffected
because workspace feature unification enables the providers.

A lane that cannot run is `UNVERIFIED`. It is never reported as green, and it is
never converted into a passing claim by a skip. Storage enforces that rule as a
test rather than as a convention: a closed matrix qualifies every tenant
provider against atomic effects, committer fencing, conditional admission,
journal progress, durable recovery, write isolation, and position parity, and a
cell whose cargo feature is disabled or whose fixture is absent degrades to
`UNVERIFIED`. A guarantee a provider does not own stays "not owned", which is a
fact about the provider and not about the host.

Physical durability is proved against real failures rather than modeled. The
`nimbus-storage` physical durability lane fails bounded SQLite write, sync, and
write-ahead-log operations through a test-only VFS shim, then reopens the store
and compares the durable head, the applied head, and the materialized position
against the last acknowledged result. Keep that fault control inside the test
tree: the production binary carries no fault configuration and no VFS selection
surface.

```bash
cargo test -p nimbus-storage sqlite_physical_durability -- --nocapture
```

## Application verification

Use `make examples-verify` for the nine-case live application lane. It requires
Node.js `>=22 <25`. Repository gates test Node.js 22 and 24. The default runs
one case at a time for diagnosis. Use bounded parallel execution when you need
the complete lane faster:

```bash
NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL=5 make examples-verify
NIMBUS_EXAMPLES_VERIFY_ONLY=convex/tasks make examples-verify
```

`NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL` accepts 1 through 9. CI uses five. The
case selector accepts one exact name from `scripts/examples-verify-cases.json`.

A reliable application run must:

- use tracked source as read-only input and compare byte digests on every exit.
- use disposable application workspaces.
- use one run-global network-state root and case-local application, data,
  control, authentication, discovery, audit, and log roots.
- consume provider-assigned product port leases and retained listeners.
- own all child processes and temporary resources.
- fail when cleanup is incomplete.
- emit validated machine-readable results that separate intended assertions,
  observed outcomes, and cleanup status.

Successful runs leave `report.json` and `junit.xml` under
`target/examples-verify-results/<run-id>/`. A failed run prints one absolute
`retained diagnostic artifacts` path on stderr. Inspect its case logs,
process records, network lease state, source comparison, and cleanup result.
The retained tree does not contain a smoke credential file. Do not delete it
until you record the first causal error and the final cleanup state.

The manifest records `push`, `polling`, or `request-response` for each case.
Push means that a subscription receives a change. Polling means that repeated
reads observe a change. It does not prove subscription support.

[`../plans/README.md`](../plans/README.md) routes the active owner and its exact
verifier contract.

## Evidence and failure diagnosis

Record the exact command, revision, host or provider, exit status, test counts,
skips, elapsed time, and artifact path. Keep raw output separate from the
verdict. When a check fails:

1. Confirm that the failure is from the process under test, not a stale
   listener or unrelated service.
2. Capture the first causal error and the final status.
3. Reproduce with the smallest repository-owned command that keeps the same
   contract.
4. Add a fail-before regression test when automation can reproduce the defect.
5. Rerun affected checks after the fix. Reserve full gates for candidate
   closeout.

Hosted CI remains the authority for platform-only, coverage-upload,
pointer-compression, Bun, external-provider, node D-Bus, and scheduled Node
compatibility evidence that a local host cannot supply.

## Sandboxed GitHub authentication

If `gh` reports an invalid token or an authentication failure inside the
sandbox, retry the same GitHub CLI operation with elevated permissions before
you treat the credential as broken. Record a credential blocker only after the
elevated command also fails.
