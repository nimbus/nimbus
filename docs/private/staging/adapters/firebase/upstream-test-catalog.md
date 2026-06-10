# Firebase Upstream Test Catalog

This catalog tracks the upstream Firebase JS SDK Firestore integration corpus
as a compatibility score for Nimbus. It is not a blanket claim that the stock
Firebase browser or Node SDKs are already supported; it is the concrete list of
which upstream tests look like early pass candidates, which ones are expected
to fail on currently documented gaps, and which ones are deferred because they
exercise WebChannel, offline cache behavior, bundles, or other intentionally
out-of-scope features.

Source checkout used for this catalog:

- `~/src/github.com/firebase/firebase-js-sdk`
- Firestore package: `packages/firestore`
- API corpus: `packages/firestore/test/integration/api/`

## Upstream Commands

These command surfaces come directly from
`~/src/github.com/firebase/firebase-js-sdk/packages/firestore/package.json`.

| Surface | Upstream command |
|---------|------------------|
| Full Firestore node integration tests | `yarn test:node` |
| Full Firestore browser/Karma tests | `yarn test:browser` |
| Firestore Lite node tests | `yarn test:lite` |
| Firestore Lite browser tests | `yarn test:lite:browser` |

The first Nimbus compatibility target should stay narrow:

```bash
cd ~/src/github.com/firebase/firebase-js-sdk/packages/firestore
yarn test:node test/integration/api/smoke.test.ts
```

## Local Harness Notes

The upstream checkout is now runnable enough to serve as a compatibility
signal, but it needed a few local setup steps on 2026-04-25:

1. `yarn install --frozen-lockfile --ignore-engines` was required first because
   the local machine defaulted to Node `25.9.0`; Firebase's workspace did not
   start cleanly before dependencies were present.
2. `config/project.json` had to exist locally so the emulator-backed node lane
   could resolve a project ID. The minimal file used here was:

   ```json
   {
     "projectId": "test-emulator",
     "apiKey": "fake-api-key"
   }
   ```

3. `yarn build:deps` was required before the Firestore package could load its
   built dependency artifacts such as `@firebase/app/dist/index.cjs.js`.
4. A Firebase-supported runtime was still needed after install/build. Nimbus
   installed `node@22` locally and ran the upstream node lane with:

   ```bash
   PATH="/opt/homebrew/opt/node@22/bin:$PATH" \
   NODE_OPTIONS="--experimental-transform-types"
   ```

   The extra `NODE_OPTIONS` flag is a local harness workaround for this
   upstream checkout under modern Node: without `--experimental-transform-types`
   the Firestore test corpus hits Node's strip-only TypeScript parser on
   parameter properties before execution.

5. A live local Nimbus server was started for the smoke lane with:

   ```bash
   target/debug/nimbus start --port 8080 --data-dir /tmp/nimbus-firebase-upstream
   ```

The result is no longer an environment-only blocker: representative upstream
pass and fail logs now exist.

## Representative Results

### Representative pass lanes

These commands passed against the upstream Firestore node harness under
Node `22.22.2`:

```bash
cd ~/src/github.com/firebase/firebase-js-sdk/packages/firestore
PATH="/opt/homebrew/opt/node@22/bin:$PATH" \
NODE_OPTIONS="--experimental-transform-types" \
../../node_modules/.bin/ts-node ./scripts/run-tests.ts \
  --main=test/register.ts \
  --emulator \
  --grep "doc\\(\\) will auto generate an ID" \
  test/integration/api/database.test.ts
```

Result:

- `(Persistence=memory_lru_gc) Database`
- `1 passing`

```bash
cd ~/src/github.com/firebase/firebase-js-sdk/packages/firestore
PATH="/opt/homebrew/opt/node@22/bin:$PATH" \
NODE_OPTIONS="--experimental-transform-types" \
../../node_modules/.bin/ts-node ./scripts/run-tests.ts \
  --main=test/register.ts \
  --emulator \
  --grep "Collection paths" \
  test/integration/api/validation.test.ts
```

Result:

- `(Persistence=memory_lru_gc) Validation: Collection paths`
- `3 passing`

### Representative fail lane

The focused stock-Firebase smoke lane now reaches Nimbus's live protocol
surface and fails for a real compatibility reason rather than for harness
setup:

