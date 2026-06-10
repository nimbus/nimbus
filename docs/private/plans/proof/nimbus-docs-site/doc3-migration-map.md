# DOC3 migration map

File-by-file disposition for the public-candidate corpus, executed by
DOC4..DOC7. Everything listed as *staged* currently lives under
`docs/private/staging/` and must be **rewritten into single-Diátaxis-mode
public pages** (never moved verbatim) or retired. Sources: the 2026-06-10
Diátaxis audit + the documentable-surface inventory.

Legend: **T**utorial · **H**ow-to · **R**eference · **E**xplanation.

## Already absorbed in DOC3

| Old path | Disposition |
| --- | --- |
| `docs/getting-started.md` | Absorbed into `get-started/{index,quickstart,self-host}.md`; staged copy retained for reference |
| Convex quickstart duplication (README × getting-started × convex README) | Single home: `get-started/quickstart.md`. README copy shrinks at DOC11; convex README copy dissolves at DOC4 |

## DOC4 — Developers (from `staging/adapters/`, `staging/runtimes/`, `staging/examples/`)

**Status: done (2026-06-10).** Every row below is published: 35 rewritten
pages across `docs/developers/` + `docs/reference/` +
`docs/concepts/nodejs-runtime.md`, plus authored gaps `first-app.md`,
`auth.md`, `convex/migrate.md`, `dynamodb/index.md`. All claims
source-verified; `docs/source-map.md` carries the audit rows. Deviations
from the table: the Convex front door landed as
`developers/convex/{index,migrate}.md` (no separate `tutorial/how-to`
split — the platform-level tutorial is `developers/first-app.md`), and the
MongoDB tutorial folded into `developers/mongodb/index.md` +
`examples.md`. DynamoDB enterprise-readiness landed at
`reference/dynamodb/readiness.md`. Staged sources retained under
`staging/` until the DOC13 retirement sweep (runtime evidence pages are
script-regenerated; retiring them is a pipeline decision). **Product gap
surfaced:** Firestore routes are config-gated with no CLI wiring — stock
`nimbus start` cannot enable them; flagged to the developer at DOC4
closeout.

| Staged file | Action | Target (public) |
| --- | --- | --- |
| `adapters/convex/README.md` | split T/H (R → reference) | `developers/convex/{tutorial,how-to}.md` + `reference/convex/project-layout.md` |
| `adapters/convex/compatibility.md` | refactor R (strip codegen-security internals) | `reference/convex/compatibility.md` |
| `adapters/convex/ai-guidelines.md` | keep as usage-rules R (also stays agent-readable) | `reference/convex/usage-rules.md` |
| `adapters/firebase/README.md` | split T/R | `developers/firebase/` + `reference/firebase/` |
| `adapters/firebase/{compatibility,websocket-listen}.md` | keep R | `reference/firebase/` |
| `adapters/firebase/migration.md` | keep H | `developers/firebase/migrate.md` |
| `adapters/firebase/auth-contract.md` | refactor R (trim to public surface) | `reference/firebase/auth.md` |
| `adapters/firebase/upstream-test-catalog.md` | private (test inventory) | stays under `private/` |
| `adapters/cloud-functions/README.md` | split T/E (strip plan links) | `developers/cloud-functions/` |
| `adapters/cloud-functions/{compatibility,migration}.md` | keep R / H | `reference/cloud-functions/compatibility.md`, `developers/cloud-functions/migrate.md` |
| `adapters/cloud-functions/*-contract.md` (4 ADRs) | private | stays under `private/` |
| `adapters/mongodb/README.md` | split T/H/R | `developers/mongodb/` + `reference/mongodb/` |
| `adapters/mongodb/{drivers,operations,tenant-isolation}.md` | keep R | `reference/mongodb/` |
| `adapters/mongodb/examples.md` | keep H | `developers/mongodb/examples.md` |
| `adapters/dynamodb/{feature-coverage,divergences,sdk-compatibility}.md` | keep R (strip test-lane columns) | `reference/dynamodb/` |
| `adapters/dynamodb/compatibility-suites.md` | private | stays under `private/` |
| `adapters/dynamodb/enterprise-readiness.md` | refactor → operator R | `operators/` or `reference/dynamodb/readiness.md` |
| **GAP: DynamoDB front door** | author new T/H | `developers/dynamodb/index.md` |
| **GAP: Convex→Nimbus migration** | author new H | `developers/convex/migrate.md` |
| **GAP: first-app tutorial / MongoDB tutorial** | author new T | `developers/{first-app,mongodb/tutorial}.md` |
| **GAP: Auth / IdP setup** | author new H | `developers/auth.md` |
| `adapters/native/README.md` | split T/R | `developers/native/` + reference |
| `adapters/native/{http-api,websocket-protocol,errors}.md` | keep R | `reference/native/` |
| `runtimes/**` (already Diátaxis-shaped; the template) | move + re-home by mode | `developers/runtimes/nodejs/` (H), `concepts/nodejs-runtime.md` (E from fundamentals), `reference/runtimes/` (R incl. generated evidence); `evidence/refreshing.md` stays private |
| `examples/nimbus-sdk-resource-model.md` | keep H | `developers/sdk/resource-model.md` |

