import { core } from "ext:core/mod.js";

core.loadExtScript("ext:deno_telemetry/telemetry.ts");
core.loadExtScript("ext:deno_telemetry/util.ts");

const abortSignal = core.loadExtScript("ext:deno_web/03_abort_signal.js");
const console = core.loadExtScript("ext:deno_web/01_console.js");
const crypto = core.loadExtScript("ext:deno_crypto/00_crypto.js");
const encoding = core.loadExtScript("ext:deno_web/08_text_encoding.js");
const event = core.loadExtScript("ext:deno_web/02_event.js");
const eventSource = core.loadExtScript("ext:deno_fetch/27_eventsource.js");
const fetch = core.loadExtScript("ext:deno_fetch/26_fetch.js");
const file = core.loadExtScript("ext:deno_web/09_file.js");
const fileReader = core.loadExtScript("ext:deno_web/10_filereader.js");
const formData = core.loadExtScript("ext:deno_fetch/21_formdata.js");
const headers = core.loadExtScript("ext:deno_fetch/20_headers.js");
const imageData = core.loadExtScript("ext:deno_web/16_image_data.js");
const request = core.loadExtScript("ext:deno_fetch/23_request.js");
const response = core.loadExtScript("ext:deno_fetch/23_response.js");
const url = core.loadExtScript("ext:deno_web/00_url.js");
const urlPattern = core.loadExtScript("ext:deno_web/01_urlpattern.js");
const loadWebSocket = core.createLazyLoader("ext:deno_websocket/01_websocket.js");
const {
  DOMException,
  QuotaExceededError,
} = core.loadExtScript("ext:deno_web/01_dom_exception.js");

// Register the WebCrypto / Web platform DOMException op-error builders.
//
// deno_core's `to_v8_error` rehydrates a Rust op error tagged with
// `#[class("DOMExceptionOperationError")]` (and siblings) by calling the JS
// builder registered for that class name via `core.registerErrorBuilder`. The
// Deno CLI registers these in `runtime/js/99_main.js`, but Nimbus runs its own
// bootstrap and never loads that file, so without this block every crypto op
// that returns an OperationError (e.g. AES-CBC bad-padding decrypt) surfaces as
// a generic TypeError instead of `DOMException` with the correct `name`. This
// is a Nimbus-local bootstrap responsibility, not a fork change: the builders
// only need `core` plus the `DOMException`/`QuotaExceededError` constructors
// already in lexical scope here. Mirror the full set Deno registers.
core.registerErrorBuilder(
  "DOMExceptionOperationError",
  function DOMExceptionOperationError(msg) {
    return new DOMException(msg, "OperationError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionQuotaExceededError",
  function DOMExceptionQuotaExceededError(msg) {
    return new QuotaExceededError(msg);
  },
);
core.registerErrorBuilder(
  "DOMExceptionNotSupportedError",
  function DOMExceptionNotSupportedError(msg) {
    return new DOMException(msg, "NotSupported");
  },
);
core.registerErrorBuilder(
  "DOMExceptionNetworkError",
  function DOMExceptionNetworkError(msg) {
    return new DOMException(msg, "NetworkError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionAbortError",
  function DOMExceptionAbortError(msg) {
    return new DOMException(msg, "AbortError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionInvalidCharacterError",
  function DOMExceptionInvalidCharacterError(msg) {
    return new DOMException(msg, "InvalidCharacterError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionDataError",
  function DOMExceptionDataError(msg) {
    return new DOMException(msg, "DataError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionInvalidStateError",
  function DOMExceptionInvalidStateError(msg) {
    return new DOMException(msg, "InvalidStateError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionSyntaxError",
  function DOMExceptionSyntaxError(msg) {
    return new DOMException(msg, "SyntaxError");
  },
);
core.registerErrorBuilder(
  "DOMExceptionIndexSizeError",
  function DOMExceptionIndexSizeError(msg) {
    return new DOMException(msg, "IndexSizeError");
  },
);

// Match the Deno runtime module name that Node polyfills import. Keep this
// intentionally smaller than the full Deno runtime global contract, but wide
// enough for Node polyfills to rely on the same shared URL / fetch / DOM
// globals they expect in the Deno family.
const windowOrWorkerGlobalScope = {
  AbortController: core.propNonEnumerable(abortSignal.AbortController),
  AbortSignal: core.propNonEnumerable(abortSignal.AbortSignal),
  Blob: core.propNonEnumerable(file.Blob),
  CloseEvent: core.propNonEnumerable(event.CloseEvent),
  Crypto: core.propNonEnumerable(crypto.Crypto),
  CryptoKey: core.propNonEnumerable(crypto.CryptoKey),
  CustomEvent: core.propNonEnumerable(event.CustomEvent),
  DOMException: core.propNonEnumerable(DOMException),
  QuotaExceededError: core.propNonEnumerable(QuotaExceededError),
  ErrorEvent: core.propNonEnumerable(event.ErrorEvent),
  Event: core.propNonEnumerable(event.Event),
  EventTarget: core.propNonEnumerable(event.EventTarget),
  // Node 22/24 gate the EventSource global behind --experimental-eventsource, so
  // it is absent by default (test/parallel/test-eventsource-disabled.js asserts
  // `typeof EventSource === 'undefined'`). The ext script stays imported for
  // opt-in callers; only the default global registration is withheld.
  File: core.propNonEnumerable(file.File),
  FileReader: core.propNonEnumerable(fileReader.FileReader),
  FormData: core.propNonEnumerable(formData.FormData),
  Headers: core.propNonEnumerable(headers.Headers),
  ImageData: core.propNonEnumerable(imageData.ImageData),
  MessageEvent: core.propNonEnumerable(event.MessageEvent),
  ProgressEvent: core.propNonEnumerable(event.ProgressEvent),
  Request: core.propNonEnumerable(request.Request),
  Response: core.propNonEnumerable(response.Response),
  reportError: core.propWritable(event.reportError),
  TextDecoder: core.propNonEnumerable(encoding.TextDecoder),
  TextEncoder: core.propNonEnumerable(encoding.TextEncoder),
  URL: core.propNonEnumerable(url.URL),
  URLPattern: core.propNonEnumerable(urlPattern.URLPattern),
  URLSearchParams: core.propNonEnumerable(url.URLSearchParams),
  WebSocket: core.propNonEnumerableLazyLoaded(
    (webSocket) => webSocket.WebSocket,
    loadWebSocket,
  ),
  console: core.propNonEnumerable(
    new console.Console((msg, level) => core.print(msg, level > 1)),
  ),
  crypto: core.propReadOnly(crypto.crypto),
  fetch: core.propWritable(fetch.fetch),
  SubtleCrypto: core.propNonEnumerable(crypto.SubtleCrypto),
};

export { windowOrWorkerGlobalScope };
