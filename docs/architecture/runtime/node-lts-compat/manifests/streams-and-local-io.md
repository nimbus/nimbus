# Streams And Local I/O Node Test Slices

Current upstream Node test-slice manifest for `NLC5`.

This file records the currently counted green denominator and staged upstream
corpus for the family. Requirements, closeout gates, and roadmap decisions
belong in `docs/plans/archive/node-lts-compatibility-plan.md`.

Source corpus:

- current Deno-family implementation baseline:
  `~/src/github.com/agentstation/deno @ v2.7.14-locker.31`
- pinned official Node22 validation corpus:
  `nodejs/node @ v22.15.0`
- pinned official Node20 supported corpus:
  `nodejs/node @ v20.20.2`
- staged future Node24 supported corpus:
  `nodejs/node @ v24.15.0`

The current work starts the same way `NLC3` and `NLC4` eventually succeeded:
import official Node files as data, batch them by shared runtime seam, and let
the focused Node22/Node20 lanes reveal the real compatibility boundaries
instead of hand-maintaining bespoke behavior claims.

## Initial Slice Map

| Family | Initial upstream test slices |
| --- | --- |
| `node:stream` | `test/parallel/test-stream-*.js`, `test/sequential/test-stream-*.js`, selected `test/wpt/test-stream*.js` and WHATWG stream bridge files where the official Node suite uses them |
| `node:fs` | `test/parallel/test-fs-*.js`, `test/sequential/test-fs-*.js`, selected `test/pummel/test-fs-*.js` |
| `node:readline` | `test/parallel/test-readline-*.js`, `test/sequential/test-readline-*.js` |
| `node:tty` | `test/parallel/test-tty-*.js`, `test/sequential/test-tty-*.js` |
| `node:os` | `test/parallel/test-os-*.js` |

## Initial Corpus Counts

The first-pass official candidate corpus from the canonical local
`~/src/github.com/nodejs/node` worktree is:

- Node22: `512` files
- Node20: `509` files
- Node24 supported: `627` files

These are intentionally broad candidate counts, not the future green
denominator. The next `NLC5` step is to carve out the first manifested batch by
shared seam instead of trying to run all `512+` files at once.

## Current Manifested Official Subset

The first manifested `NLC5` batch is now live in
[`STREAMS_AND_LOCAL_IO_BATCH`](../../../../crates/neovex-runtime/src/runtime/tests/node/mod.rs).

Current manifested batch counts:

- Node22 default lane: `317` official files
- Node20 supported lane: `311` official files
- Node24 supported lane: `308` staged official files
  - current explicit supported-lane watchpoint run: `308` passed, `0` failed
  - supported-lane denominator intentionally excludes
    `test-stream-compose-operator.js`, which is not present in the official
    `nodejs/node v24.15.0` corpus

Current manifested slice coverage:

- `node:stream` core constructors and inheritance
- readable / writable / duplex state and event-name helpers
- destroy / auto-destroy / aborted / finished-state primitives
- pipe / drain / flow listener primitives
- the current `stream/promises` / pipeline subset:
  `stream/promises.finished()`, `stream/promises.pipeline()`,
  async-iterator / duplex / listener / uncaught / empty-string pipeline
  behavior, plus transform final / flush / callback-order primitives
- the next non-socket readable-state seam: zero-highWaterMark reads,
  `readingMore`, `needReadable`, `resumeScheduled`, resume/highWaterMark
  shaping, `unshift()`, emit-readable-short-stream handling, and the current
  flow-recursion / explicit-highWaterMark infinite-read behavior
- constructor wiring, split highWaterMark / objectMode shaping, and invalid
  chunk validation
- typed-array and `Uint8Array` write/read primitives
- `setEncoding()` handling for buffered readable data and `null`
- writable default-encoding, decoded-encoding, and `needDrain` state probes
- writable end-callback, `_final()`, write-callback error, and `writev()`
  ordering/error semantics
- stream utility helpers: `map()`, `filter()`, `reduce()`, `toArray()`,
  `forEach()`, `drop()`, `take()`, `flatMap()`, and `stream/consumers`
