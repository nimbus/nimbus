import { core } from "ext:core/mod.js";

import * as abortSignal from "ext:deno_web/03_abort_signal.js";
import * as console from "ext:deno_web/01_console.js";
import * as encoding from "ext:deno_web/08_text_encoding.js";
import * as event from "ext:deno_web/02_event.js";
import * as eventSource from "ext:deno_fetch/27_eventsource.js";
import * as fetch from "ext:deno_fetch/26_fetch.js";
import * as file from "ext:deno_web/09_file.js";
import * as fileReader from "ext:deno_web/10_filereader.js";
import * as formData from "ext:deno_fetch/21_formdata.js";
import * as headers from "ext:deno_fetch/20_headers.js";
import * as imageData from "ext:deno_web/16_image_data.js";
import * as request from "ext:deno_fetch/23_request.js";
import * as response from "ext:deno_fetch/23_response.js";
import * as url from "ext:deno_web/00_url.js";
import * as urlPattern from "ext:deno_web/01_urlpattern.js";
import * as webSocket from "ext:deno_websocket/01_websocket.js";
import { DOMException, QuotaExceededError } from "ext:deno_web/01_dom_exception.js";

// Match the Deno runtime module name that Node polyfills import. Keep this
// intentionally smaller than the full Deno runtime global contract, but wide
// enough for Node polyfills to rely on the same shared URL / fetch / DOM
// globals they expect in the Deno family.
const windowOrWorkerGlobalScope = {
  AbortController: core.propNonEnumerable(abortSignal.AbortController),
  AbortSignal: core.propNonEnumerable(abortSignal.AbortSignal),
  Blob: core.propNonEnumerable(file.Blob),
  CloseEvent: core.propNonEnumerable(event.CloseEvent),
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
  TextDecoder: core.propNonEnumerable(encoding.TextDecoder),
  TextEncoder: core.propNonEnumerable(encoding.TextEncoder),
  URL: core.propNonEnumerable(url.URL),
  URLPattern: core.propNonEnumerable(urlPattern.URLPattern),
  URLSearchParams: core.propNonEnumerable(url.URLSearchParams),
  WebSocket: core.propNonEnumerable(webSocket.WebSocket),
  console: core.propNonEnumerable(
    new console.Console((msg, level) => core.print(msg, level > 1)),
  ),
  fetch: core.propWritable(fetch.fetch),
};

export { windowOrWorkerGlobalScope };
