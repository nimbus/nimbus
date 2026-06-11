# Source map

This page maps user-facing behavior claims in the published docs
(`docs/{get-started,developers,operators,concepts,reference}/`) to the files
that implement them. The docs are descriptive; treat these files as the
source-backed check before changing any behavior claim. Every page with a
load-bearing claim gets a row; `scripts/check-docs.sh` verifies the listed
sources exist.

## Get started + landing

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `get-started/quickstart.md` | `nimbus init convex` scaffold contents | `crates/nimbus-bin/src/init.rs`, `crates/nimbus-assets/embedded/templates/convex/` |
| `get-started/quickstart.md` | `nimbus dev` watch / codegen / `demo` tenant / port 3210 | `crates/nimbus-bin/src/dev.rs` |
| `get-started/quickstart.md` | Node 22 floor for npm dependency installs; codegen runs in-binary | `crates/nimbus-bin/src/node.rs`, `crates/nimbus-bin/src/codegen.rs` |
| `get-started/self-host.md` | `nimbus start` flags, localhost default, SQLite default | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/config.rs` |
| `get-started/self-host.md` | `/api/tenants`, `/documents`, `/query` endpoints | `crates/nimbus-server/src/router.rs` |
| `get-started/self-host.md` | Local admin token required; token file locations | `crates/nimbus-operator/src/paths.rs`, `crates/nimbus-operator/src/token.rs`, `crates/nimbus-operator/src/access_policy.rs` |
| `get-started/from-convex.md` | Convex function model + `convex/` layout compatibility | `packages/nimbus/src/server.ts`, `packages/convex/src/`, `crates/nimbus-convex/` |
| `index.mdx` (landing) | Speaks Convex / Firestore / Cloud Functions / MongoDB / DynamoDB | `crates/nimbus-convex/`, `crates/nimbus-firebase/`, `crates/nimbus-cloud-functions/`, `crates/nimbus-mongodb/`, `crates/nimbus-dynamodb/` |
| `index.mdx` (landing) | Single-binary storage/compute/networking/realtime/scheduling | `crates/nimbus-bin/src/main.rs`, `crates/nimbus-engine/`, `crates/nimbus-storage/`, `crates/nimbus-server/` |
| `index.mdx` (landing) | MongoDB tab: connection string shape, `directConnection=true` | `packages/mongodb/src/uri.ts`, `demos/mongodb/node/script.ts` |
| `index.mdx` (landing) | DynamoDB tab: endpoint `127.0.0.1:8000`, registered-key credentials | `crates/nimbus-dynamodb/src/config.rs`, `packages/dynamodb/src/client.ts` |

## Developers — platform guides

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/first-app.md` | Scaffold contents, schema/messages templates | `crates/nimbus-assets/embedded/templates/convex/` |
| `developers/first-app.md` | Dev loop: auto npm install, codegen, `demo` tenant, port 3210, watch-and-activate | `crates/nimbus-bin/src/dev.rs` |
| `developers/first-app.md` | `ConvexClient.onUpdate` live subscriptions, `ConvexHttpClient` | `packages/convex/src/browser.ts`, `demos/convex/node/script.ts` |
| `developers/first-app.md` | Query builder: `withIndex`, `order`, `take`, `collect` | `packages/nimbus/src/server.ts` |
| `developers/auth.md` | `auth.config.ts`/`.js` exactly one; OIDC `{domain, applicationID}`; `customJwt {issuer, jwks, algorithm RS256\|ES256, applicationID?}`; `process.env.*` at codegen time | `packages/codegen/src/auth_config.mjs` |
| `developers/auth.md` | OIDC discovery; `aud` must equal `applicationID`; multi-audience rejected; `tokenIdentifier` = `issuer\|subject` | `crates/nimbus-convex/src/auth/verifier/metadata.rs`, `crates/nimbus-convex/src/auth/verifier/identity.rs`, `crates/nimbus-convex/src/auth/jwt/models/parsed_claims.rs` |
| `developers/auth.md` | `ConvexProviderWithAuth` / `useConvexAuth` / `setAuth` / `clearAuth` client surface | `packages/convex/src/react.ts`, `packages/nimbus/src/browser.ts`, `packages/nimbus/src/react.ts` |
| `developers/auth.md` | No token → `getUserIdentity()` null; invalid token → request rejected | `crates/nimbus-server/src/application_auth.rs`, `crates/nimbus-runtime/src/runtime/bootstrap/source.rs` |
| `developers/sdk/resource-model.md` | `Nimbus` client options, endpoint/credential discovery, services/sandboxes/sessions CRUD + lifecycle | `packages/nimbus/src/index.ts` |

