#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Nimbus Docs Site plan
# (`docs/private/plans/archive/nimbus-docs-site-plan.md`, DOC0..DOC13).
#
# Exits 0 iff every condition in the plan's Verification section is satisfied.
# Ships in DOC0 so /goal is verifiable from day one; DOC1..DOC13 progressively
# flip conditions from FAIL to PASS.
#
# The site is a Next static export rendered by fumadocs over `docs/`. It was a
# Starlight site when this plan closed; the conditions moved with it, so the
# same 17 outcomes are still the gate.
#
# Run from the repo root. Condition 6 checks build artifacts by default; set
# NIMBUS_DOCS_VERIFY_BUILD=1 to run the website build inside the verifier.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 1

WEBSITE_PKG="website/package.json"
NEXT_CONFIG="website/next.config.mjs"
ROOT_LAYOUT="website/src/app/layout.tsx"
SOURCE_CONFIG="website/source.config.ts"
HOME="website/src/app/page.tsx"
LANDING="website/src/app/(docs)/docs/page.tsx"
TOKENS="website/src/styles/tokens.css"
OUT="website/out"
WRANGLER_CONFIG="website/wrangler.jsonc"
DOCS_WF=".github/workflows/docs.yml"
SOURCE_MAP="docs/source-map.md"
CHECK_DOCS="scripts/check-docs.sh"

PUBLIC_GROUPS=(get-started developers agents operators concepts reference)

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
  network-control-plane
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
C="1. website/package.json declares next + fumadocs-core + fumadocs-mdx + fumadocs-ui"
if [[ -f "${WEBSITE_PKG}" ]] \
  && grep -q '"next"' "${WEBSITE_PKG}" \
  && grep -q '"fumadocs-core"' "${WEBSITE_PKG}" \
  && grep -q '"fumadocs-mdx"' "${WEBSITE_PKG}" \
  && grep -q '"fumadocs-ui"' "${WEBSITE_PKG}" \
  && ! grep -q '"@astrojs/starlight"' "${WEBSITE_PKG}"; then
  pass "${C}"
else
  fail "${C}" "missing ${WEBSITE_PKG}, one of the four required dependencies, or a leftover Starlight dependency"
fi

# --- 2. next config: static export + canonical site URL ----------------------
# `output: 'export'` is what makes the build a directory of files rather than a
# server, and `trailingSlash` is what keeps every `docs/` link correct. The site
# URL lives in the root layout, because `metadataBase` is what resolves the
# relative canonical and social URLs the pages declare.
C="2. next.config.mjs sets output:'export' + trailingSlash; layout sets metadataBase=https://nimbusdocs.com"
if [[ -f "${NEXT_CONFIG}" ]] \
  && grep -Eq "output:\s*'export'" "${NEXT_CONFIG}" \
  && grep -Eq "trailingSlash:\s*true" "${NEXT_CONFIG}" \
  && [[ -f "${ROOT_LAYOUT}" ]] \
  && grep -q "metadataBase: new URL('https://nimbusdocs.com')" "${ROOT_LAYOUT}"; then
  pass "${C}"
else
  fail "${C}" "missing ${NEXT_CONFIG}, static export, trailing slashes, or the canonical site URL in ${ROOT_LAYOUT}"
fi

# --- 3. content loaded only from the six public docs/ groups -----------------
# The allow-list is the loader glob in `source.config.ts`: the collection reads
# `../docs` directly and compiles only `<group>/**/*.md`, so `docs/private`,
# `docs/brand` and `docs/assets` cannot publish even by accident. Nothing is
# copied into the package, so there is no second tree to keep honest.
C="3. fumadocs loads only docs/{get-started,developers,agents,operators,concepts,reference} (source.config.ts allow-list)"
listed=""
if [[ -f "${SOURCE_CONFIG}" ]]; then
  listed="$(sed -n "/^const PUBLISHED_GROUPS/,/\] as const/p" "${SOURCE_CONFIG}" \
    | grep -oE "'[a-z-]+'" | tr -d "'" | tr '\n' ' ' | sed 's/ $//')"
