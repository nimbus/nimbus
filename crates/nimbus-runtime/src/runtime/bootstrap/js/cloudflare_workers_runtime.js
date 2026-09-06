const __nimbusCloudflareWorkerUnsupported = function __nimbusCloudflareWorkerUnsupported(surface) {
  throw new Error(`Cloudflare Workers API ${surface} is not supported by Nimbus CFA4`);
};

const __nimbusCloudflareWorkerInstallUnsupportedGlobals = function __nimbusCloudflareWorkerInstallUnsupportedGlobals() {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "caches");
  if (descriptor && descriptor.configurable === false) {
    return;
  }
  Object.defineProperty(globalThis, "caches", {
    get() {
      return {
        get default() {
          return __nimbusCloudflareWorkerUnsupported("caches.default");
        },
      };
    },
    configurable: true,
    enumerable: false,
  });
};

const __nimbusCloudflareWorkerBytesToBase64 = function __nimbusCloudflareWorkerBytesToBase64(bytes) {
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const chunk = bytes.subarray(offset, offset + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
};

const __nimbusCloudflareWorkerBase64ToBytes = function __nimbusCloudflareWorkerBase64ToBytes(value) {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
};

const __nimbusCloudflareWorkerBodyValueToBase64 = async function __nimbusCloudflareWorkerBodyValueToBase64(value) {
  if (typeof value === "string") {
    return __nimbusCloudflareWorkerBytesToBase64(new TextEncoder().encode(value));
  }
  if (value instanceof ArrayBuffer) {
    return __nimbusCloudflareWorkerBytesToBase64(new Uint8Array(value));
  }
  if (ArrayBuffer.isView(value)) {
    return __nimbusCloudflareWorkerBytesToBase64(
      new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
    );
  }
  if (typeof Blob === "function" && value instanceof Blob) {
    return __nimbusCloudflareWorkerBytesToBase64(new Uint8Array(await value.arrayBuffer()));
  }
  throw new TypeError("Cloudflare Workers KV put value must be a string, ArrayBuffer, ArrayBufferView, or Blob");
};

const __nimbusCloudflareWorkerRequestBody = function __nimbusCloudflareWorkerRequestBody(rawRequest) {
  if (!rawRequest || typeof rawRequest !== "object" || rawRequest.body === undefined || rawRequest.body === null) {
    return undefined;
  }
  const body = rawRequest.body;
  if (typeof body === "string") {
    return body;
  }
  if (body && typeof body === "object" && typeof body.text === "string") {
    return body.text;
  }
  if (body && typeof body === "object" && typeof body.base64 === "string") {
    return __nimbusCloudflareWorkerBase64ToBytes(body.base64);
  }
  throw new TypeError("Cloudflare Worker fetch request body must be text or base64");
};

const __nimbusCloudflareWorkerHeaders = function __nimbusCloudflareWorkerHeaders(rawHeaders) {
  const headers = new Headers();
  if (Array.isArray(rawHeaders)) {
    for (const pair of rawHeaders) {
      if (!Array.isArray(pair) || pair.length !== 2) {
        throw new TypeError("Cloudflare Worker fetch headers array entries must be [name, value]");
      }
      headers.append(String(pair[0]), String(pair[1]));
    }
    return headers;
  }
  if (rawHeaders && typeof rawHeaders === "object") {
    for (const [name, value] of Object.entries(rawHeaders)) {
      headers.append(name, String(value));
    }
  }
  return headers;
};

const __nimbusCloudflareWorkerCreateRequest = function __nimbusCloudflareWorkerCreateRequest(rawRequest) {
  const request = rawRequest && typeof rawRequest === "object" ? rawRequest : {};
  const url = typeof request.url === "string" && request.url.length > 0
    ? request.url
    : "https://nimbus.local/";
  const method = typeof request.method === "string" && request.method.length > 0
    ? request.method.toUpperCase()
    : "GET";
  const init = {
    method,
    headers: __nimbusCloudflareWorkerHeaders(request.headers),
  };
  const body = __nimbusCloudflareWorkerRequestBody(request);
  if (body !== undefined && method !== "GET" && method !== "HEAD") {
    init.body = body;
  }
  const workerRequest = new Request(url, init);
  Object.defineProperty(workerRequest, "cf", {
    get() {
      return __nimbusCloudflareWorkerUnsupported("request.cf");
    },
    configurable: true,
    enumerable: false,
  });
  return workerRequest;
};

const __nimbusCloudflareWorkerKvValueType = function __nimbusCloudflareWorkerKvValueType(options) {
  if (typeof options === "string") {
    return options;
  }
  if (options && typeof options === "object" && typeof options.type === "string") {
    return options.type;
  }
  return "text";
};

const __nimbusCloudflareWorkerDecodeKvValue = function __nimbusCloudflareWorkerDecodeKvValue(hostResult, valueType) {
  if (valueType === "stream") {
    return __nimbusCloudflareWorkerUnsupported("KVNamespace.get stream type");
  }
  if (hostResult === null || hostResult === undefined) {
    return { value: null, metadata: null };
  }
  const metadata = hostResult.metadata === undefined ? null : hostResult.metadata;
  if (hostResult.value_base64 === undefined && hostResult.valueBase64 === undefined) {
    return {
      value: Object.prototype.hasOwnProperty.call(hostResult, "value") ? hostResult.value : null,
      metadata,
    };
  }
  const valueBase64 = hostResult.value_base64 ?? hostResult.valueBase64;
  if (valueBase64 === null || valueBase64 === undefined) {
    return { value: null, metadata };
  }
  const bytes = __nimbusCloudflareWorkerBase64ToBytes(valueBase64);
  if (valueType === "arrayBuffer") {
    return { value: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength), metadata };
  }
  const text = new TextDecoder().decode(bytes);
  if (valueType === "json") {
    return { value: JSON.parse(text), metadata };
  }
  return { value: text, metadata };
};

