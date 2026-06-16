#!/usr/bin/env bash
# Honesty gates for the public docs corpus (nimbusdocs.com).
#
# 1. Dead links — every internal link in the six public groups (and the
#    landing page) resolves to a published page or shipped asset.
# 2. Source map — every doc page and every cited source path in
#    docs/source-map.md exists in the repository.
# 3. docs/private fence — nothing published references docs/private/, and
#    the website content loader exposes only the six public groups.
# 4. Title uniqueness — every published page has a frontmatter title that
#    is unique across the corpus (case-insensitive).
#
# Run from anywhere: bash scripts/check-docs.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

python3 - <<'PYEOF'
import os, re, sys, glob

GROUPS = ["get-started", "developers", "agents", "operators", "concepts", "reference"]
LANDING = "website/src/content/docs/index.mdx"
# Build-emitted artifacts that are valid link targets but have no .md source.
EMITTED = {"/llms.txt", "/llms-full.txt", "/llms-small.txt", "/sitemap-index.xml"}

failures = []

def fail(check, msg):
    failures.append(f"[{check}] {msg}")

# --- collect the published page set -------------------------------------------
pages = set()       # site paths, normalized with trailing slash
files = []          # markdown files to scan for links
for g in GROUPS:
    for f in glob.glob(f"docs/{g}/**/*.md", recursive=True):
        files.append(f)
        rel = f[len("docs/"):-len(".md")]
        if rel.endswith("/index"):
            rel = rel[: -len("/index")]
        pages.add("/" + rel + "/")
pages.add("/")  # landing
if os.path.exists(LANDING):
    files.append(LANDING)

def strip_code(text):
    text = re.sub(r"```.*?```", "", text, flags=re.S)
    text = re.sub(r"`[^`\n]*`", "", text)
    return text

# --- 1. dead links --------------------------------------------------------------
link_re = re.compile(r"\]\(([^)\s]+)\)")
for f in files:
    body = strip_code(open(f, encoding="utf-8").read())
    base = os.path.dirname(f)
    for m in link_re.finditer(body):
        target = m.group(1).split("#", 1)[0]
        if not target:
            continue  # pure anchor
        if re.match(r"^[a-z][a-z0-9+.-]*:", target):
            continue  # external scheme (https:, mailto:, ...)
        if target.startswith("/"):
            if target in EMITTED:
                continue
            norm = target if target.endswith("/") else target + "/"
            if norm in pages:
                continue
            # shipped static asset (favicon etc.)
            if os.path.exists("website/public" + target):
                continue
            fail("links", f"{f}: dead internal link {target}")
        else:
            resolved = os.path.normpath(os.path.join(base, target))
            if not os.path.exists(resolved):
                fail("links", f"{f}: dead relative link {target}")

# --- 2. source-map resolution ----------------------------------------------------
SOURCE_MAP = "docs/source-map.md"
if not os.path.exists(SOURCE_MAP):
    fail("source-map", f"missing {SOURCE_MAP}")
else:
    cell_split = re.compile(r"(?<!\\)\|")
    tick_re = re.compile(r"`([^`]+)`")
    pathish = re.compile(r"[/.]")  # contains a slash or an extension dot
    for n, line in enumerate(open(SOURCE_MAP, encoding="utf-8"), 1):
        line = line.rstrip()
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in cell_split.split(line)[1:-1]]
        if len(cells) < 3 or cells[0] in ("Doc page", "Old path", "Staged file", "Source") \
           or set(cells[0]) <= {"-", " "}:
            continue
        # doc-page cell: every backticked *.md token must exist under docs/
        for tok in tick_re.findall(cells[0]):
            if tok.endswith(".md") and not os.path.exists("docs/" + tok):
                fail("source-map", f"line {n}: doc page docs/{tok} does not exist")
        # source cell: every path-like backticked token must exist in the repo
        for tok in tick_re.findall(cells[-1]):
            if not pathish.search(tok):
                continue
            p = tok.rstrip("/")
            if not os.path.exists(p):
                fail("source-map", f"line {n}: cited source {tok} does not exist")

# --- 3. docs/private fence --------------------------------------------------------
for f in files:
    text = open(f, encoding="utf-8").read()
    if "docs/private" in text:
        fail("fence", f"{f}: references docs/private")
    if "](/private/" in text:
        fail("fence", f"{f}: links into /private/")
    lowered = re.sub(r"[\s-]+", " ", strip_code(text).lower())
    if "open source" in lowered:
        fail("fence", f"{f}: claims 'open source' (Nimbus is source-available)")

content_dir = "website/src/content/docs"
allowed = set(GROUPS) | {"index.mdx"}
entries = set(os.listdir(content_dir))
if entries != allowed:
    fail("fence", f"{content_dir} entries {sorted(entries)} != allowed {sorted(allowed)}")
for g in GROUPS:
    p = os.path.join(content_dir, g)
    if not os.path.islink(p):
        fail("fence", f"{p} is not a symlink")
    elif os.path.realpath(p) != os.path.realpath(f"docs/{g}"):
        fail("fence", f"{p} does not resolve to docs/{g}")

if os.path.isdir("website/dist"):
    if os.path.isdir("website/dist/private"):
        fail("fence", "website/dist/private exists in build output")
    for llms in glob.glob("website/dist/llms*.txt"):
        if "docs/private" in open(llms, encoding="utf-8", errors="replace").read():
            fail("fence", f"{llms} references docs/private")

# --- 4. title uniqueness ------------------------------------------------------------
title_re = re.compile(r"^title:\s*(.+?)\s*$")
seen_titles = {}
for f in files:
    lines = open(f, encoding="utf-8").read().splitlines()
    if not lines or lines[0].strip() != "---":
        fail("titles", f"{f}: missing frontmatter block")
        continue
    title = None
    for line in lines[1:]:
        if line.strip() == "---":
            break
        m = title_re.match(line)
        if m:
            title = m.group(1).strip("'\"")
            break
    if not title:
        fail("titles", f"{f}: missing frontmatter title")
        continue
    key = title.lower()
    if key in seen_titles:
        fail("titles", f"{f}: duplicate title {title!r} (also in {seen_titles[key]})")
    else:
        seen_titles[key] = f

# --- report -----------------------------------------------------------------------
if failures:
    print(f"check-docs: FAIL ({len(failures)})")
    for f in failures:
        print("  " + f)
    sys.exit(1)
print(f"check-docs: PASS — {len(files)} pages link-clean, source map resolves, private fence intact, titles unique")
PYEOF
