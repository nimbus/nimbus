# NNC0.9 Behavior and Performance Baseline

Date: 2026-07-23

Source commit before the item: `3959df383`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Purpose

NNC0.9 records current listener lifecycle behavior and the scaling shape of the
two allocation authorities that later extraction bands will replace. This is a
characterization baseline, not an optimization or performance service-level
objective. Timing values are evidence only; pass/fail remains based on exact
behavior so ordinary CI is deterministic.

No production code or dependency edge changed:

- the existing authenticated server-shutdown smoke test now emits timing from
  the listener-bind boundary through health readiness and graceful shutdown;
- two explicit ignored sandbox tests measure current manifest-scan port
  selection and durable segment-state assignment at fixed scales; and
- all three runners assert the exact selected port, CIDR, readiness response,
  authenticated shutdown response, and clean server-task exit.

## Environment

| Property | Value |
| --- | --- |
| Host | Apple M2 Max, 32 GiB RAM |
| OS | macOS 15.7.2 (`24G325`), Darwin arm64 `24.6.0` |
| Filesystem | local journaled APFS on `/System/Volumes/Data` |
| Rust | `rustc 1.96.1 (31fca3adb 2026-06-26)` |
| Cargo | `cargo 1.96.1 (356927216 2026-06-26)` |
| Profile | Cargo `test` profile, unoptimized + debuginfo |
| Scheduling | one test thread; scale cases and samples execute serially |

Scale measurements use 21 samples per state size. The reported median is the
11th ordered sample and p95 is the nearest-rank 20th sample. Setup/seed time is
excluded from individual next-allocation measurements. Files have just been
created and are therefore expected to be page-cache warm; these numbers compare
the same implementation on the same host and are not portable capacity claims.

## Port Allocation Baseline

Command:

```text
timeout 180 cargo test -p nimbus-sandbox --lib scale_baseline \
  -- --ignored --nocapture --test-threads=1
```

The current `PortManager` scans every active sandbox manifest before selecting
the lowest free host port. Every one of the 21 samples asserted that all active
manifest ports remained reserved.

| Active manifests | Selected port | Median | p95 |
| ---: | ---: | ---: | ---: |
| 0 | 20000 | 1,500 ns | 2,625 ns |
| 64 | 20064 | 1,373,625 ns | 1,616,250 ns |
| 256 | 20256 | 6,134,416 ns | 6,804,917 ns |
| 1,024 | 21024 | 27,099,833 ns | 29,172,875 ns |

The visible scaling shape is approximately linear in manifest count. NNC3 owns
replacement with the host-global lease authority; NNC0.9 deliberately makes no
optimization.

## Segment Allocation Baseline

The same command runs the durable segment-state characterization against a
`10.0.0.0/8` node super-net with `/24` tenant segments. Seed time includes
creating the named existing tenants; each timed sample then assigns the next
unique tenant and asserts the exact lowest-free CIDR.

| Existing tenants | Seed total | Next-assignment median | Next-assignment p95 |
| ---: | ---: | ---: | ---: |
| 0 | 9,917 ns | 189,875 ns | 711,333 ns |
| 64 | 19,075,708 ns | 707,250 ns | 852,041 ns |
| 256 | 227,134,958 ns | 1,751,959 ns | 1,824,375 ns |
| 1,024 | 3,344,188,042 ns | 6,379,208 ns | 6,672,292 ns |

The current allocator rewrites its durable JSON state on each assignment, so
both seed cost and next-assignment latency grow with tenant count. NNC2 owns the
durable allocator extraction and correctness changes; this item records rather
than changes that shape.

Result: both explicit characterization tests passed; `2 passed, 0 failed`,
with 257 ordinary tests filtered out by the focused runner.

## Listener Start/Stop Smoke

Command:

```text
timeout 180 cargo test -p nimbus-server --lib \
  tests::local_admin::system_shutdown_endpoint_stops_live_server \
  -- --exact --nocapture --test-threads=1
```

Final bind-boundary measurement:

| Phase | Elapsed |
| --- | ---: |
| listener bind through successful `/health` response | 475,165,625 ns |
| authenticated shutdown request through graceful task join | 61,876,625 ns |
| total listener lifecycle | 537,042,334 ns |

Behavioral obligations all passed:

- the listener bound a kernel-assigned loopback port;
- `/health` became successful within the existing bounded five-second wait;
- the local-admin bearer received HTTP 200 and `{"accepted": true}`;
- the server task joined successfully within the existing five-second timeout;
  and
- the engine quiesced after the listener stopped.

The first exploratory measurement started before token and engine construction
and was discarded. The committed timestamp begins immediately before
`TcpListener::bind`, so later comparisons retain the same network lifecycle
boundary.

## Ordinary Regression Gates

Commands and results:

```text
timeout 240 cargo test -p nimbus-sandbox --lib -- --test-threads=1
# 243 passed, 0 failed, 16 ignored

timeout 240 cargo test -p nimbus-server --lib tests::local_admin \
  -- --test-threads=1
# 4 passed, 0 failed, 0 ignored

timeout 300 cargo clippy -p nimbus-sandbox -p nimbus-server \
  --all-targets -- -D warnings
# exit 0

cargo fmt --all --check
# exit 0

git diff --check
# exit 0

bash scripts/check-docs.sh
# 108 pages link-clean, source map resolves, private fence intact, titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

The ignored count includes the two NNC0.9 scale runners plus the previously
recorded expected-red characterization tests. Existing vendored Brotli
`unexpected_cfgs` warnings appeared during Cargo compilation; the workspace
targets and the new code remain clean under Clippy `-D warnings`.

## Regression Use

Later extraction items must rerun the same named runners and compare both:

1. exact behavior must remain green; and
2. material latency or scaling-shape regressions must be explained in the
   owning proof before the old authority is removed.

NNC0.9 establishes observations, not a fixed nanosecond gate. A wall-clock
threshold in shared CI would conflate host load with correctness and undermine
reliability; the exact semantic assertions are the blocking contract.

## Independent Review

Claude Opus 4.8 at maximum reasoning reported no accepted or actionable
findings and rated the patch correct (`0.8`). The review independently
reconciled the listener phase and total timings, verified the 21-sample median
and nearest-rank p95 indices, checked integer/range and fixture isolation,
confirmed both runners call the real current authorities, and validated the
NNC0-to-NNC1 plan/index/ledger transition.
