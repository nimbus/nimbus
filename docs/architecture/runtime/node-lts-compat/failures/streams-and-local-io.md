# Streams And Local I/O Failure Inventory

Status: `classified`

This file is the checked-in failure inventory for the currently manifested
`NLC5` streams and local I/O subset.

It records only the explicit red/skip remainder for the current family:
watchpoints, validation-lane divergences, supported-lane drift, later-family
dependencies, and profile/capability restrictions. Requirements and closeout
decisions belong in `docs/plans/archive/node-lts-compatibility-plan.md`.

## Node22 Upstream Slice Status

- Status: `green for the currently manifested subset`
- Current measured subset:
  - `317` official files passed
  - `0` failed
  - `8` explicit Node22 watchpoints outside the green subset

### Explicit Node22 Watchpoints

- `test/parallel/test-stream-finished.js`
  - classification: `nlc5_nlc6_boundary_watchpoint`
  - reason: the official file opens a local TCP server with
    `Server.listen(0)`, which currently fails under the application-profile
    no-net contract before the rest of the callback-style `stream.finished()`
    assertions run
  - owner: networking-family capability boundary, not the green pure-stream
    `stream/promises.finished()` semantics already counted in `NLC5`
  - evidence:
    `runtime::tests::node_compat::node22_stream_finished_watchpoint`

- `test/parallel/test-stream-pipeline.js`
  - classification: `nlc5_nlc6_boundary_watchpoint`
  - reason: the official file opens local TCP servers through `http` /
    `net`, so it currently fails at `Server.listen(0)` under the
    application-profile no-net contract before the rest of the callback-style
    `stream.pipeline()` assertions run
  - owner: networking-family capability boundary, not the green
    `stream/promises.pipeline()` and non-socket pipeline semantics already
    counted in `NLC5`
  - evidence:
    `runtime::tests::node_compat::node22_stream_pipeline_watchpoint`

- `test/parallel/test-fs-open.js`
  - classification: `application_profile_path_policy_divergence`
  - reason: the official file expects `ENOENT` for an absolute missing host
    path outside the generated bundle root, while Neovex intentionally denies
    that path before raw host open in the application profile
  - owner: runtime path-policy contract difference, not a Node22 positive-path
    `fs` regression
  - evidence:
    `runtime::tests::node_compat::node22_fs_open_watchpoint`

- `test/parallel/test-fs-readdir-buffer.js`
  - classification: `application_profile_path_policy_divergence`
  - reason: the official macOS-only file probes `/dev` with
    `Buffer.from('/dev')`, but the application profile intentionally denies
    host paths outside the generated bundle root instead of claiming broad
    host-filesystem parity
  - owner: runtime path-policy contract difference, not the green
    bundle-root-safe `readdir()` / `withFileTypes` slice
  - evidence:
    `runtime::tests::node_compat::node22_fs_readdir_buffer_watchpoint`

- `test/parallel/test-fs-filehandle-use-after-close.js`
  - classification: `application_profile_path_policy_divergence`
  - reason: the official file reopens `process.execPath`, which resolves to
    the host-side test binary outside the generated bundle root, so the
    application profile intentionally denies that absolute host path before the
    later closed-filehandle `EBADF` assertion can run
  - owner: runtime path-policy contract difference, not a Node22 positive-path
    `fs.promises.FileHandle` regression inside approved roots
  - evidence:
    `runtime::tests::node_compat::node22_fs_filehandle_use_after_close_watchpoint`

- `test/parallel/test-fs-write-file-sync.js`
  - classification: `cross_family_repromotion_pending`
  - reason: the old `worker_threads.isMainThread` self-skip is now gone after
    the embedded main-isolate bootstrap fix, and the official file executes
    cleanly in the focused `NLC8` worker/main-thread batch. It stays out of
    the counted `NLC5` denominator only because that family has not been
    replayed and re-promoted formally on top of the new worker baseline yet.
  - owner: cross-family follow-up after the `NLC8` main-thread bootstrap
    closeout, not a remaining sync `fs.writeFileSync()` / `appendFileSync()`
    runtime bug
  - evidence:
    `runtime::tests::node_compat::node22_nlc8_worker_main_thread_batch_fixture`

