# DUA2 rusty_v8 Alignment

status: done
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
rusty_v8_branch: nimbus/v149.2.0
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA2 is done. The DUA branch is pushed and tracked by
   draft PR #11. The source candidate lives in
   `/Users/jack/src/github.com/nimbus/rusty_v8` on `nimbus/v149.2.0`.
2. **Input baseline.** Upstream Deno `v2.8.1` expects `v8 = "149.2.0"`;
   Nimbus previously consumed `nimbus/rusty_v8@v149.0.0-nimbus.1`.
3. **Disposition table.** Every carried `rusty_v8` fork commit has an initial
   disposition below before Deno replay proceeds.
4. **Implementation evidence.** A `v149.2.0`-based candidate branch was created
   and the Nimbus locker stack replayed without conflicts.
5. **Focused verification.** `cargo fmt --all --check` passes. Archive-based
   `cargo check` / `cargo test locker` fail because the prebuilt
   `v149.2.0-nimbus.1` archive is not published yet. Escalated source builds
   pass the locker runtime and compile-fail safety tests.
6. **Broad verification.** DUA2 does not claim broad Node compatibility. DUA6
   owns post-repin Node compatibility evidence.
7. **Residual risks.** DUA5 still owns Nimbus repin and release-artifact
   consumption. The `v149.2.0-nimbus.1` tag proves the locker source
   candidate; a follow-up branch commit corrected release metadata and may
   require a superseding tag if DUA5 needs that metadata inside the immutable
   artifact source.

## Row And Status

DUA2 is done because `denoland/deno@v2.8.1` pins
`v8 = { version = "149.2.0", default-features = false, features = ["simdutf"] }`.
That makes a matching `rusty_v8` substrate a prerequisite for rebuilding the
Deno fork. DUA0 is now closed because draft PR #11 records the control surface.
This is the lockstep compatible runtime stack decision: align
`rusty_v8@v149.2.0` before Deno replay, while preserving Nimbus `Locker` and
`UnenteredIsolate` safety.

Current candidate:

| Field | Value |
| --- | --- |
| `rusty_v8` worktree | `/Users/jack/src/github.com/nimbus/rusty_v8` |
| Candidate branch | `nimbus/v149.2.0` |
| Upstream base | `denoland/rusty_v8@v149.2.0` |
| Upstream base SHA | `5d0e31e` |
| Candidate branch head | `d247474e613e8050fef0348cf11f5e01bd94cdfd` |
| Candidate tag | `v149.2.0-nimbus.1` |
| Candidate tag SHA | `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` |
| Candidate remote | pushed to `origin nimbus/v149.2.0` and `origin v149.2.0-nimbus.1` |

## Input Baseline

Previous Nimbus pin:

| Repo | Tag | SHA | Role |
| --- | --- | --- | --- |
| `nimbus/rusty_v8` | `v149.0.0-nimbus.1` | `9b77553883f1117ab3df62709b8673b803ed721b` | Current Nimbus `v8` substrate before DUA2 |

Upstream target:

| Repo | Tag | SHA | Evidence |
| --- | --- | --- | --- |
| `denoland/rusty_v8` | `v149.2.0` | `5d0e31e` | Local tag and `upstream/main` |
| `denoland/deno` | `v2.8.1` | local upstream tag | `Cargo.toml` pins `v8 = "149.2.0"` |

Direct bump rejection:

```console
git -C /Users/jack/src/github.com/nimbus/rusty_v8 grep -n "UnenteredIsolate\\|pub struct Locker\\|v8__Locker\\|new_unentered" v149.2.0 -- src tests
```

Observed: no Nimbus `Locker` / `UnenteredIsolate` Rust API or safety tests are
present on raw upstream `v149.2.0`. Directly consuming upstream `v149.2.0`
would drop required Nimbus locker safety, so DUA2 must produce a Nimbus fork
candidate or record a real source/build/safety/runtime blocker.

## Disposition Table