const __nimbusCloudflareWorkerKvPutOptions = function __nimbusCloudflareWorkerKvPutOptions(options) {
  const input = options && typeof options === "object" ? options : {};
  const expirationTtl = input.expiration_ttl ?? input.expirationTtl ?? null;
  return {
    metadata: input.metadata ?? null,
    expiration: input.expiration ?? null,
    expiration_ttl: expirationTtl,
  };
};

const __nimbusCloudflareWorkerCreateKvNamespace = function __nimbusCloudflareWorkerCreateKvNamespace(bindingName, binding) {
  const tenantId = binding.tenant_id ?? binding.tenantId;
  const namespace = binding.namespace ?? binding.namespace_id ?? binding.namespaceId ?? binding.id ?? bindingName;
  if (typeof tenantId !== "string" || tenantId.length === 0) {
    throw new Error(`Cloudflare Workers KV binding ${bindingName} is missing tenant_id`);
  }
  if (typeof namespace !== "string" || namespace.length === 0) {
    throw new Error(`Cloudflare Workers KV binding ${bindingName} is missing namespace`);
  }
  const basePayload = (key) => ({
    tenant_id: tenantId,
    namespace,
    key: String(key),
  });
  return Object.freeze({
    async get(key, options) {
      const valueType = __nimbusCloudflareWorkerKvValueType(options);
      const decoded = __nimbusCloudflareWorkerDecodeKvValue(
        await globalThis.__nimbusAsyncHostValue("op_nimbus_cf_kv_get", {
          ...basePayload(key),
          value_type: valueType,
        }),
        valueType,
      );
      return decoded.value;
    },
    async getWithMetadata(key, options) {
      const valueType = __nimbusCloudflareWorkerKvValueType(options);
      const decoded = __nimbusCloudflareWorkerDecodeKvValue(
        await globalThis.__nimbusAsyncHostValue("op_nimbus_cf_kv_get", {
          ...basePayload(key),
          value_type: valueType,
        }),
        valueType,
      );
      return { value: decoded.value, metadata: decoded.metadata };
    },
    async put(key, value, options) {
      const normalizedOptions = __nimbusCloudflareWorkerKvPutOptions(options);
      await globalThis.__nimbusAsyncHostValue("op_nimbus_cf_kv_put", {
        ...basePayload(key),
        value_base64: await __nimbusCloudflareWorkerBodyValueToBase64(value),
        metadata: normalizedOptions.metadata,
        expiration: normalizedOptions.expiration,
        expiration_ttl: normalizedOptions.expiration_ttl,
      });
    },
    async delete(key) {
      await globalThis.__nimbusAsyncHostValue("op_nimbus_cf_kv_delete", basePayload(key));
    },
    async list(options) {
      const input = options && typeof options === "object" ? options : {};
      return await globalThis.__nimbusAsyncHostValue("op_nimbus_cf_kv_list", {
        tenant_id: tenantId,
        namespace,
        prefix: input.prefix ?? null,
        cursor: input.cursor ?? null,
        limit: input.limit ?? null,
      });
    },
  });
};

