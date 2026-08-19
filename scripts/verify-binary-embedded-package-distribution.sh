#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Binary-Embedded Package
# Distribution plan (docs/private/plans/binary-embedded-package-distribution-plan.md).
#
# Exits 0 iff every condition in the plan's Completion Gate (1-27) holds.
# Shipped in BPD0 as a FAILING control gate: conditions already true at baseline
# pass, every condition tied to unimplemented BPD1-BPD8 work fails. Each later
# BPD row flips its own conditions to PASS without weakening earlier ones.
#
# Condition 22 (fmt/clippy/docs-refs/git-diff) runs the heavy toolchain checks
# only when BPD_FULL=1, so the default run is fast and purely structural. The
# final `N passed, 0 failed` therefore requires `BPD_FULL=1`.
#
# Run from anywhere; it cd's to the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/binary-embedded-package-distribution-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/binary-embedded-package-distribution-plan.md"
PROOF_DIR="docs/private/plans/proof/binary-embedded-package-distribution"
CONVEX_TMPL="crates/nimbus-assets/embedded/templates/convex/package.json.tmpl"
CF_TMPL="crates/nimbus-assets/embedded/templates/cloud-functions/functions/package.json.tmpl"
CODEGEN_RS="crates/nimbus-cli/src/codegen.rs"
NODE_RS="crates/nimbus-cli/src/node_runtime.rs"
CARGO_ASSETS="crates/nimbus-assets/Cargo.toml"
LAUNCH_PLAN="docs/private/managed-service-launch-plan.md"

if [ -f "${PLAN_ACTIVE}" ]; then
  PLAN="${PLAN_ACTIVE}"
elif [ -f "${PLAN_ARCHIVED}" ]; then
  PLAN="${PLAN_ARCHIVED}"
else
  PLAN=""
fi

# Dereference: BSD/macOS `grep -R` silently skips a bare FILE argument that is
# itself a symlink (as PLAN is in a worktree that bridges untracked
# docs/private content via a symlink), so every `has "${PLAN}"` check below
# would otherwise silently find nothing instead of failing loudly. The
# bridging symlink always carries an absolute target, so plain `readlink`
# suffices — no `readlink -f` needed.
if [ -n "${PLAN}" ] && [ -L "${PLAN}" ]; then
  PLAN="$(readlink "${PLAN}")"
fi

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  FAIL_DETAIL+=("$1")
}

# check <description> <test-command...> : pass iff the command succeeds (exit 0).
check() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

# grep helpers that are safe under `set -u`
has() { grep -RqE "$1" "${@:2}" 2>/dev/null; }      # regex present in files
absent() { ! grep -RqE "$1" "${@:2}" 2>/dev/null; } # regex absent from files

printf '\nBinary-Embedded Package Distribution — completion gate\n'
printf 'plan: %s\n\n' "${PLAN:-<missing>}"

# ---- 1: closeout — every ledger row done (no pending/in_progress cells) -------
c1() {
  [ -n "${PLAN}" ] || return 1
  # Ledger rows only (| BPDn | … | status |). The Execution Log is append-only
  # and legitimately records historical in_progress/pending checkpoints, so the
  # closeout gate ("every ledger row is done") must not be tripped by them.
  ! grep -qE '^\| BPD[0-9]+ .*\| (pending|in_progress) \|$' "${PLAN}"
}
check "1. plan exists and every ledger row is done at closeout" c1

# ---- 2: BPD0 baseline proof exists with file:line evidence [BPD0] -------------
c2() {
  [ -f "${PROOF_DIR}/bpd0-baseline.md" ] &&
    grep -qE '[A-Za-z0-9_./-]+\.(rs|mjs|ts|tsx|json|tmpl|yml|toml):[0-9]+' \
      "${PROOF_DIR}/bpd0-baseline.md"
}
check "2. BPD0 baseline proof exists with file:line evidence" c2

# ---- 3: all packages/*/package.json retain \"private\": true (7/7) ------------
c3() {
  local total priv
  total=$(ls packages/*/package.json 2>/dev/null | wc -l | tr -d ' ')
  priv=$(grep -l '"private": true' packages/*/package.json 2>/dev/null | wc -l | tr -d ' ')
  [ "${total}" = "7" ] && [ "${priv}" = "7" ]
}
check "3. all 7 packages remain private" c3

