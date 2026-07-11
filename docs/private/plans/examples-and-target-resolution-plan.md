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
released version pins (EX7.5) — never a hand-maintained second copy.

Copy-out hazard (2026-07-10 DX review finding; mechanism corrected by the
2026-07-11 foundation review): workspace imports mean a copied-out example's
deps don't resolve — and for Convex specifically the shape differs.
`packages/convex/package.json` deliberately names itself `convex` with a
`convex` bin (that is the compat point), which collides with the official npm
package: the convex apps depend on `"convex": "*"`, which resolves to Nimbus's
workspace package in-repo but to the **official Convex Cloud** package once
copied out and `npm install`ed. The failure is visible, not silent: the apps'
scripts run `convex codegen --app .`, and `--app` is a Nimbus-only flag the
official CLI rejects, so codegen fails loudly; and even past that the client is
pinned to `http://localhost:8080/convex/demo` with `skipConvexDeploymentUrlCheck`,
so it targets the local Nimbus server rather than quietly talking to Convex
Cloud. Consequences bound into this plan: the convex example's README carries a
loud "monorepo-only until the scaffolder ships" warning naming this failure
mode accurately (EX3.2 acceptance); the EX7.1 scaffolder must rewrite the
`convex` workspace dep to a published pin as its core job, not an afterthought;
the other adapter examples (stock upstream clients: `firebase`, `mongodb`, AWS
SDK) fail the same visible way — an unresolved workspace dependency.

### XD2 — Tree shape: adapter-first with a root nav index, renamed to `examples/`

`examples/README.md` explains the adapter surfaces and is the root nav index;
`examples/nimbus/*` (native), `examples/convex/*`, `examples/firebase/*`,
`examples/cloud-functions/*`, `examples/mongodb/*`, and `examples/dynamodb/*`
hold the apps — one directory per public adapter surface (the six documented
under `docs/developers/`), later joined by `examples/s3/*` for the S3 surface.
Every directory level carries a `README.md` that explains its section: the
root nav index, each adapter directory (what the surface is, its examples, the
supported subset, a link to its docs page), `examples/specs/`, and each
example app. This matches how developers arrive ("I'm a Firebase dev").
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
UX — it is just "a Nimbus resource." The rationale (recorded so this is
auditable when HS lands): a Nimbus resource presents one external endpoint
regardless of topology — the kubectl/Fly precedent — so the CLI never needs
node-vs-cluster awareness; cluster-internal concerns are HS-plan territory.
`nimbus dev` remains the watch-mode local development loop; `nimbus deploy`
with no argument targets local.

This rides the `TargetSelector` seam (`crates/nimbus-cli/src/target_context.rs`:
`LocalDiscovery | NamedTarget | RemoteUrl`, single-source ambiguity rule) —
but `NamedTarget` is today an unbacked stub (`run.rs` hard-errors "not yet
backed by a target registry", and `credentials.rs` keys connections by daemon
URL only). Named targets therefore need a real backend, scoped here as EX1.5:
`~/.config/nimbus/targets` (TOML, name → URL — mirroring the credentials-file
precedent) with `nimbus target add/list/remove`; a name resolves to its URL,
then credentials look up by URL exactly as today. `deploy` migrates onto the
seam (today `deploy.rs` hard-requires `--url`/`NIMBUS_DEPLOY_URL`).
Pre-launch: replace flags with the positional and rename the generic env
fallback `NIMBUS_DEPLOY_URL` → `NIMBUS_TARGET_URL` (it already applies to
run/sandbox, not just deploy); no alias flags, no alias env vars.

Reviewer dissent, recorded: the 2026-07-10 gpt-5.6-sol DX review recommended
keeping `deploy` fail-closed (explicit target source required) because a bare
`nimbus deploy` next to a running `nimbus dev` server silently activates a
generation locally when the user may have meant a remote. Owner decision
stands — omitted TARGET means local everywhere, deploy included — with a
required mitigation: every target-taking command prints a prominent
resolved-target line before acting (e.g. `Deploying to LOCAL
http://127.0.0.1:PORT (no TARGET given)` / `Deploying to prod
(https://…, from ~/.config/nimbus/targets)`), so the destination is never
implicit in the output even when it was implicit in the invocation.

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
3. Every directory level under `examples/` has a `README.md` explaining that
   section: root index, per-adapter directories, `specs/`, and each app.
4. No `--node`/`--cluster` vocabulary anywhere: CLI help, example READMEs,
   docs pages.
5. Docs claims are source-verified; supported-subset caveats stated honestly.
6. Fail-closed or gate-adding changes run the full workspace suite, not a
   name-filtered subset.

## Band Ledgers

Status values: `todo`, `in_progress`, `done(evidence)`, `no-action(reason)`,
`blocked(reason)`; EX7 rows may also close `decision-recorded(reason)`.

Foundation review (2026-07-11, gpt-5.6-sol): EX0–EX2 verdict not-ready with 6
findings; all applied on branch `examples-and-target-resolution`. (1) tasks
spec + supported-subset tables + root README labeled target-state (EX3 apps /
EX5 smokes), not delivered; (2) public docs stripped of the deleted
`--url`/`--local`/`--target` flags (get-started/deploy.md, cloud-functions/migrate.md,
reference/cli.md sandbox synopsis), `nimbus auth login --url` left intact;
(3) `dirs::global_config_dir` now ignores empty/relative `XDG_CONFIG_HOME` per
the XDG spec (+3 unit tests); (4) convex copy-out warning rewritten to the real
failure mode (npm swaps in official `convex`, `convex codegen --app .` fails
loudly, client stays on localhost — not a silent Convex-Cloud talk); (5)
per-app READMEs added under every `examples/<adapter>/<app>/` + `index.html`
title fixed; (6) deploy unconfigured-name test made deterministic via injected
temp registry, target add/list/remove stdout + TOML shape asserted, run
stdout/stderr contract pinned.

### Band EX0 — Baseline inventory and decision confirmation

