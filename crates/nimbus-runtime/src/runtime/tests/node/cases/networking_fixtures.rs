const NODE22_STREAM_STATE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-stream-decoder-objectmode.js",
    "test/parallel/test-stream-push-strings.js",
    "test/parallel/test-stream-readable-error-end.js",
    "test/parallel/test-stream-readable-with-unimplemented-_read.js",
    "test/parallel/test-stream-transform-hwm0.js",
    "test/parallel/test-stream-unshift-read-race.js",
    "test/parallel/test-stream-writable-clear-buffer.js",
    "test/parallel/test-stream-writable-null.js",
    "test/parallel/test-stream-writableState-ending.js",
    "test/parallel/test-stream-writableState-uncorked-bufferedRequestCount.js",
];

const NODE22_STREAM_BUFFERING_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-stream-aliases-legacy.js",
    "test/parallel/test-stream-await-drain-writers-in-synchronously-recursion-write.js",
    "test/parallel/test-stream-backpressure.js",
    "test/parallel/test-stream-big-packet.js",
    "test/parallel/test-stream-big-push.js",
    "test/parallel/test-stream-err-multiple-callback-construction.js",
    "test/parallel/test-stream-pipe-deadlock.js",
    "test/parallel/test-stream-pipe-same-destination-twice.js",
    "test/parallel/test-stream-push-order.js",
    "test/parallel/test-stream-readable-object-multi-push-async.js",
];

const NODE22_TTY_OS_TAIL_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-os-eol.js",
    "test/parallel/test-os-checked-function.js",
    "test/parallel/test-ttywrap-invalid-fd.js",
    "test/parallel/test-ttywrap-stack.js",
];

const NODE22_NETWORKING_PURE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dns-get-server.js",
    "test/parallel/test-dns-set-default-order.js",
    "test/parallel/test-dns-default-order-ipv4.js",
    "test/parallel/test-dns-default-order-ipv6.js",
    "test/parallel/test-dns-default-order-verbatim.js",
    "test/parallel/test-net-connect-options-invalid.js",
    "test/parallel/test-net-isip.js",
    "test/parallel/test-net-isipv4.js",
    "test/parallel/test-net-isipv6.js",
    "test/parallel/test-http-agent-getname.js",
    "test/parallel/test-http-agent-close.js",
    "test/parallel/test-http-agent-timeout-option.js",
    "test/parallel/test-http2-util-asserts.js",
    "test/parallel/test-http2-util-nghttp2error.js",
];

const NODE22_NETWORKING_NET_SERVER_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-net-connect-no-arg.js",
    "test/parallel/test-net-listening.js",
    "test/parallel/test-net-listen-close-server.js",
    "test/parallel/test-net-server-close.js",
    "test/parallel/test-net-server-call-listen-multiple-times.js",
    "test/parallel/test-net-server-listen-options.js",
    "test/parallel/test-net-server-listen-options-signal.js",
];

const NODE22_NETWORKING_NET_SOCKET_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-net-after-close.js",
    "test/parallel/test-net-settimeout.js",
    "test/parallel/test-net-can-reset-timeout.js",
    "test/parallel/test-net-socket-close-after-end.js",
    "test/parallel/test-net-socket-connecting.js",
    "test/parallel/test-net-local-address-port.js",
];

const NODE22_NETWORKING_HTTP_REQUEST_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-client-defaults.js",
    "test/parallel/test-http-client-get-url.js",
    "test/parallel/test-http-client-request-options.js",
    "test/parallel/test-http-client-upload.js",
    "test/parallel/test-http-client-upload-buf.js",
    "test/parallel/test-http-automatic-headers.js",
    "test/parallel/test-http-client-close-event.js",
];

const NODE22_NETWORKING_HTTP_TIMEOUT_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-client-timeout-option.js",
    "test/parallel/test-http-client-set-timeout.js",
    "test/parallel/test-http-client-response-timeout.js",
    "test/parallel/test-http-set-timeout.js",
];

const NODE22_NETWORKING_HTTP_RESPONSE_POSITIVE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-contentLength0.js",
    "test/parallel/test-http-head-request.js",
    "test/parallel/test-http-response-writehead-returns-this.js",
];