# ---- 4: no npm-publish workflow / publishConfig / registry auth ---------------
c4() {
  absent 'npm publish|publishConfig|NODE_AUTH_TOKEN|registry-url' \
    .github/workflows packages/*/package.json
}
check "4. no npm-publish workflow or publishConfig" c4

# ---- 5: binary embeds packages version-locked (rust-embed + manifest) [BPD1] --
c5() {
  has 'rust-embed' "${CARGO_ASSETS}" &&
    has 'embedded.?package|EmbeddedPackage|packages/[a-z-]+/dist' crates/nimbus-assets/src
}
check "5. binary embeds packages version-locked through nimbus-assets (rust-embed + manifest)" c5

# ---- 6: provisioned manifests dependency-closed (sanitized) [BPD1] ------------
c6() {
  # Prove FULL dependency closure for every provisioned + co-provisioned package
  # (not just convex/mongodb): every `dependencies` entry must be an embedded
  # root, every peer must be embedded or an explicitly allowed developer-supplied
  # peer. Fails if any unsupported registry dependency survives.
  node scripts/check-package-closure.mjs
}
check "6. provisioned manifests are dependency-closed (sanitized)" c6

# ---- 7: build inputs wired (Makefile + CI/release carry package payload) [BPD1]
c7() {
  # Makefile must wire the staged payload as a binary build input, including the
  # dependency sources that affect the sanitized payload. Linux CI/coverage can
  # share the Ubuntu-built artifact, but release builds must stage the payload in
  # each target job because the tooling closure embeds a native @esbuild package.
  local make_db
  make_db="$(make -pn build-packages 2>/dev/null)" || return 1
  has 'EMBEDDED_PKG_MANIFEST' Makefile &&
    has 'EMBEDDED_PKG_BUILD_SCRIPTS' Makefile &&
    printf '%s\n' "${make_db}" | grep -q 'packages/convex/build\.mjs' &&
    has 'package-lock\.json' Makefile &&
    has 'scripts/check-package-closure\.mjs' Makefile &&
    has 'build:embedded-packages' package.json &&
    has 'Upload embedded-packages artifact' .github/workflows/ci.yml &&
    has 'Download embedded package payload' .github/workflows/ci.yml &&
    has 'Upload embedded package payload' .github/workflows/coverage.yml &&
    has 'Download embedded package payload' .github/workflows/coverage.yml &&
    has 'Build target-local embedded package payload' .github/workflows/release.yml &&
    has 'NIMBUS_EMBEDDED_ESBUILD_PLATFORM' .github/workflows/release.yml &&
    absent 'Download embedded package payload' .github/workflows/release.yml
}
check "7. make build + CI/release treat package payloads as build inputs" c7

# ---- 8: rendered templates use file: specifiers, no registry ranges [BPD3] ----
c8() {
  has 'file:' "${CONVEX_TMPL}" && absent '\^\{\{CONVEX_VERSION\}\}' "${CONVEX_TMPL}"
}
check "8. init templates use file: specifiers for Nimbus packages" c8

# ---- 9: rendered templates contain no @nimbus/codegen dependency [BPD3] -------
c9() {
  # Existence precondition so the absent-check can't pass vacuously if a
  # template is renamed/deleted.
  [ -f "${CONVEX_TMPL}" ] && [ -f "${CF_TMPL}" ] &&
    absent '@nimbus/codegen' "${CONVEX_TMPL}" "${CF_TMPL}"
}
check "9. templates contain no @nimbus/codegen dependency" c9

# ---- 10: no registry range in templates + node.rs fixtures use file: [BPD3] ---
c10() {
  # Existence preconditions so these absent-checks can't pass vacuously if a
  # target file is renamed/deleted. Templates: forbid a `^{{...}}`-style Nimbus
  # version placeholder (the `{{PROJECT_NAME}}` name placeholder is legitimate;
  # the CF template's `firebase-admin`/`firebase-functions`/`typescript` caret
  # ranges are legitimate developer-supplied registry deps — CF is out of
  # contract — so we do NOT forbid caret ranges in templates). node.rs fixtures
  # must carry no caret range at all (broader than the old hyper-specific
  # `^1.0.0`), since they were rewritten to `file:` specifiers.
  [ -f "${CONVEX_TMPL}" ] && [ -f "${CF_TMPL}" ] && [ -f "${NODE_RS}" ] &&
    absent '\^\{\{' "${CONVEX_TMPL}" "${CF_TMPL}" &&
    absent '\^[0-9]+\.[0-9]+\.[0-9]+' "${NODE_RS}"
}
check "10. no registry range in templates; node.rs fixtures rewritten to file:" c10

# ---- 11: Cloud Functions in-contract offline OR documented fallback [BPD3] ----
c11() {
  has 'external Node\.js runner|preinstall|developer-supplied|fallback' \
    docs/private/adapters/cloud-functions/README.md
}
check "11. Cloud Functions classified (offline-installable or fallback)" c11

# ---- 12: provisioning writes .nimbus/packages/* + .version stamp [BPD2] -------
c12() {
  # Require the real BPD2 provisioning entrypoint that writes the payload +
  # `.version` stamp — a distinctive function/command name, not a doc-comment
  # mention of `.nimbus/packages` (the embed module references it in prose) and
  # not the generic word "provision" (which appears in machine-config code).
  has 'fn provision_packages|fn provision_app|nimbus packages install' \
    crates/nimbus-cli/src &&
    has '\.nimbus/' crates/nimbus-assets/embedded/templates/convex/gitignore
}
check "12. provisioning writes .nimbus/packages/* + .version; scaffold gitignores it" c12

# ---- 13: explicit package-provisioning command for client-only apps [BPD2] ---
c13() {
  # The explicit `nimbus packages install` subcommand: the PackagesCommand
  # enum + its run fn + the wired top-level Command::Packages dispatch. CLI
  # logic (including the Command enum + dispatch) lives in nimbus-cli;
  # nimbus-bin is a 5-line entrypoint that calls nimbus_cli::run_from_env().
  # `install` is only half a contract, so require its inverse too: a wired
  # dependency an app cannot unwire is a one-way door.
  has 'enum PackagesCommand' crates/nimbus-cli/src/provision.rs &&
    has 'Install\(InstallArgs\)' crates/nimbus-cli/src/provision.rs &&
    has 'Uninstall\(UninstallArgs\)' crates/nimbus-cli/src/provision.rs &&
    has 'Command::Packages' crates/nimbus-cli/src/lib.rs
}
check "13. explicit 'nimbus packages install'/'uninstall' commands exist" c13

# ---- 14: in-binary codegen is the default (experimental flag retired) [BPD4] --
c14() {
  # Existence precondition + the positive default-runner anchor, so this can't
  # pass vacuously if codegen.rs is deleted/renamed: the retired experimental
  # flag must be absent AND the in-binary default (EmbeddedPilot) must be live.
  [ -f "${CODEGEN_RS}" ] &&
    absent 'NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN' "${CODEGEN_RS}" &&
    has 'EmbeddedPilot' "${CODEGEN_RS}"
}
check "14. in-binary codegen is the default (experimental gate retired)" c14

# ---- 15: ExternalNode classification is internally consistent [BPD4/BPD7] -----
c15() {
  # The contract has ONE coherent story and the gate must fail if code and plan
  # disagree:
  #   * The in-contract Convex surface (schema/server/http/auth.config) runs
  #     in-binary; ExternalNode is its diagnostic/transition-only opt-out only.
  #   * Cloud Functions is the one out-of-contract surface, and ExternalNode is
  #     its SUPPORTED runner.
  # In particular this must FAIL if default code paths auto-route to ExternalNode
  # while the plan still calls ExternalNode merely diagnostic-only.

  # ExternalNode must be classified diagnostic/transition-only (for Convex) in
  # both the runner enum docs and the plan.
  has 'diagnostic/transition-only|diagnostic-only' "${CODEGEN_RS}" || return 1
  has 'diagnostic/transition-only|diagnostic-only' "${PLAN}" || return 1

  # If the default runner can auto-route to ExternalNode...
  if has 'fn resolve_default_codegen_runner' "${CODEGEN_RS}" &&
    has 'return Ok\(CodegenRunner::ExternalNode\)' "${CODEGEN_RS}"; then
    # ...it may do so ONLY for Cloud Functions — never for auth-config or any
    # other in-contract Convex surface.
    has 'fn is_cloud_functions_app' "${CODEGEN_RS}" || return 1
    absent 'fn app_has_auth_config|fn requires_external_node_runner' "${CODEGEN_RS}" || return 1
    # ...and the plan must classify that auto-route as a SUPPORTED out-of-contract
    # surface (Cloud Functions), not blanket "diagnostic-only".
    has 'Out of the in-binary/offline contract' "${PLAN}" || return 1
    has 'not a diagnostic fallback' "${PLAN}" || return 1
    # ...and the plan must keep auth.config IN-contract (never carved out). The
    # contradiction check runs on the LIVE contract (everything before the
    # append-only Execution Log, whose history may legitimately describe the
    # superseded auto-route-auth.config approach), with a tight pattern so it
    # catches "auth.config is out-of-contract"/"auth-config ... auto-route" but
    # not the in-contract list "...auth-config paths; Cloud Functions ... out-of-contract".
    has 'auth.config stays in-contract' "${PLAN}" || return 1
    if sed '/^## Execution Log/q' "${PLAN}" \
      | grep -qiE 'auth[.-]?config[^.;]{0,40}(out-of-contract|auto-route)'; then
      return 1
    fi
  fi
}
check "15. ExternalNode classification is consistent (Convex in-binary/diagnostic-opt-out; CF out-of-contract supported; auth.config in-contract)" c15

# ---- 16: documented codegen is in-binary; no stale npm-style instructions [BPD4]
# User-facing docs scope: live docs + package READMEs, EXCLUDING docs/private/plans/**
# (plans/archive legitimately keep historical `npx convex codegen` prose).
# Scope: live user docs + package READMEs. Excludes docs/private/plans/**, where
# historical plans/proofs legitimately keep old command transcripts.
# Trailing slash on the two directory entries is load-bearing: BSD/macOS
# `grep -R` silently skips a bare directory argument that is itself a symlink
# (as it is in a worktree that bridges untracked docs/private content via a
# symlink), which would make this scanner silently search zero files instead
# of failing loudly.
USER_DOCS=(docs/private/adapters/ docs/private/operating/ packages/convex/README.md
  packages/nimbus/README.md packages/codegen/README.md tests/runtime/node/published)
# Detect a stale POSITIVE codegen instruction (`npx convex codegen`,
# `convex codegen --app`, `nimbus-codegen --app`) while ALLOWING negative
# disclaimers ("there is no `npx convex codegen` step"). Markdown wraps such
# disclaimers across lines, so this is PARAGRAPH-aware: per file it drops bold
# markers, collapses whitespace into one line, then for each match checks the
# preceding ~90 chars for a negation. Returns 0 (success) only if a stale
# positive instruction survives.
stale_codegen_instruction() {
  local file
  for file in $(grep -RlE 'npx (convex|nimbus) codegen|npx nimbus-codegen|convex codegen --app|nimbus-codegen --app' \
    "${USER_DOCS[@]}" 2>/dev/null); do
    perl -0777 -ne '
      s/\*\*//g;            # drop markdown bold: **no** -> no
      s/\s+/ /g;            # join wrapped lines into one
      while (/(npx (?:convex|nimbus) codegen|npx nimbus-codegen|convex codegen --app|nimbus-codegen --app)/gi) {
        my $start = $-[0];
        my $from  = $start > 90 ? $start - 90 : 0;
        my $pre   = substr($_, $from, $start - $from);
        next if $pre =~ /there is no|no separate|no longer|not a |not an |is not|never |instead of|rather than/i;
        exit 0;            # a stale positive instruction with no preceding negation
      }
      exit 1;
    ' "$file" && return 0
  done
  return 1
}
c16() {
  # Positive claim: docs say codegen runs in-binary (the words "codegen" and
  # "in-binary" co-occur in either order, tolerating markdown bold and a few
  # words between — e.g. "Codegen runs **in-binary**", "codegen ... in-binary").
  has 'codegen[^.]{0,40}in-binary|in-binary[^.]{0,40}codegen' \
    docs/private/operating/cli.md docs/private/adapters/convex/compatibility.md || return 1
  # Negative gate: no surviving stale npm-style codegen *instruction* in user
  # docs (negative disclaimers are allowed).
  ! stale_codegen_instruction || return 1
  # Root scripts/Makefile must not preserve the retired npm-style codegen or
  # retired `serve` command as positive runnable commands.
  ! grep -qE 'npx convex codegen|convex codegen --app|nimbus-codegen --app|cargo run -p nimbus-bin -- serve|nimbus serve' \
    Makefile package.json 2>/dev/null
}
check "16. documented codegen is in-binary; no stale npm-style codegen instructions in user docs" c16

