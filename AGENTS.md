<!-- convex-ai-start -->
This project implements a [Convex](https://convex.dev)-compatible backend server.

When working on Convex-compatible code (`packages/convex/`, `examples/convex/`, or any Convex API surface), **always read `docs/private/adapters/convex/ai-guidelines.md` first** for important guidelines on how to correctly use Convex APIs and patterns. The file contains rules that override what you may have learned about Convex from training data.
<!-- convex-ai-end -->

# Nimbus

## What Nimbus Is

Nimbus is a source-available, single-binary backend for apps and AI agents. It
speaks Convex, Firestore/Firebase, Cloud Functions, MongoDB, and DynamoDB
surfaces, but routes them through one engine, storage layer, runtime, and trust
model.

Its big pieces are: server/adapters for protocol front doors, `nimbus-engine`
for reads/writes/subscriptions/scheduling, `nimbus-storage` for durable state,
`nimbus-runtime` plus `nimbus-bridge` for V8 TypeScript execution, sandbox/node
crates for agent and service workloads, and `packages/*` for SDKs, compatibility
packages, codegen, and the embedded UI.

The role of this file is to capture common mistakes and recurring confusion points for agents working in this repo.

If you hit a surprise that is likely to trip up another agent, tell the developer. Ask before adding a brief principle-first note here. If the guidance needs more than a few bullets, it probably belongs in `docs/*.md` or beside the code instead of here.

## Keep This File Small

- Put durable repo-wide rules, repeated traps, and verification commands here.
- Add new entries only with developer approval.
- Prefer principle-first notes over historical bug writeups.
- Link to canonical docs for architecture details instead of copying them here.
- Do not use this file as a changelog, ownership map, or deep implementation manual.

## Picking the right models for workflows and subagents

Rankings, higher = better. Cost reflects what I actually pay, not list price.
OpenAI is lower-cost in this setup because of generous limits, but not free.
Intelligence is how hard a problem you can hand the model unsupervised. Taste
covers UI/UX, code quality, API design, and copy.

| model | cost | intelligence | taste |
| --- | --- | --- | --- |
| gpt-5.6-sol | 9 | 9 | 8.5 |
| sonnet-5 | 5 | 5 | 7 |
| opus-4.8 | 4 | 7 | 8 |
| fable-5 | 2 | 9 | 9 |

gpt-5.6-sol's 8.5 is an owner call (2026-07-10): above opus-4.8, below
fable-5, ahead of what benchmarks can yet confirm (sol is unranked on the
independent design arenas; benchmark-supported reading is ~8, UI/UX-weighted).
Taste is axis-uneven: UI/UX is sol's strength; writing/copy ≈ 6 (EQ-Bench
prose has the GPT line well behind Claude), so prefer Claude models for
prose, docs, and copy. Re-check against WebDev/Design Arena once sol Elos
publish.

How to apply:

- These are defaults, not limits. You have standing permission to override
  them: if a cheaper model's output doesn't meet the bar, rerun or redo the
  work with a smarter model without asking. Judge the output, not the price tag.
  Escalating costs less than shipping mediocre work.
- Cost is a tie-breaker only; when axes conflict for anything that ships,
  intelligence > taste > cost.
- Bulk/mechanical work (clear-spec implementation, data analysis, migrations):
  gpt-5.6-sol – it is lower-cost and token-efficient for this workload.
  (gpt-5.6-sol succeeded gpt-5.5 as the Codex flagship, GA 2026-07-09; the
  default model + `model_reasoning_effort = "high"` are set in
  `~/.codex/config.toml` and require Codex CLI >= 0.143.0.)
- Anything user-facing (UI, copy, API design) needs taste ≥ 7.
- Reviews of plans/implementations: fable-5 or opus-4.8, optionally gpt-5.6-sol as
  an extra independent perspective.
- Never use Haiku.
- Mechanics: gpt-5.6-sol is handled natively via the `openai/codex-plugin-cc`
  plugin inside Claude Code, automatically adopting your user-level
  configuration from `~/.codex/config.toml`. Avoid writing custom Bash scripts;
  use the plugin's built-in tools and skills instead:
  - `/codex:review` - Run non-destructive, read-only code quality assessments.
    Supports `--base <ref>` for branch analysis.
  - `/codex:adversarial-review` - Perform a skeptical design review to
    pressure-test tradeoffs, auth, and reliability. Append custom focus text at
    the end of the command to steer the focus.
  - `/codex:rescue` - Subcontract active debugging, multi-file refactoring, or
    implementation loops to Codex when a second pass is required.
  - `/codex:status` / `/codex:result` / `/codex:cancel` - Use these to check,
    fetch, or abort asynchronous jobs when using the `--background` flag on
    heavy tasks.
