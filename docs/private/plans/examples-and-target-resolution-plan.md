# Examples And Target Resolution — Control Plane

Status: `active`
Owner branch: `examples-and-target-resolution` (single PR to `nimbus/nimbus` at closeout)
Control-plane updates (this file, plans README): commit direct to `main`.

## Mission

Give Nimbus a canonical, user-facing examples tree that (a) teaches developers
each adapter surface with high-quality runnable apps, (b) verifies our builds
in CI against workspace HEAD, and (c) is displayed, explained, and linked from
the public docs. In the same stroke, unify the CLI's deployment-target UX so an
example's run story is identical from laptop to single node to cluster: one
optional resolver string, local by default, with no node/cluster distinction.

## What This Plan Owns

- The `demos/` → `examples/` restructure: adapter-first tree, root README nav
  index, shared per-app behavior specs.
- The target-resolution CLI UX: one optional positional `TARGET` (URL or
  configured name) on target-taking commands, local default, unified through
  the existing `TargetSelector` seam.
- Canonical example apps (web + agent) and their headless smoke verification
  lane.
- Public docs pages that display/explain each example and link to its GitHub
  source for review.
- Cleanup of demo-era residue (parity notes, stale planned-next lists, script
  naming, insecure tenant-provisioning defaults).

Non-goals: no new adapter surfaces; no cluster features (HS plan owns those);
no separate examples repo pre-launch (mirror publishing is an EX7 row with a
default-defer decision); no changes to the engine mutation path or storage
semantics.

## Decision Records

### XD1 — Examples live in this monorepo; no separate repo pre-launch

The dual purpose (developer-facing + build verification) requires examples to
import workspace packages and run in CI against HEAD, which a separate repo
structurally cannot do. Exemplar precedent: Supabase/Next.js (in-monorepo
`examples/` + scaffolder fetch), Cloudflare (canonical templates + CI that runs
each one), versus Firebase's fragmented sample repos (the anti-pattern). A
`nimbus/examples` mirror, if ever needed, is an automated one-way publish with
released version pins (EX7.6) — never a hand-maintained second copy.

### XD2 — Tree shape: adapter-first with a root nav index, renamed to `examples/`

`examples/README.md` explains the adapter surfaces and is the root nav index;
`examples/convex/*`, `examples/firebase/*`, `examples/mongodb/*`,
`examples/nimbus/*` (native), later `examples/dynamodb/*` and `examples/s3/*`,
hold the apps. This matches how developers arrive ("I'm a Firebase dev").
The tree is renamed from `demos/` because these are the user-facing product and
"demos" reads as internal; pre-launch, the rename is free (breaking changes
preferred). Internal parity fixtures that are not user-worthy stay but move out
of the user-facing README into internal notes (EX8).

### XD3 — App parity via shared specs

Cross-adapter parity is owned by `examples/specs/<app>.md`: one behavior spec
per canonical app (schema, flows, observable assertions). Each adapter
directory implements the spec for its supported subset and says so explicitly.
The spec is the unit the smoke scripts assert against. Canonical app set:
`tasks` (CRUD + live queries — the industry hello-world), `chat` (subscriptions,
presence, pagination), `agent-chat` (durable agent, web), `agent-worker`
(headless agent), `filedrop` (blob/S3), `jobs` (cron/scheduling). Tier 1 =
`tasks` + the two agent apps; the rest are EX7 rows.

### XD4 — Target resolution: one optional resolver string, local default

There is no `nimbus deploy --node <host>` vs `nimbus deploy --cluster <name>`.
Commands that act on a Nimbus resource take one optional positional `TARGET`:

- URL-shaped (`http`/`https`) → remote URL target.
- Otherwise → configured target name / resolver slug.
- Omitted → local discovery, exactly like `nimbus dev`'s local default.

Whether the resolved resource is a single node or a cluster is invisible to the
UX — it is just "a Nimbus resource." `nimbus dev` remains the watch-mode local
development loop; `nimbus deploy` with no argument targets local. This rides
the existing `TargetSelector` seam (`crates/nimbus-cli/src/target_context.rs`:
`LocalDiscovery | NamedTarget | RemoteUrl`, env fallbacks `NIMBUS_TARGET` /
`NIMBUS_DEPLOY_URL`, single-source ambiguity rule). `deploy` migrates onto it
(today `deploy.rs` hard-requires `--url`/`NIMBUS_DEPLOY_URL`). Pre-launch:
replace flags with the positional; do not keep alias flags.

### XD5 — Docs display, explain, and link each example

