# RTI0 Fail-Before Proof

Baseline: `93124d87e` (`origin/main` when the runtime tenant-isolation plan was
promoted), inspected on 2026-07-21 before the owner implementation.

The baseline's source-level reproduction predicates were:

```text
FAIL RTIF1: RuntimeRoutingAffinity::None => Ok(None)
FAIL RTIF2: Script affinity tenant came from RuntimeBundleIdentity::tenant_label
FAIL RTIF3/4: V8 retained state was Vec<WarmPoolEntry> with no owner incarnation
FAIL RTIF6: WasmtimeStoreAuthorityKey.tenant_label was Option<String>
FAIL RTIF8: live V8 warm pool had only a worker-global flat-vector capacity
FAIL RTIF9: ConvexRegistry and CloudFunctionsRegistry owned RuntimeExecutor
```

These predicates are taken directly from the baseline versions of:

- `crates/nimbus-runtime/src/affinity.rs`;
- `crates/nimbus-runtime/src/backends/v8/warm_pool.rs`;
- `crates/nimbus-runtime/src/backends/wasmtime/store_pool.rs`;
- the Convex and Cloud Functions registry implementations.

They establish the fail-before equivalence without committing deliberately red
tests: two owners using `None` routing had no locality/tenant dimension; two
owners using an unscoped bundle under `Script` routing shared the same
bundle-derived locality; same-label recreation had no incarnation dimension;
and Wasmtime admitted a missing optional tenant into its retained Store key.
The V8 pool also had no owner retirement interface or per-owner cap.

The landed behavior tests pin the corresponding exploit shapes by name:

- V8 `None`, unscoped `Script`, different-subject/equal-incarnation, and
  same-subject/new-incarnation sentinel cases in the runtime pool tests;
- missing/different/revoked owner cases for retained Wasmtime Stores;
- barrier-based queued, dispatched-before-guest, active, response-ready drain,
  return-after-revoke, simultaneous owner/deployment, and pressure/retirement
  races in `executor::tests::retirement`;
- the shared Convex/Cloud Functions served delete/recreate sentinel harness in
  `nimbus-server::tests::runtime_owner_conformance`.

The temporary RTI0 fail-closed concept was subsumed by the mandatory common
owner admission interface in RTI1/RTI2. There is no compatibility mode: every
mutable retained pool kind requires an active owner lease, while fresh
execution, platform startup snapshots, and immutable compiled artifacts remain
ownerless where appropriate.