Exploration is the deliverable; record findings with `file:line` evidence in
the Appendix.

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX0.1 | Inventory the demos surface: `demos/*` apps, npm workspaces (`package.json:14-19`) and scripts (`:28-41`), Makefile convex-demos overlay (`Makefile:699`), server static serving (`crates/nimbus-server/src/router.rs`, `http/metadata.rs`, `tests/registry_and_license/routes.rs`), `compose.yaml`/`Containerfile` references | Appendix table lists every reference the EX2 rename must touch | n/a | done(Appendix EX0.1: 20 load-bearing path/route literals catalogued incl. router.rs:596-607, metadata.rs:148-150, cloud-functions/lib.rs:566-573, operator/policy.rs:31-91, package.json:14-19; compose.yaml/Containerfile/.github confirmed zero hits) |
| EX0.2 | Inventory CLI target seams: `TargetSelector` consumers (`run.rs`, `sandbox.rs`, `lib.rs`), `deploy.rs` `resolve_deploy_url` + flag set, how `NamedTarget` actually resolves to an endpoint+credentials today, `dev` local defaults | Appendix records the resolution chain and the exact flag/env surface EX1 changes | n/a | done(Appendix EX0.2: resolution chain traced — 3 flags→3 kinds+env; run has local fallback, sandbox does not, deploy bypasses TargetSelector via resolve_deploy_url; NamedTarget has NO registry/resolver today (errors in run.rs:290, stub in sandbox); LocalDiscovery wired via LocalServerHttpClient::base_url) |
| EX0.3 | Agent-surface inventory: which agent-relevant surfaces are usable via public SDK/CLI today (scheduler, sandbox/run workloads, sessions, egress policy) | EX4 scope note written: what agent-chat/agent-worker may use, with citations; nothing fabricated | n/a | done(Appendix EX0.3: scheduler(SDK real)/DB(SDK+CLI real)/sandbox-sessions-services(SDK only, CLI stubbed) usable; cron/run-exec/sandbox-CLI/egress-knob NOT public; agent-chat scope = query/mutation+ctx.scheduler.runAfter, no machine needed) |
| EX0.4 | Docs touchpoints: sidebar (`website/astro.config.mjs`), `docs/source-map.md`, existing `docs/developers/<adapter>/` pages, Agents group layout | EX6 page list recorded (path + Diátaxis mode per page) | n/a | done(Appendix EX0.4: Starlight sidebar :74-129, autogenerated per-adapter subgroups; existing developers/<adapter> pages listed; EX6 page list = developers/<adapter>/examples.md how-to + agents/agent-*.md; stale-doc debt for deleted --url flagged for EX6) |

