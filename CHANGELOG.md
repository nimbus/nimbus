# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.39] - 2026-06-19

### Fixed

- Publish machine-os from a guest recipe that activates Nimbus guest units from the systemd vendor layer.

**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.38...v0.1.39

## [0.1.38] - 2026-06-19



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.37...v0.1.38

## [0.1.37] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.36 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.36...v0.1.37

## [0.1.36] - 2026-06-19



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.35...v0.1.36

## [0.1.35] - 2026-06-19

### Documentation

- Update CHANGELOG.md for v0.1.34 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.34...v0.1.35

## [0.1.34] - 2026-06-19

### Added

- **website**: Theme-aware favicon with brand sky tile by @jackspirou
- **cli**: DXW3 — start serves all adapters by default, store-backed (D7) by @jackspirou
- **dev**: Firebase migration hint on no-adapter; shorter landing tab comments by @jackspirou
- **provision**: Auto-wire app package.json deps; landing shows migration commands by @jackspirou
- **firebase**: Drop-in firebase package — stock imports work unchanged by @jackspirou
- **cli**: LR12 — nimbus node run, the reconciler's production caller by @jackspirou
- **packaging**: LR10 — ship a hardened systemd unit in deb/rpm by @jackspirou
- **cli,engine**: LR9 — nimbus backup create/restore on SEQ8 archives by @jackspirou
- **server,bin**: LR8 — in-server TLS termination for the main listener by @jackspirou
- **sdk**: LR7 — rest.ts matches the server, with a 3-sided parity guard by @jackspirou
- **cli**: LR6 — nimbus start enables Firestore, MongoDB, and DynamoDB by @jackspirou
- **server,bin**: LR5 — configurable CORS origins by @jackspirou
- **bin**: LR4 — public-bind rotation gate: explicit-once, age advisory by @jackspirou
- **sdk**: LR3 — remove the dead X-Nimbus-Api-Key credential path by @jackspirou
- **cli**: LR2 — nimbus deploy passes the AdminHeaderOnly gate by @jackspirou
- **ndb7**: Default systemd-dbus + linux factory + operator doc by @jackspirou
- **ndb6**: Node-dbus-integration CI lane on ubuntu-24.04 by @jackspirou
- **ndb5**: Linux-gated live systemd integration tests by @jackspirou
- **ndb4**: Zbus error taxonomy + nimbus_core Transport/NotFound by @jackspirou
- **ndb3**: Signal-correlated completion + property encoder by @jackspirou
- **ndb2**: ZbusSystemdClient skeleton + capability probe by @jackspirou
- **ndb1**: Wire zbus_systemd + zbus behind systemd-dbus feature by @jackspirou

### Changed

- **cli**: Render wire surfaces from one presentation list + adapter status in start summary by @jackspirou
- **cli**: Dev-loop hygiene — adoption outcomes as data, cached covered set by @jackspirou
- **server**: Add WireProtocolAdapter seam for sibling listeners by @jackspirou
- **server**: Make Firestore REST auth structural via route-layer middleware by @jackspirou
- **cli**: Decompose dev.rs — tests to dev/tests/, firebase wiring to dev/firebase.rs by @jackspirou
- **repo**: Docs/private goes fully untracked; pipeline inputs move by @jackspirou
- **bin**: LR1 — finish Service->Engine naming in start/ by @jackspirou

### D5.5

- ListStreams + read-triggered retention (T5 Streams complete) by @jackspirou

### D6.1

- UpdateTimeToLive + DescribeTimeToLive (T6 begins) by @jackspirou

### D6.2

- TTL sweeper integration by @jackspirou

### D6.3

- Tagging surface (T6 complete; full T0-T6 op surface) by @jackspirou

### D7.3

- Nimbus-native persisted access-key management (T7 complete) by @jackspirou

### D8.7

- Five DynamoDB verification-harness cases (PR + nightly lanes) by @jackspirou

### D9.1

- Feature-parity coverage table (T0-T7) by @jackspirou

### D9.3

- Failure-injection + fail-closed proof by @jackspirou

### D9.4

- Tenant + auth isolation proof by @jackspirou

### D9.5

- Mixed-workload soak test by @jackspirou

### D9.6

- Performance benchmark baseline (p50/p95/p99 for every op family) by @jackspirou

### D9.7

- Enterprise-readiness closeout; verifier green (23 passed, 0 failed) by @jackspirou

### Documentation

