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
| `operators/desktop-install.md` | Casks `nimbus/tap/nimbus` + `nimbus/tap/nimbus-desktop`; macOS 14+; notarized DMG | external repo nimbus/homebrew-tap Casks (live), `scripts/install.sh` |
| `operators/desktop-install.md` | Desktop Linux x64 AppImage/deb/rpm; Windows NSIS unsigned; spawn-on-demand; updater on quit | external repo nimbus/desktop electron-builder.yml + release assets (live) |

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
| `operators/backup-restore.md` | Admin token regenerable; libSQL replica cache rebuildable | `crates/nimbus-bin/src/token.rs`, `crates/nimbus-storage/src/libsql/provider.rs` |

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
| `reference/cli.md` | `deploy` flags, `NIMBUS_DEPLOY_URL`/`NIMBUS_DEPLOY_TOKEN`/`NIMBUS_ADMIN_TOKEN`, credentials-file fallback, loopback admin-token auto-discovery | `crates/nimbus-bin/src/deploy.rs` |
| `reference/cli.md` | `codegen`, `init` (adapter values), `token rotate`, `ui` | `crates/nimbus-bin/src/codegen.rs`, `crates/nimbus-bin/src/init.rs`, `crates/nimbus-bin/src/token.rs`, `crates/nimbus-bin/src/ui.rs` |
| `reference/cli.md` | `auth` subcommands; `rotate-admin` required before `start --allow-network` | `crates/nimbus-bin/src/auth.rs` |
| `reference/cli.md` | `machine` subcommands, init defaults (2 CPUs / 2048 MiB / 20 GiB / `default`), default image, ssh target resolution | `crates/nimbus-bin/src/machine/command.rs`, `crates/nimbus-bin/src/machine/mod.rs`, `crates/nimbus-bin/src/machine/handlers/transfer.rs` |
| `reference/cli.md` | `node` subcommands; exactly-one `--systemd`/`--container`; system scope default | `crates/nimbus-bin/src/node_service.rs` |
| `reference/cli.md` | `compose` subcommands; `COMPOSE_FILE` + walk-up discovery; quadlet export modes | `crates/nimbus-bin/src/compose/commands.rs`, `crates/nimbus-bin/src/compose/discovery.rs` |
| `reference/cli.md` | `policy`, `encryption`, `packages` subcommands and value sets | `crates/nimbus-bin/src/policy.rs`, `crates/nimbus-bin/src/encryption/mod.rs`, `crates/nimbus-bin/src/provision.rs` |
| `reference/cli.md` | `NIMBUS_LICENSE_FILE` env for `--license-file` | `crates/nimbus-license/src/lib.rs` |
| `reference/configuration.md` | Precedence CLI > env > config file; JSON keys under `persistence`; unknown keys rejected | `crates/nimbus-bin/src/start/config.rs` |
| `reference/configuration.md` | Network/bind table; explicit-rotation public-bind gate (30-day age advisory); systemd socket activation (`LISTEN_FDS`/`LISTEN_PID`, fd 3) | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/network_bind.rs`, `crates/nimbus-bin/src/start/boot.rs` |
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

## Concepts — architecture (request path)

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `concepts/architecture/server-transport.md` | `RouterOptions`/`build_router`, opt-in builder surface, route families, CORS layer, loopback-origin predicate, middleware layering | `crates/nimbus-server/src/router.rs` |
| `concepts/architecture/server-transport.md` | `ServeOptions`/`serve`, pre-bound listener, graceful shutdown, MongoDB/DynamoDB sibling listeners + abort-on-exit, listener-state recording, TTL sweeper spawn | `crates/nimbus-server/src/construction.rs` |
| `concepts/architecture/server-transport.md` | Route lists: native `/api/*`, `/ws`, `/debug/*`, `/ui/*`, `/health`, `/demos`, Convex `/convex/{tenant}/*` + `/convex/{tenant}/ws`, Firestore REST + `/google.firestore.v1.Firestore/*`, gRPC-Web layer, shared `Listen` service instance, Cloud Functions fallback | `crates/nimbus-server/src/router.rs` |
| `concepts/architecture/server-transport.md` | Admin gate: origin allowlist → credential extraction → route-family gate, audit on every decision, credential modes (standard vs deploy admin-header-only) | `crates/nimbus-server/src/local_server/middleware.rs`, `crates/nimbus-server/src/local_server/mod.rs`, `crates/nimbus-operator/src/access_policy.rs` |
| `concepts/architecture/server-transport.md` | Admin token minted on disk at first boot, rotatable | `crates/nimbus-server/src/local_server/mod.rs`, `crates/nimbus-server/src/router.rs` |
| `concepts/architecture/server-transport.md` | Service-control routes authorize per-handler via principal-class checks | `crates/nimbus-server/src/http/authz.rs`, `crates/nimbus-server/src/http/sessions.rs` |
| `concepts/architecture/server-transport.md` | MongoDB listener loopback-only bind guard, per-connection tasks | `crates/nimbus-server/src/adapters/mongodb/listener.rs` |
| `concepts/architecture/server-transport.md` | DynamoDB listener: single `POST /`, body cap pre-parse, DynamoDB Local port convention, lookup-mode loopback guard | `crates/nimbus-server/src/adapters/dynamodb/listener.rs`, `crates/nimbus-dynamodb/src/config.rs` |
| `concepts/architecture/server-transport.md` | Bind policy in binary: loopback default, `--allow-network` two-stage gate, admin-token freshness tripwire, systemd socket activation fd adoption | `crates/nimbus-bin/src/start/boot.rs`, `crates/nimbus-bin/src/start/network_bind.rs` |
| `concepts/architecture/adapters.md` | Five adapter crates depend on engine/core, never on server; runtime-executing pair adds nimbus-runtime + nimbus-bridge; tenant-resolving adapters add nimbus-tenant | `crates/nimbus-convex/Cargo.toml`, `crates/nimbus-firebase/Cargo.toml`, `crates/nimbus-cloud-functions/Cargo.toml`, `crates/nimbus-mongodb/Cargo.toml`, `crates/nimbus-dynamodb/Cargo.toml` |
| `concepts/architecture/adapters.md` | "must not depend on nimbus-server or axum"; dispatch-entrypoint shape | `crates/nimbus-dynamodb/src/lib.rs` |
| `concepts/architecture/adapters.md` | Per-crate ownership rows (registries, operations, dispatch, codecs, stream registries) | `crates/nimbus-convex/src/lib.rs`, `crates/nimbus-firebase/src/lib.rs`, `crates/nimbus-firebase/src/grpc.rs`, `crates/nimbus-cloud-functions/src/lib.rs`, `crates/nimbus-mongodb/src/lib.rs`, `crates/nimbus-mongodb/src/commands/mod.rs`, `crates/nimbus-dynamodb/src/lib.rs` |
| `concepts/architecture/adapters.md` | Shim layer contents per adapter | `crates/nimbus-server/src/adapters/mod.rs`, `crates/nimbus-server/src/adapters/convex/mod.rs`, `crates/nimbus-server/src/adapters/firebase/mod.rs`, `crates/nimbus-server/src/adapters/firebase/grpc/mod.rs`, `crates/nimbus-server/src/adapters/cloud_functions/http/invocation.rs`, `crates/nimbus-server/src/adapters/mongodb/mod.rs`, `crates/nimbus-server/src/adapters/dynamodb/mod.rs` |
| `concepts/architecture/adapters.md` | nimbus-bridge modules (host_calls, capabilities, admission, read_tracking, responses, state, cancellation), `build_runtime_host_bootstrap` beginning a mutation execution unit | `crates/nimbus-bridge/src/lib.rs`, `crates/nimbus-bridge/src/read_tracking/mod.rs`, `crates/nimbus-bridge/src/admission.rs`, `crates/nimbus-bridge/src/host_calls/mod.rs` |
| `concepts/architecture/adapters.md` | Convex auth: OIDC (domain + applicationID) and custom JWT (issuer/jwks/RS256\|ES256); registry implements `ApplicationAuthVerifier`; installed as deployment-wide bearer verifier reused by Firestore REST/gRPC and Cloud Functions callable | `crates/nimbus-convex/src/auth/config.rs`, `crates/nimbus-convex/src/auth/mod.rs`, `crates/nimbus-auth/src/lib.rs`, `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/application_auth.rs`, `crates/nimbus-server/src/adapters/cloud_functions/http/callable.rs` |
| `concepts/architecture/adapters.md` | Firestore emulator mock-token auth is explicit opt-in, off by default | `crates/nimbus-firebase/src/lib.rs` |
| `concepts/architecture/adapters.md` | MongoDB SCRAM-SHA-256 conversation, unauthenticated commands refused | `crates/nimbus-mongodb/src/auth.rs`, `crates/nimbus-mongodb/src/commands/mod.rs`, `crates/nimbus-mongodb/src/lib.rs` |
| `concepts/architecture/adapters.md` | DynamoDB SigV4 Strict default, ±15-minute window, access-key→tenant binding, lookup mode loopback-only | `crates/nimbus-dynamodb/src/tenant.rs`, `crates/nimbus-dynamodb/src/config.rs`, `crates/nimbus-server/src/adapters/dynamodb/listener.rs` |
| `concepts/architecture/engine-mutation-path.md` | `Engine` struct is the top-level coordinator; tenants map, load-gated creation, providers, clock, observer registries | `crates/nimbus-engine/src/engine/mod.rs`, `crates/nimbus-engine/src/engine/tenant_load_gate.rs`, `crates/nimbus-engine/src/engine/tenants.rs` |
| `concepts/architecture/engine-mutation-path.md` | `TenantRuntime` owns store, read executor, subscriptions, schema swap, document cache, admission gate, journal, delivery worker | `crates/nimbus-engine/src/tenant.rs` |
| `concepts/architecture/engine-mutation-path.md` | Public write surface (`insert/update/delete_document*` + `_async`/`_with` variants) funnels into private `apply_mutation_with_mode` family; Immediate vs scheduled mode carrying execution id | `crates/nimbus-engine/src/engine/mutations/direct/api.rs`, `crates/nimbus-engine/src/engine/mutations/direct/execution.rs` |
| `concepts/architecture/engine-mutation-path.md` | Schema validation, authorization, and index-coupled store calls (`insert_with_indexes`, `update_with_indexes_validated`) on the single path | `crates/nimbus-engine/src/engine/mutations/direct/execution.rs` |
| `concepts/architecture/engine-mutation-path.md` | Admission gate: bounded queue, resource-exhausted rejection, CoDel shedding (5 ms target / 100 ms interval) | `crates/nimbus-engine/src/tenant/mutation/admission.rs` |
| `concepts/architecture/engine-mutation-path.md` | Journal worker: batches up to 32, overlay planning, durable batch append = ack point, ordered apply, crash recovery of durable-but-unapplied records | `crates/nimbus-engine/src/engine/mutations/journal.rs` |
| `concepts/architecture/engine-mutation-path.md` | Durable/applied head watermarks; reads wait for applied head to reach durable head (read-your-writes) | `crates/nimbus-engine/src/tenant/mutation/journal.rs`, `crates/nimbus-engine/src/engine/queries/documents.rs` |
| `concepts/architecture/engine-mutation-path.md` | Execution units: schema + persistence snapshot, snapshot sequence, staged writes, dependency tracking; OCC commit under sequence lock (schema-unchanged check + commit-log intersection); conflict error text "transaction conflict detected; retry the mutation" | `crates/nimbus-engine/src/engine/execution_units/mod.rs`, `crates/nimbus-engine/src/engine/execution_units/commit.rs`, `crates/nimbus-engine/src/engine/execution_units/staging.rs`, `crates/nimbus-engine/src/engine/execution_units/reads.rs` |
| `concepts/architecture/engine-mutation-path.md` | Dependency kinds: tables, documents, index ranges, predicates, missing tables, paginated windows | `crates/nimbus-core/src/dependency.rs` |
| `concepts/architecture/engine-mutation-path.md` | Execution-unit consumers: V8 bridge and adapter transactional surfaces | `crates/nimbus-bridge/src/lib.rs`, `crates/nimbus-mongodb/`, `crates/nimbus-firebase/`, `crates/nimbus-dynamodb/`, `crates/nimbus-server/src/adapters/cloud_functions/` |
| `concepts/architecture/engine-mutation-path.md` | Commit processing: affected ids, subscription work dispatch, trigger candidates, observers; applied batches coalesced to latest sequence | `crates/nimbus-engine/src/engine/mutations/commit_processing.rs`, `crates/nimbus-engine/src/engine/committed_mutations.rs` |
| `concepts/architecture/engine-mutation-path.md` | Scheduler: wakeup-or-next-due loop, concurrent per-tenant ticks bounded by available parallelism, execution id derived from job id (`scheduled:<job-id>`), startup recovery of running jobs, cron expansion | `crates/nimbus-engine/src/scheduler.rs`, `crates/nimbus-engine/src/engine/scheduler/coordination.rs`, `crates/nimbus-engine/src/engine/scheduler/cron.rs`, `crates/nimbus-engine/src/engine/scheduler/scheduled_jobs.rs` |
| `concepts/architecture/engine-mutation-path.md` | Subscriptions: conservative invalidation with full re-evaluation, monotonic last-delivered-sequence, coalesced wakeups, bounded channel (256) with removal on overflow, policy-revision termination | `crates/nimbus-engine/src/subscriptions.rs`, `crates/nimbus-engine/src/subscriptions/delivery.rs`, `crates/nimbus-engine/src/subscriptions/queue.rs`, `crates/nimbus-engine/src/subscriptions/registry.rs` |
| `concepts/architecture/storage.md` | Engine-side provider/persistence enums with five backend variants | `crates/nimbus-engine/src/persistence/provider.rs`, `crates/nimbus-engine/src/persistence/tenant.rs` |
| `concepts/architecture/storage.md` | Capability traits; SQLite is default embedded kind, redb alternative | `crates/nimbus-storage/src/traits/mod.rs`, `crates/nimbus-storage/src/async_storage/engine.rs` |
| `concepts/architecture/storage.md` | Isolation units: file (SQLite/redb), schema (Postgres), database (MySQL), namespace (libSQL) | `crates/nimbus-storage/src/sqlite.rs`, `crates/nimbus-storage/src/store.rs`, `crates/nimbus-storage/src/postgres/provider.rs`, `crates/nimbus-storage/src/mysql/provider.rs`, `crates/nimbus-storage/src/libsql/provider.rs` |
| `concepts/architecture/storage.md` | At-rest formats: JSON text columns (`data_json`/`typed_fields_json`) on SQL family; MessagePack documents on redb; MessagePack commit-log records with integrity validation everywhere | `crates/nimbus-storage/src/sqlite.rs`, `crates/nimbus-storage/src/document_codec.rs`, `crates/nimbus-storage/src/commit_log.rs` |
| `concepts/architecture/storage.md` | SQLite `BEGIN IMMEDIATE` writer + pooled snapshot reads | `crates/nimbus-storage/src/sqlite/config.rs` |
| `concepts/architecture/storage.md` | Postgres per-tenant schema + metadata schema + advisory locks; MySQL per-tenant database + metadata database | `crates/nimbus-storage/src/postgres/provider.rs`, `crates/nimbus-storage/src/postgres/config.rs`, `crates/nimbus-storage/src/mysql/provider.rs` |
| `concepts/architecture/storage.md` | libSQL: remote immediate-mode write transactions, local replica reads, freshness barriers and stats | `crates/nimbus-storage/src/libsql/write.rs`, `crates/nimbus-storage/src/libsql/freshness.rs`, `crates/nimbus-storage/src/libsql/remote.rs` |
| `concepts/architecture/storage.md` | Atomicity mechanism per backend (redb write txn, SQLite immediate txn, SQL `COMMIT`, libSQL immediate remote txn) | `crates/nimbus-storage/src/store/write/transaction.rs`, `crates/nimbus-storage/src/sqlite/config.rs`, `crates/nimbus-storage/src/postgres/write.rs`, `crates/nimbus-storage/src/libsql/write.rs` |
| `concepts/architecture/storage.md` | Scheduled-execution dedup table on every backend; execution id registered inside the write transaction | `crates/nimbus-storage/src/sqlite/write.rs`, `crates/nimbus-storage/src/traits/mod.rs`, `crates/nimbus-storage/src/sqlite.rs` |
| `concepts/architecture/storage.md` | Index lifecycle `Pending/Backfilling/Enabled(default)/Deleting`; maintained = Backfilling + Enabled; stable index identity preserved across schema replace | `crates/nimbus-core/src/schema.rs` |
| `concepts/architecture/storage.md` | Schema replace rebuilds index keys in the same transaction (redb); SQLite native expression indexes | `crates/nimbus-storage/src/schema_store.rs`, `crates/nimbus-storage/src/sqlite/schema.rs` |
| `concepts/architecture/storage.md` | Envelope encryption: per-file 256-bit DEK, three key providers (HKDF-SHA256 master key file, key-dir, AWS KMS behind feature), `.nimbus-enc` AEAD-bound manifests | `crates/nimbus-storage/src/encryption/mod.rs`, `crates/nimbus-storage/src/encryption/master_key_file.rs`, `crates/nimbus-storage/src/encryption/key_directory.rs`, `crates/nimbus-storage/src/encryption/aws_kms.rs`, `crates/nimbus-storage/src/encryption/manifest.rs` |
| `concepts/architecture/storage.md` | SQLCipher hook: raw 32-byte DEK via key pragma, temp store to memory, open-time verification; also applied to libSQL replica caches (`ManifestCipher::SqlCipher`) | `crates/nimbus-storage/src/sqlite/encryption.rs`, `crates/nimbus-storage/src/libsql/remote.rs`, `crates/nimbus-storage/src/libsql/provider.rs` |
| `concepts/architecture/storage.md` | redb per-page AES-256-GCM-SIV (4096-byte pages, fresh nonce, position + version in AAD) for tenant and control-plane files | `crates/nimbus-storage/src/encrypted_redb.rs`, `crates/nimbus-storage/src/store/write/store_entry.rs` |
| `concepts/architecture/storage.md` | Local embedded control-plane database separate from tenant stores | `crates/nimbus-engine/src/persistence/control.rs` |

## Concepts — architecture (execution and isolation)

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `concepts/architecture/runtime-isolates.md` | Zero workspace deps; deno_* dependency list; `bun-jsc-linked-adapter` non-default feature | `crates/nimbus-runtime/Cargo.toml` |
| `concepts/architecture/runtime-isolates.md` | Runtime vocabulary (`RuntimeBundle`, `RuntimePolicy`, `InvocationAuth`, exports) | `crates/nimbus-runtime/src/lib.rs`, `crates/nimbus-runtime/src/runtime.rs` |
| `concepts/architecture/runtime-isolates.md` | `HostBridge` trait, `HostCallOperation` enum, single host-call channel | `crates/nimbus-runtime/src/host.rs` |
| `concepts/architecture/runtime-isolates.md` | Server-side bridge impls over `Arc<Engine>`; shared plumbing crate direction | `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/mod.rs`, `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs`, `crates/nimbus-cloud-functions/src/host_bridge.rs`, `crates/nimbus-bridge/src/lib.rs` |
| `concepts/architecture/runtime-isolates.md` | Deno-family + rusty_v8 maintained forks (pinned) | `Cargo.toml`, `Cargo.lock` |
| `concepts/architecture/runtime-isolates.md` | Backend seam, V8 primary, Bun/JSC fail-closed when not linked (disabled error, no fallback), linked-timeout rejection, axis validation panics | `crates/nimbus-runtime/src/backends/mod.rs`, `crates/nimbus-runtime/src/backends/v8/mod.rs`, `crates/nimbus-runtime/src/backends/bun_jsc/mod.rs`, `crates/nimbus-runtime/src/backends/bun_jsc/adapter.rs`, `crates/nimbus-runtime/src/limits/axes.rs` |
| `concepts/architecture/runtime-isolates.md` | Compatibility targets (`WebStandardIsolate`, Node20/22/24/26), LTS lane registry; 14 grant families; Restricted = all 14 empty; Standard forbids FFI | `crates/nimbus-runtime/src/limits/axes.rs`, `crates/nimbus-runtime/src/limits/grants.rs` |
| `concepts/architecture/runtime-isolates.md` | SHA-256 re-hash on every invocation; call sites on all three execution paths | `crates/nimbus-runtime/src/runtime/bundle.rs`, `crates/nimbus-runtime/src/runtime/cooperative.rs`, `crates/nimbus-runtime/src/runtime/driver/invocation.rs`, `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs` |
| `concepts/architecture/runtime-isolates.md` | Heap limits at isolate creation + near-heap-limit terminate; watchdog thread wall-clock terminate; external cancellation via watchdog | `crates/nimbus-runtime/src/runtime/driver/construction.rs`, `crates/nimbus-runtime/src/runtime/driver/invocation.rs`, `crates/nimbus-runtime/src/watchdog.rs` |
| `concepts/architecture/runtime-isolates.md` | Per-tenant admission (active/in-flight/queued caps, round-robin promotion, queue-full rejection); worker threads; pool kinds; routing affinity | `crates/nimbus-runtime/src/executor/admission.rs`, `crates/nimbus-runtime/src/executor/admission/tenant_fairness.rs`, `crates/nimbus-runtime/src/executor/facade.rs`, `crates/nimbus-runtime/src/limits/axes.rs`, `crates/nimbus-runtime/src/affinity.rs` |
| `concepts/architecture/sandbox-machines.md` | `SandboxBackend` trait shape; `reload_egress_policy` errors by default; `remove_tenant_artifacts` defaults Ok; `SandboxBackendKind { Container, Krun }` | `crates/nimbus-sandbox/src/backend.rs`, `crates/nimbus-sandbox/src/lib.rs` |
| `concepts/architecture/sandbox-machines.md` | Container backend tooling: crun default runtime, conmon, buildah builds, netavark/aardvark-dns; `ContainerLaunchMode { Execute, PlanOnly }`; gvproxy port forwarder; `SandboxEgressProxy` enforcement | `crates/nimbus-sandbox/src/backends/container/runtime.rs`, `crates/nimbus-sandbox/src/backends/container/mod.rs` |
| `concepts/architecture/sandbox-machines.md` | Krun execute fail-closed: gate runs before image materialization; PlanOnly allowed; Linux-host requirement on execute paths | `crates/nimbus-sandbox/src/backends/krun/vm/launch.rs`, `crates/nimbus-sandbox/src/backends/krun/vm.rs`, `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs` |
| `concepts/architecture/sandbox-machines.md` | `SandboxSpec` fields; root spec / image source / owner enums; mount rules (TenantVolume-only, max 32, absolute, no `.`/`..`, protected paths); per-sandbox accounted defaults; per-tenant quota defaults | `crates/nimbus-sandbox/src/spec.rs` |
| `concepts/architecture/sandbox-machines.md` | Deny-by-default egress (empty allow list = deny-all); rule fields; `allow_internal_ips` gating; TCP rules cannot carry HTTP fields; reserved enforcement env keys rejected in specs | `crates/nimbus-sandbox/src/egress.rs`, `crates/nimbus-sandbox/src/spec.rs` |
| `concepts/architecture/sandbox-machines.md` | API redaction: rootfs/build roots rejected as operator-only input; redacted response shapes for argv/entrypoint/command/env | `crates/nimbus-server/src/http/sandbox_spec.rs` |
| `concepts/architecture/sandbox-machines.md` | Machine model: providers Krunkit/Wsl2 + capability sets; image sources OciReference / HttpUrl (mandatory `#sha256=` fragment) / LocalDisk; krunkit + gvproxy helper binaries; lifecycle states | `crates/nimbus-machine/src/lib.rs` |
| `concepts/architecture/sandbox-machines.md` | Defaults: machine name "default", `ghcr.io/nimbus/machine-os` digest-pinned on macOS, 2 CPU / 2048 MiB / 20 GiB, macOS shared volumes; init hardcodes Krunkit; subcommand list | `crates/nimbus-bin/src/machine/mod.rs`, `crates/nimbus-bin/src/machine/handlers.rs`, `crates/nimbus-bin/src/machine/command.rs` |
| `concepts/architecture/sandbox-machines.md` | WSL2 declared but lifecycle errors "not available on this host yet" | `crates/nimbus-bin/src/machine/manager/stop.rs`, `crates/nimbus-bin/src/machine/manager/readiness.rs`, `crates/nimbus-machine/src/lib.rs` |
| `concepts/architecture/sandbox-machines.md` | Topology rule by construction: guest API builds only `ContainerSandboxBackend`; host-side `ForwardedMachineApiSandboxBackend` reports Container kind, forwards over unix socket, rejects rootfs and standalone-owned specs | `crates/nimbus-bin/src/machine/api.rs`, `crates/nimbus-bin/src/machine/backend.rs` |
| `concepts/architecture/auth-trust.md` | `nimbus-auth` ownership: verifier trait, error taxonomy, principal normalization, bearer parsing, emulator mock path | `crates/nimbus-auth/src/lib.rs` |
| `concepts/architecture/auth-trust.md` | Token mint (SystemRandom, `nimbus_at_`), 0600 + atomic replace + file lock, constant-time compare, `rotated_at` semantics | `crates/nimbus-operator/src/token.rs` |
| `concepts/architecture/auth-trust.md` | Bearer/`X-Nimbus-Admin-Token` acceptance, session cookies, credential modes incl. `AdminHeaderOnly`, route families, origin allowlist, audit log | `crates/nimbus-operator/src/access_policy.rs`, `crates/nimbus-operator/src/access.rs`, `crates/nimbus-operator/src/policy.rs`, `crates/nimbus-operator/src/audit.rs`, `crates/nimbus-operator/src/paths.rs`, `crates/nimbus-server/src/local_server/middleware.rs`, `crates/nimbus-server/src/router.rs` |
| `concepts/architecture/auth-trust.md` | `--allow-network` opt-in + explicit-rotation public-bind gate; never-rotated refused; 30-day age advisory warning | `crates/nimbus-bin/src/start/network_bind.rs`, `crates/nimbus-operator/src/token.rs` |
| `concepts/architecture/auth-trust.md` | `NIMBUS_DEPLOY_TOKEN` separate credential; disabled when unset; HMAC constant-time compare; deploy route needs admin header too | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/http/deploy.rs`, `crates/nimbus-operator/src/access_policy.rs`, `crates/nimbus-bin/src/start/boot.rs`, `crates/nimbus-bin/src/deploy.rs` |
| `concepts/architecture/auth-trust.md` | Deployment-scoped resolution; fail-closed on unverifiable bearer / no verifier; anonymous default | `crates/nimbus-server/src/application_auth.rs` |
| `concepts/architecture/auth-trust.md` | Convex OIDC discovery + issuer cross-check + JWKS + alg/audience/temporal checks; custom JWT; `InvocationAuth` with verified identity | `crates/nimbus-convex/src/auth/verifier/identity.rs`, `crates/nimbus-convex/src/auth/config.rs`, `crates/nimbus-convex/src/lib.rs` |
| `concepts/architecture/auth-trust.md` | `PrincipalContext` shape (authenticated, claims, verified_claims) | `crates/nimbus-core/src/auth/mod.rs` |
| `concepts/architecture/auth-trust.md` | `TableAccessPolicy` rules; no-policy = unrestricted; `enforce_mutation_authorization` on direct + journal paths; read rules compiled to planner filters + residual check | `crates/nimbus-core/src/auth/access.rs`, `crates/nimbus-engine/src/engine/mutations/authorization.rs`, `crates/nimbus-engine/src/engine/mutations/direct/execution.rs`, `crates/nimbus-engine/src/engine/mutations/journal.rs`, `crates/nimbus-engine/src/engine/queries/authorization.rs` |
| `concepts/architecture/tenancy.md` | Admit-once `TenantIsolationDecision`; envelope contents; SHA-256 fingerprint id `tid_<hex>`; deterministic over inputs; `ensure_*` re-checks | `crates/nimbus-tenant/src/decision.rs`, `crates/nimbus-tenant/src/context.rs` |
| `concepts/architecture/tenancy.md` | Nine policy decision families; fail-closed defaults (no public exposure, no generic loopback, egress deny-all, no host binds, no ambient secret materialization, default audit redaction set) | `crates/nimbus-tenant/src/policy_input.rs` |
| `concepts/architecture/tenancy.md` | `TenantIsolationMode` with `#[default] Production`; router builder uses the default; authority classes operator/application/system | `crates/nimbus-tenant/src/authority.rs`, `crates/nimbus-server/src/router.rs` |
| `concepts/architecture/tenancy.md` | Runtime admission tiers and routing (microVM / in-process-trusted / WASM triggers) | `crates/nimbus-tenant/src/runtime_admission.rs` |
| `concepts/architecture/tenancy.md` | Image admission: digest-pinned floor (tag-only rejected), local builds denied unless allowed, exact pinned-reference match, registry allowlist, optional verification provider | `crates/nimbus-tenant/src/image_admission.rs` |
| `concepts/architecture/tenancy.md` | `_nimbus` system tenant; `_`-prefix reserved (user tenant ids rejected); sixteen system tables enumerated | `crates/nimbus-system/src/identity.rs`, `crates/nimbus-system/src/schema.rs` |
| `concepts/architecture/tenancy.md` | One storage namespace per tenant: namespace carried in storage policy decision; persistence provider opens/creates/deletes per `TenantId` | `crates/nimbus-tenant/src/policy_input.rs`, `crates/nimbus-engine/src/persistence/provider.rs` |
| `concepts/architecture/tenancy.md` | `TenantRuntime` per-tenant state (persistence, schema, subscriptions, caches, trigger queues); per-tenant `MutationAdmissionGate` + journal with bounded default capacities | `crates/nimbus-engine/src/tenant.rs` |
| `concepts/architecture/tenancy.md` | `RuntimeTenantBudget` caps (active slots, in-flight/queued invocations, worker threads, heap, timeout, nested depth) | `crates/nimbus-runtime/src/limits/resources.rs` |
| `concepts/architecture/tenancy.md` | Evidence redaction: canonical snake_case reason codes, sensitive-text replacement, event scope `nimbus.tenant_isolation` | `crates/nimbus-tenant/src/evidence.rs` |

## Concepts — architecture (operating the binary)

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `concepts/architecture/node-lifecycle.md` | `nimbus node install` renders `nimbus.service` / `nimbus.socket` / `nimbus.container`; system vs user scope; dry-run; doctor probe; Linux-only mutation gate | `crates/nimbus-bin/src/node_service.rs` |
| `concepts/architecture/node-lifecycle.md` | Provenance headers (template version, generating command, SHA-256); image reference validation (registry path pin, `latest` rejected); no raw `ExecStart`/unit-text pass-through | `crates/nimbus-bin/src/node_service.rs` |
| `concepts/architecture/node-lifecycle.md` | Socket-activation adoption (exactly one passed fd, addressed to this pid, inherited listener) | `crates/nimbus-bin/src/start/boot.rs` |
| `concepts/architecture/node-lifecycle.md` | `SystemdDbusClient` trait (capabilities/start/stop/inspect); fail-closed unavailable default; required-capability refusal | `crates/nimbus-node/src/systemd_transient.rs` |
| `concepts/architecture/node-lifecycle.md` | `ZbusSystemdClient`: org.freedesktop.systemd1 proxies, construction-time capability probe, degraded-capability reporting, system vs session bus | `crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs` |
| `concepts/architecture/node-lifecycle.md` | Transient unit naming `nimbus-<component>.service`; property allowlist; ExecStart composed only from validated absolute path; non-allowlisted properties rejected | `crates/nimbus-node/src/systemd_transient.rs`, `crates/nimbus-node/src/host_lifecycle.rs` |
| `concepts/architecture/node-lifecycle.md` | Journal selectors (systemd unit field + workload-id field); cgroup path under system slice | `crates/nimbus-node/src/systemd_transient.rs` |
| `concepts/architecture/node-lifecycle.md` | Signal ordering (Subscribe → JobRemoved stream → StartTransientUnit/StopUnit → match by job path); done/skipped = success, unknown results = failure; 30s default timeout | `crates/nimbus-node/src/systemd_transient/zbus_client/signals.rs`, `crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs` |
| `concepts/architecture/node-lifecycle.md` | State mapping (activating→submitted, active+running→running, active+other→ready, inactive→stopped, failed→failed); missing unit reported as stopped | `crates/nimbus-node/src/systemd_transient.rs`, `crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs` |
| `concepts/architecture/node-lifecycle.md` | Reconciler desired-state derivation (active→running, deleting→stopped); inspect-first outcomes; status-evidence writer seam | `crates/nimbus-node/src/reconciler.rs` |
| `concepts/architecture/node-lifecycle.md` | Reconciler/systemd backend have no production caller; admission/binding types are wired into the server | `crates/nimbus-node/src/reconciler.rs`, `crates/nimbus-system/src/tests.rs`, `crates/nimbus-server/src/local_enforcement.rs`, `crates/nimbus-services/src/manager/launch.rs`, `crates/nimbus-bridge/src/lib.rs` |
| `concepts/architecture/node-lifecycle.md` | `DirectProcessBackend` in-memory test implementation | `crates/nimbus-node/src/direct_process.rs` |
| `concepts/architecture/cli-codegen.md` | clap command tree (start/dev/deploy/codegen/init/token/auth/ui/machine/node/compose/policy/encryption/packages + hidden sandbox-supervisor) | `crates/nimbus-bin/src/main.rs` |
| `concepts/architecture/cli-codegen.md` | Boot sequence; registry wiring gated on `.nimbus/convex/functions.json` / `.nimbus/firebase/artifact.json`; generation-zero start without app dir; two-stage bind gate; license resolution order | `crates/nimbus-bin/src/start/boot.rs` |
| `concepts/architecture/cli-codegen.md` | flag > env > file precedence via fallback chain; `NIMBUS_CONFIG` JSON persistence-only file with unknown-field rejection; `./data` default; control dir defaults to data dir; provider set; cross-provider override rejection | `crates/nimbus-bin/src/start/config.rs` |
| `concepts/architecture/cli-codegen.md` | dev = in-process start on 3210 raced with watch loop; `.nimbus/dev` state; sqlite; `demo` auto tenant; local-development isolation; per-run random deploy token | `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/dev/plan.rs` |
| `concepts/architecture/cli-codegen.md` | 500ms poll / 300ms debounce; mtime+length fingerprints; skip dirs; codegen-then-deploy-to-self via HTTP POST with per-run token | `crates/nimbus-bin/src/dev/watch.rs` |
| `concepts/architecture/cli-codegen.md` | App-dir ancestor walk (nimbus/, convex/, generated functions, firebase.json) bounded at `.git`; existence check covers worktree gitfiles | `crates/nimbus-bin/src/dev/plan.rs`, `crates/nimbus-bin/src/path_boundary.rs` |
| `concepts/architecture/cli-codegen.md` | Embedded codegen runner: tooling closure into `<app>/.nimbus/tmp`; bootstrap module; Node-22 tooling limits; run-to-completion; snapshot cache; host bridge rejects host calls; external-Node auto-route for Cloud Functions; `NIMBUS_CODEGEN_RUNNER=external-node` | `crates/nimbus-bin/src/codegen.rs` |
| `concepts/architecture/cli-codegen.md` | Node major floor 22 (older rejected, newer warned) | `crates/nimbus-bin/src/node.rs` |
| `concepts/architecture/cli-codegen.md` | Codegen implemented in JS (`packages/codegen`, esbuild + TypeScript deps, bundle outputs) | `packages/codegen/package.json` |
| `concepts/architecture/cli-codegen.md` | Asset catalog; per-file SHA-256 manifest; digest re-verification at materialization; templates compiled in with project-name substitution | `crates/nimbus-assets/src/lib.rs`, `crates/nimbus-assets/src/js_packages.rs`, `crates/nimbus-assets/src/templates.rs` |
| `concepts/architecture/cli-codegen.md` | Provisioning into `.nimbus/packages/<name>/`; `.version` stamp = manifest digest, written last; idempotent short-circuit; re-provision on binary upgrade; implicit on init/dev/codegen/deploy; `file:./.nimbus/packages/...` specifiers | `crates/nimbus-bin/src/provision.rs`, `crates/nimbus-bin/src/init.rs` |
| `concepts/architecture/sdk-packages.md` | `crates/nimbus` is the Rust embedder facade re-exporting core/engine/runtime surface | `crates/nimbus/src/lib.rs` |
| `concepts/architecture/sdk-packages.md` | Seven workspace packages, all `"private": true` | `package.json`, `packages/nimbus/package.json`, `packages/convex/package.json`, `packages/codegen/package.json`, `packages/nimbus-ui/package.json`, `packages/firebase/package.json`, `packages/mongodb/package.json`, `packages/dynamodb/package.json` |
| `concepts/architecture/sdk-packages.md` | Six SDK entry points; ESM-only; optional react peers; `Nimbus` root resources client; REST transport | `packages/nimbus/package.json`, `packages/nimbus/src/index.ts`, `packages/nimbus/src/transports/rest.ts` |
| `concepts/architecture/sdk-packages.md` | Convex wrapper re-exports/aliases canonical SDK (server/react/browser); values implemented locally; `convex` CLI shim delegates codegen | `packages/convex/src/server.ts`, `packages/convex/src/react.ts`, `packages/convex/src/browser.ts`, `packages/convex/src/values.ts`, `packages/convex/src/cli.mjs`, `packages/convex/package.json` |
| `concepts/architecture/sdk-packages.md` | Codegen emits `_generated/{api.ts,server.ts,scheduled_functions.ts,dataModel.d.ts}` + `.nimbus/convex/{functions.json,schema.json,http_routes.json,auth.config.json,bundle.mjs,bundle.sha256}` | `packages/codegen/src/main.mjs`, `packages/codegen/src/emit/generated_files.mjs` |
| `concepts/architecture/sdk-packages.md` | Bundle SHA-256 enforced at load and per invocation | `crates/nimbus-convex/src/registry/loading.rs`, `crates/nimbus-runtime/src/runtime/driver/invocation.rs` |
| `concepts/architecture/sdk-packages.md` | UI embedded via rust-embed of `packages/nimbus-ui/dist/`; build script asserts built UI; served at `/ui` with SPA fallback | `crates/nimbus-assets/src/ui.rs`, `crates/nimbus-assets/build.rs`, `crates/nimbus-server/Cargo.toml`, `crates/nimbus-server/src/http/ui.rs`, `crates/nimbus-server/src/router.rs` |
| `concepts/architecture/sdk-packages.md` | Adapter helpers: firebase app+firestore over Connect-Web; `mongoUri`; `clientConfig`/`endpoint` | `packages/firebase/src/index.ts`, `packages/firebase/src/app.ts`, `packages/firebase/package.json`, `packages/mongodb/src/index.ts`, `packages/dynamodb/src/index.ts` |
| `concepts/architecture/sdk-packages.md` | Distribution: staged checksummed payload → rust-embed → `.nimbus/packages/` with `.version` stamp, closure resolution, `file:` specifiers, drift-forced reinstall, `nimbus packages verify` | `scripts/stage-embedded-packages.mjs`, `crates/nimbus-assets/src/js_packages.rs`, `crates/nimbus-bin/src/provision.rs`, `crates/nimbus-assets/embedded/templates/convex/package.json.tmpl` |
| `concepts/architecture/sdk-packages.md` | Codegen tooling embedded but not provisioned (temp-dir materialized) | `crates/nimbus-assets/src/js_packages.rs`, `scripts/stage-embedded-packages.mjs` |
| `concepts/architecture/observability.md` | `/health` public, unauthenticated `{"ok":true}` | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/http/metadata.rs` |
| `concepts/architecture/observability.md` | Five `/debug/*` routes on the local-admin router; runtime metrics always-200 stable shape; operator-class tenant context | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/http/metadata.rs`, `crates/nimbus-server/src/http/mod.rs` |
| `concepts/architecture/observability.md` | Middleware chain: route-family classify → origin allowlist → credential extraction → fail-closed gate, all audited | `crates/nimbus-server/src/local_server/middleware.rs`, `crates/nimbus-operator/src/policy.rs` |
| `concepts/architecture/observability.md` | Tenant snapshot groups (admission/journal/subscription delivery/read surface/snapshot manager/query planning/libsql freshness) | `crates/nimbus-engine/src/tenant.rs`, `crates/nimbus-engine/src/tenant/mutation/stats.rs`, `crates/nimbus-engine/src/tenant/subscription_delivery/stats.rs`, `crates/nimbus-engine/src/tenant/materialized_reads/stats.rs`, `crates/nimbus-engine/src/tenant/query_planning.rs`, `crates/nimbus-storage/src/libsql/freshness.rs` |
| `concepts/architecture/observability.md` | Consistency verifier fingerprints authoritative/shadow/replica + bootstrap cut, structured mismatches | `crates/nimbus-engine/src/verification.rs` |
| `concepts/architecture/observability.md` | `tracing` fmt to stdout; `RUST_LOG` parsed as `Targets` directives (env-filter feature not enabled anywhere) | `crates/nimbus-bin/src/main.rs`, `Cargo.toml` |
| `concepts/architecture/observability.md` | Latency WARN budgets, segment names and ms values, drop-safe timers | `crates/nimbus-server/src/latency.rs`, `crates/nimbus-engine/src/engine/latency.rs` |
| `concepts/architecture/observability.md` | Audit log: JSONL record fields, 0600 perms, `logs/access.jsonl` under state dir, gate-level emission, cross-adapter tenant attribution | `crates/nimbus-operator/src/audit.rs`, `crates/nimbus-operator/src/paths.rs`, `crates/nimbus-server/src/local_server/middleware.rs`, `crates/nimbus-server/src/state.rs` |
| `concepts/architecture/observability.md` | No Prometheus `/metrics` route; no OTel exporter (opentelemetry only transitive via forked runtime's `deno_telemetry`) | `crates/nimbus-server/src/router.rs`, `Cargo.lock` |
