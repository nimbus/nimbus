# CM3: Pin ubuntu runners to ubuntu-24.04

CM3 replaces every `runs-on: ubuntu-latest` (and the one matrix
`runner: ubuntu-latest`) with `ubuntu-24.04` across the workflow
tree. ARM runners (`ubuntu-24.04-arm`) were already explicitly pinned
and are unchanged.

## Why pin

`ubuntu-latest` is a moving alias: GitHub repoints it from one Ubuntu
LTS to the next on a phased schedule (most recently 22.04 → 24.04).
The transition window is announced but the cutover is a silent change
for any workflow that still references the alias. That is exactly the
kind of "next Tuesday CI suddenly diffs in production" trap that an
auditable CI surface should avoid.

Pinning to `ubuntu-24.04` makes the runner image explicit. Dependabot
already tracks `ubuntu-24.04` and can open a PR to bump to the next
LTS when one ships, surfacing the change as reviewable diff.

## Sites changed (25 total)

`.github/workflows/ci.yml` (14 sites): rust-format (34),
rust-clippy (52), deny (85), rust-runtime-tests (108),
rust-workspace-tests (134), external-provider-tests (182),
warm-sccache (256), ui-artifacts (296), harness (335),
harness-nightly (368), coverage (415), proof-helpers (460),
js (485), rust-gate-summary (539).

`.github/workflows/desktop-ui.yml` (1 site): desktop-ui (50).

`.github/workflows/release.yml` (2 sites): verify-release-contract
(39), publish-release-notes (546).

`.github/workflows/node-compat-nightly.yml` (2 sites):
node-compat-rust-corpus (25), node-compat-evidence (43).

`.github/workflows/apt-repo.yml` (2 sites): build (58),
deploy (252).

`.github/workflows/copr-srpms.yml` (1 site): srpm-build (58).

`.github/workflows/linux-distribution-release.yml` (1 site):
orchestrator (32).

`.github/workflows/verify-nimbus-crun-patch.yml` (1 site):
verify (54).

`.github/workflows/linux-packages.yml` (1 site): matrix entry
`amd64: ubuntu-latest` (45) → `ubuntu-24.04`. The matrix shape
stays identical to keep the arm64 sibling unchanged.

## Sites intentionally not changed

- `ubuntu-22.04` runners (release.yml line 149) stay pinned for
  glibc/abi compatibility with the release archive. The release
  build matrix is deliberate about runner image selection per
  target triple; bumping to 24.04 would change the glibc the
  binary links against.
- `ubuntu-24.04-arm` runners are already explicit.
- `macos-14` and `windows-latest` (release.yml) are outside CM3
  scope (different platforms; their pinning is owned separately).

## Verifier delta

Before CM3:

- Condition 5 (no ubuntu-latest): FAIL — 24 `runs-on:` hits + 1
  matrix `runner:` hit.

After CM3:

- Condition 5: PASS — `grep -rn "ubuntu-latest" .github/workflows/`
  returns no matches.