- the current higher-level stream/web-bridge seam:
  `stream.addAbortSignal()`, base-prototype accessors enumerability,
  catch-rejections behavior, `stream.setDefaultHighWaterMark()`,
  `Readable[Symbol.asyncDispose]()`, `ReadableStream` / queuing-strategy
  termination and strategy probes, and the shared Node20/Node22
  `stream.compose` operator fixture where that official file exists upstream
- the current pure stream state tail: decoder/objectMode chunk separation,
  push-string readable chunk ordering, readable error-after-end delivery,
  unimplemented `_read()` error surfacing, zero-highWaterMark transform
  drain sequencing, `unshift()` read-race ordering, writable buffer-count
  cleanup, writable `null` / objectMode validation, and writable end/uncork
  state probes
- the next pure stream buffering/order seam: legacy `_stream_*` alias
  exposure, backpressure handling, large-packet / large-push ordering,
  multiple-callback construction guards, pipe deadlock avoidance, duplicate
  destination pipe/unpipe behavior, readable push ordering, objectMode
  multi-push async delivery, and synchronous recursive await-drain writers
- the current `node:tty` / `node:readline` slice:
  `process.stdin` bootstrap parity for `tty` stdin end / pipe handling,
  local `internal/test/binding('tty_wrap')` backwards-API compatibility for
  `test-tty-backwards-api.js`, `readline` CSI handling,
  carriage-return-between-chunks behavior, missing-file `error` /
  async-iterator error delivery through `fs.createReadStream()`, `readline`
  async-iterator consumption, async-iterator backpressure / destroy
  behavior, classic `readline.Interface` cursor/history/prompt coverage under
  the harnessed interactive-terminal env override, `readline/promises`
  CSI handling, and `readline/promises.Interface` active-question
  abort/close rejection semantics
- the first bundle-root-safe `node:fs` slice: staged writes inside the
  generated bundle root, `Buffer` path handling, `close`, `constants`,
  `exists`, `existsSync`, callback and promise `access()` parity,
  append-file primitives, positive-path `read` /
  `readSync` coverage, `read` / `write` option-object coverage,
  `readv()` / `writev()` and promise-based vector-I/O coverage,
  `readvSync()` / `writevSync()`, positive-path `readFileSync()` /
  `readFile(fd)` coverage, callback `readFile()` large-file /
  `ERR_FS_FILE_TOO_LARGE` sparse-file parity, selected positive-path
  `openSync()` flag parsing and mode-mask coverage, callback
  `readFile({ flag: 'a+' | 'ax+' })` parity, numeric `open()` error-shape
  parity for missing-file `O_WRONLY`, async `fs.open()` lifecycle
  quiescence, fd-backed `writeFile()` cleanup parity under the embedded
  `beforeExit`/`exit` drain,
  `writeFile` / `appendFile` `{ flush }` validation and `fsync` behavior
  across sync, callback, and promise surfaces,
  `writeFile` / `writeFileSync` / typed-array writes,
  `read(fd, { offset: null })`, promisified `read()` / `write()` / `exists()`,
  filehandle `appendFile()` / `chmod()` / `stat()` / `sync()` /
  `truncate()` / `write()` / `writeFile()` / `readFile()` / `close()`
  coverage, narrow fd/error-shape probes (`closeSync` invalid-fd handling,
  `writeFile` invalid fd, `read` invalid buffer typing),
  `FileHandle[Symbol.asyncDispose]()`, filehandle-backed
  `fs.createReadStream()` / `fs.createWriteStream()`, filehandle
  `readLines()`, positive-path `fs.promises.readFile()` / `writeFile()` /
  `exists()` coverage, positive-path `fs.promises.FileHandle.read()`,
  positive-path `fs.createReadStream()` /
  `fs.createWriteStream()` fd / empty-file / encoding / end / autoClose
  behavior, and the current Node22/Node24 `Stats` shape
- the current narrow `node:os` slice: `os.EOL` plus the internal checked
  function seam exercised by the official `test-os-checked-function.js`
  fixture
- the current local `node:fs` stream-wrapper contract now routes path-based
  `createReadStream()` / `createWriteStream()` through the Neovex-owned
  `open()` and error-normalization seam while preserving builtin `fd` /
  `FileHandle` stream behavior