Public docs get example pages (Developers group per adapter; Agents group for
agent examples), each explaining what the example demonstrates and linking to
its GitHub source path on `nimbus/nimbus` for code review. Pages obey the docs
skill rules: one Diátaxis mode per page, source-available wording, no
`docs/private` links, source-map rows, no internal plumbing. Every example
README ends with the same run story: `nimbus dev` for the local loop, then
`nimbus deploy [TARGET]` — same app, any Nimbus resource.

## Invariants (binding on every band)

1. Examples import workspace packages (npm workspaces / repo paths), so a
   surface change that breaks an example breaks the same PR.
2. Every user-facing example has: README (what it shows, run story), a spec
   mapping, and a headless smoke script asserting observable behavior — not
   just compilation.
3. No `--node`/`--cluster` vocabulary anywhere: CLI help, example READMEs,
   docs pages.
4. Docs claims are source-verified; supported-subset caveats stated honestly.
5. Fail-closed or gate-adding changes run the full workspace suite, not a
   name-filtered subset.

## Band Ledgers

Status values: `todo`, `in_progress`, `done(evidence)`, `no-action(reason)`,
`blocked(reason)`; EX7 rows may also close `decision-recorded(reason)`.

### Band EX0 — Baseline inventory and decision confirmation

Exploration is the deliverable; record findings with `file:line` evidence in
the Appendix.

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX0.1 | Inventory the demos surface: `demos/*` apps, npm workspaces (`package.json:14-19`) and scripts (`:28-41`), Makefile convex-demos overlay (`Makefile:699`), server static serving (`crates/nimbus-server/src/router.rs`, `http/metadata.rs`, `tests/registry_and_license/routes.rs`), `compose.yaml`/`Containerfile` references | Appendix table lists every reference the EX2 rename must touch | n/a | todo |
| EX0.2 | Inventory CLI target seams: `TargetSelector` consumers (`run.rs`, `sandbox.rs`, `lib.rs`), `deploy.rs` `resolve_deploy_url` + flag set, how `NamedTarget` actually resolves to an endpoint+credentials today, `dev` local defaults | Appendix records the resolution chain and the exact flag/env surface EX1 changes | n/a | todo |
| EX0.3 | Agent-surface inventory: which agent-relevant surfaces are usable via public SDK/CLI today (scheduler, sandbox/run workloads, sessions, egress policy) | EX4 scope note written: what agent-chat/agent-worker may use, with citations; nothing fabricated | n/a | todo |
| EX0.4 | Docs touchpoints: sidebar (`website/astro.config.mjs`), `docs/source-map.md`, existing `docs/developers/<adapter>/` pages, Agents group layout | EX6 page list recorded (path + Diátaxis mode per page) | n/a | todo |

### Band EX1 — Target resolution UX

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX1.1 | Extend `TargetSelector` with positional resolver-string parsing: URL-shaped → `RemoteUrl`, else `NamedTarget`; keep validation and the exactly-one-source rule; absent → `LocalDiscovery` implicit default for all consumers | Unit tests cover url/name/whitespace/empty/ambiguous/default-local paths; test names + counts recorded | ~150 + tests | todo |
| EX1.2 | Migrate `nimbus deploy` onto `TargetSelector`: optional positional `TARGET`, remove required `--url` (delete, no alias), keep env fallbacks, local default uses the same local discovery as `run` | `nimbus deploy` (no args) resolves local; `nimbus deploy https://…` and `nimbus deploy prod` resolve correctly; deploy tests updated and green | ~200 + tests | todo |
| EX1.3 | Unify `run`/`sandbox` (and any other `TargetSelector` consumer found in EX0.2) onto the positional form with one shared help-text vocabulary: "TARGET is a URL or a configured target name; omitted = local" | Help output for each command shows the shared vocabulary; no per-command drift | ~100 | todo |
| EX1.4 | No node/cluster distinction audit: sweep CLI help, errors, and this repo's docs for `--node`/`--cluster`-style target vocabulary | `git grep` sweep recorded clean (or each hit justified as non-target usage) | n/a | todo |

### Band EX2 — Tree restructure (`demos/` → `examples/`)

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX2.1 | Rename `demos/` → `examples/` and update every reference from EX0.1: npm workspaces + scripts, Makefile overlay, server static route `/demos/` → `/examples/` + route tests, `index.html`, compose/container files | `git grep -n "demos/"` clean or each residual justified; route tests green; every `npm run *:example:*` command exercised | ~300 (mostly mechanical) | todo |
| EX2.2 | Rewrite `examples/README.md` as the user-facing root nav index: explains each adapter surface, tables the examples, states the uniform run story (`nimbus dev`; `nimbus deploy [TARGET]`); internal parity/status notes moved out (to EX8.1 destination) | README contains no compiled-subset changelog notes; run story uses only the XD4 pattern | ~150 doc | todo |
| EX2.3 | Seed `examples/specs/` with `tasks.md` (schema, flows, observable assertions, per-adapter supported-subset table) | Spec reviewed against each tier-1 adapter's real surface; assertions are smoke-checkable | ~120 doc | todo |

