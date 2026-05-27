# MBA5 Dual-Target Tests Proof

posture: env_selected_dual_target_probes
selector: NIMBUS_TEST_TARGET

## Coverage

| Adapter | Nimbus target | External target | Probe |
| --- | --- | --- | --- |
| convex | `nimbus` | `convex_cloud` | invalid bearer on function query surface |
| firebase | `nimbus` | `firebase_cloud` | invalid bearer on Firestore REST document read |
| cloud_functions | `nimbus` | `cloud_functions_cloud` | invalid bearer on callable HTTPS surface |
| mongodb | `nimbus` | `mongodb_cloud` | invalid SCRAM credentials through MongoDB driver |

Each probe keeps the same assertion body and swaps only the target endpoint or
URI through `NIMBUS_TEST_TARGET`. The tests can run in
`NIMBUS_DUAL_TARGET_DRY_RUN=1` mode to validate target registration without
live credentials, and they fail closed when dry-run is disabled but the target
URL/URI is missing.

## Nightly Workflow

`.github/workflows/dual-target-nightly.yml` defines the target matrix with the
Nimbus and external targets. Nimbus-local matrix entries stay in dry-run mode
until they are pointed at a local test server. Cloud matrix entries set
`requires_live: true`, which disables dry-run and requires the corresponding
GitHub secret URL/URI so missing live credentials fail loudly instead of
silently passing as registration checks.