## Developers + Reference — Convex

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/convex/index.md` | Scaffold file set; refuses existing dirs; `--install` flag | `crates/nimbus-bin/src/init.rs` |
| `developers/convex/index.md` | In-binary codegen default | `crates/nimbus-bin/src/codegen.rs` |
| `developers/convex/index.md` | Deployment URL `http://localhost:3210/convex/demo` | `crates/nimbus-server/src/router.rs`, `crates/nimbus-bin/src/dev/plan.rs` |
| `developers/convex/index.md` | ctx capability split (query read / mutation write+scheduler / action run-only) | `packages/codegen/src/planner/context_api.mjs` |
| `developers/convex/index.md` | React/HTTP client surface (`ConvexProvider`, `useQuery`, `ConvexHttpClient`) | `packages/convex/src/react.ts`, `packages/convex/src/browser.ts` |
| `developers/convex/migrate.md` | Dev provisions packages for detected Convex apps; `file:./.nimbus/packages/convex` dependency | `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/node.rs` |
| `developers/convex/migrate.md` | Generated files import from `convex/server`/`browser`/`values` | `packages/codegen/src/emit/generated_files.mjs`, `packages/codegen/src/app.mjs` |
| `developers/convex/migrate.md` | `.env.local` `NIMBUS_DEPLOYMENT=local:<slug>`; `nimbus codegen --app .` | `crates/nimbus-bin/src/dev/env_file.rs`, `crates/nimbus-bin/src/codegen.rs` |
| `reference/convex/compatibility.md` | Auth providers, OIDC discovery, audience rules, `getUserIdentity`/`getVerifiedIdentity` | `packages/codegen/src/auth_config.mjs`, `crates/nimbus-convex/src/auth/`, `crates/nimbus-runtime/src/runtime/bootstrap/source.rs` |
| `reference/convex/compatibility.md` | Registrar set; db surface get/insert/patch/delete; query builder + pagination | `packages/codegen/src/constants.mjs`, `packages/nimbus/src/server.ts`, `packages/codegen/src/planner/database_proxy.mjs` |
| `reference/convex/compatibility.md` | Validator set (11); schema rules; system fields | `packages/convex/src/values.ts`, `packages/codegen/src/schema.mjs`, `packages/codegen/src/emit/schema_types.mjs` |
| `reference/convex/compatibility.md` | Scheduling (mutations only), HTTP routes from `http.ts`, endpoint table | `packages/codegen/src/main.mjs`, `packages/codegen/src/parser/http_routes.mjs`, `crates/nimbus-server/src/router.rs` |
| `reference/convex/compatibility.md` | Node version set {20,22,24,26} default 24; `"use node"` actions-only; specifier rules | `packages/codegen/src/project_config.mjs`, `packages/codegen/src/parser.mjs`, `packages/codegen/src/module_specifiers.mjs` |
| `reference/convex/compatibility.md` | React hook list; reconnection behavior | `packages/convex/src/react.ts`, `packages/nimbus/src/browser.ts` |
| `reference/convex/project-layout.md` | Scaffold file contents; `_generated/` files; `.nimbus/convex/` artifacts; `NIMBUS_CODEGEN_RUNNER` | `crates/nimbus-assets/embedded/templates/convex/`, `packages/codegen/src/main.mjs`, `crates/nimbus-bin/src/codegen.rs` |
| `reference/convex/usage-rules.md` | Function syntax, validators, id-first db calls, `withIndex` over `filter`, pagination, run-* placement | `packages/codegen/src/planner/context_api.mjs`, `packages/nimbus/src/server.ts`, `packages/convex/src/server.ts` |

## Developers + Reference — Firestore

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/firebase/index.md` | `nimbus packages provision firebase` → `@nimbus/firebase` | `crates/nimbus-bin/src/provision.rs`, `packages/firebase/package.json` |
| `developers/firebase/index.md` | Firestore routes gated on deployment config (no CLI flag) | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/construction.rs`, `crates/nimbus-bin/src/start/boot.rs` |
| `developers/firebase/index.md` | `projectId` → tenant id; only `(default)` database | `crates/nimbus-firebase/src/operations.rs`, `crates/nimbus-firebase/src/firestore_model.rs` |
| `developers/firebase/index.md` | `connectFirestoreEmulator` flow; transport options | `packages/firebase/src/firestore.ts`, `crates/nimbus-server/src/tests/firebase/rest_crud.rs` |
| `reference/firebase/websocket-listen.md` | Listen route, subprotocols, close codes, loopback origin policy | `crates/nimbus-server/src/adapters/firebase/grpc/listen_websocket.rs`, `crates/nimbus-server/src/router.rs` |
| `reference/firebase/auth.md` | `experimentalAuthToken`, `mockUserToken` opt-in, accepted headers | `packages/firebase/src/firestore.ts`, `packages/firebase/src/internal/auth.ts`, `crates/nimbus-firebase/src/lib.rs` |