| Commit | Subject | Disposition | Reason |
| --- | --- | --- | --- |
| `665f3f1` | `fix: keep isolate annex alive during teardown (#1978)` | `upstream-replaced` | Upstream `v149.2.0` already includes the same upstream PR as `e5abf2b`; replaying the older local SHA would duplicate upstream behavior. |
| `95d2a5b` | `Add v8::Locker and v8::UnenteredIsolate` | `nimbus-embedding-specific` | Nimbus needs a Send-able unentered isolate plus RAII locker API for its embedded runtime ownership model. |
| `27099cb` | `fix(locker): add panic safety and improve documentation` | `nimbus-embedding-specific` | Strengthens the Nimbus locker wrapper's panic/drop safety and documents unsafe boundaries. |
| `f9ae687` | `feat(locker): add compile-time safety tests and unsafe documentation` | `nimbus-embedding-specific` | Adds trybuild tests that prove double-borrow, outliving scope, and non-Send locker constraints. |
| `944b24a` | `fix(locker): correct Enter/Exit ordering in Locker` | `nimbus-embedding-specific` | Corrects locker enter/exit lifecycle for the wrapper. |
| `65cc3e9` | `fix(locker): initialize HandleScope annex in Locker scope` | `nimbus-embedding-specific` | Preserves handle-scope annex setup when entering through `Locker`. |
| `e698466` | `fix(locker): reset weak handles during isolate teardown` | `nimbus-embedding-specific` | Prevents retained weak handles from surviving isolate teardown in the embedded lifecycle. |
| `ec9267b` | `fix(locker): clear active weak handles before isolate teardown` | `nimbus-embedding-specific` | Completes the weak-handle cleanup path before teardown. |
| `6a59303` | `test(locker): harden weak teardown release` | `nimbus-embedding-specific` | Adds regression coverage for weak-handle teardown under the locker lifecycle. |
| `69f42e2` | `test: bless compile_fail stderr for Rust 1.91.0` | `nimbus-embedding-specific` | Keeps the Nimbus locker trybuild evidence current with the toolchain. |
| `2e885b5` | `rename: agentstation -> nimbus` | `nimbus-embedding-specific` | Keeps fork branding/release metadata under the Nimbus fork. |
| `e1c1895` | `style: apply rustfmt to nimbus v149 port` | `nimbus-embedding-specific` | Formatting for the replayed Nimbus fork delta. |
| `9b77553` | `build: restore nimbus release contract` | `nimbus-embedding-specific` | Restores Nimbus release URLs/workflow metadata for the fork artifact contract. |
| `d247474` | `Fix Nimbus v149.2 CI release metadata` | `nimbus-embedding-specific` | Corrects the release branch trigger from the stale `nimbus/v149.0.0` branch, restores Rust/C++ format checks, documents the narrower Nimbus prebuilt surface, and makes `RUSTY_V8_VERSION` invalidate Cargo's build-script cache. This commit is on the branch head after tag `.1`; DUA5 must decide whether artifact consumption needs a superseding tag. |

No-benefit hold rejection: lack of immediate Node fixture gain is not a valid
hold reason. The Deno `v2.8.1` base requires the `v149.2.0` V8 line, and
upstream currency is part of the runtime trust posture. A hold is valid only
for exact build, safety, or runtime verification blockers.
No immediate Node fixture gain is not a valid hold reason.

The `v149.2.0-nimbus.1` candidate preserves `Locker` and `UnenteredIsolate`
source APIs and safety tests on top of upstream `v149.2.0`.

## Implementation Evidence

Commands run:

```console
git -C /Users/jack/src/github.com/nimbus/rusty_v8 switch -c nimbus/v149.2.0 v149.2.0
git -C /Users/jack/src/github.com/nimbus/rusty_v8 cherry-pick 95d2a5b 27099cb f9ae687 944b24a 65cc3e9 e698466 ec9267b 6a59303 69f42e2 2e885b5 e1c1895 9b77553
git -C /Users/jack/src/github.com/nimbus/rusty_v8 log --oneline --decorate --max-count=18
git -C /Users/jack/src/github.com/nimbus/rusty_v8 diff --stat v149.2.0..HEAD
git -C /Users/jack/src/github.com/nimbus/rusty_v8 tag -a v149.2.0-nimbus.1 -m "Nimbus rusty_v8 v149.2.0-nimbus.1"
git -C /Users/jack/src/github.com/nimbus/rusty_v8 push origin nimbus/v149.2.0 v149.2.0-nimbus.1
git -C /Users/jack/src/github.com/nimbus/rusty_v8 commit -m "Fix Nimbus v149.2 CI release metadata"
git -C /Users/jack/src/github.com/nimbus/rusty_v8 push origin nimbus/v149.2.0
gh run list --repo nimbus/rusty_v8 --branch nimbus/v149.2.0 --limit 10
```

