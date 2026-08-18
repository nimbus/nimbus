# AVR11 Integrated Acceptance

Date: 2026-08-18
Status: PR #277 open. Hosted acceptance in progress.

## Result

AVR11 isolates tests that share the process-global network authority at the
canonical Rust test entry point. `make test` now runs three sequential lanes.
They contain the runtime tests, Nextest workspace tests, and Rust documentation
tests. Nimbus still rejects a second in-process authority. The test entry point
no longer needs an undocumented `--test-threads=1` option.

The candidate also removes dependency states that a current RustSec database
rejects. The changes are narrow local patches:

- `libsql 0.9.30` drops its unused Hyper HTTP/2 feature and the vulnerable
  `h2` 0.3 dependency line from RUSTSEC-2026-0258.
- `object_store 0.14.0` and `s3s 0.14.0` pin `crc-fast` at 1.6.0 so they do not
  select its yanked `spin` 0.10.0 dependency.
- `flume 0.12.0` and `lazy_static 1.5.0` replace only their yanked `spin`
  0.9.8 dependencies with 0.12.3.

The patched crates retain their upstream source trees and licenses. Normalized
manifest comparisons show only those dependency changes. `libsql` also gains
the license file from its recorded upstream source revision. Dependency
policy, attribution, lockfile, and workspace-hack checks pass.

The final minicloud matrix used an exact byte copy of the candidate source. It
passed 662 server tests, then passed the same nine applications and 37
assertions in serial and five-worker modes. The final checksum comparison
found zero differences across 43,652 source paths.

Review correction commits `e8fc63489` and `9f3fba2da` close all eight accepted
findings from the full and narrow reviews. The final correction export passed
the two real-app modes and all focused correction tests. The 12 corrected files
matched commit `9f3fba2da` byte-for-byte.

## Fail-Before Evidence

| Finding | Observed state |
| --- | --- |
| AVRF22 | An unrestricted `cargo test -p nimbus-server --lib` run reported 601 passed, 62 `DuplicateProcessComposition` failures, and 35 ignored tests. The same binary reported 663 passed, zero failed, and 35 ignored with `--test-threads=1`. |
| RustSec refresh | The first candidate `make ci` rejected vulnerable `h2` 0.3 and yanked `spin` 0.9.8 and 0.10.0 dependency lines. |
| Minicloud durability limits | The first exact-tree server run passed 660 of 662 tests. Two fresh-process durability matrices stopped at their fixed 20-second semantic-checkpoint deadline. A focused rerun reproduced the 20.057-second failure without Nextest contention. |

The two durability tests already bounded child startup, semantic checkpoint,
crash cut, and reap behavior. The minicloud host needed about 31 seconds for
the tests' many individually durable transitions. AVR11 raises only those two
test-harness deadlines to 60 seconds. It does not change a product timeout,
semantic checkpoint, crash cut, digest, or failure rule.

