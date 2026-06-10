# Source map

This page maps user-facing behavior claims in the published docs
(`docs/{get-started,developers,operators,concepts,reference}/`) to the files
that implement them. The docs are descriptive; treat these files as the
source-backed check before changing any behavior claim. Every page with a
load-bearing claim gets a row; `scripts/check-docs.sh` verifies the listed
sources exist.

| Doc page | Claim / surface | Source |
| --- | --- | --- |
| `get-started/quickstart.md` | `nimbus init convex` scaffold contents | `crates/nimbus-bin/src/init.rs` |
| `get-started/quickstart.md` | `nimbus dev` watch / codegen / `demo` tenant / port 3210 | `crates/nimbus-bin/src/dev.rs` |
| `get-started/quickstart.md` | Node 22 external authoring baseline | `crates/nimbus-bin/src/node.rs` (`REQUIRED_NODE_MAJOR_VERSION`) |
| `get-started/self-host.md` | `nimbus start` flags, localhost default, SQLite default | `crates/nimbus-bin/src/start/mod.rs`, `crates/nimbus-bin/src/start/config.rs` |
| `get-started/self-host.md` | `/api/tenants`, `/documents`, `/query` endpoints | `crates/nimbus-server/src/router.rs` |
| `get-started/from-convex.md` | Convex function model + `convex/` layout compatibility | `packages/nimbus/src/server.ts`, `packages/convex/src/`, `crates/nimbus-convex/` |
| `index.mdx` (landing) | Speaks Convex / Firestore / Cloud Functions / MongoDB / DynamoDB | `crates/nimbus-convex/`, `crates/nimbus-firebase/`, `crates/nimbus-cloud-functions/`, `crates/nimbus-mongodb/`, `crates/nimbus-dynamodb/` |
| `index.mdx` (landing) | Single-binary storage/compute/networking/realtime/scheduling | `crates/nimbus-bin/src/main.rs`, `crates/nimbus-engine/`, `crates/nimbus-storage/`, `crates/nimbus-server/` |
