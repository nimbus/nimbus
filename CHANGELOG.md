# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.34] - 2026-06-18

### Changed

- Route macOS machine service workload execution through the guest node-agent and systemd transient-unit path instead of the direct machine API sandbox backend path.
- Add the NSR5 machine-os guest-node proof gates, packaged container runner wiring, and release-helper evidence required for published image promotion.

**Full Changelog**: https://github.com/nimbus/nimbus/compare/v0.1.33...v0.1.34

## [0.1.33] - 2026-05-26

### Documentation

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

### A8

- Close + archive desktop-ui architecture-hardening plan by @jackspirou

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

### Appearance

- Palette/mode switcher + brand-token canonicalization by @jackspirou

### BS

- Archive brand-system-plan by @jackspirou
- Close brand-system-plan; all 10 lanes done by @jackspirou

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

### CD

- **l**: Doc-honesty pass on deploy-load autostart claims by @jackspirou

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

- Capture final make-lane + archive-housekeeping verification by @jackspirou
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

### CM7

- Dependabot configuration + PR-queue audit (2026-05-22) by @jackspirou

### CM8

- Closeout — archive plan, promote canonical contract, update routing by @jackspirou

### CW0

- Scaffold CI Wall Acceleration plan + verifier + baseline proof by @jackspirou

### CW1

- Shard verification-harness corpus across shards per surface by @jackspirou

### CW2

- Shard Rust Workspace Tests via nextest --partition by @jackspirou

### CW3

- Backfill execution-log SHA for the per-provider split by @jackspirou
- Split External Provider Integration Tests by provider by @jackspirou

### CW4

- Backfill execution-log SHA for warm-sccache --tests drop by @jackspirou
- Drop --tests from warm-sccache + document deferred target-cache lane by @jackspirou

### CW5

- Backfill execution-log SHA for closeout by @jackspirou
- Closeout — archive plan, promote contract, update routing by @jackspirou

### DA1

- Auth page logo + version chip + local-only trust line by @jackspirou

### DA10

- Agent auth contract + grep gate by @jackspirou

### DA11

- Archive desktop-auth-dx plan + proof artifacts by @jackspirou

### DA11-fix

- Rewrite disposition section with real plan items by @jackspirou

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

### DR-fixes

- Promote plan + lock DR0 baselines by @jackspirou

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

### DR8

- Close + archive desktop-ui design-review-fixes plan by @jackspirou

### DS0

- Defer Windows code signing; stage first release for macOS + Linux by @jackspirou

### DS0A

- Nimbus/desktop provisioned, decisions documented, DS0B verifier wired by @jackspirou

### DS0B

- Apple credentials uploaded; DS0 satisfied by @jackspirou

### DS1

- Hello-electron loop + security baseline (desktop@6ddf65d) by @jackspirou

### DS10

- Document nimbus ui command and nimbus-desktop in distribution by @jackspirou

### DS2

- Flip desktop-shell-plan status; record execution log by @jackspirou

### DS3

- Flip desktop-shell-plan status; record execution log by @jackspirou

### DS4

- Flip desktop-shell-plan status; record execution log by @jackspirou

### DS5

- Flip desktop-shell-plan status; record execution log by @jackspirou

### DS6

- Flip status to done — per-platform packaging matrix green by @jackspirou

### DU-shell

- Revise plan with two-view IA (Developer + Operator console) by @jackspirou
- Revise plan with first-principles IA review by @jackspirou
- Promote desktop-ui shell overhaul plan by @jackspirou

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
- Capture browser proof of audited UI by @jackspirou
- Ux/ui audit — state-token compliance, modal confirms, link reservation by @jackspirou
- Data browser, schema, indexes, tenants by @jackspirou

### DU8

- Logs and runs tabs by @jackspirou

### DU9

- Settings, integrations, deploys by @jackspirou

### Documentation

