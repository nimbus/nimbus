# RRC3 Application and Adapter Smoke

Status: `RRC3_APP_ADAPTER_SMOKE_PASS`

Date: 2026-09-06

## Candidate under test

The terminal application replay used the committed debug binary at
`target/debug/nimbus`:

```text
nimbus 0.1.46
sha256 f176c17add56648db5ed644808b5913250133b8cb2760843f58241faf6b6b075
```

Its source is Nimbus `7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df` from immutable tag
`v2.9.6-nimbus.5`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

## Nine-case application lane

The repository-owned lane passed all nine selected cases in 15.264 seconds
with Node 24.20.0 and five workers. Every expected anchor and every cleanup
check passed. The
source manifest matched before and after the run.

Evidence:
`target/examples-verify-results/nimbus-examples-verify.vmjwjz-221eac9a80c1/report.json`

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

The report records the exact binary hash and the terminal pass for each case.

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

The proof retains the S3 client source and lock under `rrc3-s3-client/`. The
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

Each page reached `Live on demo` without a `?server=` override. Each page
reported zero console errors and zero warnings. The complete earlier lifecycle
replay created a task, observed the pushed list update, toggled the task,
deleted it, and returned to the empty state. The native page used a
single-use local UI launch ticket and the operator session cookie. The Convex
page used anonymous local-development tenancy and made no local-admin request.
The Firebase page used the loopback-only emulator verifier.

The production-isolation Firebase replay returned the expected missing-provider
error. Nimbus requires this fail-closed result. It is not a defect.

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
package named `firebase`. Its Nimbus version is below upstream Firebase 10.9.0.
This is a package-name false positive. The package does not contain the affected
upstream implementation, and `fixAvailable` is false. npm 11 reports zero
vulnerabilities for the same lock.

## Node Compatibility Evidence

The Node Compatibility Evidence job passed in exact release-graph run
`34025642620`. That job verifies the release train, current fixture corpora, and
upstream fixture identity. It also verifies watchpoints, supported application
and tooling canaries, and Node 20, 22, 24, and 26 oracle samples. The job rebuilt
the published status evidence.

The six broad full-corpus jobs remain red on diagnostic, non-isolate, optional,
and Current-line fixtures outside the advertised V8-isolate-required support
denominator. This is pre-existing visible compatibility debt, not an uplift
regression. Comparison with `main` run `33962214430` found no new failed or
timed-out test name in any partition. The release graph ran 531 tests: 234
passed, 260 failed, and 37 timed out. The baseline ran 529 tests: 221 passed,
271 failed, and 37 timed out. The uplift therefore adds 13 passes while adding
two tests and removing 11 failures.

Final Nimbus commit `7d0ca18a7` changes only server router preparation. It does
not change the runtime or Node compatibility graph.

## Terminal RRC3 verdict

The exact candidate passes every RRC3 application and adapter behavior on
macOS. The direct Cloudflare KV and official S3 client replays pass against the
same release graph. Exact-head KV run `34025639058` passes RESP2 and RESP3.
The supported Node compatibility evidence job passes with no broad-corpus
regression.
The earlier complete browser lifecycle proof remains valid because the final
candidate changes only server startup stack ownership after that proof. The
exact embedded UI and desktop UI walks also pass. No RRC3 product defect
remains open.

Candidate binding: Nimbus
`7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.
