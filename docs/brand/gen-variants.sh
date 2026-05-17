#!/usr/bin/env bash
# Regenerate brand-palette variants from the canonical Nimbus logo SVG.
#
# Reads:   packages/nimbus-ui/public/nimbus-logo.svg (L0 canonical)
# Writes:  docs/brand/logo/nimbus-<variant>.svg (9 files)
#
# Idempotent: running twice produces byte-identical output (verified via
# sha256sum diff in docs/plans/brand-system-plan.md §L9).
#
# See docs/plans/brand-system-plan.md for variant table source of truth.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
canonical="${repo_root}/packages/nimbus-ui/public/nimbus-logo.svg"
out_dir="${repo_root}/docs/brand/logo"

if [[ ! -f "${canonical}" ]]; then
  echo "error: canonical SVG missing at ${canonical}" >&2
  exit 1
fi

mkdir -p "${out_dir}"

# Variant table: name|stroke|fill|background
# Source: docs/plans/brand-system-plan.md §L2
variants=(
  "warm|#0F172A|#FFE7B3|#FFFAF2"
  "cool-blue|#3B82F6|#FFFFFF|#F8FAFC"
  "night-blue|#60A5FA|#1E293B|#0B1220"
  "monochrome|#111827|#FFFFFF|#FFFFFF"
  "reverse-mono|#FFFFFF|#111827|#111827"
  "sunset-red|#DC2626|#FFFFFF|#FEF2F2"
  "soft-purple|#9333EA|#FFFFFF|#FAF5FF"
  "golden-hour|#D97706|#FFFFFF|#FFFBEB"
  "slate|#475569|#FFFFFF|#F1F5F9"
)

# Read canonical SVG content once.
canonical_content="$(cat "${canonical}")"

# Extract viewBox dimensions for the background rect. The canonical is
# always "0 0 382 261"; we derive width/height to keep this script
# resilient to future viewBox edits.
viewbox_line="$(grep -o 'viewBox="[^"]*"' "${canonical}" | head -1)"
read -r vb_x vb_y vb_w vb_h <<<"$(echo "${viewbox_line}" | sed -E 's/viewBox="([^"]+)"/\1/' | tr -d '\n')"

for entry in "${variants[@]}"; do
  IFS='|' read -r name stroke fill bg <<<"${entry}"
  out="${out_dir}/nimbus-${name}.svg"

  # Substitute:
  #   1. var(--logo-fill, transparent)    -> ${fill}
  #   2. var(--logo-stroke, currentColor) -> ${stroke}
  #   3. <title>Nimbus</title>            -> <title>Nimbus (${name})</title>
  #   4. Insert background <rect> as the first child of <svg>.
  python3 - "${canonical}" "${out}" "${name}" "${stroke}" "${fill}" "${bg}" \
    "${vb_x}" "${vb_y}" "${vb_w}" "${vb_h}" <<'PY'
import sys, pathlib

src, dst, name, stroke, fill, bg, vb_x, vb_y, vb_w, vb_h = sys.argv[1:]
content = pathlib.Path(src).read_text()

content = content.replace("var(--logo-fill, transparent)", fill)
content = content.replace("var(--logo-stroke, currentColor)", stroke)
content = content.replace("<title>Nimbus</title>", f"<title>Nimbus ({name})</title>")

rect = f'  <rect x="{vb_x}" y="{vb_y}" width="{vb_w}" height="{vb_h}" fill="{bg}"/>\n'
content = content.replace("  <title>", rect + "  <title>", 1)

pathlib.Path(dst).write_text(content)
PY

  echo "wrote ${out#${repo_root}/}"
done