- path-based `fs.promises` lifecycle and read parity for the staged local
  bundle-root contract: explicit `FileHandle.prototype.fd` throw handling,
  aggregate close-error propagation, and the official `fs.promises.readFile()`
  abort/validation branches
- bundle-root-safe directory and copy primitives: `mkdir()` / `mkdirSync()`,
  mode-mask handling, in-root parent traversal during recursive `mkdir`,
  legacy `rmdir()` / `rmdirSync()` recursive deprecation semantics plus
  post-delete error-shape parity, and `copyFile()` / `copyFileSync()`
  including the staged permission-respecting overwrite branch
- bundle-root-safe metadata and resize primitives: `chmod()` / `chmodSync()`
  including mode-mask parity, plus path-based `truncate()` /
  `truncateSync()` coverage, deprecated-fd `fs.truncate()` parity, and the
  current missing-file `ENOENT` / `syscall: "open"` callback shape
- fd metadata/sync primitives and stream-close idempotence: `fchmod()` /
  `fchmodSync()` validation parity, `fchown()` / `fchownSync()` validation
  parity, `fdatasync()` / `fdatasyncSync()` / `fsync()` / `fsyncSync()`
  positive-path and invalid-fd parity, plus read-stream and write-stream
  double-close behavior
- path-based timestamp and validation helpers: `chown()` / `chownSync()`
  argument-type validation parity, `statfs()` / `statfsSync()` including
  `bigint` shapes, `fs._toUnixTimestamp()` invalid-input handling,
  `fs.utimes()` / `fs.utimesSync()` including symlink and fd follow-on
  branches, and `createReadStream()` / `createWriteStream()` start/end
  type-gate coverage
- current sync `node:fs` write lifecycle parity for the staged local contract:
  `writeFileSync()` / `appendFileSync()` now honor the current `fs` binding
  surface for monkeypatch-sensitive cleanup semantics across both the utf8
  internal-binding fast path and the `openSync()` / `writeSync()` fallback,
  and the current `fs` stream subset now includes `ready` event delivery
- bundle-root-safe directory/link helper primitives: `mkdtemp()` /
  `mkdtempSync()`, hard-link creation, symlink creation and `readlink()`
  parity, `rename()` / `unlink()` type-gate coverage, and `opendir()` /
  `fs.promises.opendir()` directory-handle close/concurrency behavior for the
  current Node22 contract
- current invalid-encoding guard parity for the staged local `node:fs`
  wrappers: `readdir`, `readlink`, `realpath`, `mkdtemp`, `watch`,
  `ReadStream`, and `WriteStream` now surface the official
  `ERR_INVALID_ARG_VALUE` contract for unsupported encodings
- current `realpath` helper primitives: `realpathSync.native()`,
  `realpath.native()`, and realpath buffer/encoding parity across `string`,
  `Buffer`, and `"buffer"` encoding shapes
- current `readdir` helper primitives: positive-path `readdir()` /
  `readdirSync()`, regular-file `ENOTDIR` parity, `withFileTypes`
  `fs.Dirent` materialization, and unknown-dirent recovery inside approved
  runtime roots
- current path-validation and URL-entrypoint parity for the staged local
  `node:fs` contract: embedded-null-byte rejection across the local
  `writeFile` / `appendFile` / `truncate` overrides, WHATWG `file:` URL
  `readFile()` entrypoints, negative-offset write guards, internal
  `validateOffsetLengthWrite()` parity, surrogate-pair filename operations,
  and symlink creation from `Buffer` target paths
- current `glob` helper primitives: bundle-root-safe `fs.glob()` /
  `fs.promises.glob()` positive-path coverage for the official
  `test-fs-glob.mjs` slice, including absolute-pattern metadata reads on
  approved-root ancestors and relative symlink targets that stay inside
  approved runtime roots
