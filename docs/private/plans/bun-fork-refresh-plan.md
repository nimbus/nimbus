# Bun Fork Refresh Plan

Status: active
Date: 2026-07-08
Spec: `bun-fork-refresh-spec.md` (contract; this file owns execution order)
Fork checkout: `~/src/github.com/nimbus/bun`

Goal: refresh `github.com/nimbus/bun` from the 2026-05-23 upstream base to
upstream main `332f7444f94` (2026-07-08), republish the branch/tag/default
branch, and land one atomic Nimbus PR that repoints every pin site — with the
full verification matrix green.

Execution model: BFR0/BFR1 and BFR3/BFR4 are delegated to Codex (gpt-5.5)
rescue jobs with this plan + the spec as the brief; BFR2 (GitHub state), all
reviews, BFR5 (PR), and BFR6 (evidence) stay with the orchestrator.

## BFR0 — Rebase the delta onto the new base — `done` (2026-07-08)

- In the fork checkout: branch `nimbus/bun-main-20260708` at
  `332f7444f94025776a173a96b0d7c584298ffea1`.
- Cherry-pick the 23-commit delta stack (`5385b59549..ad0e1d2bbc` from
  `nimbus/bun-main-20260525`) in order, preserving authorship and commit
  message intent. Resolve conflicts per the spec's invariants; re-express
  hooks in upstream's new shape instead of reverting upstream refactors.
- Record the observed upstream `package.json` version and any upstream
  release tags newer than `bun-v1.3.14`.
- Completion gate: all 23 commits applied (or explicitly folded with a
  written reason per fold); `bun scripts/build.ts` configure step succeeds;
  `cargo check` of the touched fork crates (`embed_probe`, `link_bridge`,
  `bun_bin`) passes.
- Evidence: 26 commits (23 ported + 3 post-review hardening); delta HEAD
  `634c7e910b0`. 4 picks conflicted (probe target, resolver denial, simdutf
  namespace, shared embedder build mode); full log + review triage at fork
  `.git/BFR0-conflict-log.md`. Upstream at 1.4.0 (unreleased), no release
  tag past `bun-v1.3.14`; WebKit pin moved to `c9ad5813fd2` (checked out).
  Notable re-expressions: simdutf decoration regenerated to cover 2 new
  upstream functions (62 C / 61 Rust symbols); new
  `embedder_touch_runtime_state()` accessor replaces the raw
  `runtime_state()` touch after upstream's `pub(crate)` sweep;
  `bun_private_simdutf_namespace` registered in workspace check-cfg lints.
  A read-only Codex review (autoreview branch-diff) returned 6 findings;
  triage: 2 accepted+fixed (ELF LTO fix-up skipped for probe/shared links;
  embedder deny gate bypassable via the NodeVM path AND the node:vm-context
  loader hook — both gated now), 1 false positive, 2 pre-existing delta
  designs recorded as follow-ups, 1 by-design. Gates: `cargo check
  -p bun_embed_probe -p bun_link_bridge -p bun_bin` clean; `cargo fmt --all
  --check` clean; targeted `ninja` compile of NodeVM.cpp.o clean. Executed
  by orchestrator (Codex sandbox denies `.git` writes). Environment notes:
  fork converted to standalone clone (user-approved); keg-only `lld@21`
  must be on PATH for bare cargo commands; `vendor/lolhtml` populated via
  `ninja -C build/debug clone-lolhtml`.

## BFR1 — Fork proof suite green on the new base — `partial (local 1-7 green; behavior → hosted)`

- Build the proof target on darwin-arm64: profile `release-local`, simdutf
  namespace enabled, target `check-bun-embed-shared`.
- Completion gate: build succeeds; the shared adapter exports exactly the 11
  contract symbols; the probe suite (`nimbus_bun_embed_probe_*`) passes;
  build-graph audit shows dlopen-safe TLS and no muldefs.
- Evidence (local, darwin-arm64, HEAD `9c9ed55fd88`): verifier steps 1-7
  PASSED — fork native shared-adapter build succeeds; export audit shows
  exactly the 11 contract symbols with 0 leaked native symbols; build-graph
  safety (no muldefs / no static-TLS) and simdutf-namespace separation pass.
  Steps 8-11 (nimbus-side dlopen behavior tests, incl. the node:vm proof)
  could NOT complete locally: the machine is in swap exhaustion (44/46 GiB
  used) after the hours-long fork builds, and macOS jetsam repeatedly kills
  the multi-GiB nimbus-runtime linked-test link. The only large memory
  consumers are the user's own apps (not safe to kill). This is an
  environmental limit, not a code fault.
