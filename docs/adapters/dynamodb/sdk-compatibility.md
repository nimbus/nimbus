# DynamoDB Adapter — SDK Compatibility Matrix

Which official AWS DynamoDB clients are proven against Nimbus, at what versions,
in which auth mode, and with what result — plus the quality audit of the
external suites and canary apps those clients drive.

The adapter speaks the DynamoDB JSON-1.0 wire protocol on a dedicated HTTP
listener, so any official SDK works by setting an **endpoint override** to the
Nimbus listener URL and supplying an access key bound to a tenant. No
Nimbus-specific client code is required.

## Official SDK matrix

| Client | Version | Auth mode | Endpoint | Operations exercised | Pass / fail / skip | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Rust `aws-sdk-dynamodb` | 1.111.0 (pinned in `Cargo.lock`) | lookup **and** strict SigV4 | `http://127.0.0.1:<ephemeral>` override | full T0–T7 corpus (control plane, item CRUD, query/scan, batch/transact, LSI/GSI, TTL, tagging, auth, strict-mode + persisted-key rotation) | **27 / 0 / 0** | run (release-blocking baseline) |
| Rust `aws-sdk-dynamodbstreams` | 1.99.0 (`Cargo.lock`) | lookup | endpoint override | ListStreams, DescribeStream, GetShardIterator, GetRecords + image assertions | **1 / 0 / 0** (`streams_data_plane`) | run |
| JavaScript `@aws-sdk/client-dynamodb` (JS v3) | ^3 (optional peer of `@nimbus/dynamodb`) | lookup | `@nimbus/dynamodb` `clientConfig()` | CreateTable/PutItem/GetItem smoke | recorded | recorded-blocked — JS SDK not installed in this environment; runs via `npm run test --workspace @nimbus/dynamodb -- --smoke-port <port>` |
| Python `boto3` | latest | lookup | endpoint override | item CRUD, query, batch (ExtendDB boto3 suite) | recorded | recorded-blocked — Python/boto3 not installed |
| AWS CLI v2 | latest | lookup | `--endpoint-url` | control-plane + item smoke | recorded | recorded-blocked — `aws` CLI not installed |
| Java `software.amazon.awssdk:dynamodb` v2 | latest | lookup | endpoint override | enhanced-client item CRUD | recorded | recorded-blocked — JVM toolchain not present |

**Auth modes recorded.** The Rust lane runs in both modes: lookup (the default,
any non-empty secret) and strict SigV4 (`AuthMode::Strict` — the SDK's real
signature is verified against the bound secret; wrong secret →
`InvalidSignatureException`; expired `X-Amz-Date` → rejected). See
`strict_mode_accepts_a_correctly_signed_request`,
`strict_mode_rejects_a_wrong_secret`, and
`persisted_signed_key_authenticates_and_rotates_in_strict_mode`.

**No protocol drift.** Every supported operation succeeds through the official
Rust SDK with no Nimbus-specific shimming — the SDK marshals/unmarshals against
Nimbus exactly as it would against AWS. The 6 recorded behavior divergences
(DDB-DIV-004/006/007/008/009) are semantic, not wire-protocol, differences and
do not cause any SDK call to fail to parse.

## External-suite quality audit (D9.2)

Audited dimensions for each accepted external suite:

| Suite | License | Endpoint-overridable | Targetable (DynamoDB / Local / ExtendDB / Nimbus) | SDK | Op coverage | Skip policy | Cleanup | Determinism | Runtime cost | Credential safety |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| In-repo Rust parity corpus | Nimbus (repo) | yes (ephemeral listener) | Nimbus (run); DynamoDB Local / ExtendDB ready when Docker/PG present | `aws-sdk-rust` | full T0–T7 | none skipped | per-test tempdir Service | deterministic (no wall-clock/RNG in assertions) | seconds | test creds only; no real AWS keys |
| ExtendDB boto3 suite | Apache-2.0 | yes | DynamoDB Local / ExtendDB / Nimbus | `boto3` | item + query + batch | upstream-defined | upstream | upstream | minutes | dev creds via `provision-test-credentials` |
| ExtendDB Rust suite | Apache-2.0 | yes | same | `aws-sdk-rust` | item + control plane | upstream | upstream | minutes | dev creds |
| AWS CLI corpus | (CLI is proprietary; corpus is repo-authored) | yes | all | `aws` CLI | control-plane + item | n/a | scripted | deterministic | seconds | env creds; localhost only |

## Canary-app quality audit (D9.2)

Audited dimensions: representativeness, maintenance, license, endpoint override,
lockfile stability, behavior assertions.

| Canary | Representative of | License | Endpoint override | Lockfile | Behavior assertions | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Rust parity runner | idiomatic `aws-sdk-dynamodb` app | Nimbus (repo) | yes | `Cargo.lock` | 27 scenarios assert response shape + values | run (release-blocking) |
| JS v3 document-client app | `@aws-sdk/lib-dynamodb` marshalling | repo example | yes (`@nimbus/dynamodb`) | `package-lock.json` | put/get/update/delete round-trip | recorded-blocked |
| boto3 client/resource app | Python resource + client APIs | repo example | yes | `requirements` pin | resource + client parity | recorded-blocked |
| Java v2 enhanced-client app | typed enhanced client | repo example | yes | Gradle/Maven pin | enhanced-client round-trip | recorded-blocked |

Full registry tables (commands, pins, lanes) live in
`docs/adapters/dynamodb/compatibility-suites.md`. Environment note: only the
Rust lane executes in this sandbox (no Docker daemon; JS/Python/CLI/JVM
toolchains absent); blocked lanes carry their next action there.
