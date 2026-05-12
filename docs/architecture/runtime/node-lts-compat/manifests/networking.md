# Networking Node Test Slices

Current upstream Node test-slice manifest for `NLC6`.

This file records the currently counted green denominator and staged upstream
corpus for the family. Requirements, closeout gates, and roadmap decisions
belong in `docs/plans/archive/node-lts-compatibility-plan.md`.

Source corpus:

- current Deno-family implementation baseline:
  `~/src/github.com/nimbus/deno @ v2.7.14-locker.37 (b748ccc7f66b89dd5a1048e4dcfd152e35bd9682)`
- pinned official Node22 validation corpus:
  `nodejs/node @ v22.15.0`
- pinned official Node20 supported corpus:
  `nodejs/node @ v20.20.2`
- staged future Node24 supported corpus:
  `nodejs/node @ v24.15.0`

The first `NLC6` slice follows the same fast pattern that worked for `NLC5`:
import official Node files as data, batch them by shared runtime seam, and keep
the first denominator intentionally narrow so real builtin/runtime gaps surface
before socket/listen/request noise takes over.

## Initial Slice Map

| Family | Initial upstream test slices |
| --- | --- |
| `node:dns` | `test/parallel/test-dns-*.js`, focused first on `Resolver#getServers()` and default-result-order files that probe ordering semantics without requiring successful external resolution |
| `node:net` | `test/parallel/test-net-*.js`, focused first on pure IP helpers and `createConnection()` option validation |
| `node:dgram` | `test/parallel/test-dgram-*.js`, now widened through the local-socket helper, bind/lifecycle, connected-send, callback-send, and broader fd/multicast/error waves, with cluster, host/preset, and `reusePort` boundaries still held explicit |
| `node:tls` | `test/parallel/test-tls-*.js`, with the first `https` helper/server wave now promoted and the remaining legacy TLSv1.1 protocol seam kept explicit |
| `node:http` / `node:https` | `test/parallel/test-http-*.js`, `test/parallel/test-https-*.js`, now widened through `http.Agent`, request/response/server, the first crypto-gated `https` helper/server wave, the follow-on local `https` lifecycle/socket wave, and the next client/server semantics wave |
| `node:http2` | `test/parallel/test-http2-*.js`, now widened through the first internal utility/helper wave including `getPackedSettings()`, the first compat request/response core wave, the follow-on compat server-response lifecycle wave, and the remaining compat request/control/socket wave |

## Initial Corpus Counts

The first-pass official candidate corpus from the canonical local
`~/src/github.com/nodejs/node` worktree is:

- Node22: `1243` files
- Node20: `1230` files
- Node24 supported: `1342` files

These are intentionally broad candidate counts, not the future green
denominator. The first manifested batch is much smaller on purpose: prove the
builtin front door and a pure helper seam first, then widen into local-net
servers, requests, sockets, and the heavier `tls` / `http2` families.

## Current Manifested Official Subset

The manifested `NLC6` batch is now live in
[`NETWORKING_BATCH`](../../../../crates/nimbus-runtime/src/runtime/tests/node/mod.rs).

Current manifested batch counts:

- Node22 default lane: `270` official files
- Node20 supported lane: `265` official files
- Node24 supported lane: `268` staged official files
  - current explicit supported-lane watchpoint run: `268` passed, `0` failed

## Package Canary Evidence

Pinned package canaries for the `NLC6` networking family now live under
[`tests/runtime/node/networking-canaries/`](../../../../tests/runtime/node/networking-canaries/).

Current checked-in package set:

- `express@4.19.2`
- `fastify@4.28.1`
- `socket.io@4.7.5`
- `socket.io-client@4.7.5`
- `undici@6.19.8`
- `axios@1.7.7`

Current package-canary lane mapping:

- Node22 default `Application` lane:
  `express`, `fastify`, `socket.io`, `undici`, `axios`
- Node20 supported `Application` lane:
  `express`, `fastify`

Current measured canary result:

- `runtime::tests::basic_invocation::application_node22_networking_package_canary_batch`
  is green
- `runtime::tests::basic_invocation::application_node20_networking_supported_canary_batch`
  is green

Current manifested slice coverage:

- `node:dns` `Resolver#getServers()` parity through
  `test-dns-get-server.js`
