# UIR16 Files

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `9e2f73116`. Files was a placeholder. The plan said the object
storage read and write API was already exposed by `nimbus-server`; at
task start no such route existed for the console session. The tenant's
objects were reachable through the S3 listener alone, which needs S3
credentials and a separate port. This task adds one native
session-authenticated route family under
`/api/tenants/{tenant}/objects` and rebuilds the page on it: a bucket
sub-panel, a DataTable of objects with client-derived folders, drop and
chooser uploads with a progress queue, and an object sheet with facts,
preview, download, copy link, and delete.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-storage/src/{lib.rs,traits/mod.rs,traits/object_metadata.rs,traits/provider_impls.rs}` | `list_object_buckets` on the object metadata trait: one manifest scan per tenant that folds keys into `{bucket, objectCount, totalBytes}`. Operator surface, not the S3 hot path. |
| `crates/nimbus-engine/src/{engine/objects.rs,persistence/tenant/objects.rs}` | Engine method and tenant persistence seam for the bucket list. |
| `crates/nimbus-s3/src/objects.rs` (new, 108 lines), `lib.rs` | Public `objects` seam: the blob lifecycle (put, get, delete) that the S3 listener already used, so the console route and the listener share one write path. |
| `crates/nimbus-server/src/http/objects.rs` (new, 301 lines), `http/mod.rs`, `router.rs`, `state.rs`, `construction.rs`, `adapters/s3/{listener,mod}.rs`, `adapters/http_mount.rs` | Route family: `GET /objects` (buckets), `GET /objects/{bucket}?prefix=&limit=` (default 1000, clamped 1..5000, `truncated` flag; an unknown bucket is an empty listing), `GET /objects/{bucket}/{*key}[?download=1]` (bytes with content type, quoted etag, `inline` or `attachment` disposition, `content-security-policy: sandbox`, `x-content-type-options: nosniff`, 404 when missing), `PUT` (raw body, optional content type, 201 with the object summary, replaces in place, 404 for an unknown tenant, 400 for an invalid bucket, 413 over 16 MiB), `DELETE` (204 or 404). One `ObjectStorageConfig` per process reaches the router through `RouterOptions` and `ServeOptions::with_object_storage_config`; `AppState.objects` is an `EngineS3Resolver`. |
| `crates/nimbus-cli/src/start/adapters/{mod,s3}.rs`, `start/tests/cli_surface.rs` | The S3 adapter config is hoisted out of the listener so the console route and the listener share it. |
| `crates/nimbus-server/src/tests/core_http/objects.rs` (new, 283 lines), `tests/core_http.rs`, `workload_composition/tests.rs`, `workload_saga_store/tests/composition.rs` | Four integration tests over the real router (see spec notes); the composition tests pass the new option. |
| `src/lib/api-mutations.ts` | `ObjectBucket`, `ObjectSummary`, `ObjectListing`, `UploadProgress`; `objects.{buckets,list,url,readText,upload,remove}`. Upload is XHR so progress reports bytes. |
| `src/lib/format.ts` | `formatBytes`: binary units, one fractional digit under 10, `—` for null. |
| `src/routes/developer/files/-types.ts` (new, 160 lines) | `FilesSearch` (`bucket`, `prefix`, `object`), `parseFilesSearch`, `isValidBucketName`, `deriveRows` (folders and objects from a flat listing), `prefixSegments`, `previewKind`, `shortContentType`, `TEXT_PREVIEW_MAX_BYTES` (256 KiB). |
| `src/routes/developer/files/-use-objects.ts` (new, 94 lines) | `useBuckets`, `useObjectListing` (on-demand reads with a reload version), `useReloadVersion`, `LIST_LIMIT = 1000`. |
| `src/components/files/drop-zone.tsx` (new, 93 lines) | `DropZone`: drag overlay on a wrapped child, files only, disabled state; the Upload button beside it is the keyboard path. |
| `src/components/files/upload-queue.tsx` (new, 107 lines) | `UploadQueue`: one strip per upload with percent, error, and dismiss. Story in `src/stories/upload-queue.stories.tsx`. |
| `src/routes/developer/files/-object-sheet.tsx` (new, 301 lines) | `ObjectSheet` on `?object=`: name and path, size, type, modified, etag, image preview or text preview under 256 KiB, a note that the link is the console route, Delete, Copy link, Download (`?download=1`), and a missing state. |
| `src/routes/developer/files.tsx` (rewritten, 791 lines) | `validateSearch: parseFilesSearch`; the first bucket is selected when the address names none; dynamic bucket sub-panel with count and bytes (the address's bucket is shown before its first object lands); New bucket dialog with name validation; breadcrumb over bucket and prefix; DataTable (Name with folder or file icon and item count, Size, Type, Modified, actions); truncation note at 1000 keys; row context menu (open, download, copy link, delete); uploads keyed by `${prefix}${file.name}` with toasts; delete behind `ConfirmDialog`. Subtitle within the 100-character budget. |
| `DESIGN.md` | Files (Developer) section rewritten for the shipped surface; the capability table row names buckets, browser, upload, preview, and download over the console session. |

## Spec notes

- `files.spec.tsx` (new, 26 tests): scope (6: empty tenant, bucket
  auto-select, address bucket, sub-panel items, load failure, empty
  buckets), listing (7: rows, folders, sizes, types, prefix navigation,
  empty prefix, truncation), upload (3: chooser, drop, failure), sheet
  (5: facts, text preview, image preview, no preview, missing),
  row actions (4: open, download, copy link, delete), new bucket (1).
- `-types.spec.ts` (new, 8), `drop-zone.spec.tsx` (new, 3),
  `upload-queue.spec.tsx` (new, 4), `api-mutations.spec.ts` (+4),
  `format.spec.ts` (+4 for `formatBytes`).
- Fail-before: the 26 files specs against the placeholder page fail
  (26 failed, saved as `uir16-fail-before.txt` in the session
  scratchpad, not in the proof root).
- `tests/core_http/objects.rs` (4 tests):
  `uploads_lists_downloads_and_deletes_a_whole_object`,
  `replaces_an_object_in_place_and_serves_binary_bytes_as_octet_stream`,
  `lists_with_a_limit_and_reports_truncation`,
  `rejects_unknown_tenants_invalid_buckets_and_oversized_bodies`.
- `tests/e2e/files.spec.ts` (new, chromium, desktop-only): creates
  tenant `files-e2e`, asserts the empty-buckets state, creates bucket
  `assets` through the dialog, uploads `hello.txt` through the chooser
  and reads `17 B` and `text/plain` off the row, puts `docs/readme.md`
  through the route and reads the `docs/` folder with one item and the
  sub-panel count of 2, opens the sheet with the text preview and a
  download link that answers `attachment`, closes it with Escape, walks
  into the folder and back through the breadcrumb, deletes `hello.txt`
  through the row menu and the confirm dialog, and asserts the route
  answers 404. The first run timed out on a dismiss click after
  `page.reload()`: the upload strip lives in memory and is gone after a
  reload, so the assertion was wrong and was removed.
- The subtitle measure spec caught the first Files subtitle at 102
  characters; shortened to 96.
- Net: 118 files, 1026 tests (112 files, 977 at UIR15).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-server objects` | 13 passed |
| `cargo test -p nimbus-s3` | 31 passed |
| `cargo test -p nimbus-storage` | 400 passed |
| `cargo test -p nimbus-cli` | 1072 passed, 4 ignored |
| `cargo test -p nimbus-engine` with the external provider fixtures disabled | 712 passed |
| `cargo fmt --all --check` | clean |
| `make check`, `make clippy` | ok |
| `make build` (worktree root) | ok, 1m 27s |
| `npm run lint` | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 118 files, 1026 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `npm run test:e2e` | 23 passed, 1 skipped (13 files; files spec skipped on mobile) |
| `verify.sh` | 27 ok, 0 failing |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary,
tenant `files-e2e` with bucket `assets`: `UIR16-files.png` (the bucket
sub-panel and the listing with one folder and one file),
`UIR16-sheet.png` (the object sheet on `hello.txt` with the text
preview, captured with animations settled).

## Open items

- The engine test lane runs closed unless the external provider
  fixtures are disabled through the environment variable named in
  `crates/nimbus-storage/src/provider_test_fixtures.rs`; the count
  above is from that lane.
- Reads are on demand with a reload after each write. There is no live
  query over objects, so a write from the S3 listener shows on the next
  reload.
- New bucket is a name held in the address until the first object
  lands; the server has no empty-bucket record.
- Copy link copies the console route, which answers for a signed-in
  console session only. Presigned URLs stay out of scope.
- The S3 listener keeps its own credentials; the console route rides
  the session and does not touch them.
- Listing is bounded at 1000 keys with a note; the page does not page
  further.