## Developers + Reference — Cloud Functions

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/cloud-functions/index.md` | `nimbus init cloud-functions` scaffold; serves on main port | `crates/nimbus-bin/src/init.rs`, `crates/nimbus-assets/embedded/templates/cloud-functions/`, `crates/nimbus-server/src/router.rs` |
| `developers/cloud-functions/index.md` | Walk-up rules (`nimbus dev` bounded by `.git`; `firebase.json` root marker) | `crates/nimbus-bin/src/start/boot.rs`, `crates/nimbus-bin/src/dev/plan.rs` |
| `developers/cloud-functions/index.md` | Artifact set `artifact.json`/`targets.json`/`bundle.mjs`/`bundle.sha256` | `crates/nimbus-cloud-functions/src/lib.rs` |
| `developers/cloud-functions/migrate.md` | Version-1 `targets.json` schema; service-account execution for Firestore bindings | `crates/nimbus-cloud-functions/src/lib.rs`, `packages/codegen/src/selftest/cloud_functions_fixtures.mjs` |
| `developers/cloud-functions/migrate.md` | `nimbus deploy --url/--token` + env vars | `crates/nimbus-bin/src/deploy.rs` |
| `reference/cloud-functions/compatibility.md` | Path rules; at-least-once delivery, replay, chain-depth limit | `packages/codegen/src/cloud_functions/runtime_sources.mjs`, `crates/nimbus-engine/src/triggers/execution.rs`, `crates/nimbus-server/src/adapters/cloud_functions/execution.rs` |
| `reference/cloud-functions/compatibility.md` | Admin slice coverage; options matrix; callable envelope | `crates/nimbus-cloud-functions/src/runtime_api/firebase_admin/firestore.rs`, `crates/nimbus-server/src/adapters/cloud_functions/http/callable.rs` |

## Developers + Reference — MongoDB

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/mongodb/index.md` | Endpoint enabled via `ServeOptions::with_mongodb` (no CLI flag); loopback-only listener | `crates/nimbus-server/src/construction.rs`, `crates/nimbus-server/src/adapters/mongodb/listener.rs` |
| `developers/mongodb/index.md` | `MongoDbAuthConfig` SCRAM-SHA-256; `MongoDbConfig::localhost` | `crates/nimbus-server/src/adapters/mongodb/mod.rs`, `crates/nimbus-mongodb/src/auth.rs` |
| `developers/mongodb/index.md` | `directConnection=true` required; tenant/collection auto-create | `packages/mongodb/src/uri.ts`, `crates/nimbus-mongodb/src/commands/tenant.rs` |
| `developers/mongodb/examples.md` | `mongoUri()` defaults; filter/update operator surface; transactions + `WriteConflict`; change streams unsupported | `packages/mongodb/src/uri.ts`, `crates/nimbus-mongodb/src/commands/crud/filter.rs`, `crates/nimbus-mongodb/src/commands/session.rs`, `crates/nimbus-mongodb/src/commands/aggregation/mod.rs` |
| `reference/mongodb/drivers.md` | OP_MSG, server version 7.0.0, wire versions, SCRAM-only, pre-auth handshake set | `crates/nimbus-mongodb/src/commands/handshake.rs`, `crates/nimbus-mongodb/src/auth.rs`, `crates/nimbus-mongodb/src/commands/mod.rs` |
| `reference/mongodb/operations.md` | Command dispatch table; update operators; aggregation stages; size/session limits | `crates/nimbus-mongodb/src/commands/mod.rs`, `crates/nimbus-mongodb/src/commands/crud/update.rs`, `crates/nimbus-mongodb/src/commands/aggregation/mod.rs`, `crates/nimbus-mongodb/src/commands/handshake.rs`, `crates/nimbus-mongodb/src/commands/session.rs` |
| `reference/mongodb/tenant-isolation.md` | db→tenant 1:1 mapping; `default` tenant rules; tenant-name constraints | `crates/nimbus-mongodb/src/commands/tenant.rs`, `crates/nimbus-core/src/types.rs` |

