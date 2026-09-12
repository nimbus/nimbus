# DR3 README banner

Commit 2a5e188a2 on `nimbus-docs-restyle`.

Fail-before: README had no mark at all (the wisp banner had already been removed on
main; only the console GIF remained), and `docs/brand/` held only the wisp variants.

Changes: `docs/brand/mascot/{mascot-gold,mascot-white,mascot-tile,mascot-template}.svg`,
`docs/brand/mascot/render.sh`; README `<picture>` banner (white under dark scheme, gold
default) above the canonical headline; DESIGN.md brand section now documents the mark files
instead of the wisp variant table; `icon-512.png` re-rendered from `mascot-tile.svg`.

`bash scripts/check-docs.sh`: PASS, 109 pages link-clean, source map resolves, private fence
intact, titles unique. The wisp variants under `docs/brand/logo/` and `gen-variants.sh` are
still referenced by the Starlight config and go with it in DR4.