fi
copied="$(find website/src/content -mindepth 1 2>/dev/null | head -5)"
if [[ "${listed}" == "${PUBLIC_GROUPS[*]}" ]] \
  && grep -q "dir: '../docs'" "${SOURCE_CONFIG}" \
  && grep -q 'files: PUBLISHED_GROUPS.map' "${SOURCE_CONFIG}" \
  && [[ -z "${copied}" ]]; then
  pass "${C}"
else
  fail "${C}" "allow-list is [${listed:-none}] not [${PUBLIC_GROUPS[*]}], loader does not read ../docs through it, or content was copied into [${copied:-none}]"
fi

# --- 4. public groups each have a landing page --------------------------------
C="4. docs/{get-started,developers,agents,operators,concepts,reference}/ each have a landing page"
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
C="5. docs/ top level = six groups + assets/ + brand/ + private/ + README.md + source-map.md; no published links into docs/private"
allowed="get-started developers agents operators concepts reference assets brand private README.md source-map.md"
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
C="6. website build emits out/ with index.html, the three llms files, sitemap.xml, and the search index"
if [[ "${NIMBUS_DOCS_VERIFY_BUILD:-0}" == "1" && -f "${WEBSITE_PKG}" ]]; then
  npm --prefix website run build >/dev/null 2>&1 || true
fi
missing_out=()
for artifact in index.html docs/index.html llms.txt llms-full.txt llms-small.txt sitemap.xml api/search; do
  [[ -e "${OUT}/${artifact}" ]] || missing_out+=("${artifact}")