# ---- 17: no-network init->install->dev proof [BPD7] --------------------------
c17() {
  [ -f "${PROOF_DIR}/bpd7-offline-integrity.md" ] &&
    grep -qiE 'no.?network|registry unreachable|offline' \
      "${PROOF_DIR}/bpd7-offline-integrity.md"
}
check "17. no-network init->install->dev proof exists" c17

# ---- 18: reconcile re-provisions on version drift, no-op on match [BPD5] ------
c18() {
  has 'packages/\.version|version.?drift' crates/nimbus-cli/src/provision.rs &&
    has 'force_node_reinstall' crates/nimbus-cli/src/provision.rs &&
    has 'provision::ensure' crates/nimbus-cli/src/init.rs &&
    has 'provision::ensure' crates/nimbus-cli/src/dev.rs &&
    has 'ensure_known_app_packages' crates/nimbus-cli/src/codegen.rs &&
    has 'ensure_known_app_packages' crates/nimbus-cli/src/deploy.rs
}
check "18. reconcile re-provisions on binary-version drift" c18

# ---- 19: adapter SDKs provisioned only on request/import [BPD6] ---------------
c19() {
  # Anchor to the live adapter-on-request wiring, not a prose/test token: the
  # `Selection::Adapter` variant (an explicit adapter target) plus the
  # transitive `closure` resolver that pulls only that adapter's dependency set,
  # and the CLI surface that accepts firebase|mongodb|dynamodb. All three are
  # live (non-test) code in provision.rs.
  has 'Selection::Adapter' crates/nimbus-cli/src/provision.rs &&
    has 'fn closure' crates/nimbus-cli/src/provision.rs &&
    has 'firebase.*mongodb.*dynamodb|mongodb.*dynamodb' crates/nimbus-cli/src/provision.rs
}
check "19. adapter SDKs provisioned only when requested/imported" c19

