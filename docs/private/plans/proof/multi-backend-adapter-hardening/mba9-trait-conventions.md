# MBA9 Trait Object-Safety Audit

status: done

Audit command:

```sh
rg -n "dyn [A-Za-z_][A-Za-z0-9_]*(<|\\+|>|,|\\)|;|$| )|Box<dyn|Arc<dyn|&dyn" crates packages demos scripts tests -S
```

The audit found no Nimbus-owned object-erased async trait that exposes
`async fn` directly. Runtime/plugin boundaries either return a boxed future
alias or remain intentionally synchronous. Static-dispatch storage capability
traits added for MBA2 keep `async fn` because no call site stores them behind
`dyn`.

## Object-Erased Nimbus Traits

| Trait | Dyn shape | Async posture | Result |
| --- | --- | --- | --- |
| `RuntimeHooks` | `Box<dyn RuntimeHooks>` from `PersistenceProvider::runtime_hooks` | `spawn_workers` returns `BoxFuture<'static, ()>` | object-safe |
| `ApplicationAuthVerifier` | `Arc<dyn ApplicationAuthVerifier>` in router/deployment state | `verify_bearer_token` returns `BoxFuture<'a, Result<InvocationAuth, AppError>>` | object-safe |
| `RuntimeServiceRegistry` | `Arc<dyn RuntimeServiceRegistry>` in adapters and runtime bridge | async helpers return `RuntimeServiceBindingFuture<'a>` | object-safe |
| `MachineLifecycleManager` | `Arc<dyn MachineLifecycleManager>` in machine HTTP/control paths | methods return `MachineLifecycleFuture<'a>` | object-safe |
| `SandboxBackend` | `Arc<dyn SandboxBackend>` / `Box<dyn SandboxBackend>` | methods return `SandboxFuture<T>` | object-safe |
| `SandboxCatalog` | `Arc<dyn SandboxCatalog>` | synchronous catalog lookup | object-safe |
| `SandboxServiceCatalog` | `Arc<dyn SandboxServiceCatalog>` | synchronous service declaration lookup | object-safe |
| `TenantImageVerificationProvider` | `Arc<dyn TenantImageVerificationProvider>` | synchronous admission verification | object-safe |
| `ArtifactVerifierBackend` | `Arc<dyn ArtifactVerifierBackend>` / `&dyn ArtifactVerifierBackend` | synchronous verification boundary | object-safe |
| `ArtifactVerifierCommandRunner` | `Arc<dyn ArtifactVerifierCommandRunner>` | synchronous command runner boundary | object-safe |
| `OperatorExternalPolicyBackend` | `Arc<dyn OperatorExternalPolicyBackend>` | synchronous external policy evaluation boundary | object-safe |
| `HostBridge` | `Arc<dyn HostBridge>` in runtime host/bootstrap state | `call_async` returns `HostBridgeFuture` | object-safe |
| `RuntimeBackendFactory` | factory trait used to produce boxed runtime backends | synchronous `create` returns `Box<dyn RuntimeBackend>` | object-safe |
| `RuntimeBackend` | `Box<dyn RuntimeBackend>` in runtime workers | `invoke` returns `Pin<Box<dyn Future<Output = Result<Value>> + 'a>>` | object-safe |
| `BunJscExecutionAdapterFactory` | factory trait used to produce boxed Bun/JSC adapters | synchronous `create` returns `Box<dyn BunJscExecutionAdapter>` | object-safe |
| `BunJscExecutionAdapter` | `Box<dyn BunJscExecutionAdapter>` | `invoke` returns `Pin<Box<dyn Future<Output = Result<Value>> + 'a>>` | object-safe |
| `WorkerLoopFactory` | `Arc<dyn WorkerLoopFactory>` | synchronous `create` returns `Box<dyn WorkerLoop>` | object-safe |
| `WorkerLoop` | `Box<dyn WorkerLoop>` | synchronous worker-thread loop | object-safe |
| `RuntimeWorkerQueue` | `Arc<dyn RuntimeWorkerQueue>` | synchronous queue operations | object-safe |
| `TenantPersistenceWriteOps` | `&mut dyn TenantPersistenceWriteOps` in persistence executor | synchronous write transaction adapter | object-safe |
| `TriggerInvocationExecutor` | `Arc<dyn TriggerInvocationExecutor>` | synchronous execution result classification | object-safe |
| `CommittedMutationObserver` | `Arc<dyn CommittedMutationObserver>` registry | synchronous observer callback | object-safe |
| `TableSchemaChangeObserver` | `Arc<dyn TableSchemaChangeObserver>` registry | synchronous observer callback | object-safe |
| `Clock` | `Arc<dyn Clock>` / `&dyn Clock` | synchronous deterministic time source | object-safe |
| `FaultInjector` | `Arc<dyn FaultInjector>` / `&dyn FaultInjector` | synchronous deterministic fault point | object-safe |
| `LocalKeyProvider` | `Arc<dyn LocalKeyProvider>` / `&dyn LocalKeyProvider` | synchronous key wrap/unwrap boundary | object-safe |

## Static-Dispatch Traits

The MBA2 storage capability traits in `crates/nimbus-storage/src/traits/` are
static-dispatch traits. They intentionally use `async fn` because they are not
stored as `dyn TenantLifecycle`, `dyn TenantPointRead`, `dyn TenantPointWrite`,
`dyn TenantRangeScan`, `dyn DurableJournal`, `dyn SchedulerStore`,
`dyn ControlPlaneUsage`, `dyn KeyProviderSurface`, or `dyn StorageEngine`.

This keeps the storage split readable without paying object-safety ceremony at
boundaries that do not need runtime type erasure.

## Non-Architecture Trait Objects

The remaining grep hits are standard utility/provider objects:

- cancellation closures (`dyn Fn`, `dyn FnMut`, `dyn FnOnce`)
- error erasure (`dyn std::error::Error`, `dyn Any`)
- SQL driver parameters (`dyn ToSql`)
- stream/read abstractions (`dyn Stream`, `dyn Read`)
- boxed futures owned by runtime internals rather than trait methods

These are governed by their owning APIs and do not require Nimbus trait
convention changes.

## Verification

- `docs/architecture/trait-conventions.md` records the rule: object-erased
  async traits return boxed futures; static-dispatch async traits may use
  `async fn`.
- Focused clippy/check verification is recorded in the final MBA14 closeout.