- Claude models (sonnet-5, opus-4.8, fable-5) run via the Agent/Workflow model
  parameter.

Using gpt-5.6-sol inside workflows and subagents:

- Subagents and automated workflows should call the plugin's native slash
  commands or its exposed `codex-cli-runtime` skills to delegate tasks directly,
  omitting the need for raw terminal wrappers.
- Closed-loop quality assurance runs through the user-level stop-gate
  wrapper at `~/.claude/hooks/stop-review-triage.mjs` (registered in
  `~/.claude/settings.json`), which replaces the plugin's own
  `/codex:setup --enable-review-gate` gate (keep that plugin gate OFF or
  both will run). The wrapper fingerprints the repo (`HEAD` + status + tracked
  diff + untracked-file size/mtime identity) at every stop: turns with no edits ALLOW instantly with zero API
  calls; docs-only turns
  (every changed path `.md`/`.mdx`/`.markdown`/`.txt`, worktree and
  commits alike) also skip the engine; turns that edited code get a
  triage-grade Codex stop-review at LOW reasoning effort (model from
  `~/.codex/config.toml`; explicit autoreview loops keep high effort).
  Engine failures block once per 30-minute outage window
  (per session, not per edit state — concurrent sessions in one checkout
  churn the fingerprint) with a probed diagnosis (credit exhaustion vs
  stale daemons vs unresponsive CLI) that states it is an outage, not a
  verdict; further stops in the window warn-allow. Reviewer outages block once
  per edit state, then allow with a loud warning instead of looping.
  Disable temporarily by creating
  `~/.claude/hooks-state/stop-triage/disabled`.

Codex delegation: use the plugin's native path, not a hand-rolled wrapper
(owner directive 2026-07-20, after a campaign spent building monitoring
machinery to replace a feedback channel that already existed):

- **Delegate through `codex:codex-rescue` (Agent tool) or the plugin's
  slash commands.** This is the same rule as "avoid writing custom Bash
  scripts" above, and it is the whole of the mechanism — the native path
  runs the job and returns its result, so there is nothing to poll.
- **Do not drive `codex-companion.mjs` directly** unless the native path
  genuinely cannot express the task. It is a fallback, not the default.
  If you must use it: it runs in the **foreground and blocks** by default
  (`--background` is opt-in), so **never pipe its output** — a `| head`
  closes the pipe, the process takes SIGPIPE, and the job silently
  detaches. That severed channel is what forces timer-based guessing.
- **Never infer a job's state from a timeout.** Timers cannot separate
  "thinking", "building", and "wedged". If you are reduced to inferring,
  the feedback channel is broken — fix that instead of tuning the timer.
  When you must observe a detached job, `stat` file mtimes in the
  worktree; the job log is not a progress signal (it records shell
  commands only and can sit many minutes stale while work lands).
- Liaison subagents were tried for this and are **not** recommended: two
  of four either raised a false alarm or stayed silent through a full
  wedge. They isolate context cost, which is real, but they cannot ask a
  running Codex job anything the orchestrator cannot — there is no
  mid-turn inbox.
- Judgment lives at the orchestrator's gate: independent verification
  before push/PR, and the orchestrator commits salvaged work itself.
  Delegated jobs comply poorly with "commit early" no matter how the
  brief is worded, so treat uncommitted work as the orchestrator's to
  rescue, verify, and commit.
- Review flow (no nesting): a Codex job never runs the structured
  `autoreview` helper — nested reviewer invocation is prohibited by the
  skill's contract and blocked by its session guard. The job closes with
  a manual repo-grounded audit; the orchestrating session owns the
  structured second-model review before push (which also gives
  cross-model separation: a Claude reviewer over Codex-written code).

## Pre-Launch Status

**This project has NOT launched yet.** There are no production users or data to migrate.