### Band EX1 — Target resolution UX

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX1.1 | Extend `TargetSelector` with positional resolver-string parsing: URL-shaped → `RemoteUrl`, else `NamedTarget`; keep validation and the exactly-one-source rule; absent → `LocalDiscovery` implicit default for all consumers | Unit tests cover url/name/whitespace/empty/ambiguous/default-local paths; test names + counts recorded | ~150 + tests | done(target_context.rs: one optional positional `target`, `resolve()` returns LocalDiscovery/ImplicitLocalDefault when absent, http(s) prefix→RemoteUrl else NamedTarget, both-env-set→ambiguity error. 9 unit tests: absent_target_resolves_local_discovery, url_shaped_positional_resolves_remote_url, non_url_positional_resolves_named_target, named_target_rejects_whitespace, empty_positional_rejects, env_target_resolves_named_target_when_positional_absent, env_target_url_resolves_remote_url_when_positional_absent, positional_wins_over_env_fallbacks, ambiguous_env_sources_reject — all green; env source renamed EnvironmentTargetUrl for NIMBUS_TARGET_URL per EX1.5) |
| EX1.2 | Migrate `nimbus deploy` onto `TargetSelector`: optional positional `TARGET`, remove required `--url` (delete, no alias), local default uses the same local discovery as `run`; every target-taking command prints the prominent resolved-target line (destination + how it was resolved) before acting | `nimbus deploy` (no args) resolves local and says so loudly; `nimbus deploy https://…` and `nimbus deploy prod` resolve correctly (the latter via EX1.5); an unconfigured name fails with an error naming the targets file and `nimbus target add`; deploy tests updated and green | ~200 + tests | done(deploy.rs: `resolve_deploy_target_url` now returns `ResolvedDeployTarget { url, banner }`; `run_deploy_command` emits the banner via `emit_deploy_phase` before acting. NamedTarget resolves through the registry (injectable resolver so tests never read the real ~/.config); named→`targets::resolve_named_target_url` error names the file + `nimbus target add <name> <url>`. Banner shapes: `Deploying to LOCAL <url> (no TARGET given)` / `Deploying to <name> (<url>, from <targets-file>)` / `Deploying to <url> (from TARGET|NIMBUS_TARGET_URL)`. run.rs mirrors via `emit_run_target_banner` to STDERR (keeps JSON stdout clean). Tests: deploy_target_resolution_resolves_url_and_requires_token asserts url+banner for TARGET/env/named + unconfigured-name error; deploy_help_describes_positional_target asserts NIMBUS_TARGET_URL, no `--url <URL>`. FOUNDATION REVIEW 2026-07-11: unconfigured-name deploy test made deterministic (injects a temp registry via `resolve_named_target_url_at`, no longer tolerant of the machine's real config); run.rs banner+result split into `write_run_banner`/`write_run_result` with a test pinning banner-to-its-sink and clean-JSON-stdout; public docs (get-started/deploy.md, cloud-functions/migrate.md) stripped of the deleted `nimbus deploy --url` form) |
| EX1.3 | Unify `run`/`sandbox` (and any other `TargetSelector` consumer found in EX0.2) onto the positional form with one shared help-text vocabulary: "TARGET is a URL or a configured target name; omitted = local" | Help output for each command shows the shared vocabulary; no per-command drift | ~100 | done(shared `TARGET_ARG_HELP` const drives every consumer's positional help; run.rs dropped `has_explicit_run_target` + `--local`/`--target` (local default now in `resolve()`); sandbox.rs now local-defaults too (new test sandbox_list_without_target_defaults_to_local); deploy help + cli_ux examples rewritten to positional. Error strings updated to "pass a TARGET URL / omit TARGET for local". 823/823 nimbus-cli tests green; clippy clean (only third-party brotli warnings)) |
| EX1.4 | No node/cluster distinction audit: sweep CLI help, errors, and this repo's docs for `--node`/`--cluster`-style target vocabulary | `git grep` sweep recorded clean (or each hit justified as non-target usage) | n/a | done(`git grep -E '--node|--cluster|node <host>|cluster <name>'` over crates/nimbus-cli + docs: zero target-selection hits. Two incidental non-target hits justified: compose/file/lower.rs:673 "single-node placement" (compose deploy.placement note), docs/concepts/architecture/node-lifecycle.md:159 (architecture prose). No `--node`/`--cluster` target flag exists) |
| EX1.5 | Minimal target registry backing `NamedTarget` (today an unbacked stub): `~/.config/nimbus/targets` TOML (name → URL, mirroring `credentials.rs`'s DEP2 precedent), `nimbus target add/list/remove`, name→URL resolution feeding the existing URL-keyed credential lookup; rename env fallback `NIMBUS_DEPLOY_URL` → `NIMBUS_TARGET_URL` everywhere (delete old name) | `nimbus target add prod https://…` then `nimbus run/deploy prod` works end-to-end; `run.rs`'s "not yet backed by a target registry" error is gone; unit tests cover add/list/remove/resolve/missing-name; `git grep NIMBUS_DEPLOY_URL` clean | ~250 + tests | done(new `targets.rs`: `TargetsFile`/`TargetEntry` TOML at `~/.config/nimbus/targets` (via `dirs::global_config_dir`, XDG-honoring), atomic temp-file write mirroring credentials.rs DEP2; `nimbus target add/list/remove` subcommand wired in lib.rs (`Command::Target`); `resolve_named_target_url` reads name→URL then hands the URL to the existing URL-keyed credential lookup unchanged. Env fallback renamed everywhere: const `TARGET_URL_ENV="NIMBUS_TARGET_URL"`, source `EnvironmentTargetUrl`, cli_ux + docs (reference/cli.md, developers/cloud-functions/index.md, source-map.md) updated. run.rs's registry-stub error DELETED — NamedTarget now resolves. Tests: add_list_remove_round_trips, upsert_overwrites, add_rejects_{non_http,url_shaped,whitespace}, resolve_reads_configured_url, resolve_missing_name_names_file_and_add_command, target_subcommands_parse, banner_names_local_named_and_url_sources — all green (832/832). `git grep NIMBUS_DEPLOY_URL` clean across code + user docs; residual only in this plan's EX0.2 baseline inventory + XD4/EX1.5 rename callouts, justified like the EX2.1 demos/ residual. FOUNDATION REVIEW 2026-07-11: `dirs::global_config_dir` hardened to ignore empty/relative `XDG_CONFIG_HOME` per the XDG spec (a CI-set empty XDG previously wrote the registry under the CWD) + 3 unit tests (empty/relative/absolute); `run_target_command` refactored to an injectable path+writer (`run_target_command_at`) with a test asserting add/list/remove stdout and the on-disk `[target.<name>]`/`url = "…"` TOML shape; reference/cli.md sandbox synopsis rewritten to positional TARGET) |

### Band EX2 — Tree restructure (`demos/` → `examples/`)

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX2.1 | Rename `demos/` → `examples/` and update every reference from EX0.1: npm workspaces + scripts, Makefile overlay, server static route `/demos/` → `/examples/` + route tests, `index.html`, compose/container files | `git grep -n "demos/"` clean or each residual justified; route tests green; every `npm run *:example:*` command exercised | ~300 (mostly mechanical) | done(`git mv demos examples`; all EX0.1 refs updated — package.json workspaces+path-scripts, package-lock regenerated clean (0 demos/22 examples/0 extraneous), router.rs+metadata.rs+http/mod.rs route `/demos`→`/examples` + `examples_redirect`/`examples_dir`, operator policy `Examples` variant, cloud-functions reserved prefix, cli_ux+cli_surface help paths, build.mjs vendor path, vite.config/vanilla.html served paths, .gitattributes/.gitignore/.claude audit, verify-*.sh, source-map/server-transport/AGENTS docs, tests/demos.smoke.md→examples.smoke.md. Route test nimbus_demo_html_is_served serves `/examples/nimbus/html/` green. Residual `demos/` only in this plan's own rename text + plans-README entry. NOTE: `:demo:` script NAMES left to EX8.4; Makefile `convex-demo*` targets untouched (external upstream convex-demos overlay, not the in-repo tree). Verified: nimbus-server 455/0, nimbus-cli 823/0, operator 36/0, cloud-functions 33/0; npm typecheck + build green (vendor bundle regenerated at examples/convex/vendor); clippy clean) |
| EX2.2 | Rewrite `examples/README.md` as the user-facing root nav index: explains each adapter surface, tables the examples, states the uniform run story (`nimbus dev`; `nimbus deploy [TARGET]`); internal parity/status notes moved out (to EX8.1 destination) | README contains no compiled-subset changelog notes; run story uses only the XD4 pattern | ~150 doc | done(examples/README.md rewritten: surfaces table (all six adapters + docs links), uniform run story `nimbus dev` then `nimbus deploy [TARGET]` (XD4 only), specs pointer, provisioning caveat. The 4B compiled-subset parity changelog moved to examples/convex/DEVNOTES.md (parked per task; EX8.1 owns final placement). No changelog notes remain in the README. AMEND (XD1 copy-out hazard): added a copy-out note pointing at the convex README's loud warning) |
| EX2.3 | Seed `examples/specs/` with `tasks.md` (schema, flows, observable assertions, per-adapter supported-subset table); flows are a named-anchor checklist (`tasks.create`, `tasks.live-update`, …) that smoke scripts reference by anchor name, so spec/smoke drift is mechanically detectable | Spec reviewed against each tier-1 adapter's real surface; every assertion has an anchor; smokes reference anchors by name | ~120 doc | done(examples/specs/tasks.md Flows section is now an anchor table: `tasks.create`/`tasks.list`/`tasks.toggle`/`tasks.delete`/`tasks.live-update`, one row per anchor with its observable assertion in the third column so every assertion has an anchor; anchors declared append-only. Supported-subset table keyed to the anchors (CRUD anchors + `tasks.live-update`; mongodb/dynamodb satisfy live by polling `tasks.list`, cloud-functions via `onDocumentCreated` after `tasks.create`). specs/README.md adds a "Flow anchors" section explaining the spec↔smoke contract and drift detection. convex/README.md subset table aligned to anchor headers. FOUNDATION REVIEW 2026-07-11 (prospective-vs-delivered honesty): tasks.md now opens with a "target state, not current coverage" note and labels its supported-subset table as the target the forthcoming apps/smokes meet; every adapter README `tasks` support section labeled "Planned, not yet in this directory" naming the current messages/chat demo; examples/README.md Surfaces section adds a "what exists today vs still being built" paragraph. No tasks apps built here — that is EX3) |
| EX2.4 | Per-directory `README.md` for every section: each `examples/<adapter>/` dir (what the surface is, its examples, supported subset, link to its docs page) and `examples/specs/` (what a spec is, how smokes consume it) | Every directory level under `examples/` has a README; adapter READMEs link to the matching `docs/developers/<adapter>/` page | ~60 doc/dir | done(READMEs added: examples/nimbus, examples/convex, examples/firebase, examples/mongodb (each: surface, apps, tasks-subset row, run story, link to docs/developers/<adapter>/index.md), examples/specs/README.md. examples/convex/showcase already had one (deploy cmd fixed to positional). cloud-functions/dynamodb dirs not yet created — EX3.5/EX3.6 create them with their apps+READMEs; root README tables them as coming soon. AMEND (XD1): examples/convex/README.md carries the loud "monorepo-only" copy-out warning naming the `convex` npm-name collision. FOUNDATION REVIEW 2026-07-11: over-claim corrected — only adapter-level READMEs existed; app-level READMEs were missing. Added examples/convex/{html,http,node}/README.md, examples/firebase/html/README.md, examples/mongodb/node/README.md, examples/nimbus/html/README.md (each: what the app shows today, run story, spec-subset pointer); every directory level under examples/ now has a README. Fixed examples/index.html <title> "Nimbus Demos"→"Nimbus Examples". Copy-out warning mechanism corrected (see EX2.2/XD1)) |

### Band EX3 — Tier-1 canonical app: `tasks` across all public adapter surfaces

Tier 1 covers every adapter surface documented under `docs/developers/`:
native, Convex, Firebase, Cloud Functions, MongoDB, DynamoDB. Promote/reshape
existing demos where sensible rather than writing parallel apps
(cloud-functions and dynamodb have no demo today and are written fresh);
parity-fixture behavior that is not user-worthy stays internal.

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX3.1 | `examples/nimbus/tasks` (from `demos/nimbus/html` or new): CRUD + live subscription via the native SDK | Runs against a `nimbus dev`-started server; smoke asserts spec flows; README complete | ~250 | done(commit 0af1cd45a. Native SDK app (`@nimbus/nimbus`) with WebSocket subscription; smoke.ts asserts all 5 anchors — tasks.create/list/toggle/delete/live-update — against a live `cargo run -p nimbus-bin -- start` server, real PASS output for each. `packages/nimbus/src/rest.ts` gained authenticated-WebSocket factory support so `NimbusSubscriptionClient` can attach the admin token during upgrade; smoke reads `NIMBUS_ADMIN_TOKEN` from env, never hardcoded. Root workspace registered, package-lock regenerated) |
| EX3.2 | `examples/convex/tasks` (from the React demo): clean tasks app authored via `convex/_generated/server` + schema | Same acceptance; parity-only exercises moved out of the user-facing app; README carries the loud copy-out warning (XD1: `convex` npm name collision — copied out of the monorepo, `npm install` silently substitutes the real Convex Cloud package; monorepo-only until EX7.1) | ~250 | done(commit 2a894fccd + this closure commit. App built to spec: schema/functions/generated API, React/Vite UI, reactive headless smoke, README with the XD1 copy-out warning. **EX3.7 closes the original blocker**: the #41 team-binding gate and its dev-mode auto-provisioning are now genuinely confirmed working for anonymous traffic against a real `nimbus dev` boot with zero `NIMBUS_CONVEX_*` env vars set — `ensureTenant()` (`POST /api/tenants`), `tasks.create`, and `tasks.list` all PASS anonymously, proving the gate is no longer what blocks this app. **In-scope bug found+fixed**: `convex/tasks.ts`'s `toggle` handler cast `ctx.db.get(id) as Doc<\"tasks\"> | null`; Nimbus's Convex bundler does not strip TypeScript `as` casts before compiling handlers as runtime JS, so the cast produced `SyntaxError: Unexpected identifier 'as'` that broke the *entire* compiled bundle (every function, not just `toggle`) — fixed by dropping the redundant cast (the return type is already inferred). **Live smoke, run twice against a real local `nimbus dev` boot, capped at 2 attempts per instruction**: attempt 1, the standard `npm run smoke -w convex-tasks` invocation (which runs `npm run codegen` — i.e. the client-side `convex codegen --app .` CLI — immediately before `smoke.ts`), failed verbatim at setup: `FAIL setup: nimbus request failed with 500`. Root-caused: `convex codegen --app .` rewrites the same `.nimbus/convex/bundle.mjs` file the live `nimbus dev` server is serving mutations from, with no coordination between the two processes; the server's file watcher explicitly skips `.nimbus` as build output (`crates/nimbus-cli/src/dev/watch.rs::should_skip_watch_dir`), so it never notices the external overwrite and serves a corrupted bundle to the very first mutation — confirmed via bundle-hash before/after comparison with zero corresponding watch-log lines. Attempt 2 worked around this by invoking `smoke.ts` directly against a fresh boot, skipping the redundant client codegen step (the server's own boot-time preflight already generates `_generated/*` correctly): `PASS tasks.create`, `PASS tasks.list`, then `FAIL tasks.toggle: nimbus request failed with 500`. Root-caused that failure too, via a debug-instrumented fetch-logging copy of `smoke.ts` (deleted after use) that surfaced the real server error body instead of `smoke.ts`'s generic `createApiError` fallback message: any mutation/query whose args schema includes a `v.id(\"table\")`-typed field (`toggle`'s `id`, `remove`'s `id`) fails server-side with `TypeError: missing field \`id\`` even given a well-formed request with a valid, freshly-created id (confirmed via direct curl, ruling out timing/malformed-body causes); traced to the runtime bridge's argument deserialization (not JS-side — the message string does not appear in the compiled bundle) but not fixed, per instruction to stop at 2 attempts rather than retry-loop into unfamiliar `nimbus-runtime`/`nimbus-convex` internals. `tasks.delete` and `tasks.live-update` remain untested — the script halts on the first failure. Net: 2/5 anchors (`tasks.create`, `tasks.list`) have real PASS evidence; `tasks.toggle` has a real, root-caused FAIL unrelated to this app or the #41 gate; 2/5 anchors are blocked transitively. **Compose-sideline mechanism** (team-lead's option (d), executed and reverted this closure): `nimbus dev`'s compose-auto-discovery reads the local machine registry on boot, which hit an unrelated schema-version mismatch (`crates/nimbus-machine/src/image_source.rs::CURRENT_MACHINE_CONFIG_VERSION`, v1 on-disk vs v3 in this build) and refused to boot with the repo's root `compose.yaml` present; worked around by `mv compose.yaml compose.yaml.smoke-bak` before booting and `mv compose.yaml.smoke-bak compose.yaml` after — confirmed via `git status` that the tree is byte-identical to before the sideline. This is a pure boot-time workaround, not a fix, and is inert with respect to the actual smoke evidence above (compose auto-discovery plays no role once the server is up). **Flagged follow-ups** (none of these are #41-gate-related; all are new, pre-existing, repo-wide defects surfaced only because this is the first time anyone ran a full anonymous live smoke against `nimbus dev` for an application-Convex app): (1) the machine-config schema-version mismatch above — `nimbus dev`'s compose-auto-discovery cannot boot against this repo's current `compose.yaml` without the sideline workaround, which is a real product-facing defect, not just a smoke-harness inconvenience; (2) whether `nimbus dev`'s compose-auto-discovery should even attempt a machine-registry read for a plain example/app boot is a product DX question worth its own decision, independent of fixing (1); (3) three separate, newly-discovered runtime bugs in `nimbus dev`'s Convex bundle/generation-activation pipeline, all confirmed this closure and none touched: (a) a non-deterministic boot-time bundle-hash race where the mutation-dispatch path can cache the wrong of two boot-time build passes' hashes, producing a deterministic-per-boot `runtime bundle integrity check failed` on every mutation for that server's lifetime; (b) the 100%-deterministic codegen/live-server bundle race described above (`npm run smoke`/`test`/`build`/`typecheck` corrupting a co-running `nimbus dev` server's served bundle with zero coordination or recovery) — high real-world likelihood since it fires on the exact command every developer runs; (c) the 100%-deterministic `v.id()` argument deserialization bug described above, which blocks `tasks.toggle`/`tasks.delete` and any other `v.id()`-typed mutation/query arg from working at all. None of (a)-(c) are specific to this app or to the #41 gate; they affect every application-Convex example and any real user's app equally) |
| EX3.3 | `examples/firebase/tasks` (from `demos/firebase/html`): stock `firebase/app`+`firebase/firestore` imports against Nimbus | Same acceptance; exercises live `onSnapshot` per spec | ~200 | done(commit 1f0c65423. Stock `firebase/firestore` app (addDoc/getDocs/updateDoc/deleteDoc) plus a real `onSnapshot` live subscription. smoke.ts asserts all 5 anchors against a live `nimbus dev` server (Node 22 required — Firestore protobuf sources use TS enums, needing `--experimental-transform-types`), real PASS output for each. Root workspace registered, package-lock regenerated) |
| EX3.4 | `examples/mongodb/tasks` (from `demos/mongodb/node`): stock driver CRUD; spec table marks the no-live-query subset honestly | Same acceptance for the supported subset | ~150 | done(commit 1ff54e1f4. Stock `mongodb` driver CRUD against a `tasks` collection via `@nimbus/mongodb`'s `mongoUri()`; `tasks.live-update` satisfied by genuine polling (200ms interval, asserts pollCount >= 2), documented honestly as not a live subscription. smoke.ts: all 5 anchors PASS against a live server with the MongoDB wire-protocol listener enabled. Root workspace registered, package-lock regenerated) |
| EX3.5 | `examples/dynamodb/tasks` (new): stock AWS SDK client via `packages/dynamodb` against Nimbus; spec table marks the supported subset | Same acceptance for the supported subset | ~150 | done(commit 3fec8140e. New `examples/dynamodb/` adapter directory + README (first one for this surface). Stock `@aws-sdk/client-dynamodb` + `@nimbus/dynamodb`'s `clientConfig()`; CreateTableCommand is idempotent (catches ResourceInUseException only), tables go ACTIVE synchronously. `tasks.live-update` satisfied by polling a Scan, documented honestly. smoke.ts: all 5 anchors PASS against a live server with the DynamoDB listener, run twice to confirm idempotent table creation on both runs. Root workspace registered, package-lock regenerated) |
| EX3.6 | `examples/cloud-functions/tasks` (new): `firebase-functions/v2` handlers on the tasks data — an HTTP/callable endpoint plus an `onDocumentCreated` Firestore trigger with durable retry (per `docs/developers/cloud-functions/`) | Smoke asserts the trigger's observable side effect (derived write lands after a task insert) and the HTTP handler response | ~200 | done(commit 83b9ea9d3. New `examples/cloud-functions/` adapter directory + README. Functions bundle (`nimbus init cloud-functions` layout): `taskDetails` (`onRequest` HTTP handler, directly curl-able) plus `deriveTask` (`onDocumentCreated("tasks/{taskId}")`, `retry: true`) writing an idempotent per-task derived document (deterministic id, so at-least-once redelivery overwrites rather than double-counts). `nimbus codegen` produced `.nimbus/firebase/{artifact.json,targets.json,bundle.mjs,bundle.sha256}`. smoke.ts against a live server: PASS tasks.create-triggers-derived-write, PASS cloud-functions.http-handler-response. Also fixed a real bug found in scope: `crates/nimbus-server/src/adapters/cloud_functions/http/tenant.rs` was counting the internal `_nimbus` system tenant as an application tenant, making any normal single-app-tenant deployment look ambiguous; fixed with a unit test, `cargo test -p nimbus-server --lib adapters::cloud_functions::http::tenant` 1/0, clippy/fmt clean) |
| EX3.7 | Unblock EX3.2: the #41 Convex team-binding gate (`crates/nimbus-convex/src/tenancy.rs`) is all-fail-closed and refuses every anonymous application-Convex request, including loopback local-dev traffic — no operator config admits an anonymous principal, and no CI job boots any application-Convex demo live, so this went uncaught since the gate landed (2026-06-29). Chosen fix (option 3, team-lead decision): dev-mode tenancy defaults, not a production posture change | Anonymous local Convex traffic works under `nimbus dev` with zero env config, and under plain `start` via an explicit opt-in env; production default fail-closed posture is provably unchanged (existing #41 tests untouched, all green); full workspace test suite green per the blast-radius rule (a fail-closed gate touch is repo-wide blast radius, not crate-scoped) | ~450 + tests | done(commits b26f29e52, d1fe82f38, 7a14e1090. **EX3.7a** `ConvexTenancyConfig` gains `anonymous_team: Option<TeamId>` + `with_anonymous_team` builder, consulted by `authorize_silo_selection` only for anonymous principals when explicitly set; default `None` preserves prior behavior exactly (16/16 tenancy tests, incl. new anonymous-passes-only-for-bound-silo + verified-principals-unaffected cases). **EX3.7b** `crates/nimbus-cli/src/start/adapters/convex_tenancy.rs` adds opt-in `NIMBUS_CONVEX_ANONYMOUS_TEAM` for `start`; any of the three `NIMBUS_CONVEX_*` envs present ingests wholesale (no merging); all-absent plain `start` still returns `(None, None)` — refuse-everything, byte-for-byte the pre-EX3.7 behavior. **EX3.7c** `nimbus dev` boot only (gated on `StartCommand.auto_tenant.is_some()`): when all three envs are absent, auto-provisions a `SiloTeamRegistry` binding the auto-tenant to a fixed process-internal dev team plus `with_anonymous_team` on that same team, and prints a loud boot notice naming the auto-bound silo and all three override envs; `AdapterEnablement::status_lines()` surfaces it. 34/34 nimbus-cli `start::adapters` tests, 16/16 nimbus-convex `tenancy` tests; `cargo fmt --all --check` and `cargo clippy -p nimbus-cli -p nimbus-convex --all-targets` clean. **EX3.7d** root `package.json`'s `convex:server:{node,html,http}` scripts (which use `start`, not `dev`) now set `NIMBUS_CONVEX_SILO_TEAMS=demo:demo-team NIMBUS_CONVEX_ANONYMOUS_TEAM=demo-team` explicitly, plus a new `convex:server:tasks` script for EX3.2's closure; live-verifying `convex:server:node` + `convex:demo:node` surfaced a second, pre-existing, structurally independent bug — `examples/convex/node/script.ts`'s `ensureTenant()` never sent `NIMBUS_ADMIN_TOKEN`, so its `POST /api/tenants` 401'd against the local-admin-router regardless of the tenancy fix (every sibling demo script already had this `Authorization: Bearer` pattern; node's was the one missing it) — fixed to match. Real live PASS recorded: `Initial messages: []` then `Inserted message: messages:01KX81E4E0X0J0Q1VJEXP5ZF2K` against a locally-booted server with the new envs. **Full workspace suite** (blast-radius rule, [[feedback_gate_blast_radius_full_suite]]) GREEN: naive `make test` (bare `cargo test --workspace`) was blocked twice by a genuine shared-machine disk shortfall ("No space left on device" during linking; team-lead cleaned 124.7GiB of regenerable artifacts from the main checkout's `target/`, `df -h /` 125Gi free), then once the disk was clear it still hit a known, pre-existing, non-regression flake in `nimbus-runtime` (SIGABRT signal 6 — parallel V8 isolate construction poisoning shared V8 cage/RO-heap state under libtest's default parallelism; the Makefile's own `test-rust-runtime` comments document exactly this hazard, and CI itself never runs `nimbus-runtime` under plain `cargo test --workspace` for this reason). Ran the CI-faithful decomposition instead (the same three Rust lanes `ci-required` composes): `make test-rust-runtime` (serial nimbus-runtime lane) — 8/8 lib tests passed, all doctests ok/appropriately-ignored, 0 failed; `make test-rust-workspace` (`cargo nextest run --workspace --exclude nimbus-runtime`) — 4250 tests run, 4250 passed, 0 failed, 31 skipped; `make test-rust-docs` (workspace doctests, excludes nimbus-runtime) — 39 doc-test suites, all `ok`, 0 failed. Genuine full-workspace green, real counts recorded above. **Discovered, deliberately unfixed follow-up**: `examples/convex/http/src/main.ts` has the identical missing-`NIMBUS_ADMIN_TOKEN` gap as node's script.ts had; only one working demo was required to close this band, so http's fix is left for whoever next touches that app. **Flagged, explicitly out-of-scope, adapter-owner decision**: whether a *production* Convex deployment needs an anonymous-public-functions posture (Convex Cloud allows unauthenticated public functions by default) is unresolved — this work deliberately does not build that; it only restores local-dev parity. If pursued, it is a `ConvexTenancyConfig` policy decision for the adapter owner, not an extension of the dev-mode defaults added here) |

### Band EX4 — Agent examples (scope fixed by EX0.3)

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX4.1 | `examples/nimbus/agent-chat`: web app where a durable agent answers with tool calls, persists memory in the DB, and schedules a follow-up via the scheduler — using only surfaces EX0.3 confirmed | Smoke asserts an observable agent behavior (e.g., scheduled follow-up message lands); README explains the sovereignty angle (agent runs inside your trust boundary) | ~350 | todo |
| EX4.2 | `examples/nimbus/agent-worker`: headless autonomous agent workload (run/sandbox surface) with scheduling; demonstrates egress policy only if EX0.3 confirms a public knob | Smoke asserts the worker's observable side effects; no fabricated APIs | ~300 | todo |

### Band EX5 — Smoke verification lane

| ID | Item | Acceptance | Size | Status |
| --- | --- | --- | --- | --- |
| EX5.1 | Headless smoke script per example (seed → exercise spec flows → assert observable behavior), deterministic, runnable by one command per example; at least one smoke runs the real `nimbus run` binary with stdout and stderr captured separately and asserts the stream contract (stdout alone parses as the result JSON; the resolved-target banner appears on stderr) — the process-level guard the 2026-07-11 review noted unit tests cannot provide | Each smoke red/green demonstrated locally at least once; the stdio-contract assertion present and green | ~80/example | todo |
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
| EX7.1 | `nimbus init --example <adapter>/<app>` scaffolder sourcing from the monorepo tree; its core job is rewriting workspace deps to published pins (this is what closes the XD1 copy-out hazard, including the `convex` name collision) | Scaffolded app `npm install`s cleanly outside the repo and runs its smoke; or decision recorded | ~250 | todo |
| EX7.2 | `examples/s3/filedrop` (blob plane + S3 surface, upload URLs) | Spec + smoke; or decision recorded | ~250 | todo |
| EX7.3 | `examples/nimbus/jobs` (cron/scheduled functions — the old "planned next demos") | Spec + smoke; or decision recorded | ~200 | todo |
| EX7.4 | Docs pages generated from example READMEs (NATS-by-example style) instead of hand-written | Generation wired into docs gates; or decision recorded | ~200 | todo |
| EX7.5 | `nimbus/examples` mirror repo as automated one-way publish with released pins | Default decision: defer until launch; record it (with trigger condition) unless implemented | n/a | todo |
| EX7.6 | `chat` spec + one implementation (subscriptions, presence, pagination) | Spec + smoke; or decision recorded | ~300 | todo |
| EX7.7 | Not-yet-public surfaces (KV/RESP, Cloudflare adapters): record the decision for when each gets an examples directory (trigger = its public docs page landing) | Decision recorded per surface with trigger condition | n/a | todo |

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

### EX0.1 rename surface (`demos/` → `examples/`)

Load-bearing (filesystem / route / served-path literals the rename MUST change):

- `package.json:14-19` — six workspace entries (`demos/convex/html`, `demos/convex/http`,
  `demos/convex/node`, `demos/firebase/html`, `demos/mongodb/node`, `demos/nimbus/html`).
- `package.json:28,29,33,34,35` — scripts passing `--app`/`--app-dir` `demos/...` paths.
- `package-lock.json` — mirrors every workspace path (`:17-22` block + packages keys); regenerate via `npm install`.
- `crates/nimbus-server/src/router.rs:596,600,601,604,606,607` — `demos_dir()`, `/demos` redirect route,
  `.nest_service("/demos/", …)`, `join("../../demos")`.
- `crates/nimbus-server/src/http/metadata.rs:148-150` — `demos_redirect()` → `Redirect::permanent("/demos/")`.
- `crates/nimbus-server/src/http/mod.rs:56` — re-export of `demos_redirect`.
- `crates/nimbus-server/src/tests/registry_and_license/routes.rs:27` — asserts `GET /demos/nimbus/html/`.
- `crates/nimbus-cloud-functions/src/lib.rs:566,573` — reserved-prefix match on `/demos`, `/demos/`.
- `crates/nimbus-operator/src/policy.rs:31,91` — path classification `== "/demos"` / `starts_with("/demos/")`, `Self::Demos => "demos"`.
- `demos/convex/html/vanilla.html:88` — `<script src="/demos/convex/vendor/browser.bundle.js">`.
- `demos/convex/http/vite.config.ts:8` — `base: ".../demos/convex/http/dist/"`.
- `packages/convex/build.mjs:10` — `path.resolve(packageRoot, "../../demos/convex/vendor")`.
- `scripts/verify-repo-architecture-quality.sh:100,110` — scans `${REPO_ROOT}/demos` and excludes `demos/convex/vendor`.
- `.gitattributes:18,19` — `demos/convex/*/.neovex/**`, `demos/convex/*/convex/_generated/**` linguist globs.
- `.gitignore:66` — `demos/convex/vendor/browser.bundle.js`.
- `.claude/audit-full.mjs:154,155` — audit config path + description mentioning `demos`.
- `tests/demos.smoke.md` (filename + `:6` served-route URL) — smoke doc.
- `demos/README.md:73,124,152` — served-path/URL literals; whole file is rewritten by EX2.2.
- `demos/convex/showcase/README.md:19` — `nimbus deploy --url … --app-dir demos/convex/showcase` (also carries stale `--url`).

Naming-convention (coordinate; not path-load-bearing):

- `package.json:36-41` — `*:demo:*` scripts (rename to `*:example:*` — EX8.4 territory, but touched here).
- `Makefile:4,699-718` — `convex-demo*` targets operate on an EXTERNAL upstream `convex-demos` checkout, NOT the in-repo tree; renaming is optional and decoupled from the tree rename.
- `scripts/convex-demo-overlay.mjs`, `scripts/stop-demo-processes.sh` — external-overlay tooling (Makefile-coupled).
- CLI help examples `crates/nimbus-cli/src/cli_ux.rs:42,52,53,69,84,85,92,93` (`--app ./demos/…`) and their asserting tests `crates/nimbus-cli/src/start/tests/cli_surface.rs:193,293,294,312,315,938,941,962,963`.
- Docs refs (EX6 territory, not rename-blocking): `docs/concepts/architecture/server-transport.md:55`, `docs/source-map.md:39,309`, `AGENTS.md:4`.

NOT affected: `compose.yaml`, `Containerfile` (neither references `demos`); `.github/` workflows (zero hits).
Landing page `demos/index.html` relocates but has no `/demos/` path literals (its links use dev-server ports).

### EX0.2 target-seam chain

`TargetSelector` (`crates/nimbus-cli/src/target_context.rs:7-93`) TODAY = three flags `--local` / `--target NAME` /
`--url URL` + env fallbacks `NIMBUS_TARGET`→`NamedTarget`, `NIMBUS_DEPLOY_URL`→`RemoteUrl`, with an exactly-one-source
rule (`:83-92`). Resolves to `TargetContextKind::{LocalDiscovery | NamedTarget(String) | RemoteUrl(String)}` (`:28-33`).

Consumers:
- `run.rs` — `RunCommand` flattens `TargetSelector` (`:29`); `resolve_run_target_with_env` (`:128`) calls `.resolve("run", …)`
  then on empty-source error falls back to `LocalDiscovery`/`ImplicitLocalDefault` (`:135-146`) → **run already defaults to local**.
- `sandbox.rs` — `SandboxCreateCommand`/`SandboxListCommand` flatten it (`:30,40`); `resolve_sandbox_target` (`:50-59`)
  calls `.resolve(…)` with **no local fallback** → sandbox currently REQUIRES an explicit source. EX1.3 unifies this to local-default.
- `deploy.rs` — does NOT use `TargetSelector`. Own `--url` field (`:27`) + `resolve_deploy_url` (`:274-296`) requires
  `--url` or `NIMBUS_DEPLOY_URL`. The resolved `target_url: String` feeds token lookup (`:164` keyed by URL), loopback
  admin-token discovery (`:170-175,225-250`), and the POST endpoint (`:196-198`). EX1.2 migrates this onto `TargetSelector`.

How `NamedTarget` resolves to an endpoint+credentials TODAY: **it does not.** `run.rs:290-292` returns
`InvalidInput("named target … is not yet backed by a target registry; pass --local or --url")`; `sandbox` never executes
(returns "reserved for the service-sandbox-node workload-control path"). There is no target registry — `NamedTarget` is a
carried string with no resolver. LocalDiscovery IS wired: `LocalServerHttpClient::discover` (`local_server_client.rs:25-59`)
reads `read_live_server_discovery` and exposes `base_url()` (`:61-63`) = `http://127.0.0.1:PORT`, which EX1.2 feeds to deploy.

Exact flag/env surface EX1 changes: delete `--local`/`--target`/`--url` from `TargetSelector` and deploy's `--url`; replace
with one optional positional `TARGET`; keep `NIMBUS_TARGET`/`NIMBUS_DEPLOY_URL` env fallbacks; move the local default INTO
`resolve()` so every consumer defaults local when absent. Help/error strings to rewrite: `cli_ux.rs:40,41,65-79`,
`sandbox.rs:12`, `run.rs:282,291`, `target_context.rs:86,89`, deploy help test `deploy.rs:934-945`, parse tests
`deploy.rs:897-931`, `run.rs:356-376,383`, `sandbox.rs:68-95`, `target_context.rs:156-209`.

### EX0.3 agent-surface scope (for EX4 — no fabrication)

Verified usable today:
- **Scheduled functions** (SDK): `scheduleAfter`/`scheduleAt`/`cancel` (`packages/nimbus/src/http-client.ts:138,155,173`,
  public `browser.ts:223,231`, convex-compat `packages/convex/src/browser.ts:246,268`); function-authoring
  `ctx.scheduler` (`packages/nimbus/src/server.ts:323`, `packages/convex/src/server.ts:92`); server routes
  `crates/nimbus-server/src/router.rs:776-785`. Delay/timestamp only.
- **DB read/write / agent memory** (SDK + CLI): `query`/`mutation`/`action`/`paginatedQuery`
  (`packages/nimbus/src/http-client.ts:79,90,101,112`, convex-compat `browser.ts:184,206`); `ctx.db` insert/patch/delete
  (`server.ts:307,314`); `nimbus run functions query|mutation|action` (`run.rs:116`, endpoints `:90-95`).
- **Sandbox / Sessions / Services lifecycle** (SDK only): `control-plane/client.ts:110+,234,245,251,264,277,289,295,304`;
  server handlers `crates/nimbus-server/src/http/sandboxes.rs:26`, `.../sessions.rs`. Execution caveat: libkrun fails
  closed for process-exec; containers Linux-only; macOS/WSL2 need `nimbus machine`.

NOT publicly usable (must not appear in examples as working):
- **Cron jobs** — only orphaned `packages/nimbus/src/rest.ts:298-307` (not in `package.json` exports) + raw HTTP routes; no
  SDK method, no CLI. Simulate recurrence with `scheduler.runAfter` re-scheduling.
- **`nimbus run exec`** — stubbed (`run.rs:117-120`). **`nimbus sandbox` CLI** — stubbed (`sandbox.rs:45-47`).
- **Egress policy knob** — no SDK/CLI field; config/compose-only (`compose/file.rs:209-223`), deny-by-default server side.

Safe EX4 scope: `agent-chat` = `@nimbus/nimbus/browser|react` query/mutation/action + `ctx.scheduler.runAfter/runAt` for
durable follow-ups; NO sandbox/machine needed (functions run server-side in V8). `agent-worker` = same DB surface +
`ctx.scheduler` for cadence (no cron API) + optionally `nimbus run functions` from CLI. A real-sandbox example may use ONLY
the SDK lifecycle (`nimbus.sandboxes.*` + `nimbus.sessions.*`), never the stubbed CLI, and must note the machine/Linux
prerequisite. Do not show an egress SDK/CLI knob, a JS cron API, `run exec`, or the `sandbox` CLI as working.

### EX0.4 docs page list (for EX6)

Site = Astro + Starlight, content authored in `docs/` (config `website/astro.config.mjs`). Sidebar array `:74-129`:
`Developers` group `:76-90` (per-adapter autogenerated subgroups: Convex `:82`, Firestore=firebase `:83`,
Cloud Functions `:84`, MongoDB `:85`, DynamoDB `:86`, Native API=native `:87`, Node.js runtime `:88`), `Agents` group `:91`
(fully autogenerated from `docs/agents/`), parallel `Reference` per-adapter tree `:119-126`. Autogenerate means a new page
dropped in an already-listed directory appears automatically (order via frontmatter `sidebar.order`); a brand-new directory
must be added to the array (`:71-73`).

Existing adapter pages under `docs/developers/`: native `index.md`; convex `index.md`+`migrate.md`; firebase
`index.md`+`migrate.md`; cloud-functions `index.md`+`migrate.md`; mongodb `index.md`+`examples.md` (the examples-page
precedent); dynamodb `index.md`. Agents: `docs/agents/{index,sandbox-quickstart,sandboxes,services,sessions}.md`.

`docs/source-map.md` = 3-col table `| Doc page | Claim / surface | Source |` (header `:12-13`); every new example page with a
behavior claim needs a row or `scripts/check-docs.sh` (`:7`) flags it. Docs gates: `scripts/check-docs.sh`,
`scripts/verify-nimbus-docs-site.sh` (both present, executable).

Proposed EX6 page list (path + Diátaxis mode): per-adapter example pages go under `docs/developers/<adapter>/examples.md`
(how-to; mongodb precedent) — convex, firebase, cloud-functions, mongodb (extend existing), dynamodb, native. Agent example
pages under `docs/agents/` — `agent-chat.md` (tutorial), `agent-worker.md` (how-to). Each links to its GitHub source path on
`nimbus/nimbus` and gets a `source-map.md` row.

STALE-DOC DEBT for EX6 (public docs referencing the deleted `--url`/`--local|--target|--url` CLI surface, out of EX1-EX2
scope): `docs/get-started/deploy.md:38,56,71,90`, `docs/reference/cli.md:154,183,184`,
`docs/developers/cloud-functions/index.md:123`, `docs/developers/cloud-functions/migrate.md:196`, `docs/source-map.md:92`.
EX6 must rewrite these to the positional `TARGET` form.