### Band EX3 — Tier-1 canonical app: `tasks` across adapters

Promote/reshape existing demos where sensible rather than writing parallel
apps; parity-fixture behavior that is not user-worthy stays internal.

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX3.1 | `examples/nimbus/tasks` (from `demos/nimbus/html` or new): CRUD + live subscription via the native SDK | Runs against a `nimbus dev`-started server; smoke asserts spec flows; README complete | ~250 | todo |
| EX3.2 | `examples/convex/tasks` (from the React demo): clean tasks app authored via `convex/_generated/server` + schema | Same acceptance; parity-only exercises moved out of the user-facing app | ~250 | todo |
| EX3.3 | `examples/firebase/tasks` (from `demos/firebase/html`): stock `firebase/app`+`firebase/firestore` imports against Nimbus | Same acceptance; exercises live `onSnapshot` per spec | ~200 | todo |
| EX3.4 | `examples/mongodb/tasks` (from `demos/mongodb/node`): stock driver CRUD; spec table marks the no-live-query subset honestly | Same acceptance for the supported subset | ~150 | todo |

### Band EX4 — Agent examples (scope fixed by EX0.3)

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX4.1 | `examples/nimbus/agent-chat`: web app where a durable agent answers with tool calls, persists memory in the DB, and schedules a follow-up via the scheduler — using only surfaces EX0.3 confirmed | Smoke asserts an observable agent behavior (e.g., scheduled follow-up message lands); README explains the sovereignty angle (agent runs inside your trust boundary) | ~350 | todo |
| EX4.2 | `examples/nimbus/agent-worker`: headless autonomous agent workload (run/sandbox surface) with scheduling; demonstrates egress policy only if EX0.3 confirms a public knob | Smoke asserts the worker's observable side effects; no fabricated APIs | ~300 | todo |

### Band EX5 — Smoke verification lane

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX5.1 | Headless smoke script per example (seed → exercise spec flows → assert observable behavior), deterministic, runnable by one command per example | Each smoke red/green demonstrated locally at least once | ~80/example | todo |
| EX5.2 | `make examples-verify` (single-flight wrapped) that boots a fresh local server and runs all smokes; wire into `make ci` or a dedicated required lane per EX0 findings | Target green locally; wiring decision recorded with reason | ~120 | todo |
| EX5.3 | CI lane for examples-verify; prove fail-closed once by inducing a breakage locally and observing red, then reverting | Induced-red evidence recorded; lane green on branch; full-suite run for the gate addition (blast-radius rule) | ~60 CI | todo |

### Band EX6 — Public docs

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX6.1 | Example pages per EX0.4 list: Developers group per adapter + Agents group for agent examples; each explains the example and links to its GitHub source path on `nimbus/nimbus`; one Diátaxis mode per page | Pages pass docs skill rules; every behavior claim source-verified | ~100 doc/page | todo |
| EX6.2 | Sidebar entries (`website/astro.config.mjs`), `docs/source-map.md` rows, natural cross-links (quickstart → tasks example) | Sidebar renders; source-map rows added for every new/changed page | ~50 | todo |
| EX6.3 | Docs gates green: `bash scripts/check-docs.sh` and `bash scripts/verify-nimbus-docs-site.sh` | Both gate outputs recorded | n/a | todo |

### Band EX7 — Nice-to-haves (each row ends `done` or `decision-recorded(reason)`)

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX7.1 | `nimbus init --example <adapter>/<app>` scaffolder sourcing from the monorepo tree | Scaffolded app runs its smoke; or decision recorded | ~250 | todo |
| EX7.2 | `examples/dynamodb/tasks` | Spec-subset acceptance as EX3.4; or decision recorded | ~150 | todo |
| EX7.3 | `examples/s3/filedrop` (blob plane + S3 surface, upload URLs) | Spec + smoke; or decision recorded | ~250 | todo |
| EX7.4 | `examples/nimbus/jobs` (cron/scheduled functions — the old "planned next demos") | Spec + smoke; or decision recorded | ~200 | todo |
| EX7.5 | Docs pages generated from example READMEs (NATS-by-example style) instead of hand-written | Generation wired into docs gates; or decision recorded | ~200 | todo |
| EX7.6 | `nimbus/examples` mirror repo as automated one-way publish with released pins | Default decision: defer until launch; record it (with trigger condition) unless implemented | n/a | todo |
| EX7.7 | `chat` spec + one implementation (subscriptions, presence, pagination) | Spec + smoke; or decision recorded | ~300 | todo |

