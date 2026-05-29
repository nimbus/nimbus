# NFRC10 Deno-Style Docs

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node20 `v20.20.2`, Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC10 implementation wave. The
NFRC10-specific changes add Deno-style public Node runtime docs, generate API
and package references from checked-in evidence, and harden stale-prose guards
so public docs cannot drift into stale pass rates, stale default-priority
language, Node26 preview wording, or host-heavy in-process overclaims.

## Files Changed

- Public runtime docs:
  `docs/runtimes/nodejs/README.md`,
  `docs/runtimes/nodejs/fundamentals.md`,
  `docs/runtimes/nodejs/configuration.md`,
  `docs/runtimes/nodejs/packages-and-bundling.md`
- Generated public reference pages:
  `docs/runtimes/nodejs/compatibility.md`,
  `docs/runtimes/nodejs/reference/node-apis.md`,
  `docs/runtimes/nodejs/reference/packages.md`
- Generated evidence index:
  `docs/runtimes/nodejs/evidence/README.md`
- FaaS support vocabulary and generator:
  `docs/architecture/runtime/node-faas-compatibility-profile.json`,
  `docs/architecture/runtime/node-faas-compatibility-profile.md`,
  `scripts/runtime/node/publish_docs.py`
- Stale support guard:
  `scripts/runtime/node/docs_guard.py`
- Control plane:
  `docs/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC10 followed the required wide-then-focused loop for documentation claims:

1. Generate the broad public compatibility, API, and package references from
   the current profile, dashboard, status summary, and canary registry.
2. Run the docs guard across the public support docs to get stale-claim
   feedback.
3. Fix the specific guard gaps and missing generated-doc snippets.
4. Rerun the full docs verifier and strict Markdown reference validation.

The generated pages are intentionally Deno-style: fundamentals explain how to
use the feature, the compatibility page gives the version contract, and
reference pages list API/package support boundaries backed by evidence.

## Wide Feedback And Focused Fixes

Initial docs verification:

```bash
bash scripts/verify-node-lts-docs.sh
```

The first guard run exposed four issues:

| Surface | Initial feedback | Resolution |
| --- | --- | --- |
| Negative full-Node wording | The guard treated "does not claim full Node" as an overclaim. | Narrowed the forbidden pattern to reject positive `Nimbus claims/supports/provides complete/full Node` language. |
| Node26 wording | The guard treated "not a preview label" as stale preview framing. | Narrowed the forbidden pattern to reject preview table rows or preview target/lane labels, while allowing explicit "not preview" text. |
| Generated package reference | The required diagnostic snippet missed the generated backticked `` `Diagnostic` `` wording. | Matched the generated wording exactly. |
| Public profile state | Several already-proven API families still rendered as `Planned by NFRC`. | Promoted invocation lifecycle, builtins, env/secrets, observability, Convex integration, AI/SaaS SDKs, and Node26 Current evidence to `current_evidence` with dashboard/canary refs. |

Final docs verification passed after these focused fixes.

## Generated Contract

The generated public pages now include:

- `docs/runtimes/nodejs/compatibility.md`: per-version support table,
  support vocabulary, public contract, canary summary, and reference links.
- `docs/runtimes/nodejs/reference/node-apis.md`: API-family support table with
  verification state, evidence refs, and host-heavy boundary text.
- `docs/runtimes/nodejs/reference/packages.md`: package-class table and
  canary matrix with `Support` vs `Diagnostic` evidence and support boundary.

The hand-written docs point readers to the generated contract:

- `README.md` routes new readers through fundamentals, compatibility, API, and
  package reference pages.
- `fundamentals.md` explains `"use node"`, Node24 default vs evidence
  priority, Node22/Node24 supported LTS, Node26 Current/non-LTS, Node20
  legacy-grace, permissions, packages, and host-heavy service/microVM routing.
- `configuration.md` names Node26 as Current/non-LTS rather than preview and
  links the generated API/package references.
- `packages-and-bundling.md` separates staged package support from native,
  subprocess, raw-listen, and persistent-filesystem boundaries.

## Verification

- `make node-compat-publish-docs`: pass; generated evidence docs under
  `docs/runtimes/nodejs/evidence/` and public docs under
  `docs/runtimes/nodejs/`.
- `make node-compat-publish-docs CHECK=1`: pass; generated Node.js runtime
  evidence docs are current.
- `bash scripts/verify-node-faas-compat-profile.sh`: pass, 6 statuses, 4
  lanes, 11 API families, 7 package classes, 4 doc claims, and negative
  self-tests.
- `bash scripts/verify-node-lts-docs.sh`: pass; generated docs current and
  public docs avoid stale pass-rate, support-priority, and host-heavy
  overclaim prose.
- `npm run docs:validate-refs:strict`: pass, 231 working-tree Markdown files.
- `git diff --check`: pass.
- `cargo fmt --all --check`: pass.

## Decisions

- Generate the public compatibility, API, and package reference pages from the
  checked-in FaaS profile, dashboard, status summary, and canary registry
  rather than hand-maintaining support tables.
- Keep pass-rate percentages only in generated evidence pages. Public
  fundamentals/configuration pages describe support policy and route readers to
  generated evidence.
- Treat Node26 as Current/non-LTS in public docs. It is selectable and carries
  lane-local evidence, but it is not enterprise LTS support until Node itself
  enters LTS and supported-LTS gates pass.
- Keep host-heavy rows in the generated package matrix as diagnostic
  service/microVM-required evidence, not positive in-process package support.

## Remaining Risks

- NFRC11 still owns release-train automation so new Node tags, lifecycle
  changes, and Node26 LTS promotion do not require hand discovery.
- NFRC12 still owns CI/nightly scheduling so docs guard and canaries run in
  the right PR and scheduled lanes.