- **Breaking changes are preferred.** Choose clean replacements over compatibility layers.
- **No backwards compatibility code.** Delete old behavior instead of deprecating it.
- **No migration shims.** Change the schema or API directly.
- **No feature flags for legacy behavior.** Remove the old path entirely.

If you find yourself writing compatibility code, stop and make the breaking change instead.

## Working Set

- Start with `README.md`, `ARCHITECTURE.md`, `docs/README.md`, and
  `docs/private/plans/README.md`.
- Use the active plan owner for the slice you are touching. Prefer active
  plans over archived history.
- Treat the current git worktree plus the owning active plan as progress
  state. Resume `in_progress` work before starting a new roadmap item.
- Checkpoint plan state before stopping, handing off, or any likely context
  loss.
- Load one roadmap item at a time plus only the immediately relevant code,
  tests, and docs.

### Routing

Keep routing detail in the local indexes, not in this bootstrap file:

- Private control-plane routing: `docs/private/README.md`
- Active implementation order and plan promotion: `docs/private/plans/README.md`
- Architecture, trust boundaries, runtime, sandbox, and storage seams: `docs/private/architecture/README.md`
- Operating, CI, release, deploy, install, local-dev, and node runbooks: `docs/private/operating/README.md`
- Adapter-family routing: `docs/private/adapters/README.md`
- Public docs site work: `.agents/skills/docs/SKILL.md` and `docs/README.md`

Choose the active plan owner before editing. If no plan owns a concrete
implementation topic, promote exactly one owner plan and update the roadmap map
when the work came from that roadmap. Keep completed-plan evidence in the owning
plan or proof directory, not here.

### Workspace layout

The repo is a Rust workspace + npm monorepo. Names overlap — know which you mean:

| Name | Path | What it is |
| --- | --- | --- |
| `nimbus` (facade crate) | `crates/nimbus/` | Re-exports public types for embedders |
| `nimbus-adapters` | `crates/nimbus-adapters/` | Optional adapter-family aggregation crate |
| `nimbus-auth` | `crates/nimbus-auth/` | Shared auth and identity primitives |
| `nimbus-bin` | `crates/nimbus-bin/` | CLI binary entry point |
| `nimbus-blob` | `crates/nimbus-blob/` | Content-addressed byte plane (`BlobStore`, Seam A) |
| `nimbus-core` | `crates/nimbus-core/` | Shared types and validation (zero I/O) |
| `nimbus-engine` | `crates/nimbus-engine/` | Central coordinator (`Engine`) |
| `nimbus-fs` | `crates/nimbus-fs/` | In-process isolate/WASI filesystem: mount table, `FsCaps`, backends (Seam C) |
| `nimbus-node` | `crates/nimbus-node/` | Host-local workload reconciliation and systemd integration |
| `nimbus-runtime` | `crates/nimbus-runtime/` | V8 execution (zero workspace deps) |
| `nimbus-s3` | `crates/nimbus-s3/` | S3 wire surface over the blob/metadata planes (Seam D) |
| `nimbus-sandbox` | `crates/nimbus-sandbox/` | Generic sandbox and isolation seam |
| `nimbus-server` | `crates/nimbus-server/` | HTTP/WebSocket transport |
| `nimbus-services` | `crates/nimbus-services/` | Service, sandbox, and session resource manager |
| `nimbus-storage` | `crates/nimbus-storage/` | Persistence layer |
| `nimbus-tenant` | `crates/nimbus-tenant/` | Tenant policy and workload admission decisions |
| `nimbus-testing` | `crates/nimbus-testing/` | Shared test fixtures and deterministic harness helpers |
| `nimbus` (JS SDK) | `packages/nimbus/` | Nimbus-native JavaScript SDK |
| `convex` (JS compat) | `packages/convex/` | Convex compatibility package |
| `@nimbus/codegen` | `packages/codegen/` | Code generation tool |

### Rust target layout

- Reserve `examples/` for user-facing example programs.
- Put internal benchmark or evaluation runners under `benches/` with explicit
  custom-harness targets when they are driven through `cargo bench`.
- Keep integration tests in `tests/` and support helpers beside the owning
  crate unless they are shared widely enough to justify `nimbus-testing`.

### Modularity thresholds

- Files under 1,500 lines are usually acceptable when they keep one coherent
  ownership story.