### Band EX8 — Cleanup

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX8.1 | Move demo-era parity/status notes (the 4B compiled-subset changelog in `demos/README.md`) to an internal home (`docs/private/` note or per-dir DEVNOTES) or delete if stale | User-facing README free of internal status; internal home linked from the owning adapter code area | ~50 | todo |
| EX8.2 | Delete the stale "planned next demos" list (superseded by `examples/specs/` + EX7 rows) | Gone; no dangling references | n/a | todo |
| EX8.3 | Tenant-provisioning-from-frontend default: examples must not silently ship the insecure `POST /api/tenants` pattern — dev-only guard or explicit documented flag | Each example's provisioning path reviewed; behavior + caveat recorded in its README | ~80 | todo |
| EX8.4 | npm script naming `*:demo:*` → `*:example:*`; drop dead scripts | Scripts renamed and exercised; `npm run` list clean | ~30 | todo |
| EX8.5 | Final reference sweep: no stale `demos/` references in code, tests, docs, compose/container files, CI workflows | `git grep` output recorded clean | n/a | todo |

### Band EX9 — Closeout: PR, green, merge

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX9.1 | Full local gate on the branch: `cargo fmt --all --check`, `make clippy`, `make ci`, `npm run typecheck && npm run test && npm run build`, both docs gates | Command outputs (counts, not "passed") recorded here | n/a | todo |
| EX9.2 | Push branch (explicit refspec, never `-u`) and open the PR to `nimbus/nimbus` with a summary mapping bands → changes | PR URL recorded | n/a | todo |
| EX9.3 | CI verdict CONFIRMED green by checking PR state (never assumed), then squash-merge (standing merge-on-green authorization) + post-merge routine | Merge SHA recorded; main pulled and verified | n/a | todo |
| EX9.4 | Post-merge: verify every docs GitHub source link resolves against merged main; docs site deploy green | Link-check output recorded | n/a | todo |
| EX9.5 | Archive this plan, remove its plans-README entry (fold provenance into the archive header), update agent memory | README shows no archived entry; archive committed direct to main | n/a | todo |

## Execution Order And Dependencies

EX0 first (it fixes EX1 flag decisions, EX2 rename surface, EX4 scope, EX6
page list). EX1 and EX2 are parallel-safe after EX0. EX3 needs EX2; EX4 needs
EX0.3 and EX2; EX5 needs at least one EX3 example and covers everything as
bands land; EX6 needs EX2–EX4 shapes settled (paths in links); EX7 anytime
after EX2; EX8 after EX3/EX4 settle what is user-facing; EX9 last.

## Verification Contract

- Per-item: the acceptance line in its ledger row, with evidence recorded in
  the Status cell (test names + counts, command output summary, or SHA).
- Band EX1: focused `nimbus-cli` tests plus any consumer crates' tests.
- Band EX2/EX5: the gate-blast-radius rule — route/gate changes run the full
  relevant suites, not filtered subsets.
- Band EX6: both docs gates.
- Closeout: EX9.1's full local gate before the PR; hosted CI is the merge
  source of truth; merge only on a CONFIRMED green verdict.

## Suggested Goal Prompt

Paste after `/goal`:

/goal Execute docs/private/plans/examples-and-target-resolution-plan.md bands EX0 through EX9 in the documented order. For each ledger item: do its exploration first, implement to the acceptance line, run the named verification, and update its Status cell to done with one evidence line (test names+counts, command output summary, or SHA); mark wrong or already-satisfied items no-action(reason), EX7 rows may close decision-recorded(reason), and truly blocked items blocked(reason) — then continue. All code and public-docs changes go on branch examples-and-target-resolution (never direct to main); plan-ledger and plans-README updates commit direct to main. Binding constraints: one optional positional TARGET (URL or configured name; omitted = local) with no node/cluster distinction anywhere in CLI help, examples, or docs; examples import workspace packages; example smokes assert observable behavior per examples/specs/; docs pages follow the docs skill rules and both docs gates must pass; gate-adding or fail-closed changes run the full workspace suite. Decide rather than ask. The goal is met when every EX0–EX8 ledger row is done, no-action(reason), decision-recorded(reason) (EX7 only), or blocked(reason) with evidence; make ci, the JS suite, and both docs gates are green locally; the PR to nimbus/nimbus is open with its CI verdict CONFIRMED green by checking PR state and it is squash-merged with the post-merge routine and EX9 rows done with evidence; and the plans README entry reflects completion — or stop after 70 turns and record the blocking state in this plan.

## Appendix — EX0 Inventory (filled during EX0)

- EX0.1 rename surface: (todo)
- EX0.2 target-seam chain: (todo)
- EX0.3 agent-surface scope: (todo)
- EX0.4 docs page list: (todo)