- `test/parallel/test-fs-realpath.js`
  - classification: `cross_family_realpath_symlink_seam`
  - reason: the old `worker_threads.isMainThread` skip is now gone, and the
    official file reaches a real runtime failure instead: the second relative
    symlink setup in `test_simple_relative_symlink()` currently throws
    `AlreadyExists` instead of preserving the upstream tmpdir/realpath
    contract.
  - owner: shared `node:fs` realpath/tmpdir/symlink setup semantics, exposed
    by the `NLC8` main-thread worker bootstrap fix rather than hidden behind a
    worker-family gate
  - evidence:
    `runtime::tests::node_compat::node22_nlc8_worker_main_thread_batch_fixture`

- `test/parallel/test-stream-writable-samecb-singletick.js`
  - classification: `later_family_dependency`
  - reason: the official file asserts exact `async_hooks` `TickObject`
    allocation counts during repeated `stream.write()` / `console.log()`
    usage, which is broader task-accounting behavior than the current pure
    `node:stream` contract and currently diverges before any ordinary stream
    semantics fail
  - owner: later `async_hooks` / task-accounting truth, not the current
    `node:stream` data-flow and state contract
  - evidence:
    `runtime::tests::node_compat::node22_stream_writable_samecb_singletick_watchpoint`

## Node20 Validation Slice Status

- Status: `green for the currently manifested validation subset`
- Current measured subset:
  - `311` official `nodejs/node v20.20.2` files passed
  - `0` failed
  - `5` explicit Node20 divergence watchpoints outside the green subset

### Explicit Node20 Divergence Watchpoint

- `test/parallel/test-stream-duplex-readable-end.js`
  - classification: `validation_lane_divergence`
  - reason: the official Node20 file still probes the older default
    high-water-mark flow-control path, while the current runtime matches the
    later Node22/Node24 explicit-highWaterMark shape carried by the newer
    official files
  - owner: validation-lane contract difference, not a blocking Node22 runtime
    seam
  - evidence:
    `runtime::tests::node_compat::node20_stream_duplex_readable_end_watchpoint`

- `test/parallel/test-stream-transform-split-highwatermark.js`
  - classification: `validation_lane_divergence`
  - reason: the official Node20 file still hard-codes the older split
    Transform default `16 * 1024` highWaterMark, while the current runtime
    matches the later Node22/Node24 `getDefaultHighWaterMark()` contract on
    non-Windows hosts
  - owner: validation-lane contract difference, not a blocking Node22 runtime
    seam
  - evidence:
    `runtime::tests::node_compat::node20_stream_transform_split_highwatermark_watchpoint`

- `test/parallel/test-stream-transform-split-objectmode.js`
  - classification: `validation_lane_divergence`
  - reason: the official Node20 file still expects the older split Transform
    object-mode `16 * 1024` highWaterMark defaults, while the current runtime
    matches the later Node22/Node24 non-Windows `64 * 1024` contract
  - owner: validation-lane contract difference, not a blocking Node22 runtime
    seam
  - evidence:
    `runtime::tests::node_compat::node20_stream_transform_split_objectmode_watchpoint`

- `test/parallel/test-stream-readable-infinite-read.js`
  - classification: `validation_lane_divergence`
  - reason: the official Node20 file still depends on the older default
    `Readable` highWaterMark accumulation path, while the current runtime
    matches the later Node22/Node24 explicit-highWaterMark shape carried by
    the newer official files
  - owner: validation-lane contract difference, not a blocking Node22 runtime
    seam
  - evidence:
    `runtime::tests::node_compat::node20_stream_readable_infinite_read_watchpoint`