Observed:

- Candidate branch `nimbus/v149.2.0` was created from raw upstream
  `v149.2.0`.
- The replayed Nimbus locker stack cherry-picked without conflicts.
- Candidate tag head is `ce66631`.
- Candidate tag `v149.2.0-nimbus.1` was created and pushed.
- Candidate branch head is `d247474`, which fixes the stale branch trigger and
  documentation discovered during review of `ce66631`.
- GitHub branch CI started for `d247474` as run `26767133504`; the job graph
  includes the restored `Check Rust formatting` and `Check C++ formatting`
  steps. On the `release x86_64-unknown-linux-gnu` job, both formatting steps
  completed successfully; the source build/test steps were still in progress at
  this checkpoint.
- The candidate diff from `v149.2.0` restores `Locker`,
  `UnenteredIsolate`, raw `v8::Locker` bindings, trybuild safety tests, weak
  teardown tests, Nimbus fork metadata, and release-contract wiring.

## Focused Verification

Commands run:

```console
cargo fmt --all --check
cargo check
cargo test locker -- --nocapture
V8_FROM_SOURCE=1 cargo test locker -- --nocapture
V8_FROM_SOURCE=1 cargo test --test test_locker --test test_ui -- --nocapture
git diff --check HEAD~1..HEAD
cargo fmt --check
```

Observed:

- `cargo fmt --all --check`: pass.
- `cargo check`: failed before compiling source because the Nimbus prebuilt
  archive URL `https://github.com/nimbus/rusty_v8/releases/download/v149.2.0-nimbus.1/src_binding_release_aarch64-apple-darwin.rs`
  returned `404 Not Found`. This is expected before the candidate tag/release
  artifact exists and is not a source-safety failure.
- `cargo test locker -- --nocapture`: failed for the same missing prebuilt
  archive.
- Non-escalated `V8_FROM_SOURCE=1 cargo test locker -- --nocapture`: failed
  while downloading GN from CIPD with sandboxed DNS error
  `socket.gaierror: [Errno 8] nodename nor servname provided, or not known`.
- Escalated `V8_FROM_SOURCE=1 cargo test locker -- --nocapture`: pass after
  a `20m 28s` source build. It ran `scope::raw::locker_size_matches_v8`
  (`1 passed`) and the filtered locker integration tests in
  `tests/test_locker.rs` (`7 passed`, `2 filtered out`).
- `V8_FROM_SOURCE=1 cargo test --test test_locker --test test_ui -- --nocapture`:
  pass. `tests/test_locker.rs` reported `9 passed, 0 failed`; `tests/test_ui.rs`
  reported `1 passed, 0 failed` and all 15 compile-fail fixtures passed,
  including `locker_double_borrow`, `locker_not_send`, and
  `locker_scope_outlives`.
- Post-review release metadata fix `d247474`: `git diff --check HEAD~1..HEAD`
  passes and `cargo fmt --check` passes.

## Broad Verification

DUA2 does not claim broad Node compatibility or public runtime compatibility
movement. DUA6 must rerun broad Node compatibility evidence after DUA3-DUA5
produce published fork tags and Nimbus consumes them.

## Evidence Links

- `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua1-deno-overlap-audit.md`
- `/Users/jack/src/github.com/nimbus/rusty_v8`
- `/Users/jack/src/github.com/denoland/deno`

## Residual Risks

- The `v149.2.0-nimbus.1` release artifact may not exist until the pushed tag's
  release workflow publishes it. DUA5 must verify immutable artifact
  consumption before Nimbus repin.
- The branch head after tag `.1` corrects CI/docs/build-script metadata. If
  DUA5 needs those metadata fixes in the immutable source consumed by Nimbus,
  create and verify a superseding `v149.2.0-nimbus.*` tag from `d247474` or
  later.
