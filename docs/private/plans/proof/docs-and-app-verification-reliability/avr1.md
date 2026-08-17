# AVR1 Stable Network Contracts and Private Closeout

Date: 2026-08-17

## Result

AVR1 is complete. The AVR1 work archives the completed network plan. Private
routing now states merged truth. Executable verification reads a small
versioned JSON contract instead of active plan prose. The original network
verifier remains green at 39/39. Its aggregate mutation suite remains green at
610/610.

| Checkpoint | Revision |
| --- | --- |
| AVR1 start | `28591e94d` |
| Contract extraction and archival | `3300c6b6f572e2cd8e31baa4ceb13095f488a1d7` |
| Mutation-fixture correction | `b24959165c590e9cff0dae6ef2a2f2481ebb8966` |

The two work commits change 29 paths. They change no `crates/**`,
`packages/**`, application example, workflow, public document, or product
configuration file.

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR1.1 Inventory every plan reader. | Pass. | All 13 executable readers from AVR0 were classified. No executable path under `scripts`, `crates`, `tests`, `.github`, or `Makefile` contains the former active-plan path. |
| AVR1.2 Extract stable fixtures. | Pass. | `scripts/nimbus-network-control-plane/verification-contract.json` owns completion identity, the NNC6.5 checkpoint needed by the teardown audit, seven frozen routes, shared vocabulary, and three evidence-token sets. |
| AVR1.3 Update readers without behavior drift. | Pass. | The 13 readers consume the JSON contract or an explicitly split historical path. NNCV000-NNCV038 remain green. |
| AVR1.4 Correct stale active-status claims. | Pass. | Private architecture, plan routing, horizontal-scaling, architecture-review, examples history, taxonomy, and multi-tenant verification references now state archive or completed truth. |
| AVR1.5 Archive the network plan. | Pass. | The complete plan moved to `docs/private/plans/archive/nimbus-network-control-plane-plan.md`; its relative links resolve. |
| AVR1.6 Update routing. | Pass. | The plans and architecture indexes each route to the archive. The active plan path is absent. AVRC01-AVRC04 report 4/4. |

## Stable contract boundary

The JSON contract contains only data that live verification needs:

- schema and completed status.
- archive and completion checkpoints.
- the NNC6.5 item checkpoint that delimits the teardown audit.
- the original plan and verifier counts.
- seven frozen route strings.
- portable saga vocabulary.
- NNC6.4, NNC6.4a, and NNC6.5 proof tokens.

It does not become desired network state, provider state, or a second product
authority. The archived Markdown plan remains historical evidence. The live
verifier owns the stable executable contract.

## Drift found and resolved

The archival exposed four stale verifier inputs:

1. The bind census recognized exact `cfg(test)` modules but not the existing
   `cfg(all(test, unix))` form. The classifier now accepts `test` only as the
   first exact conjunction. It still rejects disjunctions that can compile in
   production.
2. The composition census retained the pre-extraction forwarded-profile
   symbol and one stale source line. Both now match live source and retain the
   original proof identifiers.
3. The compiler-authority baseline predated the current six-package,
   five-output closure. The repository refresh command regenerated it after
   the UI and embedded-package builds.
4. Three mutation fixtures still authored plan-shaped Markdown or referenced
   the removed `OWNER_PLAN` name. They now author minimal valid JSON contracts.

The first complete mutation run rejected the three stale fixtures.

The focused correction runs passed at 13/13, 36/36, and 50/50.

The final aggregate passed at 610/610.

No change weakens a test, assertion, expected count, or fail-closed behavior.

## Verification evidence

| Command | Result |
| --- | --- |
| `bash scripts/verify-docs-app-verification.sh --task AVR1` | Pass; 4 passed, 0 failed. |
| Active-plan executable-reference search | Pass; zero matches. |
| `bash scripts/verify-nimbus-network-control-plane.sh` before the work commit | Expected durability state; 38 passed, NNCV001 alone failed because the contract was not in `HEAD`. |
| `bash scripts/verify-nimbus-network-control-plane.sh` after the work commit | Pass; 39 passed, 0 failed. |
| `bash scripts/verify-nimbus-network-control-plane.sh --self-test` | Pass; 610 passed, 0 failed. |
| Ingress, provision-decision, and provider-dispatch focused self-tests | Pass; 13/13, 36/36, and 50/50. |
| Changed shell syntax and ShellCheck | Pass; no diagnostics after following the shared mutation-runner source. |
| Changed JavaScript syntax | Pass; no diagnostics. |
| Compiler-authority baseline validation | Pass; 6 packages and 5 generated outputs. |
| `git diff --check` | Pass. |
| `bash scripts/check-docs.sh` | Pass; 108 pages, private fence intact. |
| `npm --prefix website run build` | Pass; 109 HTML pages and all three `llms` outputs. |
| `bash scripts/verify-nimbus-docs-site.sh` | Pass; 17/17. |

AVR2 can now document the public network architecture without linking public
users to private plans or making active verification depend on archived prose.
