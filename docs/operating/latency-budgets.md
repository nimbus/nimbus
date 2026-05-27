# Latency Budgets

Nimbus emits structured `WARN` events when budgeted hot-path segments exceed
their local budgets. The first MBA8 slice covers the Convex HTTP query path and
the engine query execution path because they exercise parse/auth/dispatch,
runtime invocation, storage reads, and result caching.

## Event Schema

Every over-budget event uses:

| Field | Meaning |
| --- | --- |
| `latency_segment` | Stable segment name, for example `engine.query_execute`. |
| `budgeted_segment` | Duplicate stable name for grep-based gates and log routing. |
| `elapsed_ms` | Actual elapsed time in milliseconds. |
| `budget_ms` | Configured budget in milliseconds. |

## Segment Budgets

| Segment | Budget | Owner |
| --- | ---: | --- |
| `server.auth` | 10 ms | HTTP adapter authentication and tenant context resolution |
| `server.storage` | 50 ms | Adapter-dispatched storage-backed query execution |
| `server.runtime` | 100 ms | Adapter-dispatched runtime-backed function invocation |
| `engine.tenant_load` | 50 ms | Tenant lookup/load before query execution |
| `engine.wait_visibility` | 25 ms | Query wait for latest applied journal visibility |
| `engine.query_prepare` | 5 ms | Query planning and authorization preparation |
| `engine.query_execute` | 50 ms | Storage/materialized/index query execution |
| `engine.query_cache` | 5 ms | Post-query metric and document-cache update |

These budgets are intentionally conservative local defaults. Tightening them
requires fresh baseline evidence in the owning plan proof.

## Operating Notes

`NIMBUS_QUERY_PROFILE=1` and `NIMBUS_TENANT_LOAD_PROFILE=1` remain useful for
ad hoc local profiling because they print all samples. Budget events are lower
volume: they only emit when a segment crosses its threshold.
