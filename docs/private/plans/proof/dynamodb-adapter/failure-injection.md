# DynamoDB Adapter — Failure-Injection & Fail-Closed Report (D9.3)

Adversarial inputs and cancellation paths must map to a **modeled** DynamoDB
error — a typed 4xx carrying an `__type` — never an unhandled 5xx, a panic, or a
partial-success envelope. Proven through the public `dispatch` entrypoint in
`crates/nimbus-dynamodb/tests/failure_injection.rs`.

## Injected failures and outcomes

| Injection | Surface | Expected outcome | Result |
| --- | --- | --- | --- |
| Malformed JSON body | PutItem | `SerializationException` (400), pre-auth | PASS |
| Unknown operation target | `Frobnicate` | `UnknownOperationException` (400), pre-auth | PASS |
| Missing `Authorization` header | PutItem | `MissingAuthenticationToken` (400) | PASS |
| Unbound access key | CreateTable | `UnrecognizedClientException` (400) | PASS |
| Oversize partition key (3000 B > 1500 B cap) | PutItem | `ValidationException` (400) — DDB-DIV-001 | PASS |
| Condition-failed transaction | TransactWriteItems | `TransactionCanceledException` (400); **no partial write** (the would-succeed item is verified absent) | PASS |
| Strict SigV4 with no `X-Amz-Date` | CreateTable | `IncompleteSignature` (400), fails closed before any handler | PASS |

Additional fail-closed coverage proven elsewhere:

- **Wrong SigV4 signature** → `InvalidSignatureException`, end-to-end through the
  real `aws-sdk-rust` (`strict_mode_rejects_a_wrong_secret`, parity runner).
- **Conditional write failures** → `ConditionalCheckFailedException`
  (`commands::item` unit tests).
- **Batch partial failure** → `UnprocessedItems`/`UnprocessedKeys` envelopes
  (`commands::batch` unit + parity).

## Verdict

- **0 panics.** Each test runs `dispatch` to completion; a panic would abort the
  test binary. `cargo test -p nimbus-dynamodb --test failure_injection` →
  **7 passed, 0 failed**.
- **0 unhandled 5xx.** Every adversarial input returns a modeled 4xx with a
  typed `__type`; `assert_modeled_failure` rejects any 5xx and any
  "not yet implemented" placeholder. No diff is left unresolved.
- **No partial-success envelopes.** The canceled transaction leaves no item
  written (asserted by a follow-up GetItem).
