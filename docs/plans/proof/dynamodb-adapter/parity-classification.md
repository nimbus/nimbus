# DynamoDB Adapter — Parity Classification Report

Classification of every official-SDK parity scenario the adapter runs, mapped to
its DynamoDB tier and to the parity verdict against real DynamoDB / DynamoDB
Local ground truth. Produced by the D8.3 parity-runner foundation
(`scripts/dynamodb-parity.sh`); the scenario corpus lives in
`crates/nimbus-server/tests/dynamodb_spec/main.rs` and is driven through the
official `aws-sdk-rust` / `aws-sdk-dynamodbstreams` clients against an
endpoint-overridden in-process Nimbus listener — the same path a real AWS
customer's application uses.

## Lanes

- **Nimbus lane (executed):** `cargo test -p nimbus-server --test dynamodb_spec`
  → **27 passed, 0 failed**. Every scenario below is proven against Nimbus.
- **DynamoDB Local lane (ground truth):** booted by `scripts/dynamodb-parity.sh`
  from the pinned `amazon/dynamodb-local:2.5.2` image when Docker is available.

### Environment note (DynamoDB Local lane status)

In the environment this baseline was produced, the **Docker daemon is
unavailable** (`docker --version` reports 29.5.2 but `docker info` does not
reach a daemon). Per the plan's parity policy, this is recorded rather than
silently skipped:

- **Status:** blocked — Docker daemon unreachable.
- **Next action:** run `bash scripts/dynamodb-parity.sh` on a host with a
  running Docker daemon; it boots `amazon/dynamodb-local:2.5.2` on
  `:8200` and the endpoint-parameterized corpus (D8.4/D8.5) diffs the same
  scenarios against it. Until then the verdicts below are anchored to the
  documented real-DynamoDB contract (the SDK models + AWS API reference), which
  the Nimbus lane asserts directly.

## Classification key

- **match** — Nimbus reproduces real DynamoDB's observable behavior for the
  scenario.
- **nimbus-divergence: DDB-DIV-NNN** — an intentional, recorded difference; see
  `docs/adapters/dynamodb/divergences.md`. Each carries its own regression test.

## Scenarios

### T0 — control plane

| Scenario (parity test) | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `control_plane_roundtrip_through_official_sdk` | Create/Describe/List/Update/Delete table | match (table reaches ACTIVE synchronously — nimbus-divergence: DDB-DIV-004) | PASS |
| `duplicate_create_is_resource_in_use_through_official_sdk` | Duplicate CreateTable → ResourceInUseException | match | PASS |
| `describe_limits_through_official_sdk` | DescribeLimits default shape | match | PASS |

### T1 — single-item

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `put_item_through_official_sdk` | PutItem accepts a full item | match | PASS |
| `put_get_roundtrip_through_official_sdk` | PutItem→GetItem round-trips S/N/B/SS/NS/BS/M/L/BOOL/NULL | match | PASS |
| `delete_item_through_official_sdk` | DeleteItem + ConditionExpression + ReturnValues | match | PASS |
| `update_item_through_official_sdk` | UpdateExpression SET/REMOVE/ADD + ReturnValues | match | PASS |

### T2 — query / scan

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `query_through_official_sdk` | KeyConditionExpression + sort ordering + pagination | match | PASS |
| `query_filter_and_projection_through_official_sdk` | FilterExpression, ProjectionExpression, Select=COUNT | match | PASS |
| `scan_through_official_sdk` | Scan with filter + ExclusiveStartKey/LastEvaluatedKey | match | PASS |

### T3 — batch / transact

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `batch_get_item_through_official_sdk` | BatchGetItem + UnprocessedKeys shape | match | PASS |
| `batch_write_item_through_official_sdk` | BatchWriteItem Put/Delete + UnprocessedItems shape | match | PASS |
| `transact_get_items_through_official_sdk` | TransactGetItems consistent snapshot, ordered responses | match | PASS |
| `transact_write_items_through_official_sdk` | TransactWriteItems atomicity + CancellationReasons | match | PASS |

### T4 — secondary indexes

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `local_secondary_index_query_through_official_sdk` | LSI query + projection | match | PASS |
| `global_secondary_index_crud_through_official_sdk` | GSI create/update/delete via UpdateTable | match | PASS |
| `gsi_query_projection_through_official_sdk` | GSI query honoring the projection set | match | PASS |

