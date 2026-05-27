# MBA4 Runtime Hooks Proof

posture: backend_owned_runtime_hooks
dispatch: Option<Box<dyn RuntimeHooks>>
object_safety: spawn_workers returns BoxFuture because the hook is consumed behind dyn RuntimeHooks

## Boundary

MBA4 narrows the provider-coupled worker seam without moving engine-owned
workers out of the engine.

`crates/nimbus-engine/src/persistence/runtime_hooks.rs` defines
`RuntimeHooks` and `WorkerContext`. The engine asks the selected
`PersistenceProvider` for `Option<Box<dyn RuntimeHooks>>`, records the generic
task name, and spawns the hook through the existing Service background runtime.
The engine no longer switches on a `ProviderBackgroundTask` enum or calls
backend-named worker entrypoints.

The hook owns only provider-coupled workers:

| Backend | Runtime hook posture | Worker |
| --- | --- | --- |
| redb | no backend-coupled workers | none |
| sqlite | no backend-coupled workers | none |
| postgres | RuntimeHooks for postgres | LISTEN/NOTIFY listener plus authoritative catch-up on attach/reconnect |
| mysql | RuntimeHooks for mysql | provider polling loop for schema, journal, scheduler, and unloaded scheduled tenants |
| libsql | RuntimeHooks for libsql | replica-connected SQLite provider polling loop |

Control-plane usage storage and local encryption providers have no
backend-coupled workers in the runtime process. They are intentionally outside
the hook list until they own a real runtime worker.

## Engine-Owned Workers That Stay Put

The following remain engine-owned because their behavior is independent of the
storage backend:

- mutation journal batching
- scheduler wakeups and dispatch
- trigger candidate processing and trigger execution
- subscription delivery
- request/runtime invocation lifecycle

## Verification Notes

The object-safe trait shape is deliberately stricter than the plan's initial
`async fn` sketch: a value returned as `Box<dyn RuntimeHooks>` cannot use
`async fn` directly. The implemented shape keeps static async traits available
elsewhere while using `BoxFuture` at the actual erased boundary.
