# Documentation

This tree is the source of truth for [nimbusdocs.com](https://nimbusdocs.com).
Pages are plain Markdown rendered by the Astro Starlight project in
[`website/`](../website/); the published groups are exactly:

- [`get-started/`](get-started/) — what Nimbus is, the developer quickstart,
  the self-host quickstart, and the Convex on-ramp
- [`developers/`](developers/) — tutorials and how-to guides for building
  apps on Nimbus
- [`operators/`](operators/) — self-hosting: deploy, tenants, storage
  backends, encryption, networking, observability
- [`concepts/`](concepts/) — how Nimbus works, including the source-verified
  architecture pages
- [`reference/`](reference/) — CLI, configuration, APIs, compatibility
  matrices

[`brand/`](brand/) holds logo assets. [`source-map.md`](source-map.md) maps
published behavior claims to the source files that implement them — update it
when a page's load-bearing claim changes.

`private/` is internal working state (plans, research, proofs, reviews,
prompts) and is never published or linked from the groups above.

Start with the root [README.md](../README.md) for what Nimbus is and
[ARCHITECTURE.md](../ARCHITECTURE.md) for the contributor-level deep dive.