- `node:dns` default-result-order semantics through
  `test-dns-set-default-order.js`,
  `test-dns-default-order-ipv4.js`,
  `test-dns-default-order-ipv6.js`, and
  `test-dns-default-order-verbatim.js`
- callback-style socket-backed stream completion through
  `test-stream-finished.js`
- callback-style `stream.pipeline()` request/server/socket behavior through the
  shared Node20 / Node22 official `test-stream-pipeline.js` body
- `node:net` pure helper and input-validation semantics through
  `test-net-connect-options-invalid.js`,
  `test-net-isip.js`,
  `test-net-isipv4.js`, and
  `test-net-isipv6.js`
- `node:net` local listen/server lifecycle and option-validation semantics
  through
  `test-net-connect-no-arg.js`,
  `test-net-listening.js`,
  `test-net-listen-close-server.js`,
  `test-net-server-close.js`,
  `test-net-server-call-listen-multiple-times.js`,
  `test-net-server-listen-options.js`,
  `test-net-server-listen-options-signal.js`, and
  no-arg `server.unref()` persistence through
  `test-net-server-unref-persistent.js`
- `node:http` no-arg `server.listen()` option-hook semantics through
  `test-http-server-options-incoming-message.js` and
  `test-http-server-options-server-response.js`
- `node:net` local socket lifecycle, timeout, and local-address semantics
  through
  `test-net-after-close.js`,
  `test-net-settimeout.js`,
  `test-net-can-reset-timeout.js`,
  `test-net-socket-close-after-end.js`,
  `test-net-socket-connecting.js`, and
  `test-net-local-address-port.js`
- `node:http` pure `http.Agent` helper semantics through
  `test-http-agent-getname.js`,
  `test-http-agent-close.js`, and
  `test-http-agent-timeout-option.js`
- `node:http` `http.Agent` keepalive, socket-pool, and scheduling semantics
  through
  `test-http-agent-keepalive.js`,
  `test-http-agent-keepalive-delay.js`,
  `test-http-agent-maxsockets.js`,
  `test-http-agent-maxsockets-respected.js`,
  `test-http-agent-maxtotalsockets.js`,
  `test-http-agent-scheduling.js`, and
  `test-http-agent-timeout.js`
- `node:http` `http.Agent` lifecycle, removal, idle-error, uninitialized
  socket, and abort semantics through
  `test-http-agent-false.js`,
  `test-http-agent-no-protocol.js`,
  `test-http-agent-null.js`,
  `test-http-agent-remove.js`,
  `test-http-agent-destroyed-socket.js`,
  `test-http-agent-error-on-idle.js`,
  `test-http-agent-uninitialized.js`,
  `test-http-agent-uninitialized-with-handle.js`, and
  `test-http-agent-abort-controller.js`
- `node:http` basic client/server request semantics through
  `test-http-client-defaults.js`,
  `test-http-client-get-url.js`,
  `test-http-client-request-options.js`,
  `test-http-client-upload.js`,
  `test-http-client-upload-buf.js`,
  `test-http-automatic-headers.js`, and
  `test-http-client-close-event.js`
- `node:http` client/request/response timeout semantics through
  `test-http-client-timeout-option.js`,
  `test-http-client-set-timeout.js`,
  `test-http-client-response-timeout.js`, and
  `test-http-set-timeout.js`
- `node:http` plain response/head and response-loop semantics through
  `test-http-contentLength0.js`,
  `test-http-head-request.js`, and
  `test-http-response-statuscode.js`,
  `test-http-write-head.js`, and
  `test-http-response-writehead-returns-this.js`
- `node:http` response/header/bodyless-HEAD semantics through
  `test-http-response-add-header-after-sent.js`,
  `test-http-response-remove-header-after-sent.js`,
  `test-http-response-no-headers.js`,
  `test-http-response-readable.js`,
  `test-http-response-close.js`,
  `test-http-response-cork.js`,
  `test-http-response-multi-content-length.js`,
  `test-http-response-multiheaders.js`,
  `test-http-response-setheaders.js`,
  `test-http-status-code.js`,
  `test-http-head-response-has-no-body.js`,
  `test-http-head-response-has-no-body-end.js`,
  `test-http-head-response-has-no-body-end-implicit-headers.js`,
  `test-http-head-throw-on-response-body-write.js`, and the Node22/Node24-only
  `test-http-write-head-after-set-header.js`