- current watch/watchFile helper primitives: positive-path `fs.watch()`,
  `fs.promises.watch()`, `watchFile()`, and `unwatchFile()` coverage for the
  official Node22 contract, including local polling-backed abort-signal
  handling with preserved `AbortSignal.reason` / `error.cause` parity,
  ref/unref, stop lifecycle behavior, post-delete `ENOENT` delivery,
  synthetic `watcher._handle.onchange(...)` error parity, default/`hex`/
  `buffer` filename encoding behavior, initial-`ENOENT` zeroed
  `BigIntStats` delivery for `watchFile(..., { bigint: true })`,
  close-on-destroy handling, the current option-immutability /
  watcher-surface branches exercised by `test-fs-options-immutable.js`, and
  the first recursive `fs.watch()` / `fs.promises.watch()` slice for file
  creation, file update, nested delete, folder creation, URL paths,
  recursive option validation, sync writes, file creation in newly created
  and pre-existing nested subfolders, recursive file-path watches, and
  symlink handling

Current manifested files:

- `test/parallel/test-os-eol.js`
- `test/parallel/test-os-checked-function.js`
- `test/parallel/test-tty-backwards-api.js`
- `test/parallel/test-tty-stdin-end.js`
- `test/parallel/test-tty-stdin-pipe.js`
- `test/parallel/test-ttywrap-invalid-fd.js`
- `test/parallel/test-ttywrap-stack.js`
- `test/parallel/test-readline-csi.js`
- `test/parallel/test-readline-carriage-return-between-chunks.js`
- `test/parallel/test-readline-input-onerror.js`
- `test/parallel/test-readline-async-iterators.js`
- `test/parallel/test-readline-async-iterators-backpressure.js`
- `test/parallel/test-readline-async-iterators-destroy.js`
- `test/parallel/test-readline-interface.js`
- `test/parallel/test-readline-promises-interface.js`
- `test/parallel/test-readline-promises-csi.mjs`
- `test/parallel/test-stream-construct.js`
- `test/parallel/test-stream-auto-destroy.js`
- `test/parallel/test-stream-duplex-destroy.js`
- `test/parallel/test-stream-duplex-end.js`
- `test/parallel/test-stream-end-of-streams.js`
- `test/parallel/test-stream-event-names.js`
- `test/parallel/test-stream-end-paused.js`
- `test/parallel/test-stream-error-once.js`
- `test/parallel/test-stream-events-prepend.js`
- `test/parallel/test-stream-inheritance.js`
- `test/parallel/test-stream-ispaused.js`
- `test/parallel/test-stream-readable-aborted.js`
- `test/parallel/test-stream-readable-constructor-set-methods.js`
- `test/parallel/test-stream-readable-destroy.js`
- `test/parallel/test-stream-readable-end-destroyed.js`
- `test/parallel/test-stream-duplex-props.js`
- `test/parallel/test-stream-duplex-readable-writable.js`
- `test/parallel/test-stream-passthrough-drain.js`
- `test/parallel/test-stream-pipe-after-end.js`
- `test/parallel/test-stream-pipe-await-drain.js`
- `test/parallel/test-stream-pipe-cleanup.js`
- `test/parallel/test-stream-pipe-event.js`
- `test/parallel/test-stream-pipe-flow.js`
- `test/parallel/test-stream-pipe-multiple-pipes.js`
- `test/parallel/test-stream-readable-ended.js`
- `test/parallel/test-stream-readable-data.js`
- `test/parallel/test-stream-readable-event.js`
- `test/parallel/test-stream-readable-emittedReadable.js`
- `test/parallel/test-stream-readable-default-encoding.js`
- `test/parallel/test-stream-readable-invalid-chunk.js`
- `test/parallel/test-stream-readable-next-no-null.js`
- `test/parallel/test-stream-readable-no-unneeded-readable.js`
- `test/parallel/test-stream-readable-pause-and-resume.js`
- `test/parallel/test-stream-readable-readable.js`
- `test/parallel/test-stream-reduce.js`
- `test/parallel/test-stream-readable-setEncoding-existing-buffers.js`
- `test/parallel/test-stream-readable-setEncoding-null.js`
- `test/parallel/test-stream-map.js`
- `test/parallel/test-stream-filter.js`
- `test/parallel/test-stream-forEach.js`
- `test/parallel/test-stream-toArray.js`
- `test/parallel/test-stream-drop-take.js`
- `test/parallel/test-stream-flatMap.js`
- `test/parallel/test-stream-consumers.js`
- `test/parallel/test-stream-promises.js`
- `test/parallel/test-stream-pipeline-async-iterator.js`
- `test/parallel/test-stream-pipeline-duplex.js`
- `test/parallel/test-stream-pipeline-listeners.js`
- `test/parallel/test-stream-pipeline-uncaught.js`
- `test/parallel/test-stream-pipeline-with-empty-string.js`
- `test/parallel/test-stream-compose.js`
- `test/parallel/test-stream-destroy-event-order.js`
- `test/parallel/test-stream-duplex.js`
- `test/parallel/test-stream-duplex-from.js`
- `test/parallel/test-stream-duplexpair.js`
- `test/parallel/test-stream-readable-add-chunk-during-data.js`
- `test/parallel/test-stream-readable-didRead.js`
- Node22/Node24 only: `test/parallel/test-stream-readable-infinite-read.js`
- `test/parallel/test-stream-readable-hwm-0.js`
- `test/parallel/test-stream-readable-hwm-0-async.js`
- `test/parallel/test-stream-readable-hwm-0-no-flow-data.js`
- `test/parallel/test-stream-readable-reading-readingMore.js`
- `test/parallel/test-stream-readable-flow-recursion.js`
- `test/parallel/test-stream-readable-needReadable.js`
- `test/parallel/test-stream-readable-readable-then-resume.js`
- `test/parallel/test-stream-readable-resumeScheduled.js`
- `test/parallel/test-stream-readable-resume-hwm.js`
- `test/parallel/test-stream-readable-unshift.js`
- `test/parallel/test-stream-readable-emit-readable-short-stream.js`
- `test/parallel/test-stream-transform-callback-twice.js`
- `test/parallel/test-stream-transform-constructor-set-methods.js`
- `test/parallel/test-stream-transform-final.js`
- `test/parallel/test-stream-transform-final-sync.js`
- `test/parallel/test-stream-transform-flush-data.js`
- `test/parallel/test-stream-transform-objectmode-falsey-value.js`
- Node22/Node24 only: `test/parallel/test-stream-transform-split-highwatermark.js`
- Node22/Node24 only: `test/parallel/test-stream-transform-split-objectmode.js`
- `test/parallel/test-stream-typedarray.js`
- `test/parallel/test-stream-unshift-empty-chunk.js`
- `test/parallel/test-stream-uint8array.js`
- `test/parallel/test-stream-writable-aborted.js`
- `test/parallel/test-stream-writable-change-default-encoding.js`
- `test/parallel/test-stream-writable-constructor-set-methods.js`
- `test/parallel/test-stream-writable-decoded-encoding.js`
- `test/parallel/test-stream-writable-destroy.js`
- `test/parallel/test-stream-writable-end-cb-error.js`
- `test/parallel/test-stream-writable-end-cb-uncaught.js`
- `test/parallel/test-stream-writable-end-multiple.js`
- `test/parallel/test-stream-writable-final-async.js`
- `test/parallel/test-stream-writable-final-destroy.js`
- `test/parallel/test-stream-writable-final-throw.js`
- `test/parallel/test-stream-writable-finish-destroyed.js`
- `test/parallel/test-stream-writable-finished.js`
- `test/parallel/test-stream-writable-finished-state.js`
- `test/parallel/test-stream-writable-invalid-chunk.js`
- `test/parallel/test-stream-writable-needdrain-state.js`
- `test/parallel/test-stream-writable-properties.js`
- `test/parallel/test-stream-writable-write-cb-error.js`
- `test/parallel/test-stream-writable-write-cb-twice.js`
- `test/parallel/test-stream-writable-write-error.js`
- `test/parallel/test-stream-writable-write-writev-finish.js`
- `test/parallel/test-stream-writable-writable.js`
- `test/parallel/test-stream-writable-ended-state.js`
- `test/parallel/test-stream-write-drain.js`
- `test/parallel/test-stream-write-destroy.js`
- `test/parallel/test-stream-write-final.js`
- `test/parallel/test-stream-writev.js`
- Node22/Node24 only: `test/parallel/test-stream-duplex-readable-end.js`
- `test/parallel/test-stream-duplex-writable-finished.js`
- `test/parallel/test-stream-objectmode-undefined.js`
- `test/parallel/test-stream-unpipe-event.js`
- `test/parallel/test-fs-buffer.js`
- `test/parallel/test-fs-close.js`
- Node20/Node22 only: `test/parallel/test-fs-constants.js`
- `test/parallel/test-fs-append-file-sync.js`
- `test/parallel/test-fs-append-file.js`
- `test/parallel/test-fs-access.js`
- `test/parallel/test-fs-assert-encoding-error.js`
- `test/parallel/test-fs-buffertype-writesync.js`
- `test/parallel/test-fs-exists.js`
- `test/parallel/test-fs-existssync-false.js`
- `test/parallel/test-fs-read-empty-buffer.js`
- `test/parallel/test-fs-read-zero-length.js`
- `test/parallel/test-fs-read.js`
- `test/parallel/test-fs-read-file-assert-encoding.js`
- `test/parallel/test-fs-read-file-sync.js`
- `test/parallel/test-fs-read-optional-params.js`
- `test/parallel/test-fs-readfile-empty.js`
- `test/parallel/test-fs-readfile-fd.js`
- `test/parallel/test-fs-readfile-flags.js`
- `test/parallel/test-fs-readfile.js`
- `test/parallel/test-fs-readfile-unlink.js`
- `test/parallel/test-fs-readfile-zero-byte-liar.js`
- `test/parallel/test-fs-readSync-optional-params.js`
- `test/parallel/test-fs-read-type.js`
- `test/parallel/test-fs-readv-promisify.js`
- `test/parallel/test-fs-readv-promises.js`
- `test/parallel/test-fs-readv-sync.js`
- `test/parallel/test-fs-readv.js`
- `test/parallel/test-fs-open-flags.js`
- `test/parallel/test-fs-open-numeric-flags.js`
- `test/parallel/test-fs-open-mode-mask.js`
- `test/parallel/test-fs-open-no-close.js`
- `test/parallel/test-fs-mkdir.js`
- `test/parallel/test-fs-mkdir-mode-mask.js`
- `test/parallel/test-fs-mkdir-rmdir.js`
- `test/parallel/test-fs-chmod.js`
- `test/parallel/test-fs-chmod-mask.js`
- `test/parallel/test-fs-copyfile.js`
- `test/parallel/test-fs-copyfile-respect-permissions.js`
- `test/parallel/test-fs-mkdtemp.js`
- `test/parallel/test-fs-mkdtemp-prefix-check.js`
- `test/parallel/test-fs-link.js`
- Node20/Node22 only: `test/parallel/test-fs-symlink.js`
- `test/parallel/test-fs-realpath-buffer-encoding.js`
- `test/parallel/test-fs-realpath-native.js`
- `test/parallel/test-fs-readdir.js`
- `test/parallel/test-fs-readdir-types.js`
- `test/parallel/test-fs-readlink-type-check.js`
- `test/parallel/test-fs-rename-type-check.js`
- `test/parallel/test-fs-unlink-type-check.js`
- `test/parallel/test-fs-fchmod.js`
- `test/parallel/test-fs-fchown.js`
- `test/parallel/test-fs-fsync.js`
- `test/parallel/test-fs-chown-type-check.js`
- `test/parallel/test-fs-statfs.js`
- `test/parallel/test-fs-timestamp-parsing-error.js`
- `test/parallel/test-fs-utimes.js`
- `test/parallel/test-fs-non-number-arguments-throw.js`
- Node20/Node22 only: `test/parallel/test-fs-opendir.js`
- `test/parallel/test-fs-rmdir-recursive.js`
- `test/parallel/test-fs-rmdir-type-check.js`
- `test/parallel/test-fs-rmdir-recursive-throws-not-found.js`
- `test/parallel/test-fs-rmdir-recursive-throws-on-file.js`
- `test/parallel/test-fs-rmdir-recursive-warns-not-found.js`
- `test/parallel/test-fs-rmdir-recursive-warns-on-file.js`
- `test/parallel/test-fs-rmdir-recursive-sync-warns-not-found.js`
- `test/parallel/test-fs-rmdir-recursive-sync-warns-on-file.js`
- `test/parallel/test-fs-truncate.js`
- `test/parallel/test-fs-truncate-sync.js`
- Node20/Node22 only: `test/parallel/test-fs-truncate-fd.js`
- `test/parallel/test-fs-truncate-clear-file-zero.js`
- Node22/Node24 only: `test/parallel/test-fs-stat.js`
- `test/parallel/test-fs-write-buffer.js`
- `test/parallel/test-fs-close-errors.js`
- `test/parallel/test-fs-write-file-buffer.js`
- `test/parallel/test-fs-writefile-with-fd.js`
- `test/parallel/test-fs-write-file-flush.js`
- `test/parallel/test-fs-write-file-typedarrays.js`
- `test/parallel/test-fs-write-file.js`
- `test/parallel/test-fs-write-no-fd.js`
- `test/parallel/test-fs-write-optional-params.js`
- `test/parallel/test-fs-write-sync.js`
- `test/parallel/test-fs-write-sync-optional-params.js`
- `test/parallel/test-fs-writev-promises.js`
- `test/parallel/test-fs-writev.js`
- `test/parallel/test-fs-writev-sync.js`
- `test/parallel/test-fs-read-offset-null.js`
- `test/parallel/test-fs-read-promises-optional-params.js`
- `test/parallel/test-fs-promises-write-optional-params.js`
- `test/parallel/test-fs-promisified.js`
- `test/parallel/test-fs-promises-file-handle-write.js`
- `test/parallel/test-fs-promises-file-handle-append-file.js`
- `test/parallel/test-fs-promises-file-handle-stat.js`
- `test/parallel/test-fs-promises-file-handle-chmod.js`
- `test/parallel/test-fs-promises-file-handle-truncate.js`
- `test/parallel/test-fs-promises-file-handle-sync.js`
- `test/parallel/test-fs-promises-file-handle-writeFile.js`
- Node20/Node22 only: `test/parallel/test-fs-promises-file-handle-dispose.js`
- `test/parallel/test-fs-promises-file-handle-stream.js`
- `test/parallel/test-fs-promises-file-handle-readLines.mjs`
- `test/parallel/test-fs-read-stream-file-handle.js`
- `test/parallel/test-fs-write-stream-file-handle.js`
- `test/parallel/test-fs-promises-file-handle-readFile.js`
- `test/parallel/test-fs-promises-file-handle-read.js`
- `test/parallel/test-fs-promises-file-handle-close.js`
- `test/parallel/test-fs-promises-file-handle-close-errors.js`
- `test/parallel/test-fs-promises-file-handle-op-errors.js`
- `test/parallel/test-fs-promises-file-handle-aggregate-errors.js`
- `test/parallel/test-fs-promises-exists.js`
- `test/parallel/test-fs-promises-readfile.js`
- `test/parallel/test-fs-promises-readfile-empty.js`
- `test/parallel/test-fs-promises-readfile-with-fd.js`
- `test/parallel/test-fs-promises-writefile-typedarray.js`
- `test/parallel/test-fs-promises-writefile-with-fd.js`
- `test/parallel/test-fs-promises-writefile.js`
- `test/parallel/test-fs-append-file-flush.js`
- `test/parallel/test-fs-read-stream-fd.js`
- `test/parallel/test-fs-read-stream-autoClose.js`
- `test/parallel/test-fs-read-stream-double-close.js`
- `test/parallel/test-fs-empty-readStream.js`
- `test/parallel/test-fs-read-stream-encoding.js`
- Node20/Node22 only: `test/parallel/test-fs-write-stream.js`
- `test/parallel/test-fs-write-stream-end.js`
- `test/parallel/test-fs-write-stream-double-close.js`
- Node20/Node22 only: `test/parallel/test-fs-write-stream-autoclose-option.js`
- `test/parallel/test-fs-write-stream-encoding.js`
- `test/parallel/test-fs-ready-event-stream.js`
- `test/parallel/test-fs-sync-fd-leak.js`
- `test/parallel/test-fs-null-bytes.js`
- `test/parallel/test-fs-whatwg-url.js`
- `test/parallel/test-fs-write-negativeoffset.js`
- `test/parallel/test-fs-util-validateoffsetlength.js`
- `test/parallel/test-fs-operations-with-surrogate-pairs.js`
- `test/parallel/test-fs-symlink-buffer-path.js`
- `test/parallel/test-fs-glob.mjs`
- `test/parallel/test-fs-watch-enoent.js`
- `test/parallel/test-fs-watch-encoding.js`
- `test/parallel/test-fs-watchfile-bigint.js`
- `test/parallel/test-fs-watch-recursive-promise.js`
- `test/parallel/test-fs-watch-recursive-add-file.js`
- `test/parallel/test-fs-watch-recursive-update-file.js`
- `test/parallel/test-fs-watch-recursive-delete.js`
- `test/parallel/test-fs-watch-recursive-add-folder.js`
- `test/parallel/test-fs-watch-recursive-add-file-with-url.js`
- `test/parallel/test-fs-watch-recursive-validate.js`
- `test/parallel/test-fs-watch-recursive-sync-write.js`
- `test/parallel/test-fs-watch-recursive-add-file-to-new-folder.js`
- `test/parallel/test-fs-watch-recursive-add-file-to-existing-subfolder.js`
- `test/parallel/test-fs-watch-recursive-watch-file.js`
- `test/parallel/test-fs-watch-recursive-symlink.js`

