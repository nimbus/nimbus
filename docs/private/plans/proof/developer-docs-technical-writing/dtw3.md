# DTW3 strict-mode proof

Date: 2026-08-01
Baseline: PR #272 head @ `e8d83d1bc3694a7639e13c577c866757c01b18c4`
Work commit: `37c103fd3`

## Fail-before result

Command:

```bash
/Users/jack/.agents/skills/technical-writing/scripts/technical-writing lint \
  docs/developers --mode strict --format text
```

Result: exit status 1. Strict mode reported 30 diagnostics across 15 pages.
The diagnostics contained 27 passive-voice errors, 2 indirect-verb errors,
and 1 nominalization error.

## Strict-mode result

The same command exited with status 0 after the revisions. All 23 developer
pages passed with zero diagnostics.

The human conformance review confirmed these conditions:

- Each revision names the relevant actor or uses a direct verb.
- Each page keeps its original purpose and Diataxis mode.
- Available source-map evidence continues to support every behavior claim.
- Conditions appear before their actions.
- Each procedural sentence contains one action.
- Commands, links, identifiers, values, limits, and failure behavior retain
  their original meaning.
- The revisions add no unsupported claim, invented contrast, rhetorical
  fragment, filler, or repeated conclusion.
- The repository configuration does not require a project glossary. The
  terminology review found no renamed or inconsistent concepts.

## Protected-content result

The comparison checked all 23 pages against the prior PR head. It found these
results:

- source-map coverage: 23 of 23 pages
- changed fenced code blocks: 0
- changed inline-code sets: 0
- pages revised for strict mode: 15

`git diff --check` also exited with status 0.

## Documentation gates

Commands:

```bash
bash scripts/check-docs.sh
npm --prefix website run build
bash scripts/verify-nimbus-docs-site.sh
```

Results:

- `check-docs.sh` passed 108 pages. Links, source paths, the private fence,
  and titles were valid.
- Astro built 109 pages and emitted `llms.txt`, `llms-full.txt`, and
  `llms-small.txt`.
- The site verifier passed all 17 conditions.
