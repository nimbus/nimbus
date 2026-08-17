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

## Application verification

Use `make examples-verify` for the current live application lane. The active
documentation and application verification plan owns its current gaps. They
affect fresh-checkout prerequisites, source immutability, listener ownership,
operator-state isolation, evidence, cleanup, and run time.
Treat its console pass as application smoke evidence only. Do not describe it
as hermetic or concurrency-safe.

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