- `node:http` plain status-message parsing semantics through
  `test-http-response-status-message.js`
- `node:http` broader response/status wave through
  `test-http-status-message.js`,
  `test-http-status-reason-invalid-chars.js`, and
  `test-http-write-head-2.js`,
  `test-http-response-splitting.js`
- `node:net` pre-connect end-callback semantics through
  `test-net-end-without-connect.js`
- `node:http2` pure internal-utility helper semantics through
  `test-http2-util-asserts.js`,
  `test-http2-util-assert-valid-pseudoheader.js`, and
  `test-http2-util-nghttp2error.js`
- first crypto-gated `node:https` / `node:http2` helper semantics through
  `test-https-agent-constructor.js`,
  `test-https-agent-getname.js`,
  `test-https-agent.js`,
  `test-https-agent-abort-controller.js`,
  `test-https-server-options-incoming-message.js`,
  `test-https-server-options-server-response.js`,
  `test-https-client-get-url.js`,
  `test-http2-getpackedsettings.js`,
  `test-http2-util-headers-list.js`,
  `test-http2-util-update-options-buffer.js`, and
  `test-http2-misc-util.js`
- follow-on `node:https` `https.Agent` connection/session/global-agent semantics
  through
  `test-https-agent-create-connection.js`,
  `test-https-agent-disable-session-reuse.js`,
  `test-https-agent-servername.js`,
  `test-https-agent-session-injection.js`,
  `test-https-agent-sni.js`,
  `test-https-agent-sockets-leak.js`, and
  `test-https-client-override-global-agent.js`
- follow-on `node:https` local request/server/timeout/property semantics
  through
  `test-https-abortcontroller.js`,
  `test-https-argument-of-creating.js`,
  `test-https-byteswritten.js`,
  `test-https-close.js`,
  `test-https-max-headers-count.js`,
  `test-https-request-arguments.js`,
  `test-https-server-headers-timeout.js`,
  `test-https-server-request-timeout.js`,
  `test-https-set-timeout-server.js`,
  `test-https-simple.js`,
  `test-https-timeout.js`,
  `test-https-timeout-server.js`, and
  `test-https-timeout-server-2.js`
- follow-on `node:https` server lifecycle/socket semantics through
  `test-https-server-close-all.js`,
  `test-https-server-close-destroy-timeout.js`,
  `test-https-server-close-idle.js`,
  `test-https-socket-options.js`,
  `test-https-keep-alive-drop-requests.js`, and
  `test-https-server-connections-checking-leak.js`
- follow-on `node:https` client/server request, validation, parser, and
  disposal semantics through
  `test-https-client-checkServerIdentity.js`,
  `test-https-client-reject.js`,
  `test-https-connecting-to-http.js`,
  `test-https-drain.js`,
  `test-https-eof-for-eom.js`,
  `test-https-host-headers.js`,
  `test-https-insecure-parse-per-stream.js`,
  `test-https-max-header-size-per-stream.js`,
  `test-https-options-boolean-check.js`,
  `test-https-server-async-dispose.js`, and
  `test-https-truncate.js`
- follow-on `node:https` certificate-safety and response highWaterMark
  semantics through shared-LTS
  `test-https-selfsigned-no-keycertsign-no-crash.js` plus Node22 / Node24
  `test-https-hwm.js`
- follow-on shared-LTS `node:https` credential, local-socket, and strict-auth
  semantics through
  `test-https-pfx.js`,
  `test-https-unix-socket-self-signed.js`, and
  `test-https-strict.js`
- follow-on shared-LTS `node:http2` header/status/options semantics through
  `test-http2-status-code.js`,
  `test-http2-status-code-invalid.js`,
  `test-http2-multi-content-length.js`,
  `test-http2-response-splitting.js`,
  `test-http2-options-server-request.js`,
  `test-http2-options-server-response.js`,
  `test-http2-zero-length-header.js`,
  `test-http2-multiheaders.js`, and
  `test-http2-multiheaders-raw.js`
