# RRC3 Application and Adapter Smoke

Status: `RRC3_APP_ADAPTER_SMOKE_PROVISIONAL_PASS`

Date: 2026-08-27

## Candidate under test

The final macOS browser and protocol replay used the provisional integrated
optimized binary at
`/tmp/nimbus-ws-test.0rXOFY/worktree/target/release/nimbus`:

```text
nimbus 0.1.45
sha256 dfdefaa409661baccd0f98ae824e97de5d34a2df10a6389b1abd9f78e17a9ec3
```

This binary includes the local Deno WebSocket-egress work through path
dependencies and the RRC3 repairs. It is valid defect-discovery evidence, but
it is not the exact release candidate. RRC3 therefore remains blocked after a
provisional pass until RRC1 supplies a reachable immutable Deno reference and
the clean candidate repeats these lanes.

## Nine-case application lane

The repository-owned lane passed all nine selected cases in 15.489 seconds
with Node 24.19.0. Every expected anchor and every cleanup check passed. The
source manifest matched before and after the run.

Evidence:
`rrc3-examples-results/nimbus-examples-verify.cokvom-dc4a6651a033/report.json`

| Case | Direct surfaces | Result |
|---|---|---|
| `nimbus/tasks` | Native HTTP and WebSocket push | All five task anchors passed. |
| `nimbus/agent-chat` | Convex HTTP, scheduler, WebSocket push | All four agent-chat anchors passed. |
| `nimbus/agent-worker` | Convex HTTP, scheduler, WebSocket push | Both worker anchors passed. |
| `convex/tasks` | Convex HTTP, stdio run, reactive WebSocket | All five task anchors passed. |
| `convex/runtimes` | V8 and Node runtime execution | All four runtime anchors passed. |
| `firebase/tasks` | Firestore REST and Listen WebSocket | All five task anchors passed. |
| `mongodb/tasks` | MongoDB wire protocol | All five task anchors passed. |
| `dynamodb/tasks` | DynamoDB wire protocol | All five task anchors passed. |
| `cloud-functions/tasks` | Cloud Functions HTTP and trigger execution | All expected anchors passed. |

The run used an earlier provisional binary hash recorded in its report. The
later optimized replay added only the verified RRC3 repairs described below.

## Direct protocol clients

| Surface | Client and evidence | Result |
|---|---|---|
| Native API and JavaScript SDK | `@nimbus/nimbus`, authenticated HTTP, and `nimbus.v2` WebSocket | CRUD, pagination, schema, update, delete, and push passed. |
| Convex | Nimbus's `convex` compatibility package and React client | Query, mutation, V8, Node, and reactive update passed. |
| Firebase / Firestore | Stock Firebase-shaped browser client and emulator token | CRUD and `onSnapshot` passed in loopback local-development mode. The same token failed closed in production mode. |
| MongoDB | Official `mongodb` 6.21.0 driver | The complete task flow passed over the wire listener. |
| DynamoDB | AWS SDK `@aws-sdk/client-dynamodb` 3.1119.0 | The complete task flow passed over the wire listener. |
| Cloud Functions | `firebase-functions` 7.3.2-shaped application | HTTP and trigger task anchors passed. |
| S3 | AWS SDK `@aws-sdk/client-s3` 3.1119.0 | Put, head, full and range get, conditional 412, multipart, list, wrong-secret 403, and delete passed. |
| RESP KV | `redis-rs` harness against pinned Valkey `c9e800...` | RESP2 1/1 and RESP3 1/1 passed. |
| Cloudflare KV | Direct authenticated REST contract against default SQLite | Auth rejection, metadata, pagination, delete, and same-root restart durability passed. |

The S3 client source and lock are retained under `rrc3-s3-client/`. The
Cloudflare replay script is `rrc3-cloudflare-kv-smoke.sh`.

## Browser Playwright evidence

