# Bun/JSC Gate 29: Bun Pool Owner Scaffold

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-embedder-api-and-pool-plan.md`

## Decision

Status: disabled Bun/JSC pool owner scaffold complete.

Nimbus now has a concept-owned Bun/JSC backend module under
`crates/nimbus-runtime/src/backends/bun_jsc/`. The scaffold is deliberately
not a product runtime route. It defines the shape that a future Bun backend
must satisfy before it can become selectable:

- `BunJscPoolPolicy`
- `BunJscPoolMode::{TrustedRetained, FreshDiscardOuterQuota}`
- `BunJscLifecycleState`
- `BunJscLifecycleAck`
- `BunJscLifecycleTrace`
- `BunJscPoolMetricsSnapshot`
- `BunJscRuntimeBackendFactory`

The backend factory exists so the runtime backend seam can name the future
backend, but invoking it returns a contract error until resolver, permission,
memory, cancellation, and teardown gates pass.

## Pool Contract

| Mode | Runtime metadata | Reuse | Quota posture | Product selectable |
| --- | --- | --- | --- | --- |
| Trusted retained | `BunJscTrustedRetained` + `BunJscTrustedRetainedPool` | allowed only for proof/trusted generated-wrapper profiles | no outer quota requirement in the trusted-only proof lane | no |
| Fresh/discard outer quota | `BunJscFreshDiscard` + `BunJscFreshDiscardPoolOuterQuotaRequired` | no retained untrusted VM reuse | outer quota required until hard per-VM memory bounds exist | no |

The scaffold keeps cancellation and teardown state/ack-driven. There is no
sleep-based cancellation contract in the product-facing pool shape. Valid
lifecycle movement is:

```text
Created
  -> BootstrapReady
  -> GuestEntered
  -> CancelRequested
  -> Terminated
  -> ResetOrDiscarded
  -> TeardownComplete
```

Normal completion may acknowledge `Terminated` directly from `GuestEntered`.
Cancellation before guest entry is rejected instead of counted as accepted
cancellation.

## Enforcement

The scaffold tests prove:

- trusted retained and untrusted fresh/discard policies are separate modes
- mismatched Bun/JSC trust, lockdown, lifecycle, and pool metadata is rejected
- `RuntimePolicy::new(...)` still rejects Bun/JSC even with matching
  fresh/discard metadata
- lifecycle transitions require ordered acknowledgements
- cancellation metrics increment only after a valid cancellation acknowledgement
- pool metrics track event-loop progress and teardown completions
- the public Bun/JSC pool envelope does not depend on V8/Deno runtime internals

## Verification

Passed:

```sh
cargo fmt --all --check
cargo test -p nimbus-runtime backends::bun_jsc --lib
bash scripts/verify-bun-jsc-in-process-lockdown.sh
git diff --check
```

Result:

```text
Bun/JSC scaffold tests: 4 passed
reusable Bun/JSC gate: pass
```

The reusable gate now includes the BEP4 scaffold tests in step 3 alongside the
runtime policy matrix. The passing run covered:

- 10 runtime limit/policy tests
- 4 Bun/JSC pool scaffold tests
- 10 registry/runtime metadata rejection tests
- 2 runtime diagnostics tests
- 1 ignored Bun source proof test
- Nimbus and Bun format/whitespace checks
- Bun native `check-bun-embed-probe`

## Outcome

`BEP4` is complete. The next gate is `BEP5`: prove resolver/package policy
denial or hookability in the Bun proof target.