const NODE22_NETWORKING_HTTP_RESPONSE_STATE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-response-add-header-after-sent.js",
    "test/parallel/test-http-response-remove-header-after-sent.js",
    "test/parallel/test-http-response-no-headers.js",
    "test/parallel/test-http-response-readable.js",
    "test/parallel/test-http-response-setheaders.js",
    "test/parallel/test-http-response-close.js",
    "test/parallel/test-http-response-cork.js",
    "test/parallel/test-http-response-multi-content-length.js",
    "test/parallel/test-http-head-response-has-no-body.js",
    "test/parallel/test-http-head-response-has-no-body-end.js",
    "test/parallel/test-http-head-response-has-no-body-end-implicit-headers.js",
    "test/parallel/test-http-head-throw-on-response-body-write.js",
    "test/parallel/test-http-status-message.js",
    "test/parallel/test-http-write-head-2.js",
];

const NODE22_NETWORKING_HTTP_RESPONSE_STATE_COUNTDOWN_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-write-head-after-set-header.js",
    "test/parallel/test-http-status-code.js",
    "test/parallel/test-http-response-multiheaders.js",
    "test/parallel/test-http-status-reason-invalid-chars.js",
];

const NODE22_NETWORKING_SERVER_NO_ARG_LISTEN_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-server-options-incoming-message.js",
    "test/parallel/test-http-server-options-server-response.js",
    "test/parallel/test-net-server-unref-persistent.js",
];

const NODE22_NETWORKING_HTTP_AGENT_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-agent-keepalive.js",
    "test/parallel/test-http-agent-keepalive-delay.js",
    "test/parallel/test-http-agent-maxsockets.js",
    "test/parallel/test-http-agent-maxsockets-respected.js",
    "test/parallel/test-http-agent-maxtotalsockets.js",
    "test/parallel/test-http-agent-scheduling.js",
    "test/parallel/test-http-agent-timeout.js",
];

const NODE22_NETWORKING_HTTP_AGENT_LIFECYCLE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-agent-false.js",
    "test/parallel/test-http-agent-no-protocol.js",
    "test/parallel/test-http-agent-null.js",
    "test/parallel/test-http-agent-remove.js",
    "test/parallel/test-http-agent-destroyed-socket.js",
    "test/parallel/test-http-agent-error-on-idle.js",
    "test/parallel/test-http-agent-uninitialized.js",
    "test/parallel/test-http-agent-uninitialized-with-handle.js",
    "test/parallel/test-http-agent-abort-controller.js",
];

const NODE22_NETWORKING_DGRAM_HELPER_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-bytes-length.js",
    "test/parallel/test-dgram-createSocket-type.js",
    "test/parallel/test-dgram-send-address-types.js",
    "test/parallel/test-dgram-send-bad-arguments.js",
    "test/parallel/test-dgram-send-invalid-msg-type.js",
    "test/parallel/test-dgram-close-is-not-callback.js",
    "test/parallel/test-dgram-send-empty-array.js",
    "test/parallel/test-dgram-send-empty-buffer.js",
];

const NODE22_NETWORKING_DGRAM_BIND_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-address.js",
    "test/parallel/test-dgram-bind-default-address.js",
    "test/parallel/test-dgram-bind.js",
    "test/parallel/test-dgram-close.js",
    "test/parallel/test-dgram-listen-after-bind.js",
    "test/parallel/test-dgram-ref.js",
    "test/parallel/test-dgram-unref.js",
    "test/parallel/test-dgram-implicit-bind.js",
];

const NODE22_NETWORKING_DGRAM_CONNECT_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-connect.js",
    "test/parallel/test-dgram-connect-send-callback-buffer.js",
    "test/parallel/test-dgram-connect-send-callback-buffer-length.js",
    "test/parallel/test-dgram-connect-send-callback-multi-buffer.js",
    "test/parallel/test-dgram-connect-send-default-host.js",
    "test/parallel/test-dgram-connect-send-empty-array.js",
    "test/parallel/test-dgram-connect-send-empty-buffer.js",
    "test/parallel/test-dgram-connect-send-empty-packet.js",
];

