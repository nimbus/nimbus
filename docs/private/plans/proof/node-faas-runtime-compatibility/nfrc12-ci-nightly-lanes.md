# NFRC12 CI And Nightly Lanes

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node20 `v20.20.2`, Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC12 implementation wave. The
NFRC12-specific changes wire Node FaaS compatibility into PR CI and extend the
scheduled Node compatibility workflow so expensive official-corpus refresh,
release-train live probing, watchpoint validation, and Node26 Current-line
oracle reporting run outside the PR critical path.

## Files Changed

- PR CI and scheduled workflow:
  `.github/workflows/ci.yml`,
  `.github/workflows/node-compat-nightly.yml`
- CI/nightly verifier:
  `scripts/verify-node-ci-nightly-lanes.sh`
- Control plane:
  `docs/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC12 followed the wide-then-focused loop for CI coverage:

1. Add the full PR and scheduled workflow shape first.
2. Run a structural verifier to check every required command is wired.
3. Run the actual supported-LTS Application canary lanes to catch runtime
   problems.
4. Rerun docs, release-train, latest-suite, canary-boundary, and Markdown
   reference verifiers.

PR CI now gates the declared supported-LTS FaaS app profile without forcing the
full official Node corpus into every pull request. The scheduled workflow owns
the broader nightly work: official corpus freshness, live release-feed drift,
watchpoint catalog validation, full Application/Tooling canary presets, and
Node26 Current-line oracle reporting.

## PR Gate

`.github/workflows/ci.yml` now includes `node-faas-compatibility` and
`rust-gate-summary` depends on it. The job runs:

- `bash scripts/verify-node-lts-docs.sh`
- `bash scripts/verify-node-release-train.sh`
- `bash scripts/verify-node-latest-suite-tags.sh`
- `make node-compat-canaries-bootstrap PRESET=application`
- `make node-compat-validate-claims`
- `make node-compat-canaries PRESET=application LANE=node22`
- `make node-compat-canaries PRESET=application LANE=node24`
- `bash scripts/verify-node-lts-canaries-and-oracles.sh`
- `bash scripts/verify-node-host-heavy-diagnostics.sh`

This keeps supported-LTS Application compatibility required on PRs while
leaving Node26 Current-line and official-corpus drift reporting to the nightly
lane.

## Nightly Lane

`.github/workflows/node-compat-nightly.yml` now runs:

- docs guard and latest-suite validation,
- enforced current-corpus validation with `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1`,
- release-train verification and live official-feed probing,
- Rust watchpoint catalog validation,
- seeded live slice reports,
- broad Application and Tooling canary presets,
- Node20/Node22/Node24 oracle samples,
- Node26 Current-line oracle sample,
- status, dashboard, trends, and evidence publication.

Pinned ignored watchpoints remain visible through
`make node-compat-validate-watchpoints`; they are counted as watchpoints and
unexpected passes require removing the ignore or reclassifying the entry. They
are not treated as green support.

## Wide Feedback And Focused Fixes

Initial local PR-lane canary runs without elevated local-bind permission
failed on `listen EACCES` for mock HTTP servers and local server fixtures. That
is a local sandbox limitation rather than a runtime regression, so the
supported-LTS canary lanes were rerun serially with local bind/listen approval.

Final supported-LTS canary results:

| Lane | Command | Result |
| --- | --- | --- |
| Node22 | `make node-compat-canaries PRESET=application LANE=node22` | pass; 32 canary checks passed, 0 failed |
| Node24 | `make node-compat-canaries PRESET=application LANE=node24` | pass; 32 canary checks passed, 0 failed |

## Verification

- `bash scripts/verify-node-ci-nightly-lanes.sh`: pass, 16 structural CI/nightly
  checks.
- `make node-compat-canaries PRESET=application LANE=node22`: pass with local
  bind/listen approval, 32 canary checks passed and 0 failed.
- `make node-compat-canaries PRESET=application LANE=node24`: pass with local
  bind/listen approval, 32 canary checks passed and 0 failed.
- `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`:
  pass, 4 lanes, 0 needing fixture sync, all targeted corpora current.
- `bash scripts/verify-node-lts-docs.sh`: pass; generated docs current and
  stale-overclaim guard passed.
- `bash scripts/verify-node-release-train.sh`: pass, 4 lanes, 0 drift entries,
  negative self-tests passed.
- `make node-compat-validate-watchpoints`: pass, 67 watchpoint entries.
- `bash scripts/verify-node-lts-canaries-and-oracles.sh`: pass, 12 checks and
  0 failures.
- `bash scripts/verify-node-host-heavy-diagnostics.sh`: pass, 7 checks and 0
  failures.
- `npm run docs:validate-refs:strict`: pass, 232 working-tree Markdown files.
- `git diff --check`: pass.

## Decisions

- Gate Node22 and Node24 Application canaries in PR CI because they are the
  supported LTS lanes and the public support promise depends on them.
- Keep full official-corpus refresh, live official-feed checks, and Node26
  Current-line oracle reporting in the scheduled Node compatibility workflow.
- Validate watchpoints explicitly in nightly so ignored fatal VM or harness
  watchpoints remain visible and cannot be mistaken for green support.

## Remaining Risks

- NFRC13 still owns the final all-row verifier and plan closeout.