## Developers + Reference — DynamoDB

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/dynamodb/index.md` | Default `127.0.0.1:8000`; `X-Amz-Target` dispatch; strict-auth default, fail-closed registry | `crates/nimbus-dynamodb/src/config.rs`, `crates/nimbus-server/src/adapters/dynamodb/listener.rs`, `crates/nimbus-dynamodb/src/tenant.rs` |
| `developers/dynamodb/index.md` | `with_dynamodb(DynamoDbConfig)` enablement; `insecure_dev_auth` loopback-only | `crates/nimbus-server/src/construction.rs`, `crates/nimbus-dynamodb/src/config.rs` |
| `developers/dynamodb/index.md` | `clientConfig()` defaults; tables ACTIVE immediately | `packages/dynamodb/src/client.ts`, `crates/nimbus-dynamodb/src/commands/control_plane.rs` |
| `reference/dynamodb/feature-coverage.md` | Operation coverage; conditional-write atomicity; TTL sweeper interval | `crates/nimbus-dynamodb/src/dispatch.rs`, `crates/nimbus-dynamodb/src/commands/item.rs`, `crates/nimbus-dynamodb/src/config.rs` |
| `reference/dynamodb/divergences.md` | Key-size cap, sortable encoding, `_ddb_` reserved, stream semantics, TTL/GSI divergences | `crates/nimbus-core/src/types.rs`, `crates/nimbus-dynamodb/src/key.rs`, `crates/nimbus-dynamodb/src/commands/` |
| `reference/dynamodb/sdk-compatibility.md` | Rust SDK parity suite; 16 MiB request cap | `crates/nimbus-server/tests/dynamodb_spec/main.rs`, `crates/nimbus-server/src/adapters/dynamodb/listener.rs` |
| `reference/dynamodb/readiness.md` | SigV4 strictness; `_nimbus_*` refusal; redacted key listings; plain-HTTP posture | `crates/nimbus-dynamodb/src/auth/sigv4/verify.rs`, `crates/nimbus-dynamodb/src/tenant.rs`, `crates/nimbus-dynamodb/src/key_management.rs` |

## Developers + Reference — Native API

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/native/index.md` | Admin token requirement, header forms, 401 behavior, token file locations | `crates/nimbus-operator/src/access_policy.rs`, `crates/nimbus-operator/src/paths.rs`, `crates/nimbus-operator/src/token.rs` |
| `developers/native/index.md` | Tenant/document/query endpoints + status codes | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/http/` |
| `reference/native/http-api.md` | Loopback origin rule; system fields; query/schema/scheduling shapes; service-control routes | `crates/nimbus-operator/src/access_policy.rs`, `crates/nimbus-core/src/document.rs`, `crates/nimbus-core/src/query.rs`, `crates/nimbus-core/src/schema.rs`, `crates/nimbus-core/src/scheduled.rs`, `crates/nimbus-server/src/router.rs` |
| `reference/native/websocket-protocol.md` | `nimbus.v2` negotiation, handshake, frame catalog, close codes | `crates/nimbus-server/src/ws/`, `crates/nimbus-server/src/protocol.rs`, `crates/nimbus-server/src/error_envelope.rs` |
| `reference/native/errors.md` | Error envelope, code catalog, status mapping | `crates/nimbus-server/src/error_envelope.rs`, `crates/nimbus-core/src/error.rs` |

## Developers + Concepts + Reference — Node.js runtime

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `developers/runtimes/nodejs/index.md` | `"use node"` first-statement opt-in; actions-only | `packages/codegen/src/parser.mjs` |
| `developers/runtimes/nodejs/configuration.md` | `nodeVersion` values {20,22,24,26} default 24; `--debug-node-apis`; Node 22 toolchain floor | `packages/codegen/src/project_config.mjs`, `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/codegen.rs`, `crates/nimbus-bin/src/node.rs` |
| `developers/runtimes/nodejs/packages-and-bundling.md` | `externalPackages` rules; staging paths; size references | `packages/codegen/src/project_config.mjs`, `packages/codegen/src/node_external_packages.mjs`, `packages/codegen/src/main.mjs` |
| `concepts/nodejs-runtime.md` | Compatibility targets and permission modes are separate axes; in-process V8 | `crates/nimbus-runtime/src/limits/axes.rs`, `crates/nimbus-runtime/src/limits/grants.rs`, `crates/nimbus-runtime/src/backends/v8/` |
| `reference/runtimes/node-apis.md` | API-family support table | generated evidence: `docs/private/staging/runtimes/nodejs/reference/node-apis.md` |
| `reference/runtimes/packages.md` | Package-support matrix | generated evidence: `docs/private/staging/runtimes/nodejs/reference/packages.md` |
| `reference/runtimes/node-compat.md` | Version table, contract, headline coverage numbers (2026-05-28) | generated evidence: `docs/private/staging/runtimes/nodejs/compatibility.md`, `docs/private/staging/runtimes/nodejs/evidence/latest.md` |

## Operators — install, deploy, lifecycle

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `operators/deploy-linux.md` | Install script: platforms, deps, `/usr/local/bin/nimbus`, runtime stack under `/usr/libexec/nimbus`, checksum + attestation | `scripts/install.sh` |
| `operators/deploy-linux.md` | `--port`/`--host`/`--data-dir`/`--tenant-provider`/`--allow-network` names + defaults; foreground start | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/config.rs`, `crates/nimbus-bin/src/main.rs` |
| `operators/deploy-linux.md` | Token minted on first boot before listener; JSON shape; `nimbus_at_` prefix; file 0600; path from service user's HOME/XDG | `crates/nimbus-operator/src/token.rs`, `crates/nimbus-operator/src/paths.rs`, `crates/nimbus-bin/src/start/boot.rs` |
| `operators/deploy-linux.md` | `/health` unauthenticated `{"ok":true}`; `GET /api/tenants` admin-gated | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/http/metadata.rs`, `crates/nimbus-server/src/http/tenants.rs` |
| `operators/deploy-linux.md` | No shipped systemd unit (tutorial authors one) | `crates/nimbus-assets/embedded/templates/` (absence; only machine-VM templates) |
| `operators/container-image.md` | Image `ghcr.io/nimbus/nimbus:<version>`, per-arch tags, multi-arch manifest, `latest` on stable | `.github/workflows/release.yml`, `Containerfile` |
| `operators/container-image.md` | Entrypoint/CMD, UID 10001, volume `/var/lib/nimbus`, port 8080, healthcheck, `HOME=/var/lib/nimbus` | `Containerfile` |
| `operators/container-image.md` | OCI release assets (image ref, SBOM, vulns SARIF, attestation); `gh attestation verify` | `.github/workflows/release.yml` |
| `operators/node-lifecycle.md` | `nimbus node install/status/logs/doctor/uninstall` flags and targets | `crates/nimbus-bin/src/node_service.rs`, `crates/nimbus-bin/src/cli_ux.rs` |
| `operators/node-lifecycle.md` | Unit names, install dirs, generated defaults, hardening directives, provenance comments | `crates/nimbus-bin/src/node_service.rs` |
| `operators/node-lifecycle.md` | `compose export quadlet` modes and flags; `compose up` | `crates/nimbus-bin/src/compose/commands.rs` |
| `operators/node-lifecycle.md` | `machine status` / `machine os upgrade` / `machine os apply`; `ghcr.io/nimbus/machine-os` | `crates/nimbus-bin/src/machine/command.rs` |
| `operators/updates.md` | No auto-upgrade; `brew upgrade --cask nimbus/tap/nimbus`; install-method detection | `crates/nimbus-server/src/system/install_method.rs` |
| `operators/updates.md` | Update check: 24h TTL, cache path, `NIMBUS_DISABLE_UPDATE_CHECK=1`, `/api/system/version-info` admin-gated | `crates/nimbus-server/src/system/version_check.rs`, `crates/nimbus-server/src/system/cache.rs`, `crates/nimbus-server/src/router.rs` |
| `operators/desktop-install.md` | Casks `nimbus/tap/nimbus` + `nimbus/tap/nimbus-desktop`; macOS 14+; notarized DMG | `nimbus/homebrew-tap` Casks (live), `scripts/install.sh` |
| `operators/desktop-install.md` | Desktop Linux x64 AppImage/deb/rpm; Windows NSIS unsigned; spawn-on-demand; updater on quit | `nimbus/desktop` electron-builder.yml + release assets (live) |

## Operators — data: storage, encryption, backup

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `operators/storage-backends.md` | `--tenant-provider` values; sqlite default; data dir `./data`; control dir fallback | `crates/nimbus-bin/src/start/config.rs` |
| `operators/storage-backends.md` | Per-tenant file layout `<data-dir>/<tenant-id>.sqlite3` / `.redb`; control plane `nimbus-control.db` embedded redb | `crates/nimbus-storage/src/async_storage/sqlite.rs`, `crates/nimbus-storage/src/async_storage/engine.rs`, `crates/nimbus-storage/src/async_storage/control.rs` |
| `operators/storage-backends.md` | Postgres schema-per-tenant, MySQL database-per-tenant, libSQL namespace-per-tenant; `nimbus_provider` / `tenant_` defaults | `crates/nimbus-engine/src/persistence_config.rs`, `crates/nimbus-bin/src/start/config.rs` |
| `operators/storage-backends.md` | libsql-replica required flags; pool min/max validation; cross-provider flag rejection | `crates/nimbus-bin/src/start/config.rs`, `crates/nimbus-storage/src/postgres/config.rs` |
| `operators/encryption.md` | Disabled by default; providers master-key-file/key-dir/aws-kms; orphaned-flag rejection | `crates/nimbus-bin/src/start/config.rs` |
| `operators/encryption.md` | Per-file DEK; `.nimbus-enc` manifest sidecar; plaintext-without-manifest startup failure | `crates/nimbus-storage/src/encryption/manifest.rs`, `crates/nimbus-storage/src/encryption/runtime.rs` |
| `operators/encryption.md` | Coverage: SQLCipher for SQLite/libsql cache; AES-256-GCM-SIV for redb/control; external backends control-plane-only | `crates/nimbus-engine/src/engine/encryption/mod.rs`, `crates/nimbus-storage/src/sqlite/encryption.rs` |
| `operators/encryption.md` | Master key 32 raw bytes; HKDF-SHA256 wrapping; key-dir hex descriptor naming; KMS GenerateDataKey/Decrypt/ReEncrypt | `crates/nimbus-storage/src/encryption/master_key_file.rs`, `crates/nimbus-storage/src/encryption/key_directory.rs`, `crates/nimbus-storage/src/encryption/aws_kms.rs` |
| `operators/encryption.md` | `nimbus encryption status/migrate/export/rotate-kek/rotate-dek` flags and semantics | `crates/nimbus-bin/src/encryption/mod.rs`, `crates/nimbus-bin/src/encryption/migrate.rs`, `crates/nimbus-bin/src/encryption/rotate.rs`, `crates/nimbus-bin/src/encryption/status.rs` |
| `operators/backup-restore.md` | No first-class backup/snapshot/PITR command | `crates/nimbus-bin/src/main.rs` (full CLI enum; absence) |
| `operators/backup-restore.md` | WAL mode + synchronous=FULL; `-wal`/`-shm` sidecars matter | `crates/nimbus-storage/src/sqlite/config.rs` |
| `operators/backup-restore.md` | Control DB holds usage tracking; present even with external backends | `crates/nimbus-storage/src/usage_store.rs`, `crates/nimbus-engine/src/persistence_config.rs` |
| `operators/backup-restore.md` | Encrypted DBs are SQLCipher (stock sqlite3 can't open); `nimbus encryption export` plaintext recovery | `crates/nimbus-storage/src/sqlite/encryption.rs`, `crates/nimbus-bin/src/encryption/mod.rs` |
| `operators/backup-restore.md` | Admin token regenerable; libSQL replica cache rebuildable | `crates/nimbus-bin/src/token.rs`, `crates/nimbus-storage/src/async_storage/libsql.rs` |

## Operators — administration + observability

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `operators/tenant-isolation.md` | Tenant CRUD endpoints + status codes (201/400/409/204); reserved `_` prefix; ID rules | `crates/nimbus-server/src/http/tenants.rs`, `crates/nimbus-core/src/types.rs`, `crates/nimbus-system/src/identity.rs` |
| `operators/tenant-isolation.md` | Deletion teardown order; per-provider removal (file/schema/database/namespace) | `crates/nimbus-engine/src/engine/tenants.rs`, `crates/nimbus-storage/src/postgres/provider.rs`, `crates/nimbus-storage/src/mysql/provider.rs`, `crates/nimbus-storage/src/libsql/remote.rs` |
| `operators/tenant-isolation.md` | `nimbus start` always Production isolation (no flag); `nimbus dev` LocalDevelopment + auto `demo` | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/dev.rs` |
| `operators/tenant-isolation.md` | Per-tenant runtime limit flags; queue overflow → 429 | `crates/nimbus-bin/src/start/runtime_limits.rs`, `crates/nimbus-runtime/src/executor/admission.rs`, `crates/nimbus-server/src/error_envelope.rs` |
| `operators/tenant-isolation.md` | `nimbus policy validate/explain/prove/diff` offline; accepted_risks fields | `crates/nimbus-bin/src/policy.rs`, `crates/nimbus-tenant/src/operator_policy/prove.rs` |
| `operators/observability.md` | Full `/debug/*` route inventory + `/health` + `/api/system/version-info`; no readiness endpoint | `crates/nimbus-server/src/router.rs` |
| `operators/observability.md` | License status fields + `NIMBUS_LICENSE_FILE`; encryption status shape; runtime metrics counters | `crates/nimbus-license/src/lib.rs`, `crates/nimbus-engine/src/engine/encryption/mod.rs`, `crates/nimbus-runtime/src/metrics.rs` |
| `operators/observability.md` | Tenant engine diagnostics groups; consistency report shape | `crates/nimbus-engine/src/tenant.rs`, `crates/nimbus-engine/src/verification.rs` |
| `operators/observability.md` | Logging via `RUST_LOG` (`target=level` directives, default info, stdout); latency WARN fields | `crates/nimbus-bin/src/main.rs`, `crates/nimbus-server/src/latency.rs`, `crates/nimbus-engine/src/engine/latency.rs` |
| `operators/observability.md` | Access audit log JSONL paths + record fields | `crates/nimbus-operator/src/audit.rs`, `crates/nimbus-operator/src/paths.rs` |
| `operators/hardening.md` | Local admin auth always on; token 32 bytes, 0600/0700, constant-time compare | `crates/nimbus-bin/src/start/boot.rs`, `crates/nimbus-operator/src/token.rs` |
| `operators/hardening.md` | Public bind needs `--allow-network` + token rotated within 30 days | `crates/nimbus-bin/src/start/network_bind.rs`, `crates/nimbus-operator/src/token.rs` |
| `operators/hardening.md` | CORS approval limited to localhost origins; admin route families audited | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/local_server/middleware.rs` |
| `operators/troubleshooting.md` | Bind/auth/schema/storage/encryption/license/codegen/bundle error strings (each entry quotes source text) | `crates/nimbus-bin/src/start/network_bind.rs`, `crates/nimbus-operator/src/access_policy.rs`, `crates/nimbus-core/src/error.rs`, `crates/nimbus-storage/src/encryption/runtime.rs`, `crates/nimbus-storage/src/sqlite/backend.rs`, `crates/nimbus-license/src/loading.rs`, `crates/nimbus-bin/src/codegen.rs`, `crates/nimbus-runtime/src/error.rs` |

## Concepts + Reference — tenancy and server

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `concepts/tenant-isolation.md` | Admit-once model; decision envelope; storage namespaces structural per provider | `crates/nimbus-server/src/tenant.rs`, `crates/nimbus-tenant/src/context.rs`, `crates/nimbus-tenant/src/policy_input.rs` |
| `concepts/tenant-isolation.md` | Production tier routing (in-process untrusted default; privileged/microvm/WASM routing) | `crates/nimbus-tenant/src/runtime_admission.rs` |
| `concepts/tenant-isolation.md` | Egress deny-by-default; host binds denied; digest-pinned image floor | `crates/nimbus-sandbox/src/egress.rs`, `crates/nimbus-tenant/src/policy_input.rs`, `crates/nimbus-tenant/src/image_admission.rs` |
| `concepts/tenant-isolation.md` | Application tenant claims must match route tenant; `_nimbus` operator-only | `crates/nimbus-tenant/src/context.rs`, `crates/nimbus-system/src/identity.rs` |
| `reference/configuration.md` | Storage + encryption flag ↔ `NIMBUS_*` env ↔ `persistence` config keys; unknown keys rejected | `crates/nimbus-bin/src/start/config.rs`, `crates/nimbus-bin/src/start/mod.rs` |
| `reference/deploy-admin-api.md` | `POST /api/admin/deploy`; `NIMBUS_DEPLOY_TOKEN` enablement; dual credentials (deploy bearer + admin header) | `crates/nimbus-server/src/router.rs`, `crates/nimbus-operator/src/access_policy.rs`, `crates/nimbus-server/src/local_server/middleware.rs` |
| `reference/deploy-admin-api.md` | Nested `artifacts.convex`/`artifacts.cloud_functions` schema; staging/activation semantics; no rollback | `crates/nimbus-server/src/http/deploy.rs` |

## Reference — CLI and configuration

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `reference/cli.md` | Root command list and command map (14 visible commands) | `crates/nimbus-bin/src/main.rs` |
| `reference/cli.md` | `start` flags, defaults (port 8080, host 127.0.0.1, `./data`, sqlite), `NIMBUS_*` env names, flag > env > config precedence | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/config.rs` |
| `reference/cli.md` | `dev` flags, port 3210, tail-log modes, `--no-open` semantics, `.nimbus/dev` data dir, walk-up bounded at `.git` | `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/dev/plan.rs`, `crates/nimbus-bin/src/path_boundary.rs` |
| `reference/cli.md` | `deploy` flags, `NIMBUS_DEPLOY_URL`/`NIMBUS_DEPLOY_TOKEN`, credentials-file fallback | `crates/nimbus-bin/src/deploy.rs` |
| `reference/cli.md` | `codegen`, `init` (adapter values), `token rotate`, `ui` | `crates/nimbus-bin/src/codegen.rs`, `crates/nimbus-bin/src/init.rs`, `crates/nimbus-bin/src/token.rs`, `crates/nimbus-bin/src/ui.rs` |
| `reference/cli.md` | `auth` subcommands; `rotate-admin` required before `start --allow-network` | `crates/nimbus-bin/src/auth.rs` |
| `reference/cli.md` | `machine` subcommands, init defaults (2 CPUs / 2048 MiB / 20 GiB / `default`), default image, ssh target resolution | `crates/nimbus-bin/src/machine/command.rs`, `crates/nimbus-bin/src/machine/mod.rs`, `crates/nimbus-bin/src/machine/handlers/transfer.rs` |
| `reference/cli.md` | `node` subcommands; exactly-one `--systemd`/`--container`; system scope default | `crates/nimbus-bin/src/node_service.rs` |
| `reference/cli.md` | `compose` subcommands; `COMPOSE_FILE` + walk-up discovery; quadlet export modes | `crates/nimbus-bin/src/compose/commands.rs`, `crates/nimbus-bin/src/compose/discovery.rs` |
| `reference/cli.md` | `policy`, `encryption`, `packages` subcommands and value sets | `crates/nimbus-bin/src/policy.rs`, `crates/nimbus-bin/src/encryption/mod.rs`, `crates/nimbus-bin/src/provision.rs` |
| `reference/cli.md` | `NIMBUS_LICENSE_FILE` env for `--license-file` | `crates/nimbus-license/src/lib.rs` |
| `reference/configuration.md` | Precedence CLI > env > config file; JSON keys under `persistence`; unknown keys rejected | `crates/nimbus-bin/src/start/config.rs` |
| `reference/configuration.md` | Network/bind table; 30-day admin-token freshness gate; systemd socket activation (`LISTEN_FDS`/`LISTEN_PID`, fd 3) | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/network_bind.rs`, `crates/nimbus-bin/src/start/boot.rs` |
| `reference/configuration.md` | Core storage / postgres / mysql / libsql tables (env names, config keys, defaults `nimbus_provider`, `tenant_`); min ≤ max pool rule on both postgres and mysql | `crates/nimbus-bin/src/start/config.rs`, `crates/nimbus-engine/src/persistence_config.rs`, `crates/nimbus-storage/src/postgres/config.rs`, `crates/nimbus-storage/src/mysql.rs`, `crates/nimbus-storage/src/libsql.rs` |
| `reference/configuration.md` | Runtime limit defaults (128 MB heap, 8 MB initial, 30 s timeout, 64 nested; derived instance/worker/in-flight budgets) | `crates/nimbus-bin/src/start/runtime_limits.rs`, `crates/nimbus-runtime/src/limits/resources.rs` |
| `reference/configuration.md` | App-dir resolution and required app surface; no source-tree discovery without `--app-dir` | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/boot.rs` |
| `reference/configuration.md` | Compose-file discovery order and `.git` boundary | `crates/nimbus-bin/src/compose/discovery.rs`, `crates/nimbus-bin/src/compose/file.rs` |
| `reference/configuration.md` | License resolution: flag > env > XDG path > built-in community license | `crates/nimbus-bin/src/start/boot.rs`, `crates/nimbus-license/src/loading.rs`, `crates/nimbus-bin/src/dirs.rs` |
| `reference/configuration.md` | `NIMBUS_DEPLOY_TOKEN` env-only enablement of the deploy admin API | `crates/nimbus-server/src/router.rs`, `crates/nimbus-operator/src/access_policy.rs` |