- Files from 1,500 through 1,999 lines need an explicit justification in the
  owning active plan if they remain unsplit.
- Files at 2,000 lines or above must be decomposed or documented as a strong
  ownership-based exception.
- Do not split files or lines mechanically. Group like concepts together,
  keep composition roots thin, and prefer clearer boundaries over smaller raw
  numbers.
- Once a file becomes a composition root, keep new logic in concept-owned
  children instead of rebuilding inline switchboards there.
- Prefer concept-owned names such as `bootstrap.rs`, `provider.rs`, `read.rs`,
  `write.rs`, or `state.rs` over `helpers.rs`, `common.rs`, `misc.rs`, or
  `utils.rs` unless ownership is truly shared and obvious.

## Execution Quality

This project targets enterprise-grade code. Every agent working here must
meet this bar — not "good enough," not "as a first pass," not "can be
improved later."

- **Read before edit.** Read the file, its tests, and its callers before
  changing it. Do not edit files you have not read in this session.
- **Fix root causes.** When a test fails or a warning appears, fix the
  underlying issue. Do not delete tests, weaken assertions, suppress
  warnings, or change expected values to match wrong output.
- **No deferred work inside completion gates.** If a plan's completion gate
  says to handle N cases, handle all N. Do not implement a subset and leave
  TODOs for the rest.
- **Tests verify behavior, not compilation.** Every test must assert a
  specific outcome. A test that only checks "it didn't panic" is not a
  test. Cover happy path, edge cases, and error cases.
- **Verification is evidence.** "Tests pass" without naming the test count
  or showing the output is not verification. Record what you ran and what it
  produced.
- **No lazy-exit phrases.** Do not use "good enough for now," "left as an
  exercise," "out of scope" (for in-scope work), "as a first pass," or
  "can be improved later" to justify incomplete work.

## Common Repo Gotchas

### Commands that report success without running

Three ways a verification command lies about what it measured. All three
have shipped broken work in this repo.

- **A pipe replaces the exit code.** `cargo nextest run … | tail -3 &&
  git commit` gates the commit on `tail`, not the tests, so a compile
  failure still commits. Use `set -o pipefail`, or split the command and
  check the real status. The same trap applies to `make X | tail`.
- **`find -newermt "-N minutes"` does not work here.** This machine's
  `find` is `bfs`, which rejects relative timestamps: it errors and
  prints nothing, which reads identically to "no files changed". Any
  activity check built on it is a permanent false zero. Use
  `-newer <reference-file>` or `stat -f %m`.
- **A skipped provider test is not a passing one.** Lanes without a live
  server (MySQL locally) skip silently and report green. Say which lanes
  actually ran; CI's service containers are the evidence for the rest.

Related: when a fail-before check reverts a fix, restore from a **saved
copy**, never `git checkout -- <file>`. Checkout restores from HEAD and
silently destroys any other uncommitted work in that file.

### Test hangs: bound blocking waits, bound verification commands

A test helper that parks a production thread — a blocking fault injector, a
blocking observer — must bound its release wait and fail loudly on timeout.
With an unbounded wait, any early exit from the test body (a load-induced
timeout panic before the release runs) leaves the thread parked, and the
`#[tokio::test]` wrapper then blocks forever in `drop_in_place<Runtime>`
waiting on `spawn_blocking` tasks that can never finish. The real failure is
swallowed and the process hangs at ~zero CPU.

`.config/nextest.toml` terminates slow tests (45s, 3 strikes), so nextest runs
and CI are protected; bare `cargo test` has no timeout and is where this bites.
Wrap long verification in `timeout <secs> ...`, and require the same of
delegated jobs. Diagnose a suspected hang with `sample <pid>` before killing
it: hours of elapsed time with near-zero accumulated CPU is a hard block, not
slow work, and the stack names the culprit frame.

### GitHub CLI auth under sandbox

If `gh` reports an invalid token or auth failure inside the sandbox, retry the
same GitHub CLI operation with elevated permissions before treating credentials
as broken. Record a real credential blocker only after the elevated command
fails too.

### Crate dependency rules

These are architecture invariants — do not violate them:

- **`nimbus-core` has zero I/O.** Types and validation only. No file reads, no network calls.
- **`nimbus-runtime` has zero workspace dependencies.** It defines the V8 surface and `HostBridge` trait. All Nimbus-specific integration lives in the server's bridge implementation.