- Baseline enterprise policy research by @jackspirou
- **plans**: Close ledger and execution log for ui-persona-route-rename by @jackspirou
- **plans**: Archive ui-persona-route-rename-plan by @jackspirou
- **plans**: Archive desktop-ui-ux-review-fixes-plan by @jackspirou
- Fix DS1 electron pin 41 → 42 by @jackspirou
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

### H6+H7

- Close + archive desktop-ui followup hardening plan by @jackspirou

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

### LD0

- Fix step-count + grep-scope drift in local-dev plan by @jackspirou
- Write local-dev-canonicalization plan + index entry by @jackspirou

### LD1

- Makefile dependency graph for nimbus-ui artifacts by @jackspirou

### LD2

- Delete build.rs stub fallback, error actionably by @jackspirou

### LD3

- Ci.yml — delete inlined npm orchestration, route through make by @jackspirou

### LD4

- Build-contract docs + CLAUDE.md routing entry by @jackspirou

### LD5

- Fresh-clone proof — make ci-required green in worktree by @jackspirou

### LD6

- Add /goal control-plane verifier script by @jackspirou

### LD7

- Close out local-dev canonicalization plan by @jackspirou

### PW0

- Backfill execution-log SHA for scaffold by @jackspirou
- Scaffold ci-pr-wall-sub-15 plan + verifier + baseline proof by @jackspirou

### PW1

- Backfill execution-log SHA for libsql pin + cache lane by @jackspirou
- Pin libsql image to v0.24.26 + add docker-image cache lane by @jackspirou

### PW2

- Backfill execution-log SHA for coverage.yml extraction by @jackspirou
- Extract Coverage track to .github/workflows/coverage.yml by @jackspirou

### PW3

- Backfill execution-log SHA for concurrency cap by @jackspirou
- Flip ci.yml cancel-in-progress to branch-conditional by @jackspirou

### PW4

- Backfill execution-log SHA for warm-sccache retention by @jackspirou

### PW4c

- Retain warm-sccache with measurement rationale by @jackspirou

### PW5

- Fill green proof with 3 consecutive post-PW4 main runs by @jackspirou
- Repin libsql to v0.24.33 (v0.24.26 had Host-header routing bug) by @jackspirou
- Switch libsql fixture hosts to localhost for sqld v0.24.26 by @jackspirou

### PW5+PW6

- Backfill execution-log SHAs by @jackspirou

### PW6

- Closeout — promote contract, archive plan, update routing by @jackspirou

### R0

- Read-in + before-state freeze for residue plan by @jackspirou

### R1

- Producer-side query wrapper — drop as-unknown-as casts by @jackspirou

### R10

- Smoke spec — deterministic fixture seeding by @jackspirou

### R11

- Polish — story state coverage + nit pass by @jackspirou

### R12

- Close + archive desktop-ui architecture-residue plan by @jackspirou

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

### README

- Document nimbus-desktop install path by @jackspirou

### Security

- Add CodeQL SAST workflow for Rust + JavaScript/TypeScript by @jackspirou
- Bump actions/create-github-app-token v3.2.0 -> v3 by @jackspirou
- Adopt (β+) UI-launched upgrade pattern from Podman Desktop by @jackspirou
- Closure — archive Phase 1 + Phase 2 plans by @jackspirou
- Flip Status to done; record execution-log row by @jackspirou

### UL0

- Restructure upgrade UX around DESIGN.md primitives + background brew by @jackspirou
- Tighten upgrade UX to canonical Podman density by @jackspirou
- Land update-lifecycle plan + decision 001 by @jackspirou

### UL1

- Server /api/system/version-info with stale-while-revalidate by @jackspirou

### UL2

- SPA staleness UX — status-bar slot, sonner toast, upgrade popover by @jackspirou

### UL3

- Capture full-flow screencast at .playwright-cli/ul3/full-flow.webm by @jackspirou

### UX0

- File desktop-ui UX/UI review fix plan + baseline screenshots by @jackspirou

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
