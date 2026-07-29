# SUC0.1 — Main Full-CI Triage

Date: 2026-07-29, at plan-creation main head `6dc42b793`.

| Red lane | Attribution | Disposition |
| --- | --- | --- |
| CI → Rust Workspace Tests shard 3/3 (`projection_flush_never_observes_no_marker_and_no_work`) | Pre-existing torn-snapshot hazard: `ProjectionWork::stats()` reads its fields non-atomically while the claim path registers the covering reservation before incrementing `catch_up_projection_count`; a snapshot taken mid-claim tears into (depth=0, count=1). Latent forever; the SQLite campaign's ~2.4× faster commits shifted drain timing into the window under shard load. Seam invariant itself verified intact (reservation precedes count). | **FIXED** in this PR: the test polls for one untorn snapshot showing the claimed catch-up together with its covering reservation (held projection lock keeps the pair stable). 30/30 stress-clean after; the pre-fix single-test stress could not reproduce in isolation (load-dependent), matching CI-only observation. |
| Coverage | Downstream of the same test failure. | Expected green after this fix merges; verify on next main run. |
| Node Compatibility | Failing on every run since at least 2026-07-25 — predates the SQLite campaign (started 07-27). Separate scheduled lane. | Pre-existing, non-campaign. Remains an open repo item outside this plan's ownership; recorded here so it is not silently absorbed. |

Local verification note: full `nimbus-system` suite requires the documented
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` opt-out — the
provider takeover tests fail loudly by design without live fixtures and are
CI-service-container lanes, not flakes. An earlier grep pipeline in this
session's triage misread both stress passes and these fixture failures;
counts above are from corrected runs.
