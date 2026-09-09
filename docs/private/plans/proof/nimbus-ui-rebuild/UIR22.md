# UIR22 Sandboxes

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `c1bef5ec9`. Sandboxes were visible only as a static view inside
Compute, with no list, no lifecycle action and no way to reach a running
process. The server could open a session on a sandbox and admit the
`stdio` channel, but nothing moved bytes over it. The server now streams
one channel of an open session as NDJSON frames and takes input back on
it; the console has a Sandboxes section with a polled list, stop behind
a confirmation, a create picker, a detail page with Overview, Console
and Spec tabs, and a console panel that opens a `stdio` session and
appends every frame in arrival order.

## Seam finding

The plan named "live logs" beside the exec panel. No sandbox system
events are recorded, so there is no log stream to show: the Console tab
is the live log, and the conditions on Overview (reason, message,
transition time) are the lifecycle history. The detail page has three
tabs, not four.

The bytes seam is transport-free in `nimbus-services`
(`manager/session_streams.rs`): a `SessionChannelSource` turns a session
target and a channel name into a frame receiver plus an input sender,
and the manager validates the session, fences the target generation and
keeps the sender so a later write reaches the same attachment. The
default source (`UnsupportedSessionChannelSource`) answers `opened` then
`closed` with "backend `krun` does not stream channel `stdio` bytes yet",
so the wire contract, the console and the tests exist before a backend
implements it. The wire encoding is the server's
(`http/session_channels.rs`).

The production `nimbus start` binary runs no service manager, so every
sandbox and session route answers 404 there. The list page shows that as
one plain "Sandbox routes not available on this server" state with the
server's message; the e2e walk covers the navigation and that state, and
the msw specs cover the live list, the console and the picker.