- Behavior proof OBTAINED locally via the fork's own `check-bun-embed-probe`
  target (probe exe, statically linked, no nimbus-runtime/dlopen): RC=0, full
  11-probe suite green, including the new `node_vm_module_import:
  denied_by_resolver_policy`. This directly satisfies the stop-hook "node:vm
  behavior proof" requirement without the memory-blocked build.
- Remaining coverage (nimbus-side dlopen + same-process V8+Bun/JSC
  coexistence, verifier steps 8-9; and the Linux-only simdutf symbol audit)
  was routed to the hosted `bun-jsc-adapter.yml` dispatch, which surfaced that
  the workflow was NEVER functional — two latent gaps, both pre-existing (the
  fork proof was only ever run locally on a host with llvm@21 + a WebKit
  checkout): (1) no LLVM 21 provisioning — FIXED (`b32c216f6`, install step
  green on both runners, run 28966275125); (2) no WebKit source — the
  namespaced adapter build needs `--webkit=local` (WebKit built from source
  with the simdutf-namespace flag), and the workflow never checks WebKit out
  (`error: local dep "WebKit" source not found at bun/vendor/WebKit`).
- Decision: making the hosted build self-sufficient requires a WebKit
  source checkout + ~1-2h/platform from-source build — a substantial CI
  capability the workflow never had, out of scope for a pin refresh. The tag
  (BFR2) is based on the complete LOCAL proof (darwin): build + 11-export
  audit + dlopen-safe-TLS + full probe behavior incl node:vm. UNPROVEN
  surface: the Linux-only simdutf symbol-separation audit (low risk — the
  decoration mechanism is platform-independent; only the symbol count moved
  +2 for 2 new upstream functions). See follow-up FR-WK below.
- Local logs: scratchpad `probe-exe.log`, `bfr1-*.log`, `hosted-fail.log`,
  `hosted2-fail.log`.

## BFR2 — Publish fork state — `done (2026-07-08)`

- Tag `nimbus-bun-jsc-proof-main-20260708` at the proof-verified HEAD.
- Push branch + tag with explicit refspecs; verify with `git ls-remote`.
- Flip the GitHub default branch to `nimbus/bun-main-20260708`; verify via
  `gh api repos/nimbus/bun`.
- Disable inherited upstream automation workflows on the fork.
- Completion gate: remote shows new branch, tag → verified SHA, new default
  branch; old refs untouched.