# ---- 20: no docs/READMEs/examples/launch-plan instruct registry install/publish --
c20() {
  # User-facing docs must not instruct a registry install of a Nimbus package.
  # docs/private/plans/* legitimately describes the migration/defect and is excluded.
  # Trailing slash on the two directory args: see USER_DOCS comment above (BSD
  # grep -R silently skips a bare symlinked-directory argument).
  ! grep -RqE 'npm install @nimbus/|npm install convex' \
    docs/private/adapters/ docs/private/operating/ packages/*/README.md examples 2>/dev/null &&
    { [ ! -f "${LAUNCH_PLAN}" ] || ! grep -qE '[Pp]ublish .*to npm' "${LAUNCH_PLAN}"; }
}
check "20. no Nimbus-package registry-install/publish instructions in docs" c20

# ---- 21: provisioned bytes checksum-verified + tamper negative test [BPD7] ----
c21() {
  # The embed-side checksum + tamper mechanism exists in nimbus-assets,
  # but C21 is about *provisioned* bytes verified end-to-end, proven in BPD7.
  # Require package + tooling checksum verification, provisioned-byte checking,
  # and the BPD7 offline-integrity proof.
  has 'fn verify_digest' crates/nimbus-assets/src/js_packages.rs &&
    has 'manifest\.tooling' crates/nimbus-assets/src/js_packages.rs &&
    has 'materialize_tooling' crates/nimbus-assets/src/js_packages.rs &&
    has 'verify_package_dirs' crates/nimbus-cli/src/provision.rs &&
    [ -f "${PROOF_DIR}/bpd7-offline-integrity.md" ]
}
check "21. provisioned bytes verify against manifest checksums (+ tamper test)" c21

# ---- 22: fmt / clippy / docs-refs / git-diff (heavy; BPD_FULL=1) [BPD8] -------
c22() {
  [ "${BPD_FULL:-0}" = "1" ] || return 1
  cargo fmt --all --check >/dev/null 2>&1 || return 1
  git diff --check >/dev/null 2>&1 || return 1
  npm run docs:validate-refs:strict >/dev/null 2>&1 || return 1
  make clippy >/dev/null 2>&1 || return 1
}
check "22. cargo fmt + clippy + docs-refs + git diff --check pass (BPD_FULL=1)" c22

# ---- 23: offline boundaries documented + proven in-contract only [BPD7] -------
c23() {
  grep -qE '^## Offline contract boundaries' "${PLAN}" 2>/dev/null &&
    [ -f "${PROOF_DIR}/bpd7-offline-integrity.md" ]
}
check "23. offline contract boundaries documented and proven in-contract only" c23

# ---- 24: provisioned convex dist emits all 4 exports [BPD1] -------------------
c24() {
  # Assert the sanitized convex dist manifest declares all four subpath exports
  # (./server ./values ./react ./browser) AND each target file exists — not a
  # filename-count heuristic that a partial dist could satisfy.
  local manifest="packages/convex/dist/package.json"
  [ -f "${manifest}" ] || return 1
  node -e '
    const fs = require("node:fs");
    const path = require("node:path");
    const dir = "packages/convex/dist";
    const m = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    const exp = m.exports || {};
    const required = ["./server", "./values", "./react", "./browser"];
    for (const key of required) {
      const target = typeof exp[key] === "string" ? exp[key] : exp[key] && (exp[key].default || exp[key].import);
      if (!target) { console.error(`missing export ${key}`); process.exit(1); }
      if (!fs.existsSync(path.join(dir, target))) { console.error(`missing target ${target} for ${key}`); process.exit(1); }
    }
  ' 2>/dev/null
}
check "24. provisioned convex dist emits server/values/react/browser exports" c24

# ---- 25: fresh-clone-then-install handled + lockfile policy documented [BPD2] -
c25() {
  # Two halves: (1) the policy is documented in the proof, AND (2) the actual
  # provision-before-install handling is wired in code — `provision::ensure`
  # (provision-if-absent) called from BOTH `init` (after scaffold) and `dev`
  # (before the npm install loop). The code half stops this from passing on a
  # proof keyword alone.
  #
  # Trailing slash on PROOF_DIR is load-bearing: BSD/macOS `grep -R` does not
  # descend into a bare directory argument that is itself a symlink (as it is
  # in a worktree that bridges untracked docs/private content via a symlink)
  # — it needs the trailing slash to resolve as a directory.
  grep -RqiE 'committed-lockfile|provision.*before.*npm ci|clone-then-install' \
    "${PROOF_DIR}/" 2>/dev/null &&
    has 'provision::ensure' crates/nimbus-cli/src/init.rs &&
    has 'provision::ensure' crates/nimbus-cli/src/dev.rs &&
    has 'ensure_known_app_packages' crates/nimbus-cli/src/codegen.rs &&
    has 'ensure_known_app_packages' crates/nimbus-cli/src/deploy.rs
}
check "25. fresh-clone-then-install path handled; lockfile policy documented" c25

# ---- 26: re-provision forces Node dependency reinstall [BPD5] -----------------
c26() {
  has 'invalidate.*fingerprint|force.*reinstall|reinstall.*node_modules' crates/nimbus-cli/src
}
check "26. re-provision on drift forces a Node dependency reinstall" c26

# ---- 27: managed-service SDK-distribution decision resolved + recorded [BPD6] -
c27() {
  ! grep -qE 'Resolve before BPD6' "${PLAN}" 2>/dev/null &&
    { [ ! -f "${LAUNCH_PLAN}" ] || ! grep -qE '[Pp]ublish .*to npm' "${LAUNCH_PLAN}"; }
}
check "27. managed-service SDK-distribution decision resolved and recorded" c27

# ---- summary ------------------------------------------------------------------
printf '\n%d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf '\nfailing conditions (expected until the owning BPD row lands):\n'
  for d in "${FAIL_DETAIL[@]}"; do printf '  - %s\n' "${d}"; done
  exit 1
fi
exit 0