- follow-on shared-LTS `node:http2` compat request/response core semantics
  through
  `test-http2-compat-serverresponse.js`,
  `test-http2-compat-serverresponse-end.js`,
  `test-http2-compat-serverresponse-write.js`,
  `test-http2-compat-serverresponse-writehead.js`,
  `test-http2-compat-serverresponse-writehead-array.js`,
  `test-http2-compat-serverresponse-statuscode.js`,
  `test-http2-compat-serverresponse-statusmessage.js`,
  `test-http2-compat-serverresponse-statusmessage-property.js`,
  `test-http2-compat-serverresponse-statusmessage-property-set.js`,
  `test-http2-compat-serverresponse-headers.js`,
  `test-http2-compat-serverrequest.js`,
  `test-http2-compat-serverrequest-end.js`,
  `test-http2-compat-serverrequest-headers.js`,
  `test-http2-compat-serverrequest-host.js`,
  `test-http2-compat-serverrequest-pause.js`,
  `test-http2-compat-serverrequest-pipe.js`,
  `test-http2-compat-serverrequest-settimeout.js`, and
  `test-http2-compat-serverrequest-trailers.js`
- follow-on shared-LTS `node:http2` compat server-response lifecycle and
  positive early-hints semantics through
  `test-http2-compat-serverresponse-close.js`,
  `test-http2-compat-serverresponse-destroy.js`,
  `test-http2-compat-serverresponse-drain.js`,
  `test-http2-compat-serverresponse-end-after-statuses-without-body.js`,
  `test-http2-compat-serverresponse-finished.js`,
  `test-http2-compat-serverresponse-flushheaders.js`,
  `test-http2-compat-serverresponse-headers-after-destroy.js`,
  `test-http2-compat-serverresponse-headers-send-date.js`,
  `test-http2-compat-serverresponse-settimeout.js`,
  `test-http2-compat-serverresponse-trailers.js`,
  `test-http2-compat-write-early-hints.js`, and
  `test-http2-compat-write-head-destroyed.js`
- follow-on shared-LTS `node:http2` compat invalid-argument early-hints exit
  semantics through
  `test-http2-compat-write-early-hints-invalid-argument-type.js` and
  `test-http2-compat-write-early-hints-invalid-argument-value.js`
- follow-on shared-LTS `node:http2` compat request-control, push, and socket
  semantics through
  `test-http2-compat-aborted.js`,
  `test-http2-compat-client-upload-reject.js`,
  `test-http2-compat-errors.js`,
  `test-http2-compat-expect-continue-check.js`,
  `test-http2-compat-expect-continue.js`,
  `test-http2-compat-expect-handling.js`,
  `test-http2-compat-method-connect.js`,
  `test-http2-compat-serverresponse-createpushresponse.js`,
  `test-http2-compat-short-stream-client-server.js`,
  `test-http2-compat-socket-destroy-delayed.js`,
  `test-http2-compat-socket-set.js`, and
  `test-http2-compat-socket.js`
- widened pure `node:tls` helper and local-server semantics through shared-LTS
  `test-tls-basic-validations.js`,
  `test-tls-check-server-identity.js`,
  `test-tls-connect-abort-controller.js`,
  `test-tls-connect-allow-half-open-option.js`,
  `test-tls-connect-hwm-option.js` in Node22 / Node24,
  `test-tls-connect-no-host.js`,
  `test-tls-connect-simple.js`,
  `test-tls-connect-timeout-option.js`,
  `test-tls-options-boolean-check.js`, and
  `test-tls-server-parent-constructor-options.js`
- initial `node:dgram` helper and argument-validation semantics through
  `test-dgram-bytes-length.js`,
  `test-dgram-createSocket-type.js`,
  `test-dgram-send-address-types.js`,
  `test-dgram-send-bad-arguments.js`,
  `test-dgram-send-invalid-msg-type.js`,
  `test-dgram-close-is-not-callback.js`,
  `test-dgram-send-empty-array.js`, and
  `test-dgram-send-empty-buffer.js`
- follow-on `node:dgram` bind/address/lifecycle/ref semantics through
  `test-dgram-address.js`,
  `test-dgram-bind-default-address.js`,
  `test-dgram-bind.js`,
  `test-dgram-close.js`,
  `test-dgram-listen-after-bind.js`,
  `test-dgram-ref.js`,
  `test-dgram-unref.js`, and
  `test-dgram-implicit-bind.js`
- connected `node:dgram` send/default-host semantics through
  `test-dgram-connect.js`,
  `test-dgram-connect-send-callback-buffer.js`,
  `test-dgram-connect-send-callback-buffer-length.js`,
  `test-dgram-connect-send-callback-multi-buffer.js`,
  `test-dgram-connect-send-default-host.js`,
  `test-dgram-connect-send-empty-array.js`,
  `test-dgram-connect-send-empty-buffer.js`, and
  `test-dgram-connect-send-empty-packet.js`
