# NFRC8 Realistic SDK Canaries

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC8 implementation wave, including
the vendored official Node corpora, generated Node evidence, Convex app
canaries, and proof artifacts. The NFRC8-specific changes add realistic SDK
canaries, register them as public package claims, tighten the broad canary
runner so it reports every package failure in a lane, and keep the permission
profile least-authority while allowing the read-only system fact needed by a
real SDK import path.

## Files Changed

- SDK canary app:
  `tests/runtime/node/sdk-canaries/`
- Runtime canary harness:
  `crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs`,
  `crates/nimbus-runtime/src/runtime/tests/basic_invocation/package_resolution.rs`
- Permission profile and tests:
  `crates/nimbus-runtime/src/limits/grants.rs`,
  `crates/nimbus-runtime/src/limits/tests.rs`
- Canary registry and dashboard gates:
  `tests/runtime/node/canary-registry.json`,
  `scripts/runtime/node/dashboard.py`,
  `scripts/verify-node-lts-canaries-and-oracles.sh`
- Generated evidence and public docs:
  `docs/architecture/runtime/node-compat-evidence/latest/`,
  `docs/runtimes/nodejs/evidence/`,
  `docs/architecture/runtime/node-compat-surface-matrix.md`
- Control plane:
  `docs/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC8 followed the required wide-then-focused loop:

1. Add all selected SDKs to one broad `Application` canary batch before trying
   to tune individual packages.
2. Run the broad batch to capture the complete package failure inventory.
3. Change the batch harness to collect every package failure instead of
   aborting on the first failed SDK.
4. Fix or classify each concrete failure with focused Node22, Node24, and
   Node26 SDK-batch tests.
5. Rerun the full `Application` preset and publish dashboard/docs from that
   final broad report.

This keeps the plan from spending loops on tiny green examples while missing
the larger compatibility shape.

## SDK Corpus

The canary app uses exact pinned package versions:

| Package | Version | Workflow covered |
| --- | --- | --- |
| `openai` | `6.39.1` | client construction and chat-completions request against a local mock |
| `@anthropic-ai/sdk` | `0.100.0` | messages request against a local mock |
| `ai` | `6.0.192` | stream text helpers with a deterministic local model |
| `stripe` | `22.2.0` | customers API request signing and response parsing |
| `resend` | `6.12.4` | email send API request and response parsing |
| `@aws-sdk/client-s3` | `3.1056.0` | S3 command serialization through Smithy Node HTTP handler |
| `@slack/web-api` | `7.16.0` | Web API request construction, user-agent path, and response parsing |
| `octokit` | `5.0.5` | REST request path and auth header behavior |
| `jose` | `6.2.3` | JWT signing and verification |
| `zod` | `4.4.3` | schema parse and failure shape |
| `uuid` | `14.0.0` | deterministic v5 UUID generation |
| `nanoid` | `5.1.11` | custom alphabet deterministic ID generation |
| `@upstash/redis` | `1.38.0` | HTTP Redis `set/get/pipeline` behavior against a local mock |

Each package has lane-local canary entries for Node22 and Node24. Node26 is
recorded as separate Current/non-LTS evidence and is not part of the public
supported-LTS claim set.

## Wide Feedback And Focused Fixes

Initial broad run:

```bash
make node-compat-canaries PRESET=application
```

The first SDK batch aborted on the AWS canary because the harness returned the
first runtime error immediately. That was insufficient feedback for this plan,
so the harness now returns `Result<Value, String>` for each SDK and reports all
failures in one lane.

The next broad run produced the same five package failures on Node22, Node24,
and Node26:

| Package | Failure class | Resolution |
| --- | --- | --- |
| `@anthropic-ai/sdk` | Mock base URL included `/v1` twice, producing `/v1/v1/messages`. | Use the SDK's base URL as the service root and let the client append the API path. |
| `@aws-sdk/client-s3` | Default shared-config discovery tried to read `os.homedir()`. | Keep `homedir` denied and configure the canary with explicit endpoint, credentials, retry mode, user-agent provider, S3 flags, checksum mode, and `NodeHttpHandler`. |
| `@slack/web-api` | SDK imports `os.release()` for user-agent construction. | Grant read-only `osRelease` in Application/production Node profiles and add tests that `homedir` stays denied. |
| `uuid` | Canary expected the wrong deterministic v5 value. | Update the expected value to `bc0a1831-8c89-5ac7-b2cb-a52eb2bf8222`. |
| `@upstash/redis` | Mock response shape did not match pipeline request/response behavior. | Accept pipeline array bodies and return JSON `{ result }` entries with `responseEncoding: "json"`. |

Focused verification:

```bash
cargo test -p nimbus-runtime application_node22_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime application_node24_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime application_node26_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored
```

All three lane-local SDK batches passed after the focused fixes.

Final broad application batch:

```bash
make node-compat-canaries PRESET=application
```

Result: `58` canary checks passed, `0` failed.

| Lane | Role | Canary checks | Passed | Failed |
| --- | --- | ---: | ---: | ---: |
| Node20 | legacy | 2 | 2 | 0 |
| Node22 | supported | 21 | 21 | 0 |
| Node24 | default | 21 | 21 | 0 |
| Node26 | current | 14 | 14 | 0 |

## Evidence Refresh

The final report was published after the broad rerun:

- 26 package/framework canary claims.
- 68 package/framework canary checks.
- 2 canary artifact bundles.
- 2 version-matched Node22/Node24 oracle reports.
- 0 required canary gaps.

The public dashboard reports every SDK claim as `Passed` for Node22 and Node24,
with Node26 Current/non-LTS results recorded separately where applicable.

## Verification

- `make node-compat-canaries PRESET=application`: pass, 58 canary checks
  passed and 0 failed.
- `cargo test -p nimbus-runtime application_node22_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored`:
  pass, 1 test.
- `cargo test -p nimbus-runtime application_node24_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored`:
  pass, 1 test.
- `cargo test -p nimbus-runtime application_node26_sdk_package_canary_batch -- --nocapture --test-threads=1 --ignored`:
  pass, 1 test.
- `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and
  0 failures.
