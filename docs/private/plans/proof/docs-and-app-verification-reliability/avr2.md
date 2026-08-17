# AVR2 Public Network Architecture

Date: 2026-08-17

## Result

AVR2 implementation acceptance is complete. Nimbus now has a public,
source-verified network control-plane page. The public overview, architecture
index, sandbox page, server page, capability matrix, and source map use the
same ownership model.

The phase-one campaign stays in progress until hosted checks pass and the
owner authorizes the merge. Commit `b98ee3242` is candidate-green locally.
Replacement hosted runs remain. Then the owner reconciles current main.

| Checkpoint | Revision |
| --- | --- |
| AVR2 start | `e592ac5e9b8d8e1f05011634ebce2defe5737e38` |
| Public network architecture | `4ad5a2c1b6a9cdf84ff9293004ec42b5414dd594` |
| Review corrections | `38973458201818a57190c507d0cb34124aa6c218` |
| Mutation isolation correction | `d74bce443721701cf6a03708173be2c791ab382c` |
| Hosted test correction | `b98ee3242656f40e347ac81ce9961bbdcd6df099` |
| Implementation PR 1 | [#275](https://github.com/nimbus/nimbus/pull/275) |

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR2.1 Add the public page and source-map rows. | Pass. | `docs/concepts/architecture/network-control-plane.md` maps each authority, state, capability, lifecycle, and exclusion claim to current source. Eight source-map rows route those claims. |
| AVR2.2 Change both page counts. | Pass. | Both public architecture indexes state thirteen pages. The directory contains fourteen Markdown files, including its index. |
| AVR2.3 Add lifecycle-plane cross-links. | Pass. | The product overview, concept index, sandbox architecture, server architecture, root README, root architecture, and capability matrix route to the new page. |
| AVR2.4 Inspect every generated LLM file. | Pass. | `llms.txt` is the short index for the two generated sets. `llms-full.txt` and `llms-small.txt` contain the page and cross-links. All three files are private-fence clean. |

The fail-before verifier reported `0 passed, 6 failed` for AVRC05-AVRC10.
The final AVR2 verifier reports `6 passed, 0 failed`. The phase-one aggregate
reports `10 passed, 0 failed`.

## Architecture proof

The source audit confirmed these claims:

- `nimbus-network` has `nimbus-core` as its only outgoing Nimbus workspace
  dependency.
- The crate contains no transport or provider effects. Server, KV, sandbox,
  machine, proxy, and node code retain those effects.
- Stable plan, attachment, segment, endpoint, listener, route, port-lease, and
  provider identities stay separate from addresses.
- Desired plans, durable authority, and observed status use separate types.
- The node-local store owns segment, attachment, tenant-IPAM, and port-lease
  partitions under one revision and commit domain.
- Capability evidence records management, attachment, isolation, address,
  bind, exposure, assignment, ingress, forwarding, lifecycle, TLS, locality,
  dependency, and offline-restart dimensions.
- The durable saga follows reserve, prepare, attach, activation checks,
  activate, readiness checks, publish, and observe. Teardown follows withdraw,
  drain, stop, detach, release, and record.
- Compute remains the workload saga coordinator. Services retains logical
  naming. Multi-node cluster transport remains separate and unavailable.

No public document links to a private plan or proof.

## Review disposition

The configured `phase` invocation skipped because this repository reviews at
the `pre-pr` cadence. It contacted no model and does not count as a review.

The one complete review used Codex with GPT-5.6 Sol, xhigh reasoning, and fast
service. It reviewed `origin/main..HEAD` and reported two findings:

| Finding | Disposition |
| --- | --- |
| P2, confidence 0.98: make the private-doc fence detect relative links. | Accepted. Commit `389734582` resolves inline and reference-style Markdown targets from the source file. It rejects paths that normalize into `docs/private`. |
| P3, confidence 0.99: update the proof index to the current execution state. | Accepted. Commit `389734582` routes to the completed AVR0 and AVR1 proofs and the current AVR2 checkpoint. |

Because the accepted P2 changed executable verification, one narrow correction
review ran with the same Sol, xhigh, and fast identity. It reported one finding:

| Finding | Disposition |
| --- | --- |
| P2, confidence 0.99: preserve the cross-link when mutating AVRC10. | Accepted. Commit `d74bce443` appends the relative private link to the green fixture, so the fence is the only failing predicate. |

The final mutation suite passes 24/24. A separate meta-mutation disables only
the fence and then fails specifically with `self-test AVRC10: mutation did not
fail closed`. This proves that the corrected test reaches the intended
predicate. The review cadence permits no third review, and none ran.

## Hosted CI correction

Final-head CI run `32050937772` failed
`listener_projection_failure_keeps_every_listener_active_and_retries` in the
workspace-test shard. The exact test reproduced locally before any correction:
zero passed and one failed at the assertion that projection recovery must not
terminate the listener group.

The fault injector was process-wide. After the second listener guard armed it,
either the retained connectivity projection or concurrent router startup could
consume the one-shot storage failure. When router startup won that race, the
server correctly stopped. The projection driver then recovered all three rows,
which made the test report the contradictory result.

AVRF21 makes the test fault target the exact durable record that writes both
the `listeners` and `ports` tables. Unscoped checks and unrelated tenant
records cannot consume the arm. This preserves the production retry path and
removes scheduler-dependent test identity.

Local correction evidence is:

- The exact test passes 1/1.
- Fifty fresh test-process repetitions pass 50/50.
- The complete listener-group suite passes 10/10.
- The serialized `nimbus-server` lib suite passes 663 tests, ignores 35, and
  fails none.
- `cargo clippy -p nimbus-server --lib --tests -- -D warnings` passes.

An unrestricted full libtest run separately reported 601 passed, 62 failed,
and 35 ignored. Every sampled failure was `DuplicateProcessComposition` from
managed tests racing the deliberate process-global network authority. The
same binary is green when serialized, and hosted CI uses nextest process
isolation. AVR11 owns AVRF22 and must correct the canonical local `make test`
path. That correction must preserve the product-authority guard. It is separate
from AVRF21.

This hosted correction was not an autoreview finding. The phase already used
its one full review and one permitted narrow correction review, so no third
review runs.

## Hosted Docs hardening

Docs runs `32050695317`, `32050937873`, and `32054899165` each built and
uploaded a valid preview. Each run then failed when its zero-retry PR comment
request received HTTP 503 during GitHub's incident. The third failure occurred
after the AVRF21 push. This notification-only defect then blocked the phase-one
pull request again.

AVRF20 now belongs to AVR2. The preview upload step rejects a missing URL. It
writes a valid URL to the job summary before it calls the GitHub API. The
comment step retries three times. If all attempts fail, the step records a
continued failure. A final step writes an Actions warning and a job-summary
warning. The successful build and upload keep the job green.

The site verifier now requires the job summary, bounded retries,
`continue-on-error`, and the explicit warning outcome. `actionlint`, Bash
syntax, ShellCheck, and all 17 site conditions pass. ShellCheck also found a
pre-existing unchecked verifier `cd`. The directly related cleanup now exits
if the repository-root transition fails. Work commit `a786468eb` owns this
hardening.

Docs run `32055431404` supplied the live failure proof at final head
`5e928890b`. The upload produced a valid preview URL. The PR comment then
received HTTP 503 after all three retries. The workflow emitted the explicit
warning and completed successfully. This result proves that a notification
failure cannot replace the successful preview result.

## Verification evidence

| Command or check | Result |
| --- | --- |
| `bash -n scripts/verify-docs-app-verification.sh` | Pass. |
| `shellcheck scripts/verify-docs-app-verification.sh` | Pass; no diagnostics. |
| `bash scripts/verify-docs-app-verification.sh --task AVR2` | Pass; 6 passed, 0 failed. |
| `bash scripts/verify-docs-app-verification.sh --through-phase 1` | Pass; 10 passed, 0 failed. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass; 24/24 mutations detected. |
| Fence-disabled AVRC10 meta-mutation | Expected failure at AVRC10; the other first-phase mutations reached AVRC10. |
| `bash scripts/verify-nimbus-network-control-plane.sh` | Pass; 39 passed, 0 failed. |
| `bash scripts/check-docs.sh` | Pass; 109 pages, source map resolved, private fence intact. |
| `npm --prefix website run build` | Pass; 110 HTML files, including the 404 page. |
| `bash scripts/verify-nimbus-docs-site.sh` | Pass; 17/17. |
| New page and proof-index technical-writing lint | Pass; 2 files, 0 diagnostics. |
| Edited legacy-document lint delta | Pass; 310 diagnostics before and after, zero new diagnostics. |
| Generated LLM private-fence inspection | Pass; all three files are clean. |
| `git diff --check` | Pass. |
| Exact hosted regression before correction | Expected failure; 0 passed, 1 failed. |
| Exact hosted regression after correction | Pass; 1 passed, 0 failed. |
| Fresh-process hosted-regression stress | Pass; 50/50. |
| Complete listener-group suite | Pass; 10 passed, 0 failed. |
| Serialized `nimbus-server` lib suite | Pass; 663 passed, 0 failed, 35 ignored. |
| `nimbus-server` lib and test Clippy | Pass with `-D warnings`. |
| `actionlint .github/workflows/docs.yml` | Pass; no diagnostics. |
| Docs verifier Bash syntax and ShellCheck | Pass; no diagnostics. |
| Resilient preview source contract | Pass in site condition 8. |
| Hosted resilient-preview contract | Pass in Docs run `32055431404`; upload succeeded, three comment retries exhausted on HTTP 503, warning emitted, job green. |
| Hosted CI | Pass in run `32055431425`; 47 successful jobs, three expected skips, zero failures. |
| Hosted AVRF21 regression | Pass in Rust workspace shard 2/3 of run `32055431425`. |
| Desktop UI Smoke | Pass in run `32055431555`. |
| Windows workspace check | Pass in run `32055431570`. |
| CodeQL | Pass in failed-only attempt 2 of run `32055431439`; Rust and JavaScript analysis green. |

The site build still emits Astro's `markdown.gfm` and `markdown.smartypants`
deprecation warning. AVR10 owns that low-priority cleanup. It does not affect
AVR2 output or acceptance.

The first hosted Docs run uploaded the preview and then received HTTP 503 from
GitHub's comment API. Replacement run `32050785842` passed after service
recovery. AVRF20 assigns bounded notification recovery to AVR11. AVR2 does not
change the reviewed workflow.