```bash
cd ~/src/github.com/firebase/firebase-js-sdk/packages/firestore
PATH="/opt/homebrew/opt/node@22/bin:$PATH" \
../../node_modules/.bin/ts-node ./scripts/run-tests.ts \
  --main=test/register.ts \
  --emulator \
  test/integration/api/smoke.test.ts
```

Observed result against the local Nimbus server on `127.0.0.1:8080`:

- repeated `GrpcConnection RPC 'Write' stream ... Code: 12 Message: 12 UNIMPLEMENTED`
- smoke failures reached at least:
  - `can write a single document`
  - `can read a written document`
  - `can read a written document with DocumentKey`

This is now a genuine stock-SDK compatibility gap, not an upstream workspace
setup issue.

## First-Pass Buckets

| File | Bucket | Why |
|------|--------|-----|
| `smoke.test.ts` | `expected fail` | The stock Node Firestore SDK reaches Nimbus successfully, but current smoke runs fail on the upstream `Write` bidi RPC with `12 UNIMPLEMENTED`. |
| `batch_writes.test.ts` | `pass candidate` | Maps directly to the implemented atomic `Commit` path and client batch surface. |
| `cursor.test.ts` | `pass candidate` | Exercises ordering/cursor query behavior that now exists in shared structured-query execution. |
| `transactions.test.ts` | `pass candidate` | Matches the landed transaction-session manager plus transactional query read support. |
| `array_transforms.test.ts` | `pass candidate` | Lines up with the shared array transform implementation already exercised end to end. |
| `numeric_transforms.test.ts` | `pass candidate` | Lines up with shared numeric transform behavior already exercised end to end. |
| `server_timestamp.test.ts` | `mixed` | Core server timestamp support exists, but offline/network subsections should not be used as an early gate. |
| `query.test.ts` | `mixed` | Large file with many supported query cases, but it also includes cache/offline-oriented subsections that should be split from the first gate. |
| `aggregation.test.ts` | `mixed` | Count is a good candidate; `sum` / `average` remain explicitly unsupported in Nimbus today, so those subsections should start in the expected-fail bucket. |
| `composite_index_query.test.ts` | `expected fail` | Nimbus now surfaces missing-index errors, but this file also covers richer aggregation and enterprise/index permutations beyond the current claim. |
| `fields.test.ts` | `expected fail` | Upstream nested-field/path helper coverage is broader than the current `@nimbus/firebase` field-path surface. |
| `validation.test.ts` | `mixed` | The narrow `Collection paths` subset passes under the upstream node harness; broader persistence/network/emulator timing coverage still exceeds the present claim. |
| `database.test.ts` | `mixed` | Narrow local/reference subsets pass (for example auto-ID generation), but the file also mixes CRUD, persistence, `onSnapshotsInSync`, `waitForPendingWrites`, vector APIs, and named database behavior. |
| `get_options.test.ts` | `expected fail` | Depends on cache-only and network-toggle APIs that Nimbus does not implement yet. |
| `bundle.test.ts` | `deferred` | Requires bundle loading and `namedQuery`, which are outside the current scope. |
| `snapshot_listener_source.test.ts` | `deferred` | Depends on source/cache-aware listener semantics that are not part of the current watch claim. |
| `provider.test.ts` | `deferred` | Focuses on provider/cache/persistence configuration, not current protocol parity. |
| `persistent_cache_index_manager.test.ts` | `deferred` | Persistent cache index management is outside scope. |
| `index_configuration.test.ts` | `deferred` | Persistent/local index configuration is outside scope. |
| `pipeline.test.ts` | `deferred` | Exercises Firestore pipeline/vector/search APIs that Nimbus does not claim. |
| `query_to_pipeline.test.ts` | `deferred` | Same as `pipeline.test.ts`; not part of the current Firestore adapter target. |
| `type.test.ts` | `deferred` | Type-surface checks are useful later, but not the first protocol-compatibility gate. |

## Recommended Early Gate

With the local harness steps above in place, use this staged order:

1. `validation.test.ts --grep "Collection paths"`
2. `database.test.ts --grep "doc\\(\\) will auto generate an ID"`
3. `smoke.test.ts`
4. `batch_writes.test.ts`
5. `cursor.test.ts`
6. `transactions.test.ts`
7. `array_transforms.test.ts`
8. `numeric_transforms.test.ts`

Only widen to `query.test.ts`, `aggregation.test.ts`, and the broader
validation/database files after the narrow smoke lane produces trustworthy
results.