### Mutation path

Every mutation — HTTP, WebSocket, scheduler, or V8 runtime — flows through an
engine-owned commit path. There are exactly three, and all three are engine
owned: the **queued journal path** (client mutations batched and committed in
sequence order by the per-tenant committer), the **direct path**
(`apply_mutation_with_mode*`), and the **execution-unit path**
(`MutationExecutionUnit`, used by runtime mutations so one function
invocation commits as a single transaction). There is no separate code path.
Do not create one.

Name all three when auditing. A change that reasons about only
`apply_mutation_with_mode*` will miss defects living in the other two — that
is not hypothetical: ambiguous-outcome handling silently diverged across
these paths, and only the direct route escalated to crash-and-replay.

### Storage atomicity

Document write, supporting index effects, and commit log append must remain a
single storage transaction. Never commit a document without its index entries.
Never append a commit without the document write.

### Runtime bundles

A runtime bundle that carries a recorded SHA-256 (its provenance hash) is re-hashed and verified against that hash before every invocation; a tampered or stale bundle is rejected. A path-backed bundle loaded without recorded provenance has no expected hash and is admitted on filesystem trust alone (see `verify_integrity`). Runtime host operations (`ctx.db.insert(...)` etc.) go through the same `Engine` path as direct HTTP calls — no bypass.

### Schema is optional

A table without a schema accepts any document. Setting a schema adds constraints but never removes the ability to write.

### JavaScript package naming

`packages/nimbus` is the JS SDK. `crates/nimbus` is the Rust facade. When discussing "nimbus" clarify which.
- `packages/nimbus` is the canonical JS implementation. Keep `packages/convex`
  as a compatibility wrapper via thin adapters, aliases, or re-exports when
  behavior matches instead of copy-forwarding parallel logic.

## Verification Commands

- **Format check:** `cargo fmt --all --check`
- **Workspace check:** `make check`
- **Rust test suite:** `make test`
- **Rust lint:** `make clippy`
- **Dependency audit:** `make deny`
- **Third-party attribution gate (G4):** `make verify-third-party-attribution` (unit tests: `make verify-third-party-attribution-helper`)
- **Harness focused lanes:** `make verify-harness` or `make verify-harness SURFACE=runtime`
- **Harness nightly lanes:** `make verify-harness-nightly` or `make verify-harness-nightly SURFACE=server`
- **Harness repro:** `make verify-harness-repro SURFACE=runtime MODE=pr CASE=<case-id>`
- **JS typecheck:** `npm run typecheck`
- **JS tests:** `npm run test`
- **JS build:** `npm run build`
- **JS capability-boundary lint:** `npm run lint:capability-boundary`
- **Docs gates:** `bash scripts/check-docs.sh` and `bash scripts/verify-nimbus-docs-site.sh`
- **Required local CI gate:** `make ci` (alias for `make ci-required`; hosted CI still owns coverage upload and scheduled/manual Node compatibility evidence)

See `docs/private/operating/local-dev.md` for the build contract; Node is a dev
build dependency for any Rust target that touches `nimbus-server`.

Prefer the `make` entrypoints above for long-running workspace-wide verification:
they are wrapped with the repo's single-flight guard so an accidental duplicate
invocation exits quickly instead of starting another overlapping run. Use
direct `cargo test ...` or `cargo clippy ...` when you intentionally want a
focused crate-level or test-level command.

For focused ad hoc cargo commands, prefer serialized runs against the repo's
shared `target/` so later commands reuse the same artifacts. If Cargo
contention or a stale lock shows up, heal by waiting for the active Cargo
process to finish, or by stopping the genuinely stale/hung process and rerunning
on the shared target. Do not treat alternate artifact directories as the
default recovery path.

Run `cargo fmt --all --check` and `make clippy` before opening a PR. For
PR-ready code changes, prefer `make ci` locally when feasible; it covers fmt,
clippy, deny, runtime/workspace/doc Rust tests, the required verification
harness, JS build/test, and proof helpers.

Hosted CI is broader and remains the merge source of truth. It also gates
runtime pointer-compression, the Bun runtime contract, external-provider tests,
Node/FaaS compatibility, node D-Bus integration, JS capability-boundary lint,
and separate coverage / scheduled Node-compatibility evidence workflows.