## Reference — JavaScript SDK

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `reference/sdk/index.md` | Package name, ESM-only, six entry points, react peers | `packages/nimbus/package.json` |
| `reference/sdk/index.md` | Binary-provisioned into `.nimbus/packages/` with `file:` specifiers; `convex` package wraps this SDK | `crates/nimbus-bin/src/provision.rs`, `packages/convex/package.json` |
| `reference/sdk/index.md` | Deployment URL `{origin}/convex/{tenant}` owns `/query` + `/ws` | `crates/nimbus-server/src/router.rs` |
| `reference/sdk/server.md` | Builder signatures, ctx types, db/scheduler/auth surfaces, httpRouter, defineTable/defineSchema, pagination validators | `packages/nimbus/src/server.ts` |
| `reference/sdk/server.md` | `v` validator namespace (11 constructors), `GenericId`/`Validator`/`Infer` | `packages/nimbus/src/values.ts` |
| `reference/sdk/client.md` | `NimbusClient` options, protocol (`nimbus.v2`, client_hello), auth timeout/refresh, reconnect, dedupe | `packages/nimbus/src/browser.ts`, `packages/nimbus/src/browser-utils.ts` |
| `reference/sdk/client.md` | `NimbusHttpClient` endpoints, 401-retry, auth token fetcher | `packages/nimbus/src/http-client.ts`, `crates/nimbus-server/src/router.rs` |
| `reference/sdk/client.md` | `NimbusRestClient` method/route table; methods-to-avoid caution (stale CRUD methods vs server routes) | `packages/nimbus/src/rest.ts`, `crates/nimbus-server/src/router.rs` |
| `reference/sdk/client.md` | `filters` required despite optional TS type | `crates/nimbus-core/src/query.rs` |
| `reference/sdk/resources.md` | `Nimbus` class, services/sandboxes/sessions verbs + paths, wait defaults, credential/endpoint discovery order | `packages/nimbus/src/index.ts`, `crates/nimbus-server/src/router.rs` |
| `reference/sdk/react.md` | Providers, hooks, skip semantics, paginated status machine, auth-state semantics | `packages/nimbus/src/react.ts` |