- `test/parallel/test-fs-stat.js`
  - classification: `validation_lane_divergence`
  - reason: the official Node20 file still requires the older
    `JSON.stringify(Stats)` field shape, while the current runtime intentionally
    matches the newer Node22/Node24 `Stats` contract
  - owner: validation-lane contract difference, not a blocking Node22 runtime
    seam
  - evidence:
    `runtime::tests::node_compat::node20_fs_stat_watchpoint`

## Node24 Preview Status

- Status: `supported-lane watchpoint; not a green support claim`
- Latest explicit supported-lane watchpoint run:
  - `308` passed
  - `0` failed
  - `7` explicit supported-lane divergences outside the green subset

The Node24 supported denominator intentionally excludes
`test-stream-compose-operator.js`, because that official file exists in the
Node20 and Node22 corpora but is not present in `nodejs/node v24.15.0`.
The same denominator also excludes `test-stream-writable-samecb-singletick.js`,
because that file is currently classified as a broader `async_hooks`
task-accounting dependency rather than a pure `node:stream` contract probe.

### Explicit Node24 Preview Divergence

- `test/parallel/test-fs-constants.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file expects a newer constant-surface
    `TypeError` gate that Neovex has not adopted into the current Node22
    contract
  - owner: supported-lane future contract drift, not a current Node22 blocker
  - evidence:
    `runtime::tests::node_compat::node24_fs_constants_watchpoint`

- `test/parallel/test-fs-promises-file-handle-dispose.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file now also asserts `fs.opendir()`
    `Dir[Symbol.asyncDispose]()` close semantics after repeated disposal,
  while the current runtime only matches the older Node20/Node22 filehandle
  disposal contract
  - owner: supported-lane future directory-handle disposal drift, not the
    current Node22 `fs.promises` filehandle contract
  - evidence:
    `runtime::tests::node_compat::node24_fs_promises_file_handle_dispose_watchpoint`

- `test/parallel/test-fs-write-stream.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file now also requires `fs.close()` to be
    observed when destroying `WriteStream` directly, while the current runtime
    still matches the older Node20/Node22 file semantics
  - owner: supported-lane future write-stream lifecycle drift, not the current
    Node22 `fs` contract
  - evidence:
    `runtime::tests::node_compat::node24_fs_write_stream_watchpoint`

- `test/parallel/test-fs-write-stream-autoclose-option.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file now also asserts `ERR_INVALID_THIS` when
    probing `WriteStream.prototype.autoClose`, while the current runtime still
    matches the older Node20/Node22 surface
  - owner: supported-lane future write-stream prototype-surface drift, not the
    current Node22 `fs` contract
  - evidence:
    `runtime::tests::node_compat::node24_fs_write_stream_autoclose_option_watchpoint`

- `test/parallel/test-fs-symlink.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file now expects the newer invalid-type
    `ERR_INVALID_ARG_VALUE` contract for `fs.symlink(..., type)`, while the
    current runtime intentionally preserves the Node22
    `ERR_FS_INVALID_SYMLINK_TYPE` behavior
  - owner: supported-lane future symlink validation drift, not the current
    Node22 `fs` contract
  - evidence:
    `runtime::tests::node_compat::node24_fs_symlink_watchpoint`

- `test/parallel/test-fs-opendir.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file now also asserts newer `ERR_INVALID_THIS`
    receiver checks on `Dir` handles, while the current runtime intentionally
    keeps the Node22 directory-handle surface
  - owner: supported-lane future directory-handle receiver drift, not the
    current Node22 `fs` contract
  - evidence:
    `runtime::tests::node_compat::node24_fs_opendir_watchpoint`

