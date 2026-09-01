# NFRC11 Release-Train Automation

Initial date: 2026-05-28
Last refreshed: 2026-09-01
Authoring agent: Codex
Initial repository baseline: `e7e8b9d6`
Refresh repository baseline: `af5bf1455`
Relevant Node lanes: Node20 `v20.20.2`, Node22 `v22.23.2`, Node24 `v24.20.0`, Node26 `v26.8.1`

## Git Status Summary

The worktree contains the active NFRC0-NFRC11 implementation wave. The
NFRC11-specific changes add release-train automation that validates checked-in
Node lane metadata, latest official tags, dashboard role separation, and a
proof digest gate. It also adds an optional live probe against official Node
release feeds. Future scheduled automation can detect new patch/minor tags or
lifecycle changes before public docs move.

## Source Digest Gate

Future changes to release metadata must update this proof because the verifier
requires these digest markers:

- tests/runtime/node/compat/node-lts-compat/node-lts-lanes.json sha256: bfcd0f33987e3e80beb5c08bd5043ea5e552c0fedc42f33b610ba7ffe6e210f3
- tests/runtime/node/compat/node-lts-compat/node-latest-suite-tags.json sha256: bf47d0f1c5c53d02efdb9e251e7c7f9af19ff91ae474875fe51c385ae7ef6bc8

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
- Nimbus supports Node22 as Maintenance LTS.
- Node20 is legacy-grace/EOL regression coverage.
- Node26 is Current/non-LTS, not product default and not supported LTS.
- Latest official tags match the lane registry and fixture corpus tags.
- Dashboard lane roles match registry roles: `legacy`, `supported`, `default`,
  and `current`.
- Generated release-train docs match current inputs.
- This proof file exists and contains the current lane/latest-tag source digest
  markers. The proof README lists this file.

The optional live probe reads:

- `https://nodejs.org/dist/index.json`
- `https://raw.githubusercontent.com/nodejs/Release/main/schedule.json`

Those are the official machine-readable feeds used to detect new tags and
release lifecycle changes. Web research on 2026-05-28 also confirmed Node.js
`v26.2.0` is a Current release and the Node Release Working Group schedule is
the canonical lifecycle source.

## 2026-09-01 Release-Readiness Refresh

The release-readiness replay detected newer official patch releases after the
offline checks passed. The live probe found `v22.23.2`, `v24.20.0`, and
`v26.8.1`, while the checked-in corpora still used `v22.22.3`, `v24.16.0`, and
`v26.2.0`. The refresh resolved each annotated tag to its tag object and peeled
commit. It synchronized the official fixture subtree. It also regenerated the
identity and classification catalogs and republished the evidence.

The refreshed evidence contains `20,621` official vendored test files, `7,768`
documented manifested green files, `150` explicit Rust watchpoints, `37` active
canaries, and `79` canary claims. Every official file is either in the measured
green subset or has an explicit expected-failure, known-gap, or skipped
classification. The refresh does not convert known gaps into pass claims.

A representative live replay covered core, process, stream, network, and
loader slices. It retained observed incompatibilities as failures in the raw
reports. The release-train gate verifies metadata and evidence integrity. It
does not claim complete Node compatibility.

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

The repository stores the generated summary at
`tests/runtime/node/compat/node-lts-compat/node-release-train.md` and
`node-release-train.json`.

The initial 2026-05-28 publication reported:

- Node20: `legacy_grace`, `eol_legacy`, dashboard role `legacy`.
- Node22: `supported_lts`, `maintenance_lts`, dashboard role `supported`.
- Node24: `product_default`, `active_lts`, dashboard role `default`.
- Node26: `current_non_lts`, `current_non_lts`, dashboard role `current`.
- Canary claims: `37`.
- Canary checks: `101`.
- Required canary gaps: `0`.
- Release-train drift: none.

The 2026-09-01 refresh reports the same lane roles with Node22 `v22.23.2`,
Node24 `v24.20.0`, and Node26 `v26.8.1`. It reports `79` canary claims, no
required canary gaps, and no release-train drift. The current dashboard has no
published canary execution reports, so it reports `0` canary checks instead of
reusing historical executions.

## Verification

- `python3 scripts/runtime/node/release_train.py publish`: pass. It generated
  `node-release-train.json` and `node-release-train.md`.
- `python3 scripts/runtime/node/release_train.py publish --check-proof`: pass.
  The summary has a proof file, a proof README entry, and all digest markers.
- `bash scripts/verify-node-release-train.sh`: pass. Four lanes and zero drift
  entries. The negative self-tests passed.
- `make node-compat-release-train CHECK=1`: pass. The generated release-train
  summary is current.
- `python3 scripts/runtime/node/release_train.py self-test`: pass. The negative
  tests detected tag, lifecycle, dashboard-role, and product-default drift.
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
  owns scheduled/nightly placement for live checks. NFRC11 provides and proves
  the live probe.
- Require source digest markers in this proof. A future lane/tag metadata edit
  cannot pass release-train verification without updating the proof.
- Treat search snippets as advisory only. The official schedule JSON is the
  source of truth when snippets disagree.

## Remaining Risks

- NFRC12 still owns wiring this verifier into PR and scheduled CI lanes.
- NFRC13 still owns the final all-row verifier and closeout pass.