done
if [[ -d "${OUT}" && ${#missing_out[@]} -eq 0 ]]; then
  pass "${C}"
else
  fail "${C}" "missing ${OUT} or artifacts [${missing_out[*]:-}] (run: npm --prefix website run build)"
fi

# --- 7. wrangler config: static assets, no worker script ---------------------
C="7. wrangler.jsonc sets assets.directory=./out and has no main"
if [[ -f "${WRANGLER_CONFIG}" ]] \
  && grep -q '"directory": "./out"' "${WRANGLER_CONFIG}" \
  && ! grep -Eq '"main"[[:space:]]*:' "${WRANGLER_CONFIG}"; then
  pass "${C}"
else
  fail "${C}" "missing ${WRANGLER_CONFIG}, assets directory, or unexpected main entry"
fi

# --- 8. docs.yml pipeline ------------------------------------------------------
C="8. docs.yml builds the export with npm ci + npm run build, has paths, resilient PR preview, main deploy, concurrency; actionlint clean"
if [[ -f "${DOCS_WF}" ]] \
  && grep -q 'docs/get-started/\*\*' "${DOCS_WF}" \
  && grep -q 'docs/assets/\*\*' "${DOCS_WF}" \
  && grep -q 'website/\*\*' "${DOCS_WF}" \
  && grep -q 'run: npm ci' "${DOCS_WF}" \
  && grep -q 'run: npm run build' "${DOCS_WF}" \
  && ! grep -q 'docs/private' "${DOCS_WF}" \
  && grep -q 'concurrency' "${DOCS_WF}" \
  && grep -Eq 'versions upload|wrangler-action' "${DOCS_WF}" \
  && grep -q 'GITHUB_STEP_SUMMARY' "${DOCS_WF}" \
  && grep -q 'retries: 3' "${DOCS_WF}" \
  && grep -q 'continue-on-error: true' "${DOCS_WF}" \
  && grep -q 'steps.preview_comment.outcome' "${DOCS_WF}" \
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

# --- 14. the two entrances + the palette + DESIGN.md ------------------------
# `/` is the Odyssey, the scroll-driven pitch. `/docs/` is the written entrance
# to the same story. Neither has Markdown behind it, so both are checked as
# source files.
#
# The banned list is every accent the palette has retired, by literal and by
# token name: `#b45309` (the red-shifted amber) and `#866423` (the gold
# darkened at its own hue), plus `--accent-edge` and `--accent-link`, the two
# slots those literals lived in. The accent is now two colours -- the gold and
# its ink -- and `--accent-text` picks between them, so a reappearance of any
# of these is a third gold coming back.
C="14. / renders the Odyssey, /docs/ is the landing, the palette has no retired accent, DESIGN.md documents the surface"
stale_accent="$(grep -rIlE 'b45309|866423|accent-(edge|link)' website/src 2>/dev/null | head -3 || true)"
if [[ -f "${HOME}" ]] \
  && grep -q 'Journey' "${HOME}" \
  && [[ -f "${LANDING}" ]] \
  && grep -q 'DocsPage' "${LANDING}" \
  && [[ -f "${TOKENS}" ]] \
  && grep -q '#f0b23e' "${TOKENS}" \
  && grep -q -- '--accent-text' "${TOKENS}" \
  && [[ -z "${stale_accent}" ]] \
  && grep -q 'Documentation site' DESIGN.md 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "missing Odyssey home, /docs landing, gold accent token, --accent-text, DESIGN.md entry, or a retired accent still in [${stale_accent:-none}]"
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
stale_refs="$(grep -rIl 'docs/plans/' \
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
C="17. docs/concepts/architecture/ matches the DOC7 manifest exactly, with source-map entries"
arch_missing=()
for s in "${ARCH_MANIFEST[@]}"; do
  if [[ ! -f "docs/concepts/architecture/${s}.md" && ! -f "docs/concepts/architecture/${s}.mdx" ]]; then
    arch_missing+=("${s}")
  elif [[ -f "${SOURCE_MAP}" ]] && ! grep -q "${s}" "${SOURCE_MAP}"; then
    arch_missing+=("${s}(no source-map entry)")
  fi
done
# The check runs both ways. Manifest-into-tree alone passes while a page is
# absent from the manifest, which leaves that page outside the gate and free
# to go stale or disappear unnoticed. `index` is the section landing page and
# carries no system of its own.
for f in docs/concepts/architecture/*.md docs/concepts/architecture/*.mdx; do
  [[ -e "${f}" ]] || continue
  slug="$(basename "${f}")"
  slug="${slug%.*}"
  [[ "${slug}" == "index" ]] && continue
  listed=0
  for s in "${ARCH_MANIFEST[@]}"; do
    [[ "${s}" == "${slug}" ]] && listed=1 && break
  done
  ((listed)) || arch_missing+=("${slug}(page not in manifest)")
done
if [[ ${#arch_missing[@]} -eq 0 && -f "${SOURCE_MAP}" ]]; then
  pass "${C}"
else
  fail "${C}" "missing/unmapped systems: ${arch_missing[*]:-source-map absent}"
fi

# --- 18. the two token sheets agree ------------------------------------------
# The website sheet's own header says the values are identical to the console's
# and that "a change to one sheet belongs in the other". Nothing enforced it,
# and the sheets are edited by different tasks: docs work touches one, console
# work the other. So a role could drift in one theme only, which is exactly the
# defect that is invisible until someone toggles the theme on the wrong
# surface.
#
# The comparison is per theme, because the two sheets select the themes
# oppositely: the console defaults to dark on bare `:root` and overrides light
# under `[data-theme="light"]`, while the docs default to the reader's system
# scheme, so light is the bare `:root` block and dark lives under `.dark`. Only
# the role declarations are compared; `color-scheme` is a host concern and the
# `@theme` blocks are each host's own utility namespace.
C="18. the console and website token sheets declare the same roles and values in both themes"
drift="$(
  python3 - <<'DRIFT' 2>&1 || true
import re, sys

CONSOLE = "packages/nimbus-ui/src/styles/tokens.css"
WEBSITE = "website/src/styles/tokens.css"

def block(path, selector):
    """The role declarations of one theme block, as {name: value}."""
    src = open(path, encoding="utf-8").read()
    start = src.index(selector) + len(selector)
    body = src[start : src.index("\n}", start)]
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    return {
        name: value.strip()
        for name, value in re.findall(r"(--[a-z0-9-]+)\s*:\s*([^;]+);", body)
    }

themes = {
    "dark": (block(CONSOLE, ":root {"), block(WEBSITE, ":root:is(.dark) {")),
    "light": (
        block(CONSOLE, ':root[data-theme="light"] {'),
        block(WEBSITE, ":root {"),
    ),
}

for theme, (console, website) in themes.items():
    for name in sorted(set(console) | set(website)):
        here, there = console.get(name), website.get(name)
        if here != there:
            print(f"{theme} {name}: console={here or 'absent'} website={there or 'absent'}")
DRIFT
)"
if [[ -z "${drift}" ]]; then
  pass "${C}"
else
  fail "${C}" "$(printf '%s' "${drift}" | head -6 | tr '\n' ';')"
fi

# --- 19. the two mascot drawings agree ---------------------------------------
# `packages/nimbus-ui/src/components/mascot.tsx` is the single source of the
# mark, but the docs site is a separate Next app and cannot import from the
# console package, so it carries a hand copy -- the same arrangement as the two
# token sheets above, and the same failure mode. A face added or a coordinate
# nudged on one side alone splits the mark in two, which nobody sees until the
# console and the site are open side by side.
#
# The comparison is of the drawing, not the file: the path data, the literal
# geometry on each shape, the named eye and mouth coordinates, the two face
# weights, and the set of states. The hosting differs on purpose -- the console
# takes a pixel `size` and drops its animation classes under reduced motion,
# the site sizes by class and leans on its stylesheet -- so none of that is
# compared.
C="19. the console and website mascot components draw the same mark and the same states"
drift="$(
  python3 - <<'DRIFT' 2>&1 || true
import re
from collections import Counter

CONSOLE = "packages/nimbus-ui/src/components/mascot.tsx"
WEBSITE = "website/src/components/mascot.tsx"
SHAPE = ("cx", "cy", "r", "x", "y", "width", "height", "rx", "strokeWidth")

def drawing(path):
    src = re.sub(r"//[^\n]*", "", open(path, encoding="utf-8").read())
    marks = []
    # Path data under any of the three JavaScript quotes. (The quotes are
    # spelled by code point because a literal backtick would close this
    # command substitution.)
    for q in ("'", '"', chr(96)):
        for lit in re.findall(rf"{q}([Mm][ \-0-9][^{q}]*){q}", src):
            marks.append("path " + " ".join(lit.split()))
    # Literal geometry on the shapes themselves.
    for name, value in re.findall(rf"\b({'|'.join(SHAPE)})=\"([^\"]+)\"", src):
        marks.append(f"{name} {value}")
    # The named coordinates and the two face weights.
    for name, value in re.findall(r"\bconst (EYE_[LRY]|MOUTH_Y) = ([-0-9.]+);", src):
        marks.append(f"{name} {value}")
    for w, r in re.findall(r"\{ w: ([-0-9.]+), r: ([-0-9.]+) \}", src):
        marks.append(f"weight {w} {r}")
    # The state vocabulary, from the MascotState union.
    union = re.search(r"export type MascotState =(.*?);", src, re.S).group(1)
    for state in re.findall(r"[\"']([a-z]+)[\"']", union):
        marks.append(f"state {state}")
    return Counter(marks)

console, website = drawing(CONSOLE), drawing(WEBSITE)
for mark, n in (console - website).items():
    print(f"{mark}: console has it {n} more time(s)")
for mark, n in (website - console).items():
    print(f"{mark}: website has it {n} more time(s)")
DRIFT
)"
if [[ -z "${drift}" ]]; then
  pass "${C}"
else
  fail "${C}" "$(printf '%s' "${drift}" | head -6 | tr '\n' ';')"
fi

# --- summary -------------------------------------------------------------------------
printf '\n%d/%d conditions green\n' "${PASS}" "$((PASS + FAIL))"
if [[ ${FAIL} -gt 0 ]]; then
  printf 'failing:\n'
  for d in "${FAIL_DETAIL[@]}"; do printf '  - %s\n' "${d}"; done
  exit 1
fi
exit 0