- Correctness + consistency pass across all six groups by @jackspirou
- **website**: Tighten landing hero to "your cloud, one binary" by @jackspirou
- Retire dead docs/private links from package READMEs by @jackspirou
- DXD1 — flip docs to autodetect + default-on adapter reality by @jackspirou
- **site**: Editorial pass — nimbus dev-led landing, de-self-praised voice, Diátaxis heading fixes by @jackspirou
- **site**: Agents group, value-ladder landing, deploy tutorial, title hygiene by @jackspirou
- Favicon follows the page theme; unique page titles; Firebase tab by @jackspirou
- **site**: Brand glyphs in the landing adapter tabs by @jackspirou
- **site**: Landing tabs named by surface, all six proof snippets by @jackspirou
- **agents**: LR13 — launch-readiness baseline archived by @jackspirou
- **plans**: LR0 — launch-readiness verifier + proof bundle by @jackspirou
- **plans**: Launch-readiness plan — close the 13-item docs-truth gap list by @jackspirou
- **private**: DOC13 closeout — nimbus-docs-site plan done + archived by @jackspirou
- **private**: DOC13 staging retirement sweep + editorial fix by @jackspirou
- **agents**: DOC12 — .agents/skills migration + docs skill + AGENTS.md routing by @jackspirou
- **repo**: DOC11 — README front door refactor + repo metadata by @jackspirou
- **site**: DOC8 — llms-small.txt corpus tuning + scripts/check-docs.sh honesty gate by @jackspirou
- **site**: DOC7 — public architecture pages + ARCHITECTURE.md rewrite by @jackspirou
- **site**: DOC6 — Concepts core + CLI/configuration/SDK/capabilities reference by @jackspirou
- **site**: DOC5 — Operators group, tenancy concepts, server reference by @jackspirou
- **site**: DOC4 — Developers and adapter Reference corpus by @jackspirou
- DOC3 — restructure docs/, five-group IA, landing, get-started by @jackspirou
- DOC9 CI pipeline + DOC10 custom domain — nimbusdocs.com live by @jackspirou
- Tighten verifier condition 3 against comment false-positive by @jackspirou
- DOC2 design harmonization — theme tokens + DESIGN.md docs surface by @jackspirou
- DOC0 verifier + DOC1 Starlight scaffold by @jackspirou
- Archive completed dynamodb-adapter-plan by @jackspirou
- Point NDB routing at archived plan path by @jackspirou
- Close remaining NDB review items (minor + verifier rigor) by @jackspirou
- Harden NDB plan after pre-execution review by @jackspirou

### Fixed