`useApiRead` flashed the loading skeleton on every poll tick. It now
takes a `revision` and keeps the last value on a same-path re-read.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-services/src/manager/session_streams.rs` (new), `manager.rs`, `manager/types.rs`, `manager/session_channels.rs`, `lib.rs` | `SessionChannelFrame` (`Stdout`, `Stderr`, `Exit { code }`, `Closed { reason }`), `SessionChannelAttachment`, the `SessionChannelSource` trait, `ServiceManager::attach_session_channel`, `write_session_channel`, `detach_session_channel`, `with_session_channel_source`; a 64-frame buffer per attachment. |
| `crates/nimbus-server/src/http/session_channels.rs` (new), `http/mod.rs`, `http/sessions.rs`, `router.rs` | `GET /api/sessions/{id}/channels/{channel}/stream?tenantId=` answers `application/x-ndjson`: `{kind:"opened",channel,targetGeneration}` first, then `stdout`, `stderr`, `exit`, and `closed` last. `POST .../input?tenantId=` takes `{data}` and answers 202. Both routes authorize the session the way `get_session` does. |
| `src/lib/types/sandbox.ts` (new) | Sandbox, session and condition types as the routes return them; `sandboxCanStop`, `sandboxIsTransitional`, `sandboxDisplayName`. |
| `src/lib/session-channel.ts` (new) | `parseSessionChannelFrame` (a bad line is dropped, never thrown), `readSessionChannel` (a plain `fetch` on the console session, line-buffered, delivers frames in order and stops at once on abort, cancelling the reader). |
| `src/lib/api-mutations.ts` | `sandboxes.list/get/create/stop`, `sessions.open/close/writeChannel`; `apiErrorMessage` exported. |
| `src/hooks/use-api-read.ts` | Third argument `revision`; a same-path re-read keeps the last value. |
| `src/routes/developer/sandboxes/-sandbox-read.ts` (new) | `useSandboxList`, `useSandbox`: 404 → `unavailable` / `missing` with the server's message; poll every 2 s while any row is transitional, then stop. |
| `src/routes/developer/sandboxes.tsx` (new) | List on `DataTable` (name, `StatePill`, profile and backend `CategoryPill`s, health, endpoints, updated); row menu `Open sandbox`, `Open console`, `Stop sandbox` (only when the state allows; `stopping` on the row while the request is out; refusal text under the pill); `StopSandboxDialog`; `New sandbox`; states loading, offline, error, unavailable, empty, no tenant. |
| `src/routes/developer/sandboxes/-create-sandbox-dialog.tsx` (new) | Picker: id, display name, profile (`worker`/`desktop`), backend (`krun`/`container`), OCI image, command one argument per line. `draftToRequest` refuses a bad id or a missing image client side. Success opens the detail. |
| `src/routes/developer/sandboxes_.$sandbox.tsx` (new) | Breadcrumb, pills, id copy chip, Stop behind the same dialog; tabs Overview (facts, endpoints as `host:port`, conditions), Console, Spec (owner, root image or `redacted: reason`, process with `N values, redacted`). Not found on 404 with a way back. |
| `src/routes/developer/sandboxes/-sandbox-console.tsx` (new) | Opens `POST /api/sessions` with `channels: ["stdio"]` and a 30 min TTL when the sandbox is `ready` (else waits for `Connect`), reads the stream, appends `opened`, `stdout`, `stderr`, `exit`, `closed` lines in arrival order, echoes input and posts it with a trailing newline, closes the session on unmount ("console closed") or on `Disconnect` ("operator disconnected"). |
| `src/routes/developer/sandboxes/-sandbox-sub-panel.tsx` (new), `nav-entries.ts`, `routes/index.tsx`, `compute.tsx` | Sub-panel list with `StateDot`s; `Sandboxes` in the Run group after Services; restorable section; Compute's Sandboxes view links here. |
| `DESIGN.md` | Developer IA has nine sections; new "Sandboxes (Developer)" section. |

## Spec notes

- `crates/nimbus-server/src/tests/service_manager/session_channels.rs`
  (2): `session_channel_stream_relays_frames_in_order_and_takes_input`
  installs a test source, reads the stream over HTTP as `opened`,
  `stdout`, `stderr`, `exit`, `closed` in that order, and posts input
  that the source receives; `..._closes_when_the_backend_cannot_stream`
  reads `opened` then `closed` with the backend's reason under the
  default source.
- `session-channel.spec.ts` (6): the path; each frame kind parses; a
  malformed line, an unknown kind, a missing or mistyped field is null;
  under an msw streamed body the frames arrive in order with one frame
  split across chunks; a 409 answers `{ok:false,error,status}`; an abort
  mid-stream ends `aborted: true` with delivery stopped.
- `-sandbox-console.spec.tsx` (4, **the acceptance spec**): the panel
  opens the session with the exact body and appends the five frames as
  `console-line` items in order with their kinds and texts, ends
  `data-connection="ended"` with no close call; an input line posts
  `{data:"ls -la\n"}` to the channel route and unmount closes the session
  with `"console closed"`; a `starting` sandbox opens nothing until
  Connect; a refused open is an error line with the Connect control kept.
- `sandboxes.spec.tsx` (7): pills, profile, backend and health per row;
  the unavailable state on 404 with the server's message and the create
  button disabled; no tenant; the empty state offers New sandbox; a
  stopped row has no stop item, stop on a ready row posts `/stop` only
  after the confirmation and reads the list again; Open console
  navigates with `tab: "console"`; the picker refuses an empty form, then
  posts the exact create body and navigates to the detail.
- `sandboxes_.$sandbox.spec.tsx` (6): Overview with pill, endpoints,
  conditions and labels; the console tab mounts the panel with the
  sandbox and its state; the spec tab shows the image and the redacted
  counts; not found on 404 with the server's message; Stop behind the
  dialog posts `/stop` and the pill reads `stopped` after the re-read
  with Stop disabled; no tenant.
- `-sandbox-read.spec.ts` (4): 404 → unavailable and missing; the list
  polls (`pollMs=20`) while a row is `starting` and stops after it reads
  `ready` (three reads, none after); a revision bump reads again.
- `use-api-read.spec.tsx` (+1): a revision bump reads the same path
  again and keeps the last value until the new one lands.
- `api-mutations.spec.ts` (+2): the sandbox paths and bodies; the session
  open, input (202 → `{ok:true,data:null}`) and close bodies with the
  tenant in the query.
- `tests/e2e/sandboxes.spec.ts` (1): the sidebar row reaches
  `/ui/developer/sandboxes`, the scope chip names the tenant, the
  unavailable state names the missing routes and New sandbox is disabled;
  `/ui/developer/sandboxes/ghost?tab=console` lands on not found and Back
  to Sandboxes returns to the list.
- Fail-before: `$S/uir22-fail-before.txt` records both server tests
  failing before the routes existed (`0 passed; 2 failed`). The first
  spec run failed the abort case because the reader delivered buffered
  frames after the abort; the reader now stops at once and cancels. A
  reused msw 404 `Response` read its body twice; the spec builds one per
  request. The first e2e run used a jest-dom matcher in Playwright.
- Net: 129 files, 1118 tests (1088 at UIR21).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-server --lib session_channel_stream -- --test-threads=1` | 2 passed |
| `cargo test -p nimbus-services session` | 14 passed |
| `cargo fmt --all --check`, `make check`, `make clippy` | clean |
| `npm run lint` | clean, 1 pre-existing warning, 3 infos |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 129 files, 1118 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` | ok |
| `npm run test:e2e` | 26 passed, 1 skipped on the first run (sandboxes failed on the matcher); the sandboxes walk 1 passed after the fix |
| `verify.sh` | 0 failing |

Screenshots at 1280×720 from the e2e walk, tenant `sandbox-e2e`:
`UIR22-sandboxes-unavailable.png` (the list page on a server without
sandbox routes, with the server's message), `UIR22-sandbox-not-found.png`
(a direct link to a missing sandbox).

## Open items

- No backend streams `stdio` bytes yet: the default source answers
  `opened` then `closed` with the reason, which the console shows as one
  line. The wire contract and the panel are ready for the first backend.
- The e2e server runs no service manager, so the live list, the console
  and the picker are proven under msw only.
- The console keeps every line in memory with no cap; a long-running
  shell grows the log until the panel unmounts.
- The `desktop` profile has no console channel beyond `stdio`; `files`
  is admitted by the session route but has no panel.
