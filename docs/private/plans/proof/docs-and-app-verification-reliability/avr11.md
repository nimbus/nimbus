# AVR11 Integrated Acceptance

Date: 2026-08-18
Status: candidate validation

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
| AVR11.6 Commit and freeze. | Pending. | Freeze follows the remaining final-tree command row and proof update. |
| AVR11.7 Run one Sol review. | Pending. | Review runs once after the owner commits and freezes the complete candidate. |
| AVR11.8 Open PR 3. | Pending. | No PR exists for the AVR8-AVR11 candidate yet. |
| AVR11.9 Resolve hosted failures. | Pending. | Hosted acceptance starts after PR 3 opens. |
| AVR11.10 Merge with owner authority. | Pending. | The owner has not yet authorized this future merge. |

## Behavioral and Repository Evidence

| Command or proof | Result |
| --- | --- |
| Focused final durability tests, local | Pass. 2/2 in 13.118 and 13.508 seconds. |
| Focused final durability tests, minicloud | Pass. 2/2 in 30.975 and 31.579 seconds. |
| `cargo nextest run -p nimbus-server --lib --test-threads=4`, minicloud | Pass. 662/662, with 35 documented skips, in 476.092 seconds. |
| `make test`, local | Pass. Runtime 517 passed with 134 ignored; workspace 7,419/7,419 passed with 107 skipped; all documentation tests passed. |
| `make ci`, local | Pass. Runtime 517 passed with 134 ignored; workspace 7,419 passed with 107 skipped; required harnesses, JavaScript checks, and proof helpers passed. |
| `make examples-verify`, local serial | Pass. 9/9 applications and 37/37 anchors in 45,533 ms; cleanup passed; source matched. |
| `make examples-verify`, local five-worker | Pass. 9/9 applications and 37/37 anchors in 14,743 ms; cleanup passed; source matched. |
| Focused `nimbus-blob` tests | Pass. 248/248. |
| Focused `nimbus-s3` tests | Pass. 20/20. |
| Focused `nimbus-storage` tests | Pass. 448/448 with two documented fixture skips. |
| Cargo deny, attribution, lock, and workspace-hack drift checks | Pass. No rejected advisory, yanked package, attribution gap, or generated-manifest drift remains. |
| Full AVR verifier and self-test | Pass. AVRC01-AVRC24 are 24/24; mutation self-test is 24/24. |
| Both documentation gates and site build | Pass. 109 pages are link-clean, 17/17 site conditions are green, and the build emits 110 HTML pages. |
| Technical-writing lint | Pass. Five changed owner-written Markdown files have zero diagnostics. |
| `cargo fmt --all --check` and `git diff --check` | Pass. |

Warnings emitted from unchanged upstream source in locally patched crates are
advisory. Nimbus code remains subject to the repository's deny-warnings Clippy
gate, which passed.

## Local Application Evidence

| Field | Serial | Five workers |
| --- | --- | --- |
| Report | `target/examples-verify-results/nimbus-examples-verify.okwas8-0c41187eaeae/report.json` | `target/examples-verify-results/nimbus-examples-verify.illfnf-ab4f14ea2d37/report.json` |
| Result | 9/9 applications; 37/37 anchors; 45,533 ms | 9/9 applications; 37/37 anchors; 14,743 ms |
| Cleanup | Passed | Passed |
| Binary | `nimbus 0.1.45`; SHA-256 `1099d72981b29a3ad46911dcef431562f8f26fe751e1a0848b1aef7bfac9050e` | Same |
| Manifest | Schema 1; SHA-256 `9fd462b34d03d8af214f98aff26335636d7e89ee9af0221aa413bfac3c1c4a77` | Same |
| Source evidence | Before and after SHA-256 `7fe61edf22cb942273debbfce850aadefcd56b9d87c7a2c846fd95f30b439e93` | Same |
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
