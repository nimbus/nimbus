# Firebase field-path repair

Baseline: `b527e1742ab3c86f8e896765b7d4e63b964116ab`.
Changed source: `packages/firebase/src/internal/document-data.ts`.
Regression: `testFieldPathOwnPropertySurface` in the Firebase selftest.
Host: macOS arm64. Node: 24.19.0.

## Defect and repair

`setDoc` followed an inherited `__proto__` property and changed `Object.prototype`.
The baseline failed the regression because the prototype gained the test marker.

`Object.hasOwn` now limits path traversal to document fields.

`Object.defineProperty` writes ordinary data properties without invoking the inherited prototype setter.
The repair preserves field names and the existing JSON wire format.

The selftest covers eight write cases through `setDoc`, `updateDoc`, and `mergeFields`.
It checks nested `__proto__`, a leaf `__proto__`, `constructor.prototype`, and an own `toString` field.
Three snapshot reads return undefined for absent inherited fields.
Three merge selections reject absent inherited fields before a transport request.

## Verification

| Command | Result |
| --- | --- |
| `npm run test -w firebase` on baseline plus regression | FAIL: prototype mutation |
| `npm run test -w firebase` after repair | PASS: complete Firebase selftest, including 14 new cases |
| `npm run typecheck -w firebase` | PASS |
| `npm run build -w firebase` | PASS |
| `npm run lint:capability-boundary` | PASS |
| `cargo fmt --all --check` | PASS |
| `make clippy` | PASS: workspace and all targets, 69 seconds |
| `bash scripts/check-docs.sh` | PASS: 109 pages |
| `bash scripts/verify-nimbus-docs-site.sh` | FAIL: 16/17 conditions pass. Existing docs/assets directory violates condition 5. |
| `timeout 900 make ci` | UNVERIFIED: stopped with exit 143 after tests contacted the active development server |
| CLI path-fixture recheck through Nextest | PASS: both tests after replacing the target symlink with a local directory |
| Nimbus autoreview, `pre-pr` gate | Pending commit |

Clippy first stopped because the shared development install lacked UI dependencies.
A separate `npm ci` from the checked-in lockfile repaired the verification worktree.
The later Clippy run passed.

The website build passed. The docs layout failure also exists in the baseline.
The baseline contains `docs/assets/2026-09-09_nimbus-console.gif`.

The CI run passed Clippy, dependency deny, and 528 runtime tests. The runtime lane ignored 99 tests.
The workspace lane passed 4,803 tests before cancellation.
It recorded 18 CLI failures from live-server discovery and two path-comparison failures from the shared target symlink.
Cancellation terminated 12 tests and left 2,948 unrun. The runner skipped 111 tests.

The local CLI machine wrapper discovers the host server before it uses the test roots.
The host server returned HTTP 404 because its machine lifecycle manager was unavailable.
The run stopped to prevent further calls to that active server.

Both path-comparison tests pass after correction of the verification setup.
The worktree now uses a real local target directory for temporary fixtures.
Cargo still uses the original shared build artifacts through an absolute CARGO_TARGET_DIR.

Two intermediate regression runs used an incorrect BatchGet response fixture.
The final fixture follows the existing JSON-lines transport contract.

## Manual audit

The write extraction and merge mask callers create their own output documents.
Each intermediate lookup checks ownership before reusing a child object.
Both intermediate and leaf writes create enumerable, writable, configurable data properties.
Snapshot and merge selection reads use the same ownership check.
Existing Firebase REST, gRPC, converter, transaction, transform, and watch selftests pass.

The three engine mutation routes remain unchanged: queued journal, direct, and execution-unit paths.
This repair changes only JavaScript document preparation and reads.
Alerts #3 and #4 remain open until the merged commit passes GitHub CodeQL.

## Evidence location

Raw logs and the branch recovery bundle remain at `/Users/jack/nimbus-cleanup-2026-09-11` on this host.
The proof binds these results to the changed source and regression named above.
The final PR and review result will complete this record.
