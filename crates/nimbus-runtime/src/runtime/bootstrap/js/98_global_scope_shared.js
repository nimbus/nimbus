import { core } from "ext:core/mod.js";
import * as webSocket from "ext:deno_websocket/01_websocket.js";

const abortSignal = core.loadExtScript("ext:deno_web/03_abort_signal.js");
const console = core.loadExtScript("ext:deno_web/01_console.js");
const crypto = core.loadExtScript("ext:deno_crypto/00_crypto.js");
const encoding = core.loadExtScript("ext:deno_web/08_text_encoding.js");
const event = core.loadExtScript("ext:deno_web/02_event.js");
core.loadExtScript("ext:deno_telemetry/telemetry.ts");
core.loadExtScript("ext:deno_telemetry/util.ts");
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
const { DOMException, QuotaExceededError } = core.loadExtScript(
  "ext:deno_web/01_dom_exception.js",
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
  EventSource: core.propWritable(eventSource.EventSource),
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
  WebSocket: core.propNonEnumerable(webSocket.WebSocket),
  console: core.propNonEnumerable(
    new console.Console((msg, level) => core.print(msg, level > 1)),
  ),
  crypto: core.propReadOnly(crypto.crypto),
  fetch: core.propWritable(fetch.fetch),
  SubtleCrypto: core.propNonEnumerable(crypto.SubtleCrypto),
};

export { windowOrWorkerGlobalScope };