const NODE22_NETWORKING_DGRAM_SEND_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-send-callback-buffer-empty-address.js",
    "test/parallel/test-dgram-send-callback-buffer-length-empty-address.js",
    "test/parallel/test-dgram-send-callback-buffer-length.js",
    "test/parallel/test-dgram-send-callback-buffer.js",
    "test/parallel/test-dgram-send-callback-multi-buffer-empty-address.js",
    "test/parallel/test-dgram-send-callback-multi-buffer.js",
    "test/parallel/test-dgram-send-callback-recursive.js",
    "test/parallel/test-dgram-send-cb-quelches-error.js",
    "test/parallel/test-dgram-send-default-host.js",
    "test/parallel/test-dgram-send-empty-packet.js",
    "test/parallel/test-dgram-send-multi-buffer-copy.js",
    "test/parallel/test-dgram-send-multi-string-array.js",
    "test/parallel/test-dgram-sendto.js",
];

const NODE22_NETWORKING_DGRAM_REMAINING_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-abort-closed.js",
    "test/parallel/test-dgram-bind-error-repeat.js",
    "test/parallel/test-dgram-bind-fd-error.js",
    "test/parallel/test-dgram-bind-fd.js",
    "test/parallel/test-dgram-bind-socket-close-before-lookup.js",
    "test/parallel/test-dgram-blocklist.js",
    "test/parallel/test-dgram-close-during-bind.js",
    "test/parallel/test-dgram-close-in-listening.js",
    "test/parallel/test-dgram-close-signal.js",
    "test/parallel/test-dgram-connect-send-multi-buffer-copy.js",
    "test/parallel/test-dgram-connect-send-multi-string-array.js",
    "test/parallel/test-dgram-create-socket-handle-fd.js",
    "test/parallel/test-dgram-create-socket-handle.js",
    "test/parallel/test-dgram-custom-lookup.js",
    "test/parallel/test-dgram-membership.js",
    "test/parallel/test-dgram-msgsize.js",
    "test/parallel/test-dgram-multicast-loopback.js",
    "test/parallel/test-dgram-multicast-set-interface.js",
    "test/parallel/test-dgram-multicast-setTTL.js",
    "test/parallel/test-dgram-oob-buffer.js",
    "test/parallel/test-dgram-recv-error.js",
    "test/parallel/test-dgram-send-error.js",
    "test/parallel/test-dgram-send-queue-info.js",
    "test/parallel/test-dgram-setBroadcast.js",
    "test/parallel/test-dgram-setTTL.js",
    "test/parallel/test-dgram-socket-buffer-size.js",
    "test/parallel/test-dgram-udp4.js",
];

const NODE22_NETWORKING_DGRAM_LOCAL_PATCH_REGRESSION_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-close-in-listening.js",
    "test/parallel/test-dgram-connect-send-multi-buffer-copy.js",
    "test/parallel/test-dgram-custom-lookup.js",
    "test/parallel/test-dgram-msgsize.js",
    "test/parallel/test-dgram-multicast-loopback.js",
    "test/parallel/test-dgram-send-error.js",
    "test/parallel/test-dgram-setBroadcast.js",
    "test/parallel/test-dgram-udp4.js",
];

const NODE22_NETWORKING_CRYPTO_GATED_HELPER_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-https-agent-constructor.js",
    "test/parallel/test-https-agent-getname.js",
    "test/parallel/test-https-agent.js",
    "test/parallel/test-https-agent-abort-controller.js",
    "test/parallel/test-https-server-options-incoming-message.js",
    "test/parallel/test-https-server-options-server-response.js",
    "test/parallel/test-https-client-get-url.js",
    "test/parallel/test-http2-getpackedsettings.js",
    "test/parallel/test-http2-util-headers-list.js",
    "test/parallel/test-http2-util-update-options-buffer.js",
    "test/parallel/test-http2-misc-util.js",
];

