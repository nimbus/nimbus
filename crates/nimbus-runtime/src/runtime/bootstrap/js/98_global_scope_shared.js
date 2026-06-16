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

Object.defineProperty(globalThis, "__nimbusDenoFetchModule", {
  value: fetch,
  configurable: true,
  enumerable: false,
  writable: true,
});

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

function hasNodeExecArgvFlag(flag) {
  const execArgv = globalThis.process?.execArgv;
  return Array.isArray(execArgv) && execArgv.includes(flag);
}

let sessionStorageValue;
function createInMemorySessionStorage() {
  const entries = new Map();
  const storage = {
    get length() {
      return entries.size;
    },
    key(index) {
      return Array.from(entries.keys())[Number(index)] ?? null;
    },
    getItem(key) {
      key = String(key);
      return entries.has(key) ? entries.get(key) : null;
    },
    setItem(key, value) {
      entries.set(String(key), String(value));
    },
    removeItem(key) {
      entries.delete(String(key));
    },
    clear() {
      entries.clear();
    },
  };
  return new Proxy(storage, {
    deleteProperty(_target, key) {
      entries.delete(String(key));
      return true;
    },
    get(target, key, receiver) {
      if (typeof key === "symbol" || key in target) {
        return Reflect.get(target, key, receiver);
      }
      return entries.has(key) ? entries.get(key) : undefined;
    },
    getOwnPropertyDescriptor(target, key) {
      if (typeof key === "symbol" || key in target) {
        return Reflect.getOwnPropertyDescriptor(target, key);
      }
      if (!entries.has(key)) {
        return undefined;
      }
      return {
        configurable: true,
        enumerable: true,
        value: entries.get(key),
        writable: true,
      };
    },
    has(target, key) {
      return key in target || entries.has(String(key));
    },
    ownKeys() {
      return Array.from(entries.keys());
    },
    set(_target, key, value) {
      entries.set(String(key), String(value));
      return true;
    },
  });
}

const eventSourceGlobalDescriptor = Object.getOwnPropertyDescriptor({
  get EventSource() {
    return hasNodeExecArgvFlag("--experimental-eventsource")
      ? eventSource.EventSource
      : undefined;
  },
}, "EventSource");
eventSourceGlobalDescriptor.enumerable = false;

const sessionStorageGlobalDescriptor = Object.getOwnPropertyDescriptor({
  get sessionStorage() {
    return sessionStorageValue ??= createInMemorySessionStorage();
  },
}, "sessionStorage");

const nodeUrlContextLabel = "  Symbol(context): URLContext {";
const node22UrlContextLabel = "  [Symbol(context)]: URLContext {";
const denoPrivateCustomInspectSymbol = Symbol.for("Deno.privateCustomInspect");
const nodeCustomInspectSymbol = Symbol.for("nodejs.util.inspect.custom");

function nodeMajorVersionFromProcess() {
  const version = globalThis.process?.versions?.node;
  if (typeof version !== "string") {
    return 0;
  }
  return Number(version.split(".", 1)[0]) || 0;
}

function normalizeUrlInspectOutput(output) {
  if (typeof output !== "string") {
    return output;
  }
  const major = nodeMajorVersionFromProcess();
  if (major === 22) {
    return output.replace(nodeUrlContextLabel, node22UrlContextLabel);
  }
  if (major >= 24) {
    return output.replace(node22UrlContextLabel, nodeUrlContextLabel);
  }
  return output;
}

function installUrlInspectNormalizer() {
  const prototype = url.URL?.prototype;
  if (prototype === undefined) {
    return;
  }

  const denoInspectDescriptor = Object.getOwnPropertyDescriptor(
    prototype,
    denoPrivateCustomInspectSymbol,
  );
  const denoInspect = denoInspectDescriptor?.value;
  if (typeof denoInspect === "function") {
    const normalizedDenoInspect = {
      [denoPrivateCustomInspectSymbol](inspect, inspectOptions) {
        return normalizeUrlInspectOutput(
          denoInspect.call(this, inspect, inspectOptions),
        );
      },
    }[denoPrivateCustomInspectSymbol];
    Object.defineProperty(prototype, denoPrivateCustomInspectSymbol, {
      ...denoInspectDescriptor,
      value: normalizedDenoInspect,
    });
  }

  const nodeInspectDescriptor = Object.getOwnPropertyDescriptor(
    prototype,
    nodeCustomInspectSymbol,
  );
  const nodeInspect = nodeInspectDescriptor?.value;
  if (typeof nodeInspect === "function") {
    const normalizedNodeInspect = {
      [nodeCustomInspectSymbol](depth, inspectOptions, inspect) {
        return normalizeUrlInspectOutput(
          nodeInspect.call(this, depth, inspectOptions, inspect),
        );
      },
    }[nodeCustomInspectSymbol];
    Object.defineProperty(prototype, nodeCustomInspectSymbol, {
      ...nodeInspectDescriptor,
      value: normalizedNodeInspect,
    });
  }
}

installUrlInspectNormalizer();

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
  EventSource: eventSourceGlobalDescriptor,
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
  sessionStorage: sessionStorageGlobalDescriptor,
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

// Remove the non-standard `Intl.v8BreakIterator` that V8 exposes by default.
// The Deno CLI strips it in `runtime/js/99_main.js`, but Nimbus runs its own
// bootstrap and never loads that file, so the deletion has to happen here to
// match Node (test/parallel/test-intl-v8BreakIterator.js asserts
// `!('v8BreakIterator' in Intl)`). This only scrubs the main realm's `Intl`;
// V8's `Intl` is per-realm, so fresh `vm.createContext` realms re-expose it and
// must be cleaned in the contextify setup path, not here.
if (typeof Intl !== "undefined") {
  delete Intl.v8BreakIterator;
}

export { windowOrWorkerGlobalScope };