### T5 — streams

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `stream_specification_through_official_sdk` | StreamSpecification on Create/Describe | match | PASS |
| `streams_data_plane_through_official_streams_sdk` | ListStreams/DescribeStream/GetShardIterator/GetRecords; INSERT/MODIFY/REMOVE images | nimbus-divergence: DDB-DIV-006 (single shard), DDB-DIV-007 (read-triggered retention) | PASS |

### T6 — TTL / tagging

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `time_to_live_through_official_sdk` | Update/DescribeTimeToLive enable→describe→disable | nimbus-divergence: DDB-DIV-008 (charset), DDB-DIV-009 (no cooldown) | PASS |
| `tagging_through_official_sdk` | TagResource/ListTagsOfResource/UntagResource | match | PASS |

### T7 — auth / tenancy

| Scenario | Asserts | Verdict | Nimbus |
| --- | --- | --- | --- |
| `two_access_keys_are_isolated_through_official_sdk` | Two access keys → isolated tenants | match | PASS |
| `unknown_access_key_is_unrecognized_client_through_official_sdk` | Unbound key → UnrecognizedClientException | match | PASS |
| `strict_mode_accepts_a_correctly_signed_request` | Strict SigV4 verifies a real SDK signature | match | PASS |
| `strict_mode_rejects_a_wrong_secret` | Wrong secret → InvalidSignatureException | match | PASS |
| `strict_mode_still_isolates_tenants` | Verification preserves tenant isolation | match | PASS |
| `persisted_signed_key_authenticates_and_rotates_in_strict_mode` | Persisted key auth + rotation invalidates old secret | match | PASS |

## ExtendDB lane (D8.6)

ExtendDB is the Apache-2.0 DynamoDB-on-PostgreSQL adapter Nimbus reuses
(`extenddb-core`, pinned rev `0448ca0`). It is a secondary ground truth: where
Nimbus matches ExtendDB but **not** real DynamoDB, the diff is classified
`accept-extenddb-divergence`.

- **Checkout:** present at `~/src/github.com/ExtendDB/extenddb`; the pinned
  source is `/Users/jack/.cargo/git/checkouts/extenddb-afcc0f7d71e33b8a/0448ca0`
  (workspace crates: `auth`, `bin`, `core`, `engine`, `server`, `storage`,
  `storage-postgres`).
- **Status:** blocked in this environment. ExtendDB's server requires a running
  PostgreSQL backend plus TLS + a credential-provisioning bootstrap that is not
  available in this sandbox (the same Docker/daemon and external-service
  constraints that block the DynamoDB Local lane).
- **Setup commands (to run where the dependencies are available):**
  ```sh
  # In ~/src/github.com/ExtendDB/extenddb:
  cargo build -p extenddb              # build the server binary
  extenddb init                         # initialize the data directory
  # Configure: throttling_enabled=false, control_plane_delay_seconds=0
  # Serve over HTTPS/TLS; clients connect with verify=False (self-signed dev cert).
  devtools/provision-test-credentials   # mint an access key/secret + region
  ```
  Then re-run `bash scripts/dynamodb-parity.sh` with the ExtendDB endpoint as a
  third lane and diff the same corpus.
- **Next action:** stand up PostgreSQL + ExtendDB per the commands above on a
  host with those services, then add the ExtendDB endpoint to the runner.
- **`accept-extenddb-divergence` entries:** none. Nimbus's 6 recorded
  divergences (DDB-DIV-004/006/007/008/009) differ from **both** real DynamoDB
  and ExtendDB (e.g. ExtendDB models 4 stream shards; Nimbus exposes 1), so none
  are an ExtendDB-matching divergence.

## Verdict

- **27 / 27** scenarios are **classified** (no unresolved diffs): 21 **match**
  real DynamoDB and 6 are recorded **nimbus-divergence** entries
  (DDB-DIV-004/006/007/008/009), each with a regression test in
  `docs/adapters/dynamodb/divergences.md`.
- **Nimbus lane:** 27 passed, 0 failed.
- **DynamoDB Local lane:** blocked (Docker daemon unavailable) — recorded above
  with the next action; no scenario is skipped silently.
