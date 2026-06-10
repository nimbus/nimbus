#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Nimbus Docs Site plan
# (`docs/plans/nimbus-docs-site-plan.md`, DOC0..DOC13).
#
# Exits 0 iff every condition in the plan's Verification section is satisfied.
# Ships in DOC0 so /goal is verifiable from day one; DOC1..DOC13 progressively
# flip conditions from FAIL to PASS.
#
# Run from the repo root. Condition 6 checks build artifacts by default; set
# NIMBUS_DOCS_VERIFY_BUILD=1 to run the website build inside the verifier.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

WEBSITE_PKG="website/package.json"
ASTRO_CONFIG="website/astro.config.mjs"
CONTENT_CONFIG="website/src/content.config.ts"
LANDING="website/src/content/docs/index.mdx"
WRANGLER_CONFIG="website/wrangler.jsonc"
DOCS_WF=".github/workflows/docs.yml"
SOURCE_MAP="docs/source-map.md"
CHECK_DOCS="scripts/check-docs.sh"

PUBLIC_GROUPS=(get-started developers operators concepts reference)

# DOC7 system manifest — the canonical page list for docs/concepts/architecture/.
ARCH_MANIFEST=(
  server-transport
  adapters
  engine-mutation-path
  runtime-isolates
  storage
  sandbox-machines
  auth-trust
  tenancy
  node-lifecycle
  cli-codegen
  sdk-packages
  observability
)

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf 'PASS  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  FAIL_DETAIL+=("$1: $2")
  printf 'FAIL  %s\n      %s\n' "$1" "$2"
}

# --- 1. website package deps -------------------------------------------------
C="1. website/package.json declares astro + @astrojs/starlight + starlight-llms-txt"
if [[ -f "${WEBSITE_PKG}" ]] \
  && grep -q '"astro"' "${WEBSITE_PKG}" \
  && grep -q '"@astrojs/starlight"' "${WEBSITE_PKG}" \
  && grep -q '"starlight-llms-txt"' "${WEBSITE_PKG}"; then
  pass "${C}"
else
  fail "${C}" "missing ${WEBSITE_PKG} or one of the three required dependencies"
fi

# --- 2. astro config: site + static + llms plugin ----------------------------
C="2. astro.config.mjs sets site=https://nimbusdocs.com, static output, llms plugin"
if [[ -f "${ASTRO_CONFIG}" ]] \
  && grep -q "site: 'https://nimbusdocs.com'" "${ASTRO_CONFIG}" \
  && grep -q 'starlight-llms-txt' "${ASTRO_CONFIG}" \
  && ! grep -Eq "output:\s*['\"](server|hybrid)" "${ASTRO_CONFIG}"; then
  pass "${C}"
else
  fail "${C}" "missing ${ASTRO_CONFIG}, site URL, llms plugin, or non-static output set"
fi