## Concepts — system, data, runtime

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `concepts/how-nimbus-works.md` | Five layers map to crate boundaries; server is the integration point | `crates/nimbus-server/src/lib.rs`, `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/construction.rs` |
| `concepts/how-nimbus-works.md` | Adapters are transport-agnostic libraries on the engine, not the server | `crates/nimbus-convex/`, `crates/nimbus-firebase/`, `crates/nimbus-cloud-functions/`, `crates/nimbus-mongodb/`, `crates/nimbus-dynamodb/`, `crates/nimbus-server/src/adapters/` |
| `concepts/how-nimbus-works.md` | `Engine` is the central coordinator; runtime is standalone V8 behind `HostBridge` with zero workspace deps | `crates/nimbus-engine/src/engine/mod.rs`, `crates/nimbus-runtime/Cargo.toml`, `crates/nimbus-runtime/src/host.rs` |
| `concepts/how-nimbus-works.md` | Bundles SHA-256 integrity-checked before invocation | `crates/nimbus-runtime/src/runtime/bundle.rs`, `crates/nimbus-runtime/src/runtime/driver/invocation.rs` |
| `concepts/how-nimbus-works.md` | Storage providers and per-tenant namespaces (file / schema / database / namespace) | `crates/nimbus-storage/src/sqlite.rs`, `crates/nimbus-storage/src/postgres/provider.rs`, `crates/nimbus-storage/src/mysql/provider.rs`, `crates/nimbus-storage/src/libsql/provider.rs` |
| `concepts/how-nimbus-works.md` | Per-adapter transport surfaces (Convex HTTP+WS, Firestore REST+gRPC, Cloud Functions HTTP, MongoDB wire listener, DynamoDB endpoint) | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/adapters/` |
| `concepts/data-and-mutations.md` | Documents = JSON + `_id`/`_creationTime`/`_updateTime`; tables created on first write in the same transaction | `crates/nimbus-core/src/document.rs`, `crates/nimbus-storage/src/store/write/direct.rs` |
| `concepts/data-and-mutations.md` | Schemaless tables accept any document; declared-field checks only | `crates/nimbus-core/src/schema.rs`, `crates/nimbus-engine/src/engine/mutations/direct/execution.rs` |
| `concepts/data-and-mutations.md` | Single engine-owned mutation path; scheduled dedup by execution id; OCC commit for runtime mutations | `crates/nimbus-engine/src/engine/mutations/`, `crates/nimbus-engine/src/engine/execution_units/`, `crates/nimbus-storage/src/store/write/direct.rs` |
| `concepts/data-and-mutations.md` | Document + index effects + commit-log append are one storage transaction on every backend | `crates/nimbus-storage/src/store/write/transaction.rs`, `crates/nimbus-storage/src/sqlite/write.rs`, `crates/nimbus-storage/src/postgres/write.rs` |
| `concepts/data-and-mutations.md` | Subscription invalidation is conservative; delivery is monotonic and coalesced | `crates/nimbus-engine/src/engine/mutations/commit_processing.rs`, `crates/nimbus-engine/src/subscriptions/delivery.rs` |
| `concepts/adapter-boundary.md` | Five protocol front doors over one engine; per-protocol auth (OIDC/JWT, SCRAM-SHA-256, SigV4) | `crates/nimbus-server/src/adapters/`, `crates/nimbus-convex/src/auth/`, `crates/nimbus-mongodb/src/auth.rs`, `crates/nimbus-dynamodb/src/dispatch.rs` |
| `concepts/adapter-boundary.md` | Namespace→tenant mapping; provider-shaped errors; adapters execute engine atomic write batches | `crates/nimbus-dynamodb/src/tenant.rs`, `crates/nimbus-mongodb/src/error.rs`, `crates/nimbus-engine/src/engine/transactions.rs` |
| `concepts/adapter-boundary.md` | Reactivity derives from committed-mutation events; `ctx.db` host calls share the engine path | `crates/nimbus-engine/src/engine/committed_mutations.rs`, `crates/nimbus-bridge/src/` |
| `concepts/runtime-permissions.md` | Grant families, exact-name semantics, mode ceilings (Restricted = all 14 families empty) | `crates/nimbus-runtime/src/limits/grants.rs` |
| `concepts/runtime-permissions.md` | Queries/mutations deny ambient authority; only actions carry grants; `"use node"` selects a target, not permissions | `crates/nimbus-runtime/src/runtime_capabilities.rs`, `crates/nimbus-runtime/src/limits/axes.rs` |
| `concepts/runtime-permissions.md` | Production admission routes risky grants to sandbox/trusted tiers; production default | `crates/nimbus-tenant/src/runtime_admission.rs`, `crates/nimbus-tenant/src/authority.rs` |
| `concepts/runtime-permissions.md` | Use-time grant re-checks; heap/watchdog/per-tenant invocation budgets | `crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs`, `crates/nimbus-runtime/src/limits/resources.rs`, `crates/nimbus-runtime/src/watchdog.rs` |

## Concepts + Reference — resources, scaling, capabilities

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `concepts/resource-model.md` | Services by tenant+name, sandboxes by id, sessions target exactly one | `crates/nimbus-server/src/http/sessions.rs`, `crates/nimbus-server/src/router.rs` |
| `concepts/resource-model.md` | Backend kinds (sandbox/built-in/external); built-in provider allowlist; only sandbox-backed services launch | `crates/nimbus-services/src/catalog.rs`, `crates/nimbus-services/src/manager/definitions.rs`, `crates/nimbus-services/src/manager/launch.rs` |
| `concepts/resource-model.md` | Sandbox root (rootfs/OCI), profiles worker/desktop, container + krun backends, krun execute fail-closed | `crates/nimbus-sandbox/src/spec.rs`, `crates/nimbus-server/src/http/sandboxes.rs`, `crates/nimbus-sandbox/src/backend.rs`, `crates/nimbus-sandbox/src/backends/krun/vm/launch.rs` |
| `concepts/resource-model.md` | Deny-by-default egress; API redacts launch inputs | `crates/nimbus-sandbox/src/egress.rs`, `crates/nimbus-server/src/http/sandbox_spec.rs` |
| `concepts/resource-model.md` | Session TTL default 15 min / cap 1 h; per-target channel rules; generation-pinned target snapshots | `crates/nimbus-services/src/manager/sessions.rs` |
| `concepts/resource-model.md` | Exact service grant / sandbox reach required; session ops audited | `crates/nimbus-server/src/http/sessions.rs` |
| `concepts/scaling.md` | No clustering/coordination substrate exists (single-process baseline) | workspace `Cargo.toml`/`Cargo.lock` (absence) |
| `concepts/scaling.md` | Per-tenant admission queue, shedding, journal/apply lag; invocation caps → 429 | `crates/nimbus-engine/src/tenant/mutation/admission.rs`, `crates/nimbus-runtime/src/executor/admission.rs`, `crates/nimbus-server/src/error_envelope.rs` |
| `concepts/scaling.md` | libSQL remote-primary writes + per-tenant replica reads with measured freshness | `crates/nimbus-storage/src/libsql/provider.rs`, `crates/nimbus-storage/src/libsql/freshness.rs` |
| `reference/current-capabilities.md` | Native API route families (tenants, documents, query, schema, schedule, ws) | `crates/nimbus-server/src/router.rs` |
| `reference/current-capabilities.md` | Single-field + composite index planning, backfill lifecycle; index-aware subscriptions | `crates/nimbus-core/src/schema.rs`, `crates/nimbus-engine/src/tenant/query_planning.rs`, `crates/nimbus-engine/src/subscriptions/` |
| `reference/current-capabilities.md` | Adapter enablement statuses (Convex/Cloud Functions CLI-wired; Firestore config-gated; MongoDB/DynamoDB embedding API) | `crates/nimbus-bin/src/start/boot.rs`, `crates/nimbus-server/src/construction.rs`, `crates/nimbus-server/src/adapters/` |
| `reference/current-capabilities.md` | Storage/encryption/sandbox/machine/resource API statuses | `crates/nimbus-bin/src/start/config.rs`, `crates/nimbus-sandbox/src/backends/`, `crates/nimbus-bin/src/machine/mod.rs`, `crates/nimbus-server/src/router.rs` |