- `test/parallel/test-fs-promises-watch.js`
  - classification: `supported_lane_divergence`
  - reason: the official Node24 file adds `maxQueue` and `overflow` option
    validation that Neovex has not adopted into the current Node22-based
    `fs.watch()` / `fs.promises.watch()` contract
  - owner: supported-lane future watch option-validation drift, not a current
    Node22 blocker
  - evidence:
    `runtime::tests::node_compat::node24_fs_promises_watch_watchpoint`

## Current Holdout Summary

The remaining Node22 watchpoints are currently classified as later-family
boundaries or documented application-profile limitations:

- `test-stream-finished.js` and `test-stream-pipeline.js`
  - `NLC5` / `NLC6` networking boundary (`Server.listen(0)`)
- `test-stream-writable-samecb-singletick.js`
  - later `async_hooks` / task-accounting ownership
- `test-fs-open.js`, `test-fs-readdir-buffer.js`, and
  `test-fs-filehandle-use-after-close.js`
  - application-profile path/capability divergences
- `test-fs-realpath.js` and `test-fs-write-file-sync.js`
  - later `worker_threads` ownership

## Current Local Evidence

- `runtime::tests::node_compat::node22_default_lane_executes_manifested_streams_and_local_io_subset`
- `runtime::tests::node_compat::node20_supported_lane_executes_official_streams_and_local_io_subset`
- `runtime::tests::node_compat::node22_stream_finished_watchpoint`
- `runtime::tests::node_compat::node22_stream_pipeline_watchpoint`
- `runtime::tests::node_compat::node22_fs_open_watchpoint`
- `runtime::tests::node_compat::node22_fs_readdir_buffer_watchpoint`
- `runtime::tests::node_compat::node22_fs_filehandle_use_after_close_watchpoint`
- `runtime::tests::node_compat::node22_fs_write_file_sync_watchpoint`
- `runtime::tests::node_compat::node22_fs_realpath_watchpoint`
- `runtime::tests::node_compat::node22_fs_glob_fixture`
- `runtime::tests::node_compat::node24_fs_glob_fixture`
- `runtime::tests::node_compat::node20_tty_backwards_api_fixture`
- `runtime::tests::node_compat::node22_tty_backwards_api_fixture`
- `runtime::tests::node_compat::node24_tty_backwards_api_fixture`
- `runtime::tests::node_compat::node20_readline_interface_fixture`
- `runtime::tests::node_compat::node22_readline_interface_fixture`
- `runtime::tests::node_compat::node24_readline_interface_fixture`
- `runtime::tests::node_compat::node22_readline_promises_csi_fixture`
- `runtime::tests::node_compat::node20_readline_promises_interface_fixture`
- `runtime::tests::node_compat::node22_readline_promises_interface_fixture`
- `runtime::tests::node_compat::node24_readline_promises_interface_fixture`
- `runtime::tests::node_compat::node22_stream_buffering_batch_fixture`
- `runtime::tests::node_compat::node20_stream_duplex_readable_end_watchpoint`
- `runtime::tests::node_compat::node20_stream_transform_split_highwatermark_watchpoint`
- `runtime::tests::node_compat::node20_stream_transform_split_objectmode_watchpoint`
- `runtime::tests::node_compat::node20_stream_readable_infinite_read_watchpoint`
- `runtime::tests::node_compat::node20_fs_stat_watchpoint`
- `runtime::tests::node_compat::node24_fs_constants_watchpoint`
- `runtime::tests::node_compat::node24_fs_write_stream_watchpoint`
- `runtime::tests::node_compat::node24_fs_write_stream_autoclose_option_watchpoint`
- `runtime::tests::node_compat::node24_fs_promises_file_handle_dispose_watchpoint`
- `runtime::tests::node_compat::node24_fs_symlink_watchpoint`
- `runtime::tests::node_compat::node24_fs_opendir_watchpoint`
- `runtime::tests::node_compat::node24_supported_lane_executes_manifested_streams_and_local_io_subset`
- `docs/architecture/runtime/node-lts-compat/manifests/streams-and-local-io.md`
