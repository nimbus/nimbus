# Tenant Function Autoscaling Proof

This proof is the closeout ledger for
`docs/private/plans/tenant-function-autoscaling-plan.md`.

## TFA0

- Verifier: `bash scripts/verify-tenant-function-autoscaling.sh`
- Scope: public tenant scaling contract, operator resource envelope, shared
  admission seam, diagnostics, and closeout evidence.

## TFA1

- Public tenant knobs are limited to `preset`, `min_warm`, `max_warm`, and
  `scale_down_delay`.
- Autoscaling is inferred from `preset` and the requested min/max range.
- Public `activation_warm`, `autoscaling`, and `live_scaling` inputs reject as
  unknown fields.

## TFA2

- Operator policy owns a resource-first runtime envelope.
- Pool-count caps are retained only as runtime-safety guardrails.

## TFA3

- `nimbus start`, `nimbus dev`, `nimbus explain functions`,
  `nimbus validate functions`, and `nimbus run functions` use the same
  operator admission seam.
- `nimbus start` carries a selector-aware admitted plan set into the server and
  runtime registries instead of reducing all function scaling to the default
  selector.
- `nimbus start --policy FILE` loads the explicit operator policy used for
  scaling admission.

## TFA4

- Runtime request types do not expose activation or live-controller toggles.
- Tenant-inferred autoscaling does not enable live adaptive-controller
  actuation.

## TFA5

- Focused tests cover public-field rejection, default scale-to-zero behavior,
  fixed/range inference, resource-derived admission, and diagnostics.
- Focused tests cover start-path selector overrides, explicit operator-policy
  rejection, runtime selector lookup, and Convex runtime-lane propagation.

## TFA6

- Closeout must record exact outputs for:
  - `cargo fmt --all --check`
  - focused cargo tests for runtime, tenant, and CLI scaling modules
  - `bash scripts/verify-profile-aware-isolate-runtime.sh`
  - `bash scripts/verify-tenant-function-autoscaling.sh`
  - `git diff --check`

## Closeout Evidence

- `cargo fmt --all --check`: passed with no formatter output.
- `cargo check -p nimbus-runtime -p nimbus-tenant -p nimbus-convex -p nimbus-cloud-functions -p nimbus-server -p nimbus-bin`:
  finished successfully.
- `cargo test -p nimbus-bin function_scaling::tests --bin nimbus`: 11 passed,
  0 failed, 0 ignored, 751 filtered out.
- `cargo test -p nimbus-bin start::tests::cli_surface::start_function_scaling_admission --bin nimbus`:
  2 passed, 0 failed, 0 ignored, 760 filtered out.
- `cargo test -p nimbus-tenant runtime_scaling --lib`: 4 passed, 0 failed,
  0 ignored, 86 filtered out.
- `cargo test -p nimbus-runtime runtime_policy_clone_with_effective_plan_preserves_operational_controls --lib`:
  1 passed, 0 failed, 0 ignored, 1126 filtered out.
- `cargo test -p nimbus-convex convex_registry_applies_selector_scaling_plan_set_to_runtime_lanes --lib`:
  1 passed, 0 failed, 0 ignored, 22 filtered out.
- `bash -n scripts/verify-profile-aware-isolate-runtime.sh`: passed.
- `bash -n scripts/verify-tenant-function-autoscaling.sh`: passed.
- `bash scripts/verify-profile-aware-isolate-runtime.sh`: 108 passed, 0 failed.
- `bash scripts/verify-tenant-function-autoscaling.sh`: 32 passed, 0 failed.
