# AVR10 Example Documentation and Comments

Date: 2026-08-18
Work commit: `04a675b29483976073e5569ae2d6a120e828baf9`

## Result

AVR10 makes the validated case manifest the source for application-verification
documentation. The example README now reports nine cases and 37 smoke
assertions. It distinguishes push, polling, and request-response behavior. It
also gives the tested Node range, serial and bounded-parallel commands, result
paths, retained-artifact instructions, and fail-closed recovery guidance.

The operator runbooks now give the same commands and semantics. The runner
comment describes its current bounded scheduler. One manifest-derived test
checks every case, update mode, command, report path, retention instruction,
and stale claim.

The UI route generator no longer interprets 18 support modules as routes.
Those modules use one explicit `-` prefix, and the generator rejects support
files without that prefix or route files that use it. The generated route tree
did not change. Typecheck, tests, and the production build passed.

The documentation renderer now uses this stack:

| Package | Version |
| --- | --- |
| Astro | 7.2.2 |
| Starlight | 0.41.7 |
| `starlight-llms-txt` | 0.11.0 |

The package records the Astro runtime floor of Node.js 22.12. A clean Node.js
22 install reported zero vulnerabilities. It built 110 pages without the
deprecated Markdown-processor warning.

## Fail-Before Evidence

| Finding | Observed state |
| --- | --- |
| AVRC24 | The AVR10 task verifier reported `0 passed, 1 failed`. |
| AVRF14 | `examples/README.md` said that five of six task examples were green and that Convex smoke was only partial. |
| AVRF15 | The text did not distinguish subscription push from repeated-read polling. |
| AVRF19 | The Astro build warned that `markdown.gfm` and `markdown.smartypants` were deprecated. |
| AVRF23 | UI route code generation emitted 18 support-file warnings. |
| AVRF25 | The three edited Markdown files had 18 baseline writing diagnostics. |

## Acceptance Ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR10.1 Derive counts. | Pass. | The documentation test reads `scripts/examples-verify-cases.json` and verifies nine table rows and 37 manifest assertions. |
| AVR10.2 State the nine-app result. | Pass. | The example README states that all nine cases pass against a real Nimbus binary. |
| AVR10.3 Separate update semantics. | Pass. | Each case has one manifest-matching `push`, `polling`, or `request-response` row. The prose limits each claim. |
| AVR10.4 Document jobs and artifacts. | Pass. | Serial, five-worker, exact-selector, JSON, JUnit, and retained-artifact instructions match tested paths. |
| AVR10.5 Delete stale text. | Pass. | The old partial-Convex and five-of-six claims are absent. The runner comment describes bounded execution. |
| AVR10.6 Record the writing delta. | Pass. | The same three files have 10 diagnostics after the change, down from 18 on `origin/main`. New AVR10 text has zero diagnostics. |

## Verification Evidence

| Command or proof | Result |
| --- | --- |
| `bash scripts/examples-verify-contract-test.sh --task AVR10` | Pass. AVRC24 1/1, manifest documentation 6/6, and warning-free UI codegen. |
| `bash scripts/verify-docs-app-verification.sh --task AVR10` | Pass. AVRC24 1/1. |
| `bash scripts/verify-docs-app-verification.sh --through-phase 3` | Pass. AVRC01-AVRC24 24/24. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. Mutations 24/24. |
| `npm run typecheck -w nimbus-ui` | Pass. Route codegen emitted no support-file warning. |
| `npm run test -w nimbus-ui` | Pass. 51 files and 336 tests. |
| `npm run build -w nimbus-ui` | Pass. 2,560 modules transformed. The generated route tree has no diff. |
| `PATH=/opt/homebrew/opt/node@22/bin:$PATH npm --prefix website ci` | Pass. 418 packages installed and zero vulnerabilities found. |
| `PATH=/opt/homebrew/opt/node@22/bin:$PATH npm --prefix website run build` | Pass. 110 pages built with no deprecated Markdown-processor warning. |
| `bash scripts/check-docs.sh` | Pass. 109 pages link-clean; source map and private fence passed. |
| `bash scripts/verify-nimbus-docs-site.sh` | Pass. 17/17 conditions. |
| ShellCheck and Node syntax | Pass. No diagnostics. |
| Technical-writing baseline delta | Pass. 18 diagnostics before and 10 after. No new diagnostic. |
| `git diff --check` | Pass. |

The optional package-wide `npm run lint -w nimbus-ui` remains red on the
pre-existing UI Biome baseline: 68 errors and six warnings across unrelated UI
files. It is not an AVR10 acceptance gate. AVR10 did not change those rules or
weaken any assertion.

## Ownership Boundary

AVR10 changes documentation, documentation rendering, route-file ownership,
and verifier text only. It does not add a network authority, listener, policy,
transport, provider effect, or source-restoration path. Application case
identity and behavior remain owned by the validated manifest. The product
providers remain the only port authority.
