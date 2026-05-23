# CM8: Closeout

CM8 archives the CI Modernization plan and promotes the canonical
contract.

## What changed

1. `docs/plans/ci-modernization-plan.md` →
   `docs/plans/archive/ci-modernization-plan.md`. The verifier's
   `plan_file()` helper already accepts both paths, so condition 1
   stays green throughout the move.
2. `docs/operating/ci-modernization.md` written as the canonical
   synthesis of CM1-CM6 contracts. This is the file future
   contributors read to understand the composite action, SHA-pin
   discipline, runner pinning, job-summary shape, CodeQL setup,
   and Dependabot configuration.
3. Plan ledger marked CM8 `done` and Execution Log appended with
   real commit SHAs.
4. Routing entries:
   - `docs/plans/README.md`: move CM entry from active to archived
     section, point at the archived path.
   - `CLAUDE.md`: routing block updated to the archived path and
     mention of `docs/operating/ci-modernization.md` as the
     canonical contract.

## Retro

What worked:

- **Composite first (CM1) gave the rest of the wave a small
  surface.** SHA-pinning third-party actions (CM2) only touched 3
  references in the composite + a handful of one-offs because
  CM1 had already collapsed 12 duplicated bootstrap blocks.
- **Each CM as a separate commit** kept the Execution Log SHAs
  individually auditable. The CM1 hotfix (`faa9ffd2`) is its own
  row in the log; the CM6 hotfix (`624e5ec5`) is too. Future
  archaeology is easy.
- **The composite-aware CC verifier update** in the CM1 hotfix
  kept both gates passing simultaneously through the transition.
  No flaky "passing gates simultaneously" gap.

What surprised:

- GitHub Actions evaluates `${{ ... }}` expressions even inside
  the `description:` field of a composite-action manifest. The
  initial CM1 had a prose example referencing
  `${{ secrets.GOOGLESOURCE_COOKIE }}` in the description, which
  failed manifest validation because composite actions cannot read
  `secrets.*`. Hotfix in `faa9ffd2`.
- The verifier's SHA-pin regex initially required exactly two path
  segments (`org/repo@sha`). `github/codeql-action/init@sha`
  is the official GitHub shape for sub-path actions and has three
  segments. CM6 hotfix in `624e5ec5` relaxed the regex.

What did not happen:

- No `dependabot_security_updates` enablement (UI toggle, not a
  workflow change — documented as follow-up in CM7 proof).
- No secret-scanning enablement (same — out of CM scope).

## Verifier delta

Before CM8:

- Condition 11 (ledger done): FAIL (CM8 still pending).
- Condition 12 (CI green): conditional on push + CI completing.

After CM8 (post-push, post-CI green):

- All 12 conditions PASS.

## Final state

- Plan: `docs/plans/archive/ci-modernization-plan.md`
- Canonical contract: `docs/operating/ci-modernization.md`
- Proof artifacts (this directory): `cm0-baseline.md`,
  `cm1-composite-action.md`, `cm2-sha-pinning.md`,
  `cm3-ubuntu-pin.md`, `cm4-actions-major-pin.md`,
  `cm5-job-summaries.md`, `cm6-codeql.md`,
  `cm7-dependabot-audit.md`, `cm8-closeout.md`.
- Verifier: `scripts/verify-ci-modernization.sh` (12/12 PASS).