- broader `node:dgram` send/callback/default-host semantics through
  `test-dgram-send-callback-buffer-empty-address.js`,
  `test-dgram-send-callback-buffer-length-empty-address.js`,
  `test-dgram-send-callback-buffer-length.js`,
  `test-dgram-send-callback-buffer.js`,
  `test-dgram-send-callback-multi-buffer-empty-address.js`,
  `test-dgram-send-callback-multi-buffer.js`,
  `test-dgram-send-callback-recursive.js`,
  `test-dgram-send-cb-quelches-error.js`,
  `test-dgram-send-default-host.js`,
  `test-dgram-send-empty-packet.js`,
  `test-dgram-send-multi-buffer-copy.js`,
  `test-dgram-send-multi-string-array.js`, and
  `test-dgram-sendto.js`
- broader pure `node:dgram` local-socket/fd/multicast/error semantics through
  `test-dgram-abort-closed.js`,
  `test-dgram-bind-error-repeat.js`,
  `test-dgram-bind-fd-error.js`,
  `test-dgram-bind-fd.js`,
  `test-dgram-bind-socket-close-before-lookup.js`,
  `test-dgram-blocklist.js`,
  `test-dgram-close-during-bind.js`,
  `test-dgram-close-in-listening.js`,
  `test-dgram-close-signal.js`,
  `test-dgram-connect-send-multi-buffer-copy.js`,
  `test-dgram-connect-send-multi-string-array.js`,
  `test-dgram-create-socket-handle-fd.js`,
  `test-dgram-create-socket-handle.js`,
  `test-dgram-custom-lookup.js`,
  `test-dgram-membership.js`,
  `test-dgram-msgsize.js`,
  `test-dgram-multicast-loopback.js`,
  `test-dgram-multicast-set-interface.js`,
  `test-dgram-multicast-setTTL.js`,
  `test-dgram-oob-buffer.js`,
  `test-dgram-recv-error.js`,
  `test-dgram-send-error.js`,
  `test-dgram-send-queue-info.js`,
  `test-dgram-setBroadcast.js`,
  `test-dgram-setTTL.js`,
  `test-dgram-socket-buffer-size.js`, and
  `test-dgram-udp4.js`

