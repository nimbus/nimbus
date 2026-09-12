# DR0 Baseline

- Baseline: `main @ b527e1742`, branch `nimbus-docs-restyle` created 2026-09-12.
- Site: Astro Starlight under `website/` (astro ^7.2.2, @astrojs/starlight ^0.41.7,
  starlight-llms-txt ^0.11.0). Content = symlinks `website/src/content/docs/<group>`
  to `docs/<group>` plus `index.mdx` splash.
- Published pages by group: get-started 5, developers 23, agents 8, operators 15,
  concepts 23, reference 34 (108 total).
- `bash scripts/verify-nimbus-docs-site.sh` on the baseline: **16/17 conditions green**.
  Condition 5 fails on main before this plan: `docs/assets/` (console GIF, commit
  706e94ac3) is an unexpected top-level entry. Output in `dr0-verify.txt`.
  DR7 owns the rewrite of this verifier and must allow `assets/`.
- Pre-existing dirty files not owned by this plan: `examples/nimbus/agent-chat/package.json`,
  `package-lock.json`.