## DOC5 — Operators (from `staging/operating/`, `staging/tenant-isolation.md`)

| Staged file | Action | Target |
| --- | --- | --- |
| `operating/cli.md` | keep R | `reference/cli.md` (DOC6 owns) |
| `operating/storage-backends.md` | split H (R flags → reference) | `operators/storage-backends.md` + `reference/configuration.md` |
| `operating/encryption.md` | keep H (flags → reference) | `operators/encryption.md` |
| `operating/tenant-isolation.md` | refactor H (strip `make verify-*` / plan links) | `operators/tenant-isolation.md` |
| `operating/{container-image,node-lifecycle,updates,desktop-install}.md` | keep H | `operators/` |
| `operating/deploy-admin-api.md` | keep R | `reference/deploy-admin-api.md` |
| `operating/latency-budgets.md` | keep R (thin; merge if natural) | `reference/latency-budgets.md` |
| `operating/{ci-caching,ci-modernization,ci-pr-wall,local-dev,deno-fork-workflow,fork-health,node-dbus-binding,multi-backend-adapter-hardening}.md` | private (contributor/CI contracts) | stay under `private/` (move out of staging) |
| `tenant-isolation.md` (root) | split: public E + private evidence | `concepts/tenant-isolation.md` + private remainder |
| **GAPs: production deploy tutorial, backup/restore, observability H, hardening checklist, troubleshooting** | author new | `operators/` |

## DOC6 — Concepts + Reference (from root pages + `staging/architecture/` curation)

| Source | Action | Target |
| --- | --- | --- |
| `ARCHITECTURE.md` (root, 1497 lines, drift) | distill E overview (full rewrite happens DOC7) | `concepts/how-nimbus-works.md` |
| `staging/architecture/runtime/permission-model.md` | refactor E | `concepts/runtime-permissions.md` |
| `staging/architecture/sandbox/service-sandbox-session-model.md` | refactor E | `concepts/resource-model.md` |
| `staging/architecture/horizontal-scaling.md` | extract public E | `concepts/scaling.md` |
| `staging/current-capabilities.md` | keep R | `reference/current-capabilities.md` |
| README "Node compatibility contract" (4 copies) | single home R | `reference/runtimes/node-compat.md` |
| CLI / config / SDK / native API / deploy API | author R from source | `reference/` (per the plan's DOC6 spec) |

## DOC7 — Architecture (rewrite, not move)

Twelve manifest pages under `concepts/architecture/` (slugs fixed by
`scripts/verify-nimbus-docs-site.sh` condition 17): server-transport,
adapters, engine-mutation-path, runtime-isolates, storage, sandbox-machines,
auth-trust, tenancy, node-lifecycle, cli-codegen, sdk-packages,
observability. The `staging/architecture/**` deep-dives are raw material
only; generated evidence (`node-lts-compat/**`), krun validation logs, fork
ledgers, proof harnesses, and "for contributors" seams stay private. After
DOC7, `staging/architecture/` should be empty or retired into `private/`.

## Landing follow-up (DOC4)

**Status: done (2026-06-10), Firebase tab deliberately omitted.** The
landing `<Tabs>` demo now ships TypeScript, MongoDB, DynamoDB, and curl
tabs — the MongoDB connection string and DynamoDB endpoint snippet were
verified against source, and each adapter tab links to its enablement
guide. No Firebase tab: the Firestore routes are config-gated with no CLI
wiring (`nimbus start` 404s them), so any landing snippet would be
unrunnable. Add the tab if/when CLI enablement lands.
