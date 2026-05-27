# MBA8 Latency Budgets Proof

baseline_evidence: local code audit plus existing query/tenant profile timers; no live CI latency corpus available in this workspace

## Instrumented Segments

| Segment | File | Budget |
| --- | --- | ---: |
| `server.auth` | `crates/nimbus-server/src/adapters/convex/handlers/function_routes/queries.rs` | 10 ms |
| `server.storage` | `crates/nimbus-server/src/adapters/convex/handlers/function_routes/queries.rs` | 50 ms |
| `server.runtime` | `crates/nimbus-server/src/adapters/convex/handlers/function_routes/queries.rs` | 100 ms |
| `engine.tenant_load` | `crates/nimbus-engine/src/service/queries/query_api.rs` | 50 ms |
| `engine.wait_visibility` | `crates/nimbus-engine/src/service/queries/query_api.rs` | 25 ms |
| `engine.query_prepare` | `crates/nimbus-engine/src/service/queries/query_api.rs` | 5 ms |
| `engine.query_execute` | `crates/nimbus-engine/src/service/queries/query_api.rs` | 50 ms |
| `engine.query_cache` | `crates/nimbus-engine/src/service/queries/query_api.rs` | 5 ms |

## Rationale

The first budgeted path is intentionally narrow and high-signal. Convex HTTP
query handlers are a representative server request path, and the engine query
API already had timing slices for tenant load, visibility wait, prepare,
execute, cache, and total. MBA8 converts those slices into structured warning
events without replacing the existing opt-in profiles.

The budget values are local guardrails rather than product SLOs. They are wide
enough to avoid warning on normal local development for small queries, but low
enough to catch accidental blocking calls, cold tenant-load regressions, and
query-plan mistakes in focused profiling runs.
