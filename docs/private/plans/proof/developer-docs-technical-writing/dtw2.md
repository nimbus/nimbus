# DTW2 final-gate proof

Date: 2026-08-01
Baseline: main @ `1ba104f876078551207ef0eabf90e214c9d1e4e3`

## Technical-writing conformance

Command:

```bash
/Users/jack/.agents/skills/technical-writing/scripts/technical-writing lint \
  docs/developers --mode developer --format text
```

Result: exit status 0. All 23 pages passed with zero errors and 30 warnings.
The warning rate stayed below the configured 1.5 warnings per 100 words.

The human review confirmed these conditions:

- Each page keeps its original purpose and Diataxis mode.
- Conditions precede actions when the condition changes the result.
- Procedural sentences contain one instruction.
- Commands, identifiers, links, values, limits, and failure behavior retain
  their original meaning.
- The revised prose does not add claims, filler, invented contrasts, or
  unsupported recommendations.
- The remaining passive-voice and indirect-verb warnings preserve object
  state, established platform terms, or exact contract language.

## Claim and protected-content checks

The source-map coverage check found 23 developer pages, zero pages without a
source-map row, and zero changed fenced code blocks. `git diff --check` also
exited with status 0.

## Documentation gates

Commands:

```bash
bash scripts/check-docs.sh
npm ci --prefix website
npm --prefix website run build
bash scripts/verify-nimbus-docs-site.sh
```

Results:

- `check-docs.sh` passed 108 pages. Links, source paths, the private fence,
  and titles were valid.
- The locked website dependencies installed successfully.
- Astro built 109 pages and emitted `llms.txt`, `llms-full.txt`, and
  `llms-small.txt`.
- The site verifier passed all 17 conditions.

The dependency installation reported 14 audit findings in the repository's
existing website dependency lock: 1 low, 4 moderate, and 9 high. This writing
change does not modify the dependency manifest or lock file.

## Pre-PR review

Command:

```bash
NIMBUS_AUTOREVIEW="${AGENTS_HOME:-$HOME/.agents}/skills/nimbus-autoreview/scripts/nimbus-autoreview"
"$NIMBUS_AUTOREVIEW" --gate pre-pr --mode auto
```

Result: exit status 0. The automatic gate classified the branch as having no
substantive code changes and skipped code review. It reported no findings.
