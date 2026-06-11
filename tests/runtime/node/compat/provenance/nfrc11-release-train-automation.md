# NFRC11 Release-Train Automation

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node20 `v20.20.2`, Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC11 implementation wave. The
NFRC11-specific changes add release-train automation that validates checked-in
Node lane metadata, latest official tags, dashboard role separation, and a
proof digest gate. It also adds an optional live probe against official Node
release feeds so future scheduled automation can detect new patch/minor tags
or lifecycle changes before public docs move.

## Source Digest Gate

Future changes to release metadata must update this proof because the verifier
requires these digest markers:

- tests/runtime/node/compat/node-lts-compat/node-lts-lanes.json sha256: beaa3816420eb6263aa186340217a662978e9e9098efb1241fb27e23f61bb7e1
- tests/runtime/node/compat/node-lts-compat/node-latest-suite-tags.json sha256: 48d7181e4be7e5928342e0a87eae81c62adf47d0546ad186c3630e0985a98038

## Files Changed

- Release-train automation and verifier:
  `scripts/runtime/node/release_train.py`,
  `scripts/verify-node-release-train.sh`,
  `Makefile`
- Release-train schema and generated summary:
  `tests/runtime/node/schemas/node-release-train.schema.json`,
  `tests/runtime/node/compat/node-lts-compat/node-release-train.json`,
  `tests/runtime/node/compat/node-lts-compat/node-release-train.md`
- Release metadata docs:
  `tests/runtime/node/compat/node-lts-compat/node-lts-lanes.md`,
  `tests/runtime/node/compat/node-lts-compat/node-latest-suite-tags.md`
- Control plane:
  `docs/private/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/private/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC11 followed the wide-then-focused loop for release drift:

1. Add a broad release-train analyzer over lane registry, latest-suite tags,
   generated status/dashboard evidence, and optional official live feeds.
2. Run offline self-tests and a live probe to capture tag, lifecycle, role, and
   proof-gate feedback.
3. Fix the specific lifecycle mismatch exposed by live official schedule data.
4. Rerun the live probe and offline verifier, then publish a checked-in
   release-train summary.

## Release-Train Contract

`scripts/runtime/node/release_train.py` validates:

- Node24 is the product default.
- Node22 is supported Maintenance LTS.
- Node20 is legacy-grace/EOL regression coverage.
- Node26 is Current/non-LTS, not product default and not supported LTS.
- Latest official tags match the lane registry and fixture corpus tags.
- Dashboard lane roles match registry roles: `legacy`, `supported`, `default`,
  and `current`.
- Generated release-train docs match current inputs.
- This proof file exists, is listed by the proof README, and contains the
  current lane/latest-tag source digest markers.

The optional live probe reads:

- `https://nodejs.org/dist/index.json`
- `https://raw.githubusercontent.com/nodejs/Release/main/schedule.json`

Those are the official machine-readable feeds used to detect new tags and
release lifecycle changes. Web research on 2026-05-28 also confirmed Node.js
`v26.2.0` is a Current release and the Node Release Working Group schedule is
the canonical lifecycle source.

## Wide Feedback And Focused Fixes

Initial live probe:

```bash
python3 scripts/runtime/node/release_train.py probe-live
```

The first sandboxed run failed on DNS, as expected under restricted network
execution. The escalated live run then reached the official feeds and exposed a
real lifecycle mismatch:

| Surface | Initial feedback | Resolution |
| --- | --- | --- |
| Node26 maintenance date | A search-result snippet suggested `2027-10-27`, but the official schedule JSON returned `2027-10-20`. | Keep the registry aligned with the official schedule JSON and record the live-probe result as the source of truth. |

Final live probe:

```bash
python3 scripts/runtime/node/release_train.py probe-live
```

Result: pass, `4` lanes matched official release feeds.

## Generated Summary

The generated summary is checked in at
`tests/runtime/node/compat/node-lts-compat/node-release-train.md` and
`node-release-train.json`.

It reports:

- Node20: `legacy_grace`, `eol_legacy`, dashboard role `legacy`.
- Node22: `supported_lts`, `maintenance_lts`, dashboard role `supported`.
- Node24: `product_default`, `active_lts`, dashboard role `default`.
- Node26: `current_non_lts`, `current_non_lts`, dashboard role `current`.
- Canary claims: `37`.
- Canary checks: `101`.
- Required canary gaps: `0`.
- Release-train drift: none.

## Verification

- `python3 scripts/runtime/node/release_train.py publish`: pass; generated
  `node-release-train.json` and `node-release-train.md`.
- `python3 scripts/runtime/node/release_train.py publish --check-proof`: pass;
  generated release-train summary with proof file present, proof README listed,
  and no missing digest markers.
- `bash scripts/verify-node-release-train.sh`: pass; 4 lanes, 0 drift entries,
  negative self-tests passed.
- `make node-compat-release-train CHECK=1`: pass; generated release-train
  summary is current.
- `python3 scripts/runtime/node/release_train.py self-test`: pass; negative
  self-tests detected tag drift, lifecycle drift, dashboard role drift, and
  product-default drift.
- `python3 scripts/runtime/node/release_train.py probe-live`: pass with
  network approval, 4 lanes matched official release feeds.
- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing
  fixture sync, negative self-tests passed.
- `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes, product default
  `node24`.
- `npm run docs:validate-refs:strict`: pass, 232 working-tree Markdown files.
- `git diff --check`: pass.

## Decisions

- Keep live official feed reads out of the default offline verifier. NFRC12
  owns scheduled/nightly placement for live checks; NFRC11 makes the live probe
  available and proves it.
- Require source digest markers in this proof so a future lane/tag metadata
  edit cannot pass release-train verification without updating the proof.
- Treat search snippets as advisory only. The official schedule JSON is the
  source of truth when snippets disagree.

## Remaining Risks

- NFRC12 still owns wiring this verifier into PR and scheduled CI lanes.
- NFRC13 still owns the final all-row verifier and closeout pass.