const NODE22_NETWORKING_HTTP2_HEADER_STATUS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-status-code.js"),
    shared_official_batch_case!("test/parallel/test-http2-status-code-invalid.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-multi-content-length.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-response-splitting.js"),
    shared_official_batch_case!("test/parallel/test-http2-options-server-request.js"),
    shared_official_batch_case!("test/parallel/test-http2-options-server-response.js"),
    shared_official_batch_case!("test/parallel/test-http2-zero-length-header.js"),
    shared_official_batch_case!("test/parallel/test-http2-multiheaders.js"),
    shared_official_batch_case!("test/parallel/test-http2-multiheaders-raw.js"),
];

const NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-end.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-write.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-writehead.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-writehead-array.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-statuscode.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-statusmessage.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-statusmessage-property.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-statusmessage-property-set.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-headers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-end.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-headers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-host.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-pause.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-serverrequest-pipe.js",
        COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-trailers.js"),
];

const NODE22_NETWORKING_HTTP2_COMPAT_SERVERRESPONSE_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-close.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-serverresponse-destroy.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-drain.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-end-after-statuses-without-body.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-finished.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-flushheaders.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-headers-after-destroy.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-headers-send-date.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-trailers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-write-early-hints.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-write-head-destroyed.js"),
];

const NODE22_NETWORKING_HTTP2_COMPAT_REMAINDER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-compat-aborted.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-client-upload-reject.js",
        COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-errors.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-continue-check.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-continue.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-handling.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-method-connect.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-createpushresponse.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-short-stream-client-server.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket-destroy-delayed.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket-set.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket.js"),
];

const NODE22_NETWORKING_HTTPS_AGENT_SESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-create-connection.js",
        "node24/test/parallel/test-https-agent-create-connection.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-disable-session-reuse.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-servername.js",
        "node24/test/parallel/test-https-agent-servername.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-session-injection.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-sni.js",
        "node24/test/parallel/test-https-agent-sni.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-sockets-leak.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-client-override-global-agent.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_LOCAL_SERVER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-abortcontroller.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-argument-of-creating.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-byteswritten.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-close.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-max-headers-count.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-request-arguments.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-headers-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-request-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-set-timeout-server.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-simple.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout-server.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout-server-2.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_SERVER_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-all.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-destroy-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-idle.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-socket-options.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-keep-alive-drop-requests.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-connections-checking-leak.js",
        COMMON_TLS_KEY_COUNTDOWN_GC_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_CLIENT_SERVER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-checkServerIdentity.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-reject.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-connecting-to-http.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-drain.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-eof-for-eom.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-host-headers.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-insecure-parse-per-stream.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-max-header-size-per-stream.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-options-boolean-check.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-async-dispose.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-truncate.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_TLS_SESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-selfsigned-no-keycertsign-no-crash.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-resume.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-resume-after-renew.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-https-agent-session-reuse.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-https-agent-session-reuse.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TLS_KEY_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_TLS_LOCAL_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-tls-basic-validations.js"),
    shared_official_batch_case!("test/parallel/test-tls-check-server-identity.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-abort-controller.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-allow-half-open-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-hwm-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-no-host.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-simple.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-tls-connect-timeout-option.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-options-boolean-check.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-server-parent-constructor-options.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_ADDRESS_BOUNDARY_FIXTURES: &[&str] = &[
    "test/parallel/test-https-localaddress-bind-error.js",
    "test/parallel/test-https-connect-address-family.js",
];

const NODE22_NETWORKING_DGRAM_CLUSTER_BOUNDARY_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-bind-socket-close-before-cluster-reply.js",
    "test/parallel/test-dgram-cluster-bind-error.js",
    "test/parallel/test-dgram-cluster-close-during-bind.js",
    "test/parallel/test-dgram-cluster-close-in-listening.js",
    "test/parallel/test-dgram-exclusive-implicit-bind.js",
    "test/parallel/test-dgram-unref-in-cluster.js",
];

const NODE22_NETWORKING_DGRAM_HOST_PRESET_BOUNDARY_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-error-message-address.js",
    "test/parallel/test-dgram-ipv6only.js",
    "test/parallel/test-dgram-udp6-link-local-address.js",
    "test/parallel/test-dgram-udp6-send-default-host.js",
];