const __nimbusCloudflareWorkerCreateEnv = function __nimbusCloudflareWorkerCreateEnv(rawEnv) {
  const env = Object.create(null);
  const bindings = rawEnv && typeof rawEnv === "object" ? rawEnv : {};
  for (const [name, binding] of Object.entries(bindings)) {
    if (binding && typeof binding === "object" && binding.type === "kv_namespace") {
      env[name] = __nimbusCloudflareWorkerCreateKvNamespace(name, binding);
    } else if (binding && typeof binding === "object" && Object.prototype.hasOwnProperty.call(binding, "value")) {
      env[name] = binding.value;
    } else {
      env[name] = binding;
    }
  }
  return Object.freeze(env);
};

const __nimbusCloudflareWorkerCreateCtx = function __nimbusCloudflareWorkerCreateCtx() {
  let passThroughOnException = false;
  return Object.freeze({
    waitUntil(promise) {
      return globalThis.__nimbusWaitUntil(Promise.resolve(promise));
    },
    passThroughOnException() {
      passThroughOnException = true;
    },
    get passThroughOnExceptionCalled() {
      return passThroughOnException;
    },
  });
};

const __nimbusCloudflareWorkerSerializeResponse = async function __nimbusCloudflareWorkerSerializeResponse(response, ctx) {
  if (!(response instanceof Response)) {
    throw new TypeError("Cloudflare Worker fetch must return a Response");
  }
  return {
    status: response.status,
    statusText: response.statusText,
    headers: Array.from(response.headers.entries()),
    body: await response.text(),
    passThroughOnException: ctx.passThroughOnExceptionCalled,
  };
};

// HG0/HG5 (Band B-FIX, CAPTURE-ORDERING): this bootstrap runs before the guest
// worker module ever loads (bootstrap installation precedes
// main-module loading in driver/loading.rs), so this is the FIRST
// and only writer of this slot. Object.defineProperty with
// configurable:false, writable:false closes the window a plain assignment
// left open: once the guest worker module evaluates (and its microtasks
// drain) ahead of the host's post-drain capture in captured_dispatch.rs, a
// top-level or queueMicrotask-queued `globalThis.__nimbusInvokeCloudflareWorkerFetch
// = impostor` now throws instead of being captured as the impostor.
// Object.freeze on the function value itself (not just the slot) matches the
// HG9 lesson: slot-hardening alone does not protect a mutable value's own
// properties from a guest that reaches the function object through some
// other path.
Object.defineProperty(globalThis, "__nimbusInvokeCloudflareWorkerFetch", {
  value: Object.freeze(async function(moduleNamespacePromise, request) {
    __nimbusCloudflareWorkerInstallUnsupportedGlobals();
    const moduleNamespace = await moduleNamespacePromise;
    const workerEntrypoint = moduleNamespace && moduleNamespace.default;
    if (!workerEntrypoint || typeof workerEntrypoint.fetch !== "function") {
      throw new TypeError("CloudflareWorker default export must provide fetch(request, env, ctx)");
    }
    const args = request && typeof request === "object" && request.args && typeof request.args === "object"
      ? request.args
      : {};
    const workerRequest = __nimbusCloudflareWorkerCreateRequest(args.request);
    const env = __nimbusCloudflareWorkerCreateEnv(args.env ?? args.bindings);
    const ctx = __nimbusCloudflareWorkerCreateCtx();
    const response = await workerEntrypoint.fetch(workerRequest, env, ctx);
    return await __nimbusCloudflareWorkerSerializeResponse(response, ctx);
  }),
  configurable: false,
  enumerable: false,
  writable: false,
});
