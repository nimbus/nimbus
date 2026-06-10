# DynamoDB Adapter — Compatibility Suites & Canary Matrix

Two registries that bound how broadly the adapter is exercised beyond the
in-repo parity runner:

- **External-suite registry (D8.8)** — large external compatibility/conformance
  suites run against Nimbus by endpoint override.
- **Canary-app matrix (D8.9)** — real applications/libraries (distinct from test
  suites) exercised against Nimbus.

Each entry records its pin, command, lane (`pr` / `nightly` / `manual`), and
run status. Where an entry cannot execute in this environment (no Docker daemon,
no network to fetch an SDK, no external service), the status is **recorded**
with the next action rather than silently skipped.

Environment note: in the sandbox this baseline was produced, the Docker daemon
is unreachable and the AWS JS/Python SDKs and CLI are not installed; only the
**Rust** lane (the in-repo parity runner driving the real `aws-sdk-rust`) runs
here. The runnable Rust lane already exercises all 27 official-SDK scenarios
(see `docs/plans/proof/dynamodb-adapter/parity-classification.md`).

## External-suite registry (D8.8)

| Suite | SDK / tool | Pin | Command | Lane | Vendored? | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Nimbus official-SDK parity corpus | `aws-sdk-dynamodb` 1.111.0 + `aws-sdk-dynamodbstreams` 1.99.0 (Rust) | workspace `Cargo.lock` | `cargo test -p nimbus-server --test dynamodb_spec` | `pr` | in-repo (`crates/nimbus-server/tests/dynamodb_spec`) | **27 passed, 0 failed** |
| Streams data-plane suite | `aws-sdk-dynamodbstreams` (Rust) | `Cargo.lock` | `cargo test -p nimbus-server --test dynamodb_spec streams_data_plane` | `pr` | in-repo | **PASS** |
| `@nimbus/dynamodb` JS v3 smoke | `@aws-sdk/client-dynamodb` ^3 (JS) | optional peer | `npm run test --workspace @nimbus/dynamodb -- --smoke-port <port>` | `manual` | path-referenced | recorded-blocked — AWS JS SDK not installed; runs where the peer + a listener are present |
| ExtendDB Python/boto3 suite | `boto3` (Python) | ExtendDB rev `0448ca0` | ExtendDB `devtools` suite against the Nimbus endpoint | `manual` | path-referenced (`~/src/github.com/ExtendDB/extenddb`) | recorded-blocked — needs Python/boto3 + the ExtendDB harness; see ExtendDB lane in the parity report |
| ExtendDB Rust SDK suite | `aws-sdk-dynamodb` (Rust) | ExtendDB rev `0448ca0` | ExtendDB `tests/rust` against the Nimbus endpoint | `manual` | path-referenced | recorded-blocked — needs the ExtendDB test crate wired to the Nimbus endpoint |
| AWS CLI corpus | `aws` CLI v2 | n/a | `aws dynamodb … --endpoint-url http://127.0.0.1:8000` | `manual` | path-referenced | recorded-blocked — `aws` CLI not installed in this environment |

**Next action (blocked rows):** on a host with Docker + the respective SDK/CLI,
point each suite at a running Nimbus DynamoDB listener (endpoint override) and
record the suite name, command, environment, SDK, upstream release/SHA/version,
pass/fail/skip counts, artifacts, and lane here.

## Canary-app matrix (D8.9)

Real app/library usage, distinct from conformance suites.

| Canary | Language / SDK | Version pin | Endpoint / auth | Operation families | Assertions | Lane | Status | Release-blocking |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Parity runner (idiomatic Rust client) | Rust `aws-sdk-dynamodb` | `Cargo.lock` 1.111.0 | endpoint override + access-key creds | control-plane, item CRUD, query/scan, batch/transact, GSI/LSI, streams, TTL, tagging, auth | 27 scenarios assert response shape + values | `pr` | **PASS** (27/27) | yes |
| JS v3 document-client app | JS `@aws-sdk/lib-dynamodb` + `client-dynamodb` ^3 | `package.json` peer | `@nimbus/dynamodb` `clientConfig()` | item CRUD via DocumentClient marshalling | put/get/update/delete round-trip | `manual` | recorded-blocked — JS SDK not installed | no (until wired) |
| boto3 client/resource app | Python `boto3` | latest | endpoint override | item CRUD, query, batch | resource + client API parity | `manual` | recorded-blocked — Python/boto3 not installed | no (until wired) |
| Java v2 app | Java `software.amazon.awssdk:dynamodb` | latest | endpoint override | item CRUD | enhanced-client round-trip | `manual` | recorded-blocked — JVM toolchain not present | no (until wired) |

**Next action:** stand up each canary against a running Nimbus listener on a
host with the toolchain, then record version, lockfile, endpoint/auth config,
operation families, assertions, pass/fail counts, lane, and release-blocking
status. The Rust canary is the release-blocking baseline and is green today.
