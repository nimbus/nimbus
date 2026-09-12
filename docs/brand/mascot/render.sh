#!/usr/bin/env bash
# Render the raster mark set from the mascot SVGs in this directory.
#
# Writes into the console:
#   packages/nimbus-ui/public/icon-512.png   (apple-touch-icon; gold mascot on the night tile)
# Writes into the docs site:
#   website/public/og.png                    (1200x630 social card)
# Writes into DESKTOP_DIR/buildResources when DESKTOP_DIR is set:
#   icon.png (512), icon.icns, icon.ico, trayTemplate.png (24x16), trayTemplate@2x.png (48x32)
#
# Needs rsvg-convert, magick, and iconutil (macOS).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${here}/../../.." && pwd)"
tile="${here}/mascot-tile.svg"
template="${here}/mascot-template.svg"
og="${here}/og.svg"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

for tool in rsvg-convert magick iconutil; do
  command -v "${tool}" >/dev/null || { echo "error: ${tool} not found" >&2; exit 1; }
done

rsvg-convert -w 512 -h 512 "${tile}" -o "${repo_root}/packages/nimbus-ui/public/icon-512.png"
echo "wrote packages/nimbus-ui/public/icon-512.png"

rsvg-convert -w 1200 -h 630 "${og}" -o "${repo_root}/website/public/og.png"
echo "wrote website/public/og.png"

if [[ -z "${DESKTOP_DIR:-}" ]]; then
  exit 0
fi
out="${DESKTOP_DIR}/buildResources"
[[ -d "${out}" ]] || { echo "error: ${out} is not a directory" >&2; exit 1; }

cp "${repo_root}/packages/nimbus-ui/public/icon-512.png" "${out}/icon.png"

iconset="${work}/icon.iconset"
mkdir -p "${iconset}"
for size in 16 32 128 256 512; do
  rsvg-convert -w "${size}" -h "${size}" "${tile}" -o "${iconset}/icon_${size}x${size}.png"
  double=$((size * 2))
  rsvg-convert -w "${double}" -h "${double}" "${tile}" -o "${iconset}/icon_${size}x${size}@2x.png"
done
iconutil -c icns "${iconset}" -o "${out}/icon.icns"

ico_parts=()
for size in 16 24 32 48 64 128 256; do
  rsvg-convert -w "${size}" -h "${size}" "${tile}" -o "${work}/ico-${size}.png"
  ico_parts+=("${work}/ico-${size}.png")
done
magick "${ico_parts[@]}" "${out}/icon.ico"

# The tray template is alpha only: macOS tints it for the menu bar.
rsvg-convert -w 24 -h 16 "${template}" -o "${out}/trayTemplate.png"
rsvg-convert -w 48 -h 32 "${template}" -o "${out}/trayTemplate@2x.png"
echo "wrote ${out}/{icon.png,icon.icns,icon.ico,trayTemplate.png,trayTemplate@2x.png}"