## Next Batch Strategy

Keep widening by shared seam instead of by isolated file:

1. keep widening by shared local-I/O seam, but the next honest owner pocket is
   no longer basic watch delivery; it is the remaining `fs.readFile`
   large-file / sparse-file gap and the local `tty_wrap` / interactive
   terminal watchpoints
2. use the already-staged official corpus in larger batches instead of another
   bespoke proof slice, especially if the next batch can clarify whether the
   remaining non-interactive `os` / `readline` helpers are isolated positives
   or symptoms of a broader unfinished seam
3. keep the local-TCP `stream.finished()` / `stream.pipeline()` boundary, the
   application-preset host-path/capability divergences, and the later
   `worker_threads` dependencies explicit instead of forcing them back into the
   `NLC5` denominator
4. treat the richer interactive `readline` surface as a later deliberate
   harness/TTY contract decision while the default `TERM=dumb` node_compat lane
   remains intentionally non-interactive

## Notes

- `~/src/github.com/nodejs/node` is the canonical local source for file
  selection and Node20/Node22 drift review.
- `~/src/github.com/agentstation/deno` remains the shared implementation and
  harness reference, not the truth source for staged fixture content.
- The shared `test/common` shim must follow the official Node corpus version,
  not the runtime's raw global surface. Current examples: Node20/Node22 omit
  `Float16Array` from `getArrayBufferViews()`, while Node24 includes it; the
  newer Node24 `stream` corpus also expects `common.mustSucceed()`.
- Shared test fixtures may need narrow staged data files when the official
  corpus requires them. Current examples: `test-stream-flatMap.js` needs
  `test/fixtures/x.txt`, while the widened `fs` `readFile*` seam now stages
  upstream `test/fixtures/a.js`, `test/fixtures/baz.js`,
  `test/fixtures/empty.txt`, `test/fixtures/elipses.txt`, and
  `test/fixtures/utf8_test_text.txt` from the local
  `~/src/github.com/nodejs/node` worktree.
- The imported official `test-fs-readfilesync-enoent.js` file remains staged
  in the corpus but is intentionally not counted in the current macOS-focused
  manifested denominator because it is a Windows-only test that self-skips on
  non-Windows hosts.
- The current application-preset local-I/O contract now allows writes inside
  the generated bundle root while continuing to deny escape writes outside the
  approved runtime roots. That owner change was needed to make the first
  bundle-root-safe `fs` batch truthful instead of tooling-only.
- Future manifested green counts and classified watchpoints should live beside
  this file and the matching failure inventory.