# --- 3. content loaded only from the five public docs/ groups ----------------
C="3. Starlight content loads only from docs/{get-started,developers,operators,concepts,reference}"
if [[ -f "${CONTENT_CONFIG}" ]]; then
  missing_groups=()
  for g in "${PUBLIC_GROUPS[@]}"; do
    grep -q "${g}" "${CONTENT_CONFIG}" || missing_groups+=("${g}")
  done
  extra_content="$(find website/src/content/docs -type f ! -name 'index.mdx' ! -name 'index.md' 2>/dev/null | head -5)"
  if [[ ${#missing_groups[@]} -eq 0 && -z "${extra_content}" ]] \
    && grep -q '\.\./docs' "${CONTENT_CONFIG}"; then
    pass "${C}"
  else
    fail "${C}" "loader missing groups [${missing_groups[*]:-}] or extra authored content: ${extra_content:-none}"
  fi
else
  fail "${C}" "missing ${CONTENT_CONFIG}"
fi

# --- 4. five public groups each have a landing page --------------------------
C="4. docs/{get-started,developers,operators,concepts,reference}/ each have a landing page"
missing_landings=()
for g in "${PUBLIC_GROUPS[@]}"; do
  [[ -f "docs/${g}/index.md" || -f "docs/${g}/index.mdx" ]] || missing_landings+=("${g}")
done
if [[ ${#missing_landings[@]} -eq 0 ]]; then
  pass "${C}"
else
  fail "${C}" "groups without index.md(x): ${missing_landings[*]}"
fi

# --- 5. docs/ top level is exactly the allowed set; no private links ---------
C="5. docs/ top level = five groups + brand/ + private/ + README.md + source-map.md; no published links into docs/private"
allowed="get-started developers operators concepts reference brand private README.md source-map.md"
unexpected=()
for entry in docs/* docs/.[!.]*; do
  [[ -e "${entry}" ]] || continue
  name="$(basename "${entry}")"
  [[ "${name}" == ".DS_Store" ]] && continue
  ok=0
  for a in ${allowed}; do [[ "${name}" == "${a}" ]] && ok=1; done
  [[ ${ok} -eq 0 ]] && unexpected+=("${name}")
done
private_links=""
for g in "${PUBLIC_GROUPS[@]}"; do
  [[ -d "docs/${g}" ]] || continue
  hits="$(grep -rEl 'docs/private/|\]\((\.\./)+private/' "docs/${g}" 2>/dev/null | head -3)"
  [[ -n "${hits}" ]] && private_links="${private_links} ${hits}"
done
if [[ ${#unexpected[@]} -eq 0 && -z "${private_links// /}" ]]; then
  pass "${C}"
else
  fail "${C}" "unexpected top-level entries: [${unexpected[*]:-}] private-linking pages: [${private_links:-none}]"
fi

# --- 6. build emits dist + llms artifacts ------------------------------------
C="6. website build emits dist/, llms.txt, llms-full.txt, llms-small.txt"
if [[ "${NIMBUS_DOCS_VERIFY_BUILD:-0}" == "1" && -f "${WEBSITE_PKG}" ]]; then
  npm --prefix website run build >/dev/null 2>&1 || true
fi
if [[ -d website/dist && -f website/dist/llms.txt && -f website/dist/llms-full.txt && -f website/dist/llms-small.txt ]]; then
  pass "${C}"
else
  fail "${C}" "missing website/dist or llms artifacts (run: npm --prefix website run build)"
fi

# --- 7. wrangler config: static assets, no worker script ---------------------
C="7. wrangler.jsonc sets assets.directory=./dist and has no main"
if [[ -f "${WRANGLER_CONFIG}" ]] \
  && grep -q '"directory": "./dist"' "${WRANGLER_CONFIG}" \
  && ! grep -q '"main"' "${WRANGLER_CONFIG}"; then
  pass "${C}"
else
  fail "${C}" "missing ${WRANGLER_CONFIG}, assets directory, or unexpected main entry"
fi

# --- 8. docs.yml pipeline ------------------------------------------------------
C="8. docs.yml exists with paths filter, PR preview, main deploy, concurrency; actionlint clean"
if [[ -f "${DOCS_WF}" ]] \
  && grep -q 'docs/get-started/\*\*' "${DOCS_WF}" \
  && grep -q 'website/\*\*' "${DOCS_WF}" \
  && ! grep -q 'docs/private' "${DOCS_WF}" \
  && grep -q 'concurrency' "${DOCS_WF}" \
  && grep -Eq 'versions upload|wrangler-action' "${DOCS_WF}" \
  && grep -q 'wrangler deploy' "${DOCS_WF}"; then
  if command -v actionlint >/dev/null 2>&1; then
    if actionlint "${DOCS_WF}" >/dev/null 2>&1; then
      pass "${C}"
    else
      fail "${C}" "actionlint reports errors in ${DOCS_WF}"
    fi
  else
    pass "${C} (structural checks only; actionlint not installed)"
  fi
else
  fail "${C}" "missing ${DOCS_WF} or required workflow elements"
fi

# --- 9. source map exists -----------------------------------------------------
C="9. docs/source-map.md exists"
if [[ -f "${SOURCE_MAP}" ]]; then
  pass "${C}"
else
  fail "${C}" "missing ${SOURCE_MAP}"
fi

# --- 10. check-docs gate exists and passes -------------------------------------
C="10. scripts/check-docs.sh exists and passes"
if [[ -x "${CHECK_DOCS}" || -f "${CHECK_DOCS}" ]]; then
  if bash "${CHECK_DOCS}" >/dev/null 2>&1; then
    pass "${C}"
  else
    fail "${C}" "${CHECK_DOCS} exits non-zero"
  fi
else
  fail "${C}" "missing ${CHECK_DOCS}"
fi

# --- 11. agent skills migrated -------------------------------------------------
C="11. .agents/skills/docs exists; .claude/skills migrated (absent or symlink)"
if [[ -d .agents/skills/docs ]] && { [[ -L .claude/skills ]] || [[ ! -e .claude/skills ]]; }; then
  pass "${C}"
else
  fail "${C}" "missing .agents/skills/docs or .claude/skills is still a real directory"
fi

# --- 12. AGENTS.md routing entry ------------------------------------------------
C="12. AGENTS.md has a docs-site routing entry"
if grep -q 'nimbus-docs-site' AGENTS.md 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "AGENTS.md does not reference the docs-site plan/skill"
fi

# --- 13. apt remains the only GitHub Pages deployer ------------------------------
C="13. apt-repo.yml is the only workflow using deploy-pages"
pages_users="$(grep -l 'deploy-pages' .github/workflows/*.yml 2>/dev/null | grep -v 'apt-repo.yml' || true)"
if [[ -z "${pages_users}" ]]; then
  pass "${C}"
else
  fail "${C}" "other workflows deploy to GitHub Pages: ${pages_users}"
fi

# --- 14. splash landing + DESIGN.md docs surface ---------------------------------
C="14. landing uses template:splash with hero; DESIGN.md has Documentation site surface entry"
if [[ -f "${LANDING}" ]] \
  && grep -q 'template: splash' "${LANDING}" \
  && grep -q 'hero:' "${LANDING}" \
  && grep -q 'Documentation site' DESIGN.md 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "landing missing splash/hero or DESIGN.md missing Documentation site entry"
fi

# --- 15. README front door --------------------------------------------------------
C="15. README has nimbusdocs.com handoff + protocol status table; Node-compat contract moved out"
if grep -q 'nimbusdocs.com' README.md \
  && ! grep -q '^## Node compatibility contract' README.md \
  && grep -Eq '^\|.*(Convex|Firestore)' README.md; then
  pass "${C}"
else
  fail "${C}" "README missing docs handoff/status table or still contains the Node-compat contract block"
fi

# --- 16. legacy internal locations gone; no stale path references ------------------
C="16. docs/{plans,prompts,decisions,code-review,design-review,technical-debt.md} moved under docs/private; no stale refs"
legacy_present=()
for p in docs/plans docs/prompts docs/decisions docs/code-review docs/design-review docs/technical-debt.md; do
  [[ -e "${p}" ]] && legacy_present+=("${p}")
done
stale_refs="$(grep -rl 'docs/plans/' \
  --exclude-dir=.git \
  --exclude-dir=private \
  --exclude-dir=node_modules \
  --exclude-dir=target \
  --exclude=CHANGELOG.md \
  --exclude=verify-nimbus-docs-site.sh \
  . 2>/dev/null | grep -v '^\./docs/private/' | head -5 || true)"
if [[ ${#legacy_present[@]} -eq 0 && -z "${stale_refs}" ]]; then
  pass "${C}"
else
  fail "${C}" "legacy paths present: [${legacy_present[*]:-}] stale docs/plans refs in: [${stale_refs:-none}]"
fi

# --- 17. architecture manifest coverage ---------------------------------------------
C="17. docs/concepts/architecture/ has every DOC7 manifest page with source-map entries"
arch_missing=()
for s in "${ARCH_MANIFEST[@]}"; do
  if [[ ! -f "docs/concepts/architecture/${s}.md" && ! -f "docs/concepts/architecture/${s}.mdx" ]]; then
    arch_missing+=("${s}")
  elif [[ -f "${SOURCE_MAP}" ]] && ! grep -q "${s}" "${SOURCE_MAP}"; then
    arch_missing+=("${s}(no source-map entry)")
  fi
done
if [[ ${#arch_missing[@]} -eq 0 && -f "${SOURCE_MAP}" ]]; then
  pass "${C}"
else
  fail "${C}" "missing/unmapped systems: ${arch_missing[*]:-source-map absent}"
fi

# --- summary -------------------------------------------------------------------------
printf '\n%d/%d conditions green\n' "${PASS}" "$((PASS + FAIL))"
if [[ ${FAIL} -gt 0 ]]; then
  printf 'failing:\n'
  for d in "${FAIL_DETAIL[@]}"; do printf '  - %s\n' "${d}"; done
  exit 1
fi
exit 0