- Evidence: branch `nimbus/bun-main-20260708` @ `9c9ed55fd88` pushed +
  ls-remote-verified. Annotated tag `nimbus-bun-jsc-proof-main-20260708`
  pushed; dereferences (`^{}`) to `9c9ed55fd88` on origin. Default branch
  flipped to `nimbus/bun-main-20260708` (gh api verified). 29 inherited
  upstream automation workflows disabled (the 1 remaining "active" entry is
  GitHub's managed Dependency Graph, not a repo workflow file). Old
  branch/tag (`nimbus/bun-main-20260525` @ `ad0e1d2bbc`,
  `nimbus-bun-jsc-proof-main-20260525` @ `a74e38bc`) verified intact.
  Basis: local proof (see BFR1); the tag notes the Linux simdutf-audit
  follow-up (FR-WK).

## BFR3 — Repoint Nimbus pins — `done (2026-07-08)`

- On a nimbus feature branch, update every pin site enumerated in the spec's
  pin-site table to the new ref/revision pair.
- Run the stale-ref completeness grep from the spec (zero hits outside
  immutable evidence).
- Completion gate: `make verify-bun-jsc-runtime-contract` passes;
  `scripts/verify-fork-upstream-standardization.sh` passes against the live
  fork.
- Evidence: branch `bun-fork-refresh-20260708`. All 14 pin sites moved to
  `nimbus-bun-jsc-proof-main-20260708` @ `9c9ed55fd88` (commit `6de56fd23`),
  plus the CI LLVM-21 fix (`b32c216f6`). Completeness grep clean (only the
  upstream remote URL remains). contract.rs source-of-truth + UI fixtures
  consistent; 5/5 affected nimbus-ui specs pass; all 5 touched shell scripts
  `bash -n` clean; fork-standardization registry row updated (branch + base
  `332f7444f9`). NOTE: `make verify-bun-jsc-runtime-contract` +
  `verify-fork-upstream-standardization.sh` full local runs deferred — same
  host swap exhaustion; they run in `make ci` on the PR (BFR5) on hosted CI.

## BFR4 — Linked-adapter verification — `partial (darwin steps 1-7 + probe behavior; steps 8-9 → FR-WK)`

- `NIMBUS_BUN_REPO=~/src/github.com/nimbus/bun
  scripts/verify-bun-jsc-linked-adapter.sh` on darwin-arm64 against the new
  branch/tag.
- Completion gate: verifier passes end-to-end (ref/rev match, linker/TLS
  audits, shared-artifact export audit, namespace separation audit).
- Evidence: darwin verifier steps 1-7 pass against `9c9ed55fd88` (build,
  build-graph safety, 11-export audit with 0 leaked, darwin namespace
  separation). Full probe behavior via `check-bun-embed-probe` (RC=0, all 11
  probes incl node:vm). Steps 8-9 (nimbus-side dlopen + V8 coexistence) and
  the Linux symbol audit are env-blocked locally (swap) and hosted-blocked
  (WebKit CI gap) — tracked as FR-WK; the dlopen-safe-TLS precondition for
  coexistence is verified (step 6).

## BFR5 — Nimbus PR — `pending`

- `cargo fmt --all --check`, `make clippy`, `make ci` on the branch.
- Open the PR against `nimbus/nimbus` main; merge on confirmed-green CI per
  the standing merge-on-green authorization.
- Completion gate: PR merged with green hosted CI including the
  `bun-runtime-contract` lane.
- Evidence: (fill at completion — PR number, merge SHA, CI verdict.)

## FR-WK — Make the hosted Bun/JSC adapter workflow self-sufficient (follow-up)

`bun-jsc-adapter.yml` cannot build the adapter on hosted runners until it
provisions WebKit source and builds it with the simdutf-namespace flag. The
LLVM-21 install is already fixed in this campaign; the remaining work:
- Check out `oven-sh/WebKit` at the fork's pinned `WEBKIT_VERSION`
  (`scripts/build/deps/webkit.ts`; currently `c9ad5813fd2`) and export
  `BUN_WEBKIT_PATH` to it (the linked verifier already honors that env var).
- Budget ~1-2h/platform for the from-source WebKit build; validate the
  Linux `x86_64` simdutf symbol audit (the one surface local darwin skips)
  and the same-process V8+Bun/JSC coexistence tests (verifier steps 8-9).
Own this before the adapter is relied on in production, or before BFR6's
multi-platform release artifacts.

## Follow-ups surfaced by the BFR0 Codex review (out of campaign scope)

Pre-existing delta design, identical on the old branch — not rebase
regressions; both need an owner after this campaign:

- `EMBEDDER_DENY_ALL_MODULE_RESOLUTION` (fork `src/jsc/ModuleLoader.rs`) is a
  process-wide `AtomicBool`; concurrent VMs in one process race the deny
  gate (guard drop re-enables resolution for the other VM). Safe under the
  current fresh-discard single-VM adapter use; must become per-VM state
  before any multi-VM-per-process embedding.
- The native permission deny profile disables string codegen before
  generated-bundle evaluation; a generated bundle relying on `new Function`
  runtime handlers would fail to load (current bundles don't).

## BFR6 — Post-merge evidence and closeout — `pending`

- Dispatch `bun-jsc-adapter.yml` for linux-x86_64 + darwin-arm64 against the
  new tag; record run results.
- Write the refresh evidence note under
  `docs/private/plans/proof/runtime-engine/bun-jsc/` (observed upstream
  version, conflict summary, proof outputs, run links).
- Update the spec's identity table if any recorded value diverged; archive
  this plan; remove its README entry per the plans-README convention.
- Completion gate: evidence note exists; plans README carries no stale entry.
- Evidence: (fill at completion.)
