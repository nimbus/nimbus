# REC4 Runtime Context And Codegen Alignment Proof

Status: `done`.

REC4 aligned runtime context shape with the execution-plan vocabulary while
keeping host-op observation as the trust boundary.

## Implementation

- Runtime bootstrap `__nimbusCreateContext({ request })` now derives a
  request-kind capability shape:
  - `query` / `paginated_query`: reader-only `db`, no scheduler, and
    `nestedCalls.query` only
  - `mutation`: writer `db` plus scheduler, and `nestedCalls.query` plus
    `nestedCalls.mutation`
  - `action` / `http_action`: scheduler and the full Convex nested-call matrix
    without direct `db`
  - request-less dynamic/internal contexts keep the legacy broad surface for
    low-level tests and compatibility paths
- Query context write/scheduler/mutation/action attempts fail before host
  dispatch with request-kind-specific context errors.
- Mutation context query/mutation nested calls remain present, while action
  nested calls fail before host dispatch.
- Action context direct `db` attempts fail before host dispatch while nested
  call and scheduler functions remain present.
- Codegen is aligned through `packages/codegen/src/planner/context_api.mjs` and
  `packages/codegen/src/selftest/context_fixtures.mjs`: generated query proxies
  expose read-only DB plus `runQuery`, mutation proxies expose DB writes,
  scheduler, `runQuery`, and `runMutation`, and action / HTTP action proxies
  expose scheduler plus the full Convex nested-call matrix without DB.
- REC2/REC3 host-op observation remains active below context shape. A raw
  runtime host-op call that bypasses `ctx` still receives the typed execution
  plan effect violation before host bridge dispatch.

## Verification

```text
cargo test -p nimbus-runtime runtime_query_context_is_reader_only_when_request_kind_is_present --lib -- --nocapture
```

Result:

```text
running 1 test
test runtime::tests::host_bridge::runtime_query_context_is_reader_only_when_request_kind_is_present ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1066 filtered out
```

```text
cargo test -p nimbus-runtime runtime_mutation_context_exposes_query_and_mutation_nested_calls --lib -- --nocapture
```

Result:

```text
running 1 test
test runtime::tests::host_bridge::runtime_mutation_context_exposes_query_and_mutation_nested_calls ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1232 filtered out
```

```text
cargo test -p nimbus-runtime runtime_action_context_exposes_nested_calls_without_direct_db --lib -- --nocapture
```

Result:

```text
running 1 test
test runtime::tests::host_bridge::runtime_action_context_exposes_nested_calls_without_direct_db ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1066 filtered out
```

```text
cargo test -p nimbus-runtime rec3_query_write_effect_violation_rejects_before_host_dispatch --lib -- --nocapture
```

Result:

```text
running 2 tests
test runtime::tests::cooperative::rec3_query_write_effect_violation_rejects_before_host_dispatch_subprocess ... ignored, runs in a subprocess to isolate cooperative locker V8 state
test runtime::tests::cooperative::rec3_query_write_effect_violation_rejects_before_host_dispatch ... ok

test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 1065 filtered out
```

```text
npm run test --workspace @nimbus/codegen
```

Result:

```text
> @nimbus/codegen@0.1.44 test
> node ./src/selftest.mjs

runtime remap fixtures: ok (4 cases)
```

The codegen selftest now includes the Convex nested-call matrix directly:
query allows `runQuery`; mutation allows `runQuery` and `runMutation`; action
and HTTP action allow `runQuery`, `runMutation`, and `runAction`.

Expected verifier closeout after REC4: `Summary: 20 passed, 0 failed`.

## REC5 Handoff

REC5 must run the PIR-aligned numeric validation and closeout checks. It should
record benchmark artifacts or measured exceptions, verify there are no direct
`InvocationKind` scheduler paths, and close the REC plan only after the REC and
PIR verifiers are green.
