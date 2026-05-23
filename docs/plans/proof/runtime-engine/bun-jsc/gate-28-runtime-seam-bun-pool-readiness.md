# Bun/JSC Gate 28: Runtime Seam Bun Pool Readiness

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

## Decision

Status: Nimbus runtime seam prepared; Bun/JSC remains fail-closed.

Nimbus now has engine-owned pool metadata for the future Bun/JSC backend:

- `RuntimePoolKind::BunJscTrustedRetained`
- `RuntimePoolKind::BunJscFreshDiscard`

These variants are diagnostics and admission metadata only. They do not route
to the existing V8/Deno runtime pool and they do not make Bun/JSC selectable.
The V8/Deno backend still accepts only:

- `RuntimePoolKind::StartupSnapshotCache`
- `RuntimePoolKind::WarmPool`

The Bun/JSC backend now requires matching trust, lockdown, lifecycle, and pool
profiles before it reaches the existing non-selectable product gates.

## Runtime Matrix

| Backend | Pool kind | State semantics | Reset contract | Product route |
| --- | --- | --- | --- | --- |
| V8/Deno | `StartupSnapshotCache` | fresh per invocation | op, bootstrap, and user module state refreshed | supported |
| V8/Deno | `WarmPool` | warm per bundle | op and bootstrap state refreshed; user module state retained | supported with `CooperativeLocker` |
| Bun/JSC | `BunJscTrustedRetained` | warm per bundle | op and bootstrap state refreshed; user module state retained | blocked until trusted generated-wrapper route is deliberately productized |
| Bun/JSC | `BunJscFreshDiscard` | fresh per invocation | op, bootstrap, and user module state refreshed | blocked until untrusted lockdown and outer quota are proven |

## Enforcement

`RuntimePolicy::new(...)` rejects:

- V8/Deno with Bun/JSC lifecycle or pool metadata.
- Bun/JSC with V8/Deno pool metadata.
- Bun/JSC proof-only metadata even when the Bun/JSC pool profile matches.
- Bun/JSC trusted generated-wrapper metadata even when the Bun/JSC retained
  pool profile matches.
- Bun/JSC in-process-untrusted metadata even when the fresh/discard pool
  profile matches.
- Any Bun/JSC trust, lockdown, lifecycle, and pool mismatch.

The V8 runtime internals also treat Bun/JSC pool variants as unreachable after
policy validation, so the future Bun pool cannot accidentally fall through into
the V8 warm-pool implementation.

## Verification

Passed:

```sh
cargo fmt --all --check
cargo test -p nimbus-runtime limits::tests --lib
cargo test -p nimbus-server registry_and_license::registry --lib
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib
bash scripts/verify-bun-jsc-in-process-lockdown.sh
git diff --check
```

Result:

```text
runtime limits: 10 passed
server registry: 10 passed
server runtime metrics: 2 passed
reusable Bun/JSC gate: pass
```

The reusable gate also passed the ignored Bun source proof lane, Bun
`cargo fmt --all --check`, native `check-bun-embed-probe`, and Bun whitespace
diff check against `/Users/jack/src/github.com/oven-sh/bun`.

## Outcome

`BEP3` is complete. The next gate is `BEP4`: define and scaffold the dedicated
Bun/JSC pool owner without enabling a product runtime route.
