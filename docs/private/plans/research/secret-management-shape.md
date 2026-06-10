# Research: Secret Management Shape (Superseded)

> **Status:** Superseded on 2026-05-19 by
> `docs/plans/secret-management-plan.md` (canonical execution plan) and
> `docs/plans/research/secret-management-prior-art.md` (canonical
> prior-art research).

This note was the original gap-identification artifact: a sketch of
the missing tenant-scoped secret-store surface, written when no real
plan owned the work. Every load-bearing item from the original note —
required properties, rough storage shape, host-bridge API, admission
gate, cluster shape, migration path, and open questions — now lives in
its canonical home:

| Original section | New canonical home |
|---|---|
| What We Have Today / Why This Gap Matters Now | `docs/plans/secret-management-plan.md` § Why Secret Management Needs A Plan |
| Required Properties | `docs/plans/secret-management-plan.md` § Required Invariants |
| Rough Shape Of The Answer (storage, host-bridge, admission, cluster) | `docs/plans/secret-management-plan.md` § Proposed Internal Shape |
| Migration from env vars | `docs/plans/secret-management-plan.md` § Migration from existing env-var indirection |
| Existing Consumers To Migrate | `docs/plans/secret-management-plan.md` § Relationship To Other Plans |
| Open Questions For The Future Plan | `docs/plans/secret-management-plan.md` Phase S0 inputs + `docs/plans/research/secret-management-prior-art.md` § Decisions A Future Plan Must Make |
| External + internal references | `docs/plans/research/secret-management-prior-art.md` § References |

The shape note is retained as this redirect so existing links keep
working. **Do not edit the historical content below; update the
canonical plan instead.** For any new "what's the secret-management
story?" question, start at `docs/plans/secret-management-plan.md`.