The final no-override replay used Node 22 for `nimbus dev` authoring and loaded
all three optimized browser builds from Nimbus's same-origin `/examples/`
route:

```text
http://127.0.0.1:18480/examples/nimbus/tasks/dist/
http://127.0.0.1:18480/examples/convex/tasks/dist/
http://127.0.0.1:18480/examples/firebase/tasks/dist/
```

Each page reached `Live on demo` without a `?server=` override and reported
zero console errors and zero warnings. The complete earlier lifecycle replay
for each page created a task, observed the pushed list update, toggled the
task, deleted it, and returned to the empty state. The native page used a
single-use local UI launch ticket and the operator session cookie. The Convex
page used anonymous local-development tenancy and made no local-admin request.
The Firebase page used the loopback-only emulator verifier.

The production-isolation Firebase replay returned `no application auth
providers are configured for the active deployment`. This is the required
fail-closed result, not a defect.

## Fail-before ledger and repairs

| ID | Severity | Fail-before evidence | Terminal verdict |
|---|---|---|---|
| RRC3-001 | P1 | Cloudflare KV was mounted by default, but its first PUT on the default SQLite provider returned HTTP 500 because `TenantKvStore` existed only for redb. | Fixed in `4de2b28bb` and exhaustive follow-up `b63addb52`. SQLite now owns atomic KV operations, TTL, batches, pagination, restart durability, and rollback tests. |
| RRC3-002 | P2 | An exact final Cloudflare KV list page returned `list_complete:false` because the adapter exposed the storage cursor without a one-item lookahead. | Fixed in `6ca9321ee`. REST and Worker host calls share validated lookahead pagination; exact final pages and `limit=0` have regressions. |
| RRC3-003 | P1 | The CORS layer allowed loopback origins on any port and configured exact origins, but the earlier origin gate rejected both before CORS could answer. All Vite browser apps failed. | Fixed in `5caf5c2cf`. Both gates now share one exact policy; configured and loopback preflights return 200, unconfigured origins return 403, and the operator UI remains same-origin. |
| RRC3-004 | P2 | Browser builds emitted root-relative assets, two apps hard-coded port 8080 while `nimbus dev` defaults to 3210, and the Convex page made an unnecessary local-admin tenant request. Missing favicons also produced console noise. | Fixed in `ef589a825`. Builds use relative assets, pages default to their current origin, Convex relies on the dev-provisioned tenant, and all three pages carry an inline favicon and corrected run instructions. |

No accepted RRC3 defect remains open.

## Focused verification

The following checks passed:

```text
cargo test -p nimbus-operator validate_origin -- --nocapture
  2 passed
cargo test -p nimbus-server --lib --no-default-features \
  cors_preflight_enforces_the_complete_browser_origin_policy -- --nocapture
  1 passed; 701 filtered
cargo clippy -p nimbus-operator --all-targets -- -D warnings
cargo clippy -p nimbus-server --lib --no-default-features -- -D warnings
cargo fmt --all --check
npm run build -w nimbus-tasks -w convex-tasks -w firebase-tasks
npm run typecheck -w nimbus-tasks -w convex-tasks -w firebase-tasks
```

The optimized binary rebuild completed successfully in 22 minutes 54 seconds.
The direct CORS replay returned 200 with the exact allow-origin header for a
loopback development port and a configured HTTPS origin. It returned 403 for
an unconfigured HTTPS origin.

npm 10 reports `GHSA-3wf4-68gx-mph8` against the private Nimbus compatibility
package named `firebase` because its Nimbus version is below upstream Firebase
10.9.0. This is a package-name false positive: the package does not contain
the affected upstream implementation, `fixAvailable` is false, and npm 11
reports zero vulnerabilities for the same lock.

## Terminal RRC3 verdict

The provisional integrated candidate passes every RRC3 application, adapter,
protocol, and browser behavior on macOS. Exact-candidate replay remains
blocked only by the RRC1 Deno reference. No RRC3 product defect remains open.