- `bash scripts/runtime/node/validate-claims.sh`: pass, 26 active claim
  mappings against 26 registered canaries.
- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 4
  vendored corpora and 2 supported LTS lanes with zero unclassified published
  results.
- `make node-compat-publish-docs CHECK=1`: pass, generated Node.js runtime
  evidence docs are current.
- `cargo test -p nimbus-runtime application_preset_supports_node_lts_targets -- --nocapture`:
  pass, 1 test.
- `cargo test -p nimbus-runtime node_permission_profiles_are_separated_by_deployment_intent -- --nocapture`:
  pass, 1 test.
- `cargo fmt --all --check`: pass.
- `git diff --check`: pass.
- `npm run docs:validate-refs:strict`: pass, 227 working-tree Markdown files.

## Decisions

- Keep SDK canaries in the broad `Application` preset rather than creating a
  separate green-only lane. This forces future application compatibility runs
  to exercise package behavior developers actually use.
- Keep public canary claims scoped exactly to Node22 and Node24. Node26 canary
  results are useful Current-line evidence, but they do not promote Node26 to
  supported LTS.
- Allow `osRelease` because it is a read-only system fact used by real package
  user-agent code and is comparable to the already allowed limited system
  metadata. Keep `homedir` denied because it exposes user-specific host paths
  and can trigger ambient shared-config discovery.
- Configure AWS SDK explicitly instead of granting broad home-directory access
  or generic shared-config filesystem authority.
- Use local deterministic mock services for SaaS/HTTP clients so canaries test
  request construction, auth headers, serialization, response parsing, and
  runtime imports without requiring live third-party credentials.

## Remaining Risks

- NFRC9 still owns host-heavy negative canaries for child process, workers,
  native addons, inspector, REPL, `node --test`, persistent filesystem
  assumptions, raw server listen behavior, Prisma engine routing, sharp, and
  esbuild.
- NFRC10 still owns public Deno-style package/API reference pages generated
  from this evidence.
- NFRC11 and NFRC12 still own release-train automation and CI/nightly gates so
  new Node releases and SDK drift keep producing broad feedback automatically.
