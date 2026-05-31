# DynamoDB Adapter — Mixed-Workload Soak Report (D9.5)

A sustained, varied operation stream driven through the public `dispatch`
entrypoint, asserting fail-closed behavior under load.
Test: `crates/nimbus-dynamodb/tests/soak.rs ::
mixed_workload_soak_fails_closed_without_panics`.

## Workload

400 iterations, each issuing a mix of: PutItem, GetItem, UpdateItem, a
HASH+RANGE PutItem, a Query (Limit 5), a conditional PutItem
(`attribute_not_exists`), plus periodic TagResource + UpdateTimeToLive metadata
churn and periodic auth failures (an unbound access key). Two tables (a simple
HASH table and a HASH+RANGE table) are created up front.

## Results

| Metric | Value |
| --- | --- |
| Duration | ~6.1 s (in-process, single tempdir-backed `Service`) |
| Total operations | **2620** |
| Successful (2xx) | **2162** |
| Modeled errors (4xx) | **458** (conditional-check failures + auth failures) |
| Unhandled 5xx | **0** |
| Panics | **0** |
| Task leaks | **0** (the dispatch path is synchronous — it spawns no background tasks) |
| Memory high-water | bounded — a single in-process `Service`; no growth path beyond the bounded keyspace (16 partition keys) |

## Verdict

- Every operation resolved to either a 2xx success or a **modeled** 4xx error
  carrying a typed `__type`; the test asserts `ok + modeled_errors == total`, so
  there were **0 unhandled 5xx** and **0** "not yet implemented" placeholders.
- The test ran to completion, which is the **0-panics / 0-task-leaks** proof (a
  panic aborts the test binary; the synchronous dispatch path leaks no tasks).
- The conditional-write and auth-failure paths produced the expected modeled
  errors (458), confirming the failure paths stay healthy under sustained load.