## Acceptance Ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR11.1 Confirm PR 2 merge. | Pass. | PR #276 merged as `b58ef8c35bb7478a9d31faa7b6d1d822d9215496` after all required checks passed. |
| AVR11.2 Reconcile current main. | Pass. | Reconciliation commit `ec6d2414c` retained the AVR8 recovery checkpoint after the PR #276 merge. |
| AVR11.3 Isolate the canonical Rust entry point. | Pass. | `make test` composes the runtime, Nextest workspace, and documentation-test lanes. The direct duplicate-authority rejection remains green. |
| AVR11.4 Run local and minicloud matrices. | Pass. | Local and minicloud server and application matrices are green. Both hosts passed 9/9 applications and 37/37 anchors in serial and five-worker modes with exact cleanup and matching source hashes. |
| AVR11.5 Run repository gates. | Pass. | Final-tree `make ci` exited zero after format, Clippy, deny, Rust, harness, JavaScript, and proof-helper gates. |
| AVR11.6 Commit and freeze. | Pass. | Candidate commit `056c243bced3798fa34c09e2851728afe5ca45f6` contains the complete acceptance-green AVR8-AVR11 tree. |
| AVR11.7 Run one Sol review. | Pass. | The full Sol/xhigh/fast review scored 0.98 and produced six accepted findings. The one narrow correction review scored 0.96 and produced two accepted closure findings. Commits `e8fc63489` and `9f3fba2da` close all eight. No further review is due. |
| AVR11.8 Open PR 3. | Pass. | Implementation PR [#277](https://github.com/nimbus/nimbus/pull/277) opened from candidate checkpoint `f81c6cb7a`. |
| AVR11.9 Resolve hosted failures. | In progress. | Hosted acceptance is active on PR #277. |
| AVR11.10 Merge with owner authority. | Pending. | The owner has not yet authorized this future merge. |

## Behavioral and Repository Evidence

| Command or proof | Result |
| --- | --- |
| Focused final durability tests, local | Pass. 2/2 in 13.118 and 13.508 seconds. |
| Focused final durability tests, minicloud | Pass. 2/2 in 30.975 and 31.579 seconds. |
| `cargo nextest run -p nimbus-server --lib --test-threads=4`, minicloud | Pass. 662/662, with 35 documented skips, in 476.092 seconds. |
| `make test`, local | Pass. Runtime 517 passed with 134 ignored; workspace 7,419/7,419 passed with 107 skipped; all documentation tests passed. |
| `make ci`, local | Pass on the correction tree. Runtime 517 passed with 134 ignored; workspace 7,419 passed with 107 skipped; required harnesses, JavaScript checks, and proof helpers passed. |
| `make examples-verify`, local serial | Pass on the correction tree. 9/9 applications and 37/37 anchors in 50,654 ms; cleanup passed; source matched. |
| `make examples-verify`, local five-worker | Pass on the correction tree. 9/9 applications and 37/37 anchors in 31,837 ms; cleanup passed; source matched. |
| Focused `nimbus-blob` tests | Pass. 248/248. |
| Focused `nimbus-s3` tests | Pass. 20/20. |
| Focused `nimbus-storage` tests | Pass. 448/448 with two documented fixture skips. |
| Cargo deny, attribution, lock, and workspace-hack drift checks | Pass. No rejected advisory, yanked package, attribution gap, or generated-manifest drift remains. |
| Full AVR verifier and self-test | Pass. AVRC01-AVRC24 are 24/24; mutation self-test is 24/24. |
| Both documentation gates and site build | Pass. 109 pages are link-clean, 17/17 site conditions are green, and the build emits 110 HTML pages. |
| Technical-writing lint | Pass. Changed private Markdown has zero diagnostics. The two changed `examples/README.md` lines add no diagnostics to its 10-line AVR10 baseline. |
| `cargo fmt --all --check` and `git diff --check` | Pass. |

## Review Dispositions

The full candidate review used GPT-5.6 Sol with xhigh reasoning and the fast
service tier. It reviewed two bounded chunks and reported six findings with
0.98 overall confidence. The implementation owner accepted all six findings.

| Priority | Finding | Disposition |
| --- | --- | --- |
| P2 | Git source snapshots can miss `assume-unchanged` and `skip-worktree` bytes. | Accepted. Reject every index-hidden path before source proof. Add a real Git regression for both flags. |
| P2 | Benchmark summaries and JUnit can contradict their referenced report. | Accepted. Derive every report-owned field again and require exact canonical JUnit. |
| P2 | A started case without a record becomes `not-run`. | Accepted. Project it as a failed incomplete case and reserve `not-run` for unclaimed cases. |
| P2 | A signaled worker can claim or start a later case. | Accepted. Publish a stop barrier, check it around claims, and make a signal terminal between cases. |
| P3 | The documentation test hard-codes the word `nine`. | Accepted. Derive the numeric case count from the manifest. |
| P2 | The website declares Node.js 22.12 while locked Undici requires 22.19. | Accepted. Raise the website package and lockfile floor to Node.js 22.19. |

Commit `e8fc63489` applied those corrections. The one permitted narrow review
used GPT-5.6 Sol with xhigh reasoning and the fast service tier. It scored 0.96
and found two incomplete closures. The implementation owner accepted both.

| Priority | Narrow-review finding | Disposition |
| --- | --- | --- |
| P2 | A benchmark wall interval can shift away from its canonical report. | Accepted. Require the sample interval to contain the report interval. Test both the late-start and early-completion boundaries. |
| P3 | The manifest-derived count regex accepts `19` when the count is `9`. | Accepted. Add numeric word boundaries and a larger-count rejection test. |

Commit `9f3fba2da` closes both findings. The focused and real-app proof rows below
use that final tree. The plan permits no second correction review, and none ran.

The review helper made two pre-model stops. Automatic selection did not choose
Codex, and the explicit Sol invocation rejected three upstream binary test
fixtures. Neither stop contacted a reviewer. The accepted review used a
synthetic parent with pristine crates.io source. Its reviewed commit
`f9382ab4f8bea44f48250e23d1d4969488217a05` has the exact tree of owner commit
`056c243bc`. The omitted fixtures are byte-identical to crates.io:

- `snapshot.snap`: `38a7a4f42b2da722093324657491246e04946ea24635a37483a1543561f392a7`.
- `template.db`: `00d4b935fa63507872b5f7973e423ca830effe2bddcd350555b53937135920b8`.
- `test.db`: `286f27ba3ba35005c54d44747cb79f9cb9d19956d51479a01641190e5619b6e8`.

Warnings emitted from unchanged upstream source in locally patched crates are
advisory. Nimbus code remains subject to the repository's deny-warnings Clippy
gate, which passed.

## Correction Evidence

| Accepted finding | Correction proof |
| --- | --- |
| Git index-hidden source bytes | Workspace behavior 10/10. A real temporary Git repository rejects modified paths marked `assume-unchanged` or `skip-worktree`. |
| Benchmark evidence can contradict the report | Evaluator 6/6. The validator derives report-owned fields, requires exact canonical JUnit, and requires the wall interval to contain the report interval. It rejects altered fields, JUnit, duration, start, and completion values. |
| Started case becomes `not-run` | Report behavior 9/9. A start record without a terminal record projects a failed case; only an unclaimed case projects `not-run`. |
| Signaled worker starts a later case | Scheduler 2/2 and fault cuts 7/7. The stop barrier prevents another claim or start while active cases drain. |
| Documentation test hard-codes `nine` | Documentation behavior 6/6. The test derives the numeric count from the manifest and rejects a larger count with the same final digit. |
| Website Node floor is too low | Package and lock root require Node.js 22.19 or later. The site builds 110 pages under Node.js 22.23.1 and Node.js 24.16.0. |

The complete affected ladder passed lifetime behavior 12/12 and product
lifetime behavior 1/1. ShellCheck and both documentation gates passed. The AVR
verifier and its mutation suite each passed 24/24. The local application runs
used one exact binary with SHA-256
`dabc5f44930f039f2bc8a4d0ef38e171a7bd1fb5a855f57464b3978b3d4ab50f`.

## Local Application Evidence

| Field | Serial | Five workers |
| --- | --- | --- |
| Report | `target/examples-verify-results/nimbus-examples-verify.mecldq-7798b1856418/report.json` | `target/examples-verify-results/nimbus-examples-verify.hrt5ka-7e6073c85fbd/report.json` |
| Result | 9/9 applications; 37/37 anchors; 50,654 ms | 9/9 applications; 37/37 anchors; 31,837 ms |
| Cleanup | Passed | Passed |
| Binary | `nimbus 0.1.45`; SHA-256 `dabc5f44930f039f2bc8a4d0ef38e171a7bd1fb5a855f57464b3978b3d4ab50f` | Same |
| Manifest | Schema 1; SHA-256 `9fd462b34d03d8af214f98aff26335636d7e89ee9af0221aa413bfac3c1c4a77` | Same |
| Source evidence | Before and after SHA-256 `ebc17fd06c3d0b277a3574526048434d565c4501e0d8050bebfd00cd6fadcf08` | Same |
| Runtime | Node.js 24.16.0 | Node.js 24.16.0 |

## Minicloud Evidence

| Field | Value |
| --- | --- |
| Host | `minicloud`; Rust 1.97.1; Node.js 24.16.0 |
| Exact export | `/home/nimbus/avr11-final.byJf2D` |
| Server matrix | 662 passed, 35 skipped, zero failed; 476.092 seconds |
| Serial applications | 9/9 cases, 37/37 assertions, cleanup passed; 126,409 ms |
| Parallel applications | 9/9 cases, 37/37 assertions, cleanup passed; 67,406 ms at five workers |
| Binary | `nimbus 0.1.45`; SHA-256 `bbae900901351f7dda00cffafea1801bfb11159790dbb70d178256bc138e593d` |
| Manifest | Schema 1; SHA-256 `9fd462b34d03d8af214f98aff26335636d7e89ee9af0221aa413bfac3c1c4a77` |
| Source evidence | Before and after SHA-256 `ce85cbf009411ad23da5da5ff2cae9edf9c7ab053329d2495b8252ec8495f756` in both application runs |
| Cross-host parity | 43,652 candidate paths; zero byte differences after tests and application runs |
| Serial report | `target/examples-verify-results/nimbus-examples-verify.gkcaae-ed5836910333/report.json` |
| Parallel report | `target/examples-verify-results/nimbus-examples-verify.cvvto3-86e1563575fb/report.json` |

### Final correction export

| Field | Serial | Five workers |
| --- | --- | --- |
| Exact export | `/home/nimbus/avr11-correction.9f3fba2da` | Same |
| Report | `target/examples-verify-results/nimbus-examples-verify.xgidul-d098bbb3a477/report.json` | `target/examples-verify-results/nimbus-examples-verify.ss53iy-4eb04f19b34f/report.json` |
| Result | 9/9 applications; 37/37 anchors; 121,009 ms | 9/9 applications; 37/37 anchors; 67,876 ms |
| Cleanup | Passed | Passed |
| Binary | `nimbus 0.1.45`; SHA-256 `bbae900901351f7dda00cffafea1801bfb11159790dbb70d178256bc138e593d` | Same |
| Manifest | Schema 1; SHA-256 `9fd462b34d03d8af214f98aff26335636d7e89ee9af0221aa413bfac3c1c4a77` | Same |
| Source evidence | Before and after SHA-256 `ce85cbf009411ad23da5da5ff2cae9edf9c7ab053329d2495b8252ec8495f756` | Same |
| Runtime | Node.js 24.16.0 | Node.js 24.16.0 |

The final export passed workspace behavior 10/10, report behavior 9/9, and
benchmark behavior 6/6. Documentation behavior 6/6 and scheduler behavior 2/2
also passed. SHA-256 comparison matched all 12 corrected files to commit
`9f3fba2da`.

The first minicloud build used default development debug information and
exhausted the 8 GB host during the final link. The accepted build kept the same
source and development profile, disabled debug information and incremental
artifacts, and used four compile jobs. The final server run used four Nextest
threads. These are host resource controls, not product or acceptance changes.

## Corrected Non-Product Runs

One local attempt linked the owner worktree's `target` path to another
worktree. Two CLI path assertions correctly rejected that path identity. The
accepted local run uses the repository's shared Cargo target through
`CARGO_TARGET_DIR` while the owner worktree retains a real ignored `target`
directory. This correction did not change the product or an assertion.

The host preflight stopped the first final local application command. The
interactive shell selected unsupported Node.js 26. The accepted runs use the
installed Node.js 24.16.0 toolchain. A second attempt set the shared Cargo
target without the harness's explicit binary input. Cargo built the binary in
the shared target. The harness looked in its default local target.

The accepted runs supply the same shared-target binary through
`NIMBUS_EXAMPLES_VERIFY_BIN`. Both stopped attempts preserved source bytes. They
ran no incomplete application acceptance matrix.

The first correction-tree `make ci` attempt exhausted the local disk while
Nextest linked workspace tests. It had already passed format, Clippy, deny, and
the runtime lane. The completed network-audit worktree held 124.8 GiB of
disposable Cargo artifacts. `cargo clean` removed only that clean worktree's
`target/` content. The immediate rerun retained this worktree's cache and
passed the full command with 102 GiB free. No source or acceptance assertion
changed in response to the infrastructure failure.

The first narrow-review command used a `phase` gate. The helper skipped it
before model contact because the configured cadence is `pre-pr`. The accepted
pre-PR invocation contacted GPT-5.6 Sol once. No duplicate review ran.

One affected-test bundle omitted the AVR verifier selector and used ambient
Node.js 26 for the scheduler. Its output is not acceptance evidence. The
fail-fast rerun used `--through-phase 3`. It selected Node.js version
`24.16.0`.

The AVR verifier passed 24/24, and mutations passed 24/24. AVR9 behavior passed
1/1 plus 6/6 and 2/2. AVR10 behavior passed 1/1 plus 6/6.

The first cross-host hash loop used zsh's reserved `path` variable. It removed
the command search path and produced empty hashes, so the result was invalid.
The accepted loop used `file_name`, required nonempty hashes, and matched all
12 files.

The first minicloud server run used the default Nextest fanout. Its two
failures reproduced alone, so reduced concurrency did not hide them. The
accepted four-thread run passed the complete matrix after AVR11 corrected the
two explicit test deadlines.

## Ownership Boundary

AVR11 changes test composition, test-only durability deadlines, and narrowly
patched dependency manifests. It does not add network authority, provider
effects, listener allocation, transport, tenant policy, source restoration,
or a compatibility path. `nimbus-network -> nimbus-core` remains its only
workspace edge. Product providers remain the only port and socket authority.