This denominator is intentionally narrower than a real networking support
claim, but it is no longer pure-helper-only. It now proves that the Node22
loader admits the networking builtin family, that the first `dns` / `net` /
`http.Agent` / pure-`http2` helper semantics are green, that callback-style
local request/response/server/socket behavior is green through
`stream.finished()` / `stream.pipeline()`, that the first plain `node:net`
local listen/server lifecycle slice is green across the supported lanes, that
the no-arg `server.unref()` listen path is green too, that the next local
socket lifecycle / timeout / local-address slice is green, that the first
basic `http` client/server request wave is green, that the next plain `http`
timeout/request timer seam is green as well, that the no-arg `http.Server`
option-hook slice is green, that the first plain `http` response/head slice is
green, and that the broader shared-LTS response/status wave is green too:
response close/cork behavior, repeated header handling, multi-content-length
rejection, over-the-wire status-message behavior, invalid status-message
character rejection, invalid status-code request-loop handling, `writeHead()`
object/array override handling, raw-socket invalid-header-content protection,
and pre-connect `net.Socket().end(cb)` callback delivery are all now counted
in-family on the current canonical local-fork Deno baseline, and
the next shared-LTS `http.Agent` keepalive/pool/scheduling wave is green too:
keepalive reuse, keepalive initial delay propagation, `maxSockets`,
`maxTotalSockets`, pooled socket scheduling, and timed-out socket replacement
are all now counted in-family, and the follow-on `http.Agent` lifecycle wave is
green as well: `agent: false`, null/default-protocol handling, `agentRemove`,
destroyed queued-socket replacement, idle free-socket error eviction,
uninitialized free-socket reuse, and `AbortController` cancellation now all
pass in the manifested subset. The first pure `node:dgram` helper wave is
green too: message-length callback delivery, socket-type validation,
address-argument validation, send-argument bounds/type checking, invalid
message-type rejection, non-function `close()` callback handling, and
zero-length datagram delivery now all pass in the manifested subset. The
follow-on `node:dgram` bind/address/lifecycle wave is green as well: socket
address reporting, default bind-address selection, repeated `bind()` rejection
after a successful bind, close-before-lookup safety, `listening`-after-`bind`
delivery, `ref()` / `unref()` behavior with and without a live handle, and
implicit bind during repeated `send()` calls now all pass in the manifested
subset. The connected `node:dgram` wave is green too: explicit UDP
`connect()`, connected default-host delivery, callback byte-count reporting
across Buffer and multi-buffer paths, and empty-array / empty-buffer /
empty-packet sends now all pass in-family. The broader unconnected
`node:dgram` send/callback/default-host wave is green as well: empty-address
callback overloads, recursive callback scheduling, callback-quelched error
delivery, default-host sends, empty-packet delivery, multi-buffer copy
semantics, string-array sends, and `sendto()` argument validation now all
pass in the manifested subset. The broader pure `node:dgram`
local-socket/fd/multicast/error wave is green too: abort-after-close handling,
bind-error repeatability, fd-open and `_createSocketHandle()` validation,
close-during-bind paths, close-signal delivery, custom lookup wiring,
membership and multicast helpers, send/recv error delivery, send-queue
introspection, socket buffer-size semantics, broadcast and TTL controls, and
the broader UDP4 smoke path now all pass in the manifested subset. The first
crypto-gated `https` / `http2` helper wave is also green now: `https.Agent`
construction/getName, repeated HTTPS agent/server/client request behavior, the
two `https.createServer()` option-hook files, `NODE_TLS_REJECT_UNAUTHORIZED=0`
URL-path client behavior, `http2.getPackedSettings()`, and the next
`internal/test/binding('http2')` helper files all now pass in-family. The
follow-on shared-LTS `https.Agent` connection/session/global-agent wave is
green too: legacy `Agent#createConnection()` overload handling, session reuse
disablement, explicit session injection, SNI/servername propagation, socket
leak prevention, and `https.globalAgent` override behavior now all pass in the
manifested subset. The next shared-LTS local `https` request/server/timeout
wave is green as well: `AbortSignal` request cancellation, `createServer()`
argument normalization including `ALPNCallback`, HTTPS socket `bytesWritten`,
close-on-shutdown behavior, client/server max-header-count handling,
`https.request()` URL-plus-options argument merging, server `headersTimeout`
and `requestTimeout` property semantics, server/request/response timeout
callbacks, handshake-timeout `clientError` delivery, and the simple
request/response smoke path now all pass in the manifested subset. The
remaining explicit Node22 watchpoints in this family are now outside that UDP,
`https`, and `http.Agent` contract:
`test-http-agent-reuse-drained-socket-only.js` currently blocks in
`process.report.getReport()` and then reaches `process.exit()`, so it stays
pinned as a cross-family process/report and embedded-exit dependency rather
than a pure networking-owner seam, and
`test-https-agent-additional-options.js` now stays explicit as the legacy
TLSv1.1 / `TLSv1_1_method` secureProtocol boundary in the current
rustls-backed TLS owner layer rather than an ordinary `https.Agent` helper
regression. The follow-on TLS session/ticket/keylog wave is green now too:
the shared `test-https-client-resume.js` and
`test-https-resume-after-renew.js` files, Node22
`test-https-agent-session-reuse.js`, and the Node22/Node24
`test-https-agent-keylog.js` files all now pass in-family on the published
`v2.7.14-locker.36` Deno baseline. The follow-on PFX, unix-socket, and
strict-auth remainder is green now too after the canonical Deno
certificate-verifier fix plus the Nimbus-owned short-path `PIPE` harness
cleanup. The next shared-LTS `http2` header/status/options wave is green now
too: status-code acceptance and rejection, content-length single-value
enforcement, response-splitting stripping, custom `Http2ServerRequest` /
`Http2ServerResponse` option hooks, zero-length header handling, and
multiheader / raw-header ordering all now pass in-family. The remaining
explicit watchpoints in this family are now the legacy TLSv1.1 boundary, the
cross-family `process.report` / embedded-exit dependency, the host/preset
and `dgram` boundary batches, the Node20 supported divergences, and the
Node24-only `test-stream-pipeline.js` drift.