- **engine**: Close lost-wakeup race in applied-visibility wait by @jackspirou
- **cli**: Close artifact-order race in convex adoption test by @jackspirou
- **bin**: Machine API client deadlocked on Connection: close responses by @jackspirou
- **ci,server**: Ring-backed rustls + make-wrapped LR12 lane by @jackspirou
- **ci**: Finish the docs/private relocation sweep + D-Bus lane UI deps by @jackspirou
- **repo**: Restore docs/private/* gitignore pattern + recover orphans by @jackspirou
- **release**: LR11 — apt channel live + release->distribution dispatch by @jackspirou
- Pool floor 8, pinned rustls provider, Waker::noop — three CI reds by @jackspirou
- **storage**: Bounded wait before sqlite read-pool exhaustion by @jackspirou
- **bin**: Retry one-shot machine-API test requests on accept races by @jackspirou
- **runtime**: Convert warm-pool partition test to invocation-kind reuse by @jackspirou
- **nds**: Release-train proof gate paths + regenerated artifacts by @jackspirou
- **runtime,ci**: Restore node22 grant contracts + finish stale-path sweep by @jackspirou
- **ci**: Repair the two remaining red-main causes beyond the path hotfix by @jackspirou
- Repair stale docs/private/staging/architecture paths in crates + NDS scripts by @jackspirou
- Remediate full code review findings by @jackspirou
- **ndb3**: Idempotent Manager.Subscribe (AlreadySubscribed) by @jackspirou

### Miscellaneous

- Point dev-autodetect verifier at the archived plan path by @jackspirou
- Baseline service backend refactor by @jackspirou
- Baseline workspace before service backend refactor by @jackspirou

### Styling

- Rustfmt the NDB systemd D-Bus binding by @jackspirou

### Design

- Nav lockup spacing, ink-cropped transparent marks, favicon tile by @jackspirou
- Unify the sky-cycle default theme across console, docs, and brand by @jackspirou

### Dev

- DXL2 — mid-session app-adapter adoption through the boot-time flow by @jackspirou
- DXL1 — live manifest re-detection with presentation-only adoption by @jackspirou
- DXW2 — shared persisted wire credentials + Nimbus-owned .env.local keys by @jackspirou
- D7 — start serves all adapters by default; reshape verifier condition 3 by @jackspirou
- D6 — always-available wire listeners; reshape verifier condition 10 by @jackspirou
- DXW1 — wire-surface detection reads runtime dependencies only by @jackspirou

### Dev-autodetect

- DXF5 — client-app loop semantics by @jackspirou
- DXF4 — projectId→tenant mapping with live round-trip by @jackspirou
- DXF1-DXF3 — scan-gated FirestoreClient detection + wiring by @jackspirou
- DXA2 — always-on Firestore routes in dev by @jackspirou
- DXA1 — app-adapter/wire-surface model split by @jackspirou
- DXA0 — completion-gate verifier scaffold by @jackspirou

### Hardening

- **H7**: Evidence rigor + doc accuracy; plan complete by @jackspirou
- **H6**: Query skips non-scalar/absent index keys instead of aborting by @jackspirou
- **H5**: Reserved-tenant guard + redacted access-key listing by @jackspirou
- **H4**: DeleteTable reclaims stream/streamseq/ttl/tag sidecars by @jackspirou
- **H3**: Atomic stream capture for batch/transact + atomic sequencing by @jackspirou
- **H2**: Atomic single-item + catalog writes, close conditional TOCTOU by @jackspirou
- **H1**: Bind SigV4 body, harden auth robustness, strict-by-default by @jackspirou
- Scaffold verifier + promote plan to in_progress by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.33...v0.1.34

## [0.1.33] - 2026-05-26

### Documentation

- Update CHANGELOG.md for v0.1.33 by @github-actions[bot]
- Update CHANGELOG.md for v0.1.32 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.32...v0.1.33

## [0.1.32] - 2026-05-26

### A1+A2

- Codegen types every query result; lift ServiceDoc; drop casts by @jackspirou

### A3

- Decompose admin/settings.tsx into concept-owned children by @jackspirou

### A4

- Router-level loaders for five data routes by @jackspirou

### A5

- Component catalog — 11 stories under Storybook by @jackspirou

### A6

- CI browser-smoke harness via playwright-cli by @jackspirou

### A7

- Decompose observability route into tab-body siblings by @jackspirou

### AP

- Wire artifact provenance enforcement paths by @jackspirou

### AP0

- Use maintained OCI reference parser for image admission by @jackspirou

### AP1

- Add artifact verifier adapter contract by @jackspirou

### AP2

- Add cosign verifier backend by @jackspirou

### AP3

- Add SLSA verifier backend by @jackspirou

### AP4

- Gate executable artifact provenance by @jackspirou

### AP5

- Add SBOM evidence backend by @jackspirou

### AP6

- Support offline verifier roots by @jackspirou

### AP7

- Add artifact provenance conformance gate by @jackspirou

### Added

- Add node workload reconciler by @jackspirou
- Wire tenant lifecycle evidence by @jackspirou
- Add compose quadlet export by @jackspirou
- Add node service install surface by @jackspirou
- Add systemd transient backend seam by @jackspirou
- Add direct process backend by @jackspirou
- Add host lifecycle seam by @jackspirou
- Add local enforcement binding by @jackspirou

### Appearance

- Palette/mode switcher + brand-token canonicalization by @jackspirou

### BS

- Archive brand-system-plan by @jackspirou

### BS0-3+9

- Brand-system plan + canonical logo + 9 variants + DESIGN.md tier split by @jackspirou

### BS4-5

- Wire favicon + sidebar mark in nimbus-ui by @jackspirou

### Baseline

- Capture in-flight plan, system-tenant, machine, and verification work by @jackspirou

### CA0

- Scaffold Coverage Acceleration plan + verifier + baseline proof by @jackspirou

### CA1

- Install mold linker in setup-rust-cached composite by @jackspirou

### CA2

- Re-enable parallel coverage link under mold (-j 4) by @jackspirou

### CA3

- Shard Coverage into 3 lanes + cargo llvm-cov reducer by @jackspirou

### CA4

- Migrate release.yml to setup-rust-cached composite by @jackspirou

### CA5

- Closeout — archive plan, promote canonical contract, update routing by @jackspirou

### CC0

- Scaffold ci-caching-canonicalization plan + verifier + baseline proof by @jackspirou

### CC1

- Wire sccache into Coverage job (pilot before full rollout) by @jackspirou

### CC2

- Expand sccache across every Rust job + rotate Swatinem v1→v2 by @jackspirou

### CC3

- Rerun-safe Swatinem saves + main-branch save gate by @jackspirou

### CC4

- Ui-artifacts leader job + harness/coverage consumers by @jackspirou

### CC5

- Warm-sccache leader job + ci-caching contract doc by @jackspirou

### CC8

- Closeout — archive plan, update routing, mark gate complete by @jackspirou

### CC9

- Bump sccache-action v0.0.6→v0.0.10, retract save-always, audit stale pins by @jackspirou

### CD1-CD5

- Canonicalize nimbus start/dev/ui CLI surface by @jackspirou

### CD6+CD7

- Verify Electron CWD contract + land walk-up regression suite by @jackspirou

### CD7

- **j**: Cargo fmt on the new deploy-restart test by @jackspirou
- **j**: Pin deployed-app rehydrate contract across Service restart by @jackspirou

### CD8

- Documentation pass for CLI daemon canonicalization by @jackspirou

### CD9

- Tighten --ensure grep gate, capture smoke matrix in execution log by @jackspirou
- Close + archive CLI daemon canonicalization plan by @jackspirou

### CI/CD

- Unblock workspace + desktop-ui + cargo deny lanes by @jackspirou
- Generate nimbus-ui convex codegen before cargo workspace jobs by @jackspirou

### CM0

- Scaffold CI Modernization plan + verifier + baseline proof by @jackspirou

### CM1

- Extract setup-rust-cached composite action, migrate 12 sites by @jackspirou

### CM2

- SHA-pin every third-party action with version-name comment by @jackspirou

### CM3

- Pin ubuntu runners to ubuntu-24.04 by @jackspirou

### CM5

- Emit job summaries from 4 high-value CI jobs by @jackspirou

### CM8

- Closeout — archive plan, promote canonical contract, update routing by @jackspirou

### CW0

- Scaffold CI Wall Acceleration plan + verifier + baseline proof by @jackspirou

### CW1

- Shard verification-harness corpus across shards per surface by @jackspirou

### CW2

- Shard Rust Workspace Tests via nextest --partition by @jackspirou

### CW3

- Split External Provider Integration Tests by provider by @jackspirou

### CW4

- Drop --tests from warm-sccache + document deferred target-cache lane by @jackspirou

### CW5

- Closeout — archive plan, promote contract, update routing by @jackspirou

### Changed

- Extract nimbus node crate by @jackspirou
- Extract pure tenant crate by @jackspirou
- Move artifact verifier effects out of tenant by @jackspirou
- Audit tenant crate boundary by @jackspirou
- Rename tenant isolation module path by @jackspirou

### DA1

- Auth page logo + version chip + local-only trust line by @jackspirou

### DA10

- Agent auth contract + grep gate by @jackspirou

### DA12

- Post-audit cleanup — bind-gate split, rotate-admin polish by @jackspirou

### DA2

- Nimbus auth url command + login/status/logout scaffold by @jackspirou

### DA3

- Flip nimbus dev to auto-open + add --no-open opt-out by @jackspirou

### DA4

- Emit one-shot first-boot launch URL banner from nimbus start by @jackspirou

### DA5

- Auth page polish — lede, hint, error state, disclosure, brand accent by @jackspirou

### DA6

- Cross-CLI sign-in microcopy cleanup + grep gate by @jackspirou

### DA8

- Deploy auth — login/status/logout + credentials file by @jackspirou

### DA9

- Network-bind guardrails — --allow-network + rotation tripwire by @jackspirou

### DR1

- Copy hygiene + canonical EmptyState (F1) by @jackspirou

### DR2

- Gate ⌘\ system tenant lens to Developer view (F2) by @jackspirou

### DR3

- Section truth on Observability + Schedules (F3, F4) by @jackspirou

### DR4

- Auto-default activeTenant on /app + drop ScopeChip "all" fallback (F5, F12) by @jackspirou

### DR5

- Real shells on /admin index + /admin/observability (F10, F11) by @jackspirou

### DR6

- Prune admin service detail to Placement-only by @jackspirou

### DR7

- Polish breadcrumb, tab casing, sub-drawer grouping by @jackspirou

### DU1

- Embed UI assets at /ui/* with SPA fallback by @jackspirou

### DU10

- Testing pyramid, storybook, react compiler eval by @jackspirou

### DU11

- Hardening — disposable server fixture, rotate/shutdown E2E, perf lane by @jackspirou

### DU2

- Open operator console via nimbus ui (Chromium preferred) by @jackspirou

### DU3

- Scaffold + shell layout by @jackspirou

### DU4

- Overview tab by @jackspirou

### DU5

- Machines tab by @jackspirou

### DU6

- Services and functions tabs by @jackspirou

### DU6.5

- Function runner by @jackspirou

### DU7

- Second-pass audit — token values, focus restoration, destructive confirm by @jackspirou
- Ux/ui audit — state-token compliance, modal confirms, link reservation by @jackspirou
- Data browser, schema, indexes, tenants by @jackspirou

### DU8

- Logs and runs tabs by @jackspirou

### DU9

- Settings, integrations, deploys by @jackspirou

### Documentation

- Scaffold node dbus client binding plan by @jackspirou
- Clarify node lifecycle surfaces by @jackspirou
- Define local enforcement boundary by @jackspirou
- Align tenant module naming by @jackspirou
- Update CHANGELOG.md for v0.1.31 by @github-actions[bot]

### EPS0-EPS2

- Add operator policy spine by @jackspirou

### EPS3-EPS4a

- Add sandbox egress policy seam by @jackspirou

### EPS4b0

- Add egress enforcement contract by @jackspirou

### EPS4b1

- Add sandbox supervisor entrypoint by @jackspirou

### EPS4b2a

- Select supervisor egress launch contracts by @jackspirou

### EPS4b2b

- Fail closed krun egress launches by @jackspirou
- Add container egress smoke proof by @jackspirou
- Deny direct container egress intent by @jackspirou

### EPS4b3

- Prove live container egress reload by @jackspirou
- Wire container egress proxy by @jackspirou
- Add sandbox egress proxy core by @jackspirou

### EPS5

- Export tenant isolation audit events by @jackspirou

### EPS6

- Add external policy backend seam by @jackspirou

### EPS7

- Add denied egress policy drafts by @jackspirou

### EPS8

- Add policy prove advisories by @jackspirou

### EPS9

- Publish policy egress conformance by @jackspirou

### Fixed

- Catch up materialized serving snapshots by @jackspirou

### H1

- Ratify dual-persona Services into DESIGN.md (BLOCKER) by @jackspirou

### H2

- Type-safety pass — derive tab unions, extract TenantScope ADT by @jackspirou

### H3

- Offline + error envelopes — LoadingValue<T>, /admin/tenants 404 envelope, status-bar tenant canonicalization by @jackspirou

### H4

- Surface polish — casing, command palette scroll-fit, encryption dot, lens chevron, EmptyState mono, /admin/network default section by @jackspirou

### H5

- Cleanup + spec backfills — shared tenants fetch, abort guard, narrowing throws by @jackspirou

### I2

- Capture Ubuntu 24.04 fresh install proof; flip to done by @jackspirou

### I3

- Capture Debian 13 + Fedora 42 fresh dep install proofs; flip to done by @jackspirou

### I4

- Capture Apple Silicon macOS cask + machine running proof; flip to done by @jackspirou

### I5

- Wire install.sh into releases + hosted curl|sh end-to-end proof; flip to done by @jackspirou

### IS

- Archive install-script-plan; all 5 lanes done by @jackspirou

### LD1

- Makefile dependency graph for nimbus-ui artifacts by @jackspirou

### LD2

- Delete build.rs stub fallback, error actionably by @jackspirou

### LD3

- Ci.yml — delete inlined npm orchestration, route through make by @jackspirou

### LD4

- Build-contract docs + CLAUDE.md routing entry by @jackspirou

### LD6

- Add /goal control-plane verifier script by @jackspirou

### Miscellaneous

- Update tenant crate lockfile by @jackspirou
- Checkpoint current workspace baseline by @jackspirou

### PW0

- Scaffold ci-pr-wall-sub-15 plan + verifier + baseline proof by @jackspirou

### PW1

- Pin libsql image to v0.24.26 + add docker-image cache lane by @jackspirou

### PW2

- Extract Coverage track to .github/workflows/coverage.yml by @jackspirou

### PW3

- Flip ci.yml cancel-in-progress to branch-conditional by @jackspirou

### PW4c

- Retain warm-sccache with measurement rationale by @jackspirou

### PW5

- Repin libsql to v0.24.33 (v0.24.26 had Host-header routing bug) by @jackspirou
- Switch libsql fixture hosts to localhost for sqld v0.24.26 by @jackspirou

### PW6

- Closeout — promote contract, archive plan, update routing by @jackspirou

### R1

- Producer-side query wrapper — drop as-unknown-as casts by @jackspirou

### R10

- Smoke spec — deterministic fixture seeding by @jackspirou

### R11

- Polish — story state coverage + nit pass by @jackspirou

### R2

- Loaderize _.$service.tsx sibling queries by @jackspirou

### R3

- Codegen specs + audit-comment + JsonValue dedup + convention decision by @jackspirou

### R4

- Loaderize compute_.runs_.$runId.tsx by @jackspirou

### R5

- Loader-error envelope coverage on the four A4 service routes by @jackspirou

### R6

- Extract shared filter + table-cell primitives by @jackspirou

### R7

- LoaderDeps for tenant-switch invalidation by @jackspirou

### R8

- A3 residue cleanup — dead dialogRefs + typed settings sub-drawer by @jackspirou

### R9

- CSP test tolerates attribute-bearing tags + workflow paths widened by @jackspirou

### RAQ0

- Add repo architecture guardrail by @jackspirou

### RAQ1

- Split tenant isolation root by @jackspirou

### RAQ2

- Split system tenant evidence by @jackspirou

### RAQ3

- Canonicalize server construction by @jackspirou

### RAQ4

- Split runtime policy and local ops by @jackspirou

### RAQ5

- Split sandbox service manager lifecycle by @jackspirou

### RAQ6

- Split policy and adapter surfaces by @jackspirou

### RAQ7

- Split CLI workflow surfaces by @jackspirou

### RAQ8

- Split JS compatibility surfaces by @jackspirou

### RAQ9

- Add evidence taxonomy guardrails by @jackspirou

### README

- Document nimbus-desktop install path by @jackspirou

### Security

- Add CodeQL SAST workflow for Rust + JavaScript/TypeScript by @jackspirou
- Bump actions/create-github-app-token v3.2.0 -> v3 by @jackspirou
- Closure — archive Phase 1 + Phase 2 plans by @jackspirou

### Testing

- Add tenant node extraction verifier by @jackspirou

### UL1

- Server /api/system/version-info with stale-while-revalidate by @jackspirou

### UL2

- SPA staleness UX — status-bar slot, sonner toast, upgrade popover by @jackspirou

### UL3

- Capture full-flow screencast at .playwright-cli/ul3/full-flow.webm by @jackspirou

### UX1

- Fix lens path resolution to read /app|/admin segment + lift dev-only gate by @jackspirou

### UX2

- Launch ticket bootstrap and styled /ui/auth page by @jackspirou

### UX3

- Clear toast above status bar via shared --statusbar-height token by @jackspirou

### UX4

- Ship styled Select shell component, migrate observability filter by @jackspirou

### UX5

- Branch storage empty state on tenant existence by @jackspirou

### UX6

- Runtime diagnostics returns 200 with null fields when no app is active by @jackspirou

### UX7

- Ship SegmentedControl shell, migrate mode toggle and view switcher by @jackspirou

### UX8

- Tint light-mode page bg so cards read without relying on the border by @jackspirou

### UX9

- Shell-component grep gate, catalog + DESIGN.md sync, after/ proof captures by @jackspirou

### Auth

- Collapse copy chips + add `nimbus auth token` + `--open` URL flag by @jackspirou

### Auth-page

- Unify How to login copy + rename terminal-chrome label by @jackspirou
- Full-width shell-block recovery surface + How to login catalog by @jackspirou
- Rename label to Enter auth token + hero-scale standalone chip by @jackspirou
- Lift Auth Token chip above Other ways to login disclosure by @jackspirou
- Rename label to Local Token, demote token chip into disclosure by @jackspirou

### Nimbus-bin

- Silence unused-import warnings on Windows release by @jackspirou

### Ui

- Scrub leftover /app and /admin refs missed by rename pass by @jackspirou
- Rename persona URL prefixes /app→/developer, /admin→/operator by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.31...v0.1.32

## [0.1.31] - 2026-05-14

### Documentation

- Update CHANGELOG.md for v0.1.30 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.30...v0.1.31

## [0.1.30] - 2026-05-14

### Documentation

- Update CHANGELOG.md for v0.1.29 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.29...v0.1.30

## [0.1.29] - 2026-05-14

### Documentation

- Update CHANGELOG.md for v0.1.28 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.28...v0.1.29

## [0.1.28] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.27...v0.1.28

## [0.1.27] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.26...v0.1.27

## [0.1.26] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.25...v0.1.26

## [0.1.25] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.24...v0.1.25

## [0.1.24] - 2026-05-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.23...v0.1.24

## [0.1.23] - 2026-05-14

### CI/CD

- Stabilize test lanes and node compat catalogs by @jackspirou
- Title case harness check names by @jackspirou
- Make harness gate names event-neutral by @jackspirou
- Speed workspace tests with nextest by @jackspirou
- Clarify workflow gate names by @jackspirou
- Stabilize checks after locker repin by @jackspirou
- Split Rust gates and trim coverage by @jackspirou
- Fix linux sqlcipher package proof by @jackspirou

### Documentation

- Update runtime compatibility and rename plans by @jackspirou
- Archive encryption at rest plan by @jackspirou
- Add generated node lts baseline by @jackspirou
- Update CHANGELOG.md for v0.1.22 by @github-actions[bot]

### Fixed

- Satisfy runtime linux clippy by @jackspirou
- Declare runtime libc dependency by @jackspirou
- Complete neovex→nimbus rename in remaining files by @jackspirou
- Fix hex encoding allocation, stale doc reference, add license path tests by @jackspirou
- Fix sanitize_dir_name edge cases, hoist allocation in env_local writer by @jackspirou

### Cli

- Harden onboarding flow and add node runtime plan by @jackspirou

### Deps

- Repin Deno fork to rusty_v8 locker release by @jackspirou
- Repin Deno fork security release by @jackspirou

### Rename

- Complete neovex→nimbus rebrand across entire codebase by @jackspirou

### Runtime

- Land node22 groundwork and lts plan by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.22...v0.1.23

## [0.1.22] - 2026-04-24

### Codegen

- Replace compile-time new Function paths by @jackspirou

### Engine

- Move provider behavior behind capability methods by @jackspirou

### Runtime

- Make service activation async and type the host ABI by @jackspirou

### Server

- Harden localhost access surface by @jackspirou

### Workspace

- Curate facade and JS verification contract by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.21...v0.1.22

## [0.1.21] - 2026-04-23

### Added

- Support native neovex source roots by @jackspirou

### Build

- Refresh Cargo.lock for v0.1.21 by @jackspirou
- Patch rustls-webpki and stabilize runtime coverage by @jackspirou
- Refresh vite and typescript toolchain by @jackspirou

### CI/CD

- Refresh GitHub Actions versions by @jackspirou

### Documentation

- Promote maintainability control plan by @jackspirou
- Update CHANGELOG.md for v0.1.20 by @github-actions[bot]

### Testing

- Serialize Postgres provider fixtures by @jackspirou

### Release

- V0.1.21 by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.20...v0.1.21

## [0.1.20] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.18 by @github-actions[bot]

### Fixed

- Gate cli progress helpers to unix builds by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.19...v0.1.20

## [0.1.19] - 2026-04-19

### Added

- Close out CLI alignment and add install tooling by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.18...v0.1.19

## [0.1.18] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.17 by @github-actions[bot]

### Testing

- Widen postgres repeated CRUD timeout by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.17...v0.1.18

## [0.1.17] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.16 by @github-actions[bot]
- Update CHANGELOG.md for v0.1.15 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.16...v0.1.17

## [0.1.16] - 2026-04-19



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.15...v0.1.16

## [0.1.15] - 2026-04-19

### Documentation

- Update CHANGELOG.md for v0.1.14 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.14...v0.1.15

## [0.1.14] - 2026-04-18

### Machine

- Reflect guest override in non-unix stub by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.13...v0.1.14

## [0.1.13] - 2026-04-18

### Documentation

- Add storage and rename planning research by @jackspirou

### Testing

- Harden runtime isolation under coverage by @jackspirou
- Bound postgres repeated crud lane by @jackspirou
- Fix machine contract assertions off macOS by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.12...v0.1.13

## [0.1.12] - 2026-04-18

### Documentation

- Update CHANGELOG.md for v0.1.11 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.11...v0.1.12

## [0.1.11] - 2026-04-18

### Build

- Add linux distribution release tooling by @jackspirou

### Documentation

- Fix mermaid edge label syntax in bootc evaluation by @jackspirou
- Add bootc adoption evaluation research by @jackspirou
- Update CHANGELOG.md for v0.1.10 by @github-actions[bot]

### Cargo

- Inherit workspace package metadata by @jackspirou

### Dist

- Ship bundled gvproxy for macos by @jackspirou

### Engine

- Relax concurrent materialized load assertion by @jackspirou

### Machine

- Fix stale client fixtures and clippy by @jackspirou
- Harden macos convergence path by @jackspirou
- Harden guest api and service control by @jackspirou

### Sandbox

- Fix windows process handle typing by @jackspirou
- Make pid liveness probing windows-safe by @jackspirou
- Add podman-aligned oci builder by @jackspirou

### Server

- Collapse index read tracking match guards by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.10...v0.1.11

## [0.1.10] - 2026-04-17

### CI/CD

- Restore release target caching safely by @jackspirou
- Avoid stale release target caches by @jackspirou

### Fixed

- Gate unix-only protocol imports by @jackspirou
- Gate unix machine types on windows by @jackspirou
- Repair v0.1.10 ci lanes by @jackspirou

### Release

- Prepare v0.1.10 by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.9...v0.1.10

## [0.1.9] - 2026-04-17

### Documentation

- Add machine flow and deferred machine plans by @jackspirou
- Update CHANGELOG.md for v0.1.8 by @github-actions[bot]

### Testing

- Fix krun fake buildah unshare parsing by @jackspirou
- Harden executable test stubs by @jackspirou
- Run krun fake buildah via shell by @jackspirou
- Harden fake buildah script publishing by @jackspirou

### Release

- Prepare v0.1.9 by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.8...v0.1.9

## [0.1.8] - 2026-04-16

### CI/CD

- Opt release workflow into node24 actions by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.7 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.7...v0.1.8

## [0.1.7] - 2026-04-16

### CI/CD

- Make machine-os watcher attempt-aware by @jackspirou
- Document rerun-safe artifact naming by @jackspirou
- Stabilize machine-os staged artifact naming by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.5 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.6...v0.1.7

## [0.1.6] - 2026-04-16

### CI/CD

- Release machine-os before neovex by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.5...v0.1.6

## [0.1.5] - 2026-04-15

### CI/CD

- Dispatch machine-os publish workflow by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.4 by @github-actions[bot]



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.4...v0.1.5

## [0.1.4] - 2026-04-15

### Build

- Use stable machine-os workflow ref by @jackspirou
- Repin machine-os workflow refs by @jackspirou
- Cache rusty_v8 artifacts by @jackspirou
- Repin machine-os performance updates by @jackspirou
- Shorten release critical path by @jackspirou
- Fix machine-os workflow pin by @jackspirou
- Reuse staged machine-os release bundles by @jackspirou
- Switch machine-os release flow to app auth by @jackspirou
- Repin machine-os reusable workflow by @jackspirou
- Use reusable machine-os release workflow by @jackspirou
- Dispatch native machine-os releases by @jackspirou

### CI/CD

- Harden workflow timeouts and permissions by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.3 by @github-actions[bot]

### Fixed

- Grant reusable machine-os workflow write access by @jackspirou
- Pin valid machine-os workflow commit by @jackspirou
- Use valid release workflow step ids by @jackspirou
- Match machine-os release run names by @jackspirou
- Account worker load before dispatch send by @jackspirou

### Testing

- Invoke fake buildah via shell launcher by @jackspirou
- Close fake buildah temp path before exec by @jackspirou
- Harden fake buildah helper creation by @jackspirou

### New Contributors
* @github-actions[bot] made their first contribution


**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.3...v0.1.4

## [0.1.3] - 2026-04-15

### Build

- Bump workspace to v0.1.3 by @jackspirou
- Pin machine-os release workflow contract by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.2...v0.1.3

## [0.1.2] - 2026-04-15

### Build

- Bump workspace to v0.1.2 by @jackspirou

### Fixed

- Narrow windows machine compilation seams by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.1...v0.1.2

## [0.1.1] - 2026-04-15

### Build

- Bump workspace to v0.1.1 by @jackspirou
- Patch rustls-webpki advisory by @jackspirou

### Fixed

- Gate machine module on unix hosts by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.0...v0.1.1

## [0.1.0] - 2026-04-15

### Documentation

- Harden machine image release contract by @jackspirou

### Testing

- Derive machine image version from crate version by @jackspirou



**Full Changelog**: https://github.com/nimbus/nimbus/compare/machine-os/v0.1.2...v0.1.0

## [machine-os/v0.1.2] - 2026-04-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/machine-os/v0.1.1...machine-os/v0.1.2

## [machine-os/v0.1.1] - 2026-04-14



**Full Changelog**: https://github.com/nimbus/nimbus/compare/machine-os/v0.1.0...machine-os/v0.1.1

## [machine-os/v0.1.0] - 2026-04-14

### CI/CD

- Use authenticated googlesource path and update Cargo.lock by @jackspirou
- Add googlesource auth and cache-on-failure to all Rust jobs by @jackspirou
- Add Rust toolchain and cargo cache to deny job by @jackspirou
- Mark all workspace crates as unpublished for cargo-deny by @jackspirou
- Fix deny.toml for workspace custom license and path deps by @jackspirou
- Fix deny.toml for cargo-deny 0.19.0 by @jackspirou
- Fix deny.toml config, add weekly audit schedule, dependabot, and codecov config by @jackspirou

### Documentation

- Add macos machine support control plane by @jackspirou
- Archive external SQL provider plan by @jackspirou
- Restructure repo guidance and codex roadmap control plane by @jackspirou

### Fixed

- Isolate cooperative locker tests and annotate V8 reset repro by @jackspirou
- **deps**: Update Cargo.lock to submodule-free rusty_v8 tag by @jackspirou

### Miscellaneous

- Checkpoint remaining workspace changes by @jackspirou

### Testing

- Ignore snapshot-aware reset repro that SIGABRTs on cycle 2 by @jackspirou

### New Contributors
* @jackspirou made their first contribution


<!-- generated by git-cliff -->
