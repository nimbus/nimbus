function cloudFunctionsEntrySource(project) {
  const imports = [];
  const modules = [];
  if (project.kind === "firebase_project") {
    for (const [index, codebase] of project.codebases.entries()) {
      const localName = `__neovexCloudFunctionsModule${index}`;
      const relativeEntrypoint = ensureRelativeImport(codebase.relativeEntrypoint);
      imports.push(`import * as ${localName} from ${JSON.stringify(relativeEntrypoint)};`);
      modules.push(`{
  codebase: ${JSON.stringify(codebase.name)},
  exports: ${localName},
}`);
    }
  } else {
    const relativeEntrypoint = ensureRelativeImport(project.relativeEntrypoint);
    imports.push(`import ${JSON.stringify(relativeEntrypoint)};`);
  }

  return `
import {
  collectExportedTargets,
  collectRegisteredFrameworkTargets,
  createInvocationDispatcher,
} from "__neovex_cloud_functions_shared__";

${imports.join("\n")}

const __neovexCollectedTargets = [
  ...collectExportedTargets([
${modules.join(",\n")}
  ]),
  ...collectRegisteredFrameworkTargets(),
];
export const __neovexTargets = __neovexCollectedTargets.map((target) => target.target);
globalThis.__neovexInvoke = createInvocationDispatcher(__neovexCollectedTargets);

export {};
`;
}

function cloudFunctionsSharedSource() {
  return `
const TARGET_MARKER = "__neovexCloudFunctionTarget";
const SUPPORTED_GLOBAL_OPTIONS = new Set(["retry"]);
const SUPPORTED_DOCUMENT_OPTIONS = new Set(["document", "database", "retry"]);
const SUPPORTED_HTTPS_OPTIONS = new Set([]);
const SUPPORTED_CALLABLE_OPTIONS = new Set([]);
const CALLABLE_ERROR_CODE_MAP = Object.freeze({
  "cancelled": { status: "CANCELLED", httpStatus: 499 },
  "unknown": { status: "UNKNOWN", httpStatus: 500 },
  "invalid-argument": { status: "INVALID_ARGUMENT", httpStatus: 400 },
  "deadline-exceeded": { status: "DEADLINE_EXCEEDED", httpStatus: 504 },
  "not-found": { status: "NOT_FOUND", httpStatus: 404 },
  "already-exists": { status: "ALREADY_EXISTS", httpStatus: 409 },
  "permission-denied": { status: "PERMISSION_DENIED", httpStatus: 403 },
  "resource-exhausted": { status: "RESOURCE_EXHAUSTED", httpStatus: 429 },
  "failed-precondition": { status: "FAILED_PRECONDITION", httpStatus: 400 },
  "aborted": { status: "ABORTED", httpStatus: 409 },
  "out-of-range": { status: "OUT_OF_RANGE", httpStatus: 400 },
  "unimplemented": { status: "UNIMPLEMENTED", httpStatus: 501 },
  "internal": { status: "INTERNAL", httpStatus: 500 },
  "unavailable": { status: "UNAVAILABLE", httpStatus: 503 },
  "data-loss": { status: "DATA_LOSS", httpStatus: 500 },
  "unauthenticated": { status: "UNAUTHENTICATED", httpStatus: 401 },
});

function invalidInput(message) {
  throw new Error(message);
}

function unsupportedPhaseSurface(surface) {
  throw new Error(\`\${surface} is deferred to a later Cloud Functions phase.\`);
}

function validatePlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    invalidInput(\`\${label} must be an object.\`);
  }
}

const globalState = globalThis.__neovexCloudFunctionsState ??= {
  globalDefaults: {},
  frameworkTargets: [],
};

function normalizeRetry(value, label) {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value !== "boolean") {
    invalidInput(\`\${label} must be a boolean when provided.\`);
  }
  return value;
}

function setGlobalOptions(options = {}) {
  validatePlainObject(options, "firebase-functions/v2 setGlobalOptions()");
  for (const key of Object.keys(options)) {
    if (!SUPPORTED_GLOBAL_OPTIONS.has(key)) {
      invalidInput(
        \`global option "\${key}" is not covered for firestore_document in the first cloud functions slice\`,
      );
    }
  }
  globalState.globalDefaults = {
    retry: normalizeRetry(options.retry, "firebase-functions/v2 setGlobalOptions().retry"),
  };
}

function onInit() {
  invalidInput(
    "firebase-functions/v2 root API \\"onInit()\\" is deferred; first slice only covers \\"setGlobalOptions()\\" plus namespace imports",
  );
}

function normalizeDocumentOptions(documentOrOptions) {
  if (typeof documentOrOptions === "string") {
    if (documentOrOptions.trim().length === 0) {
      invalidInput("Firestore document triggers require a non-empty document path.");
    }
    return {
      document: documentOrOptions.trim(),
      database: "(default)",
      retry: globalState.globalDefaults.retry,
    };
  }

  validatePlainObject(documentOrOptions, "Firestore document trigger options");
  for (const key of Object.keys(documentOrOptions)) {
    if (!SUPPORTED_DOCUMENT_OPTIONS.has(key)) {
      invalidInput(
        \`Firestore document option "\${key}" is not covered in the first cloud functions slice\`,
      );
    }
  }
  const document = typeof documentOrOptions.document === "string"
    ? documentOrOptions.document.trim()
    : "";
  if (document.length === 0) {
    invalidInput("Firestore document triggers require a non-empty document option.");
  }
  const database = documentOrOptions.database === undefined
    ? "(default)"
    : String(documentOrOptions.database).trim();
  if (database.length === 0) {
    invalidInput("Firestore document triggers require a non-empty database id.");
  }
  return {
    document,
    database,
    retry: normalizeRetry(
      documentOrOptions.retry === undefined
        ? globalState.globalDefaults.retry
        : documentOrOptions.retry,
      "Firestore document trigger retry",
    ),
  };
}

function defineFirestoreDocumentTarget(eventType, documentOrOptions, handler) {
  if (typeof handler !== "function") {
    invalidInput("Firestore document triggers require a handler function.");
  }
  const options = normalizeDocumentOptions(documentOrOptions);
  return Object.freeze({
    [TARGET_MARKER]: true,
    target: {
      authoring_surface: "firebase_v2",
      signature_type: "cloud_event",
      binding: {
        binding_kind: "firestore_document",
        event_type: eventType,
        database: options.database,
        document: options.document,
        execution: "service",
      },
    },
    invoke: async (args, request) => handler(materializeFirebaseFirestoreEvent(args), request),
    defaults: {
      retry: options.retry,
    },
  });
}

function normalizeFirebaseHandlerRegistration(
  surface,
  optionLabel,
  supportedOptions,
  optionsOrHandler,
  maybeHandler,
) {
  let handler = maybeHandler;
  let options = {};
  if (typeof optionsOrHandler === "function") {
    if (maybeHandler !== undefined) {
      invalidInput(\`\${surface} does not accept a second argument when called as \${surface}(handler).\`);
    }
    handler = optionsOrHandler;
  } else if (optionsOrHandler === undefined) {
    invalidInput(\`\${surface} requires a handler function.\`);
  } else {
    validatePlainObject(optionsOrHandler, \`\${surface} options\`);
    options = optionsOrHandler;
  }
  if (typeof handler !== "function") {
    invalidInput(\`\${surface} requires a handler function.\`);
  }
  for (const key of Object.keys(options)) {
    if (!supportedOptions.has(key)) {
      invalidInput(
        \`\${optionLabel} "\${key}" is not covered in the first cloud functions slice\`,
      );
    }
  }
  return { handler };
}

function defineFirebaseHttpsRequestTarget(optionsOrHandler, maybeHandler) {
  const { handler } = normalizeFirebaseHandlerRegistration(
    "firebase-functions/v2/https onRequest()",
    "HTTPS option",
    SUPPORTED_HTTPS_OPTIONS,
    optionsOrHandler,
    maybeHandler,
  );
  return Object.freeze({
    [TARGET_MARKER]: true,
    target: {
      authoring_surface: "firebase_v2",
      signature_type: "http",
      binding: {
        binding_kind: "https",
        exposure: "http",
        execution: "request",
      },
    },
    invoke: async (args) => {
      const { req, res, state } = createFrameworkHttpSession(args);
      const result = await handler(req, res);
      return finalizeFrameworkHttpResponse(state, result);
    },
  });
}

function defineFirebaseHttpsCallableTarget(optionsOrHandler, maybeHandler) {
  const { handler } = normalizeFirebaseHandlerRegistration(
    "firebase-functions/v2/https onCall()",
    "Callable option",
    SUPPORTED_CALLABLE_OPTIONS,
    optionsOrHandler,
    maybeHandler,
  );
  return Object.freeze({
    [TARGET_MARKER]: true,
    target: {
      authoring_surface: "firebase_v2",
      signature_type: "http",
      binding: {
        binding_kind: "https",
        exposure: "callable",
        execution: "request",
      },
    },
    invoke: async (args) => {
      const { request, response } = createFirebaseCallableSession(args);
      try {
        const result = await handler(request, response);
        return finalizeFirebaseCallableResponse(result);
      } catch (error) {
        return finalizeFirebaseCallableError(normalizeCallableError(error));
      }
    },
  });
}

class HttpsError extends Error {
  constructor(code, message, details) {
    const metadata = resolveCallableErrorCodeMetadata(code);
    super(message);
    this.code = code;
    this.details = details;
    this.httpErrorCode = Object.freeze({
      status: metadata.status,
      canonicalName: metadata.status,
    });
    Object.setPrototypeOf(this, HttpsError.prototype);
  }

  toJSON() {
    const encoded = {
      status: this.httpErrorCode.status,
      message: this.message,
    };
    if (this.details !== undefined) {
      encoded.details = cloneValue(this.details);
    }
    return encoded;
  }
}

function resolveCallableErrorCodeMetadata(code) {
  if (typeof code !== "string" || !(code in CALLABLE_ERROR_CODE_MAP)) {
    invalidInput(
      \`HttpsError requires a supported FunctionsErrorCode, received \${JSON.stringify(code)}.\`,
    );
  }
  return CALLABLE_ERROR_CODE_MAP[code];
}

function normalizeCallableError(error) {
  if (error instanceof HttpsError) {
    return error;
  }
  if (error instanceof Error && typeof error.message === "string" && error.message.length > 0) {
    return new HttpsError("internal", error.message);
  }
  return new HttpsError("internal", "INTERNAL");
}

function createFirebaseCallableSession(rawRequest) {
  validatePlainObject(rawRequest, "Firebase callable request");
  const { req } = createFrameworkHttpSession(rawRequest);
  const callable = rawRequest.callable ?? {};
  validatePlainObject(callable, "Firebase callable request envelope");
  const request = Object.freeze({
    acceptsStreaming: false,
    app: materializeFirebaseCallableApp(callable.app),
    auth: materializeFirebaseCallableAuth(callable.auth),
    data: cloneValue(callable.data),
    instanceIdToken: callable.instance_id_token === undefined
      ? undefined
      : String(callable.instance_id_token),
    rawRequest: req,
  });
  const response = Object.freeze({
    sendChunk() {
      unsupportedPhaseSurface("firebase-functions/v2/https callable streaming responses");
    },
    signal: undefined,
  });
  return { request, response };
}

function materializeFirebaseCallableApp(rawApp) {
  if (rawApp === undefined || rawApp === null) {
    return undefined;
  }
  validatePlainObject(rawApp, "Firebase callable App Check context");
  return Object.freeze(cloneValue(rawApp));
}

function materializeFirebaseCallableAuth(rawAuth) {
  if (rawAuth === undefined || rawAuth === null) {
    return undefined;
  }
  validatePlainObject(rawAuth, "Firebase callable auth context");
  const token = rawAuth.token ?? {};
  validatePlainObject(token, "Firebase callable auth token");
  return Object.freeze({
    uid: rawAuth.uid === undefined || rawAuth.uid === null ? null : String(rawAuth.uid),
    token: cloneValue(token),
  });
}

function finalizeFirebaseCallableResponse(value) {
  return {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
    body_kind: "json",
    body: {
      data: value === undefined ? null : cloneValue(value),
    },
  };
}

function finalizeFirebaseCallableError(error) {
  const metadata = resolveCallableErrorCodeMetadata(error.code);
  return {
    status: metadata.httpStatus,
    headers: {
      "content-type": "application/json",
    },
    body_kind: "json",
    body: {
      error: error.toJSON(),
    },
  };
}

function materializeFirebaseFirestoreEvent(rawEvent) {
  validatePlainObject(rawEvent, "Firestore trigger event");
  const firestore = rawEvent.firestore ?? {};
  const cloudEvent = rawEvent.cloud_event ?? {};
  return Object.freeze({
    data: materializeFirebaseFirestoreEventData(rawEvent),
    id: String(cloudEvent.id ?? ""),
    source: String(cloudEvent.source ?? ""),
    specversion: String(cloudEvent.specversion ?? "1.0"),
    subject: cloudEvent.subject === undefined ? undefined : String(cloudEvent.subject),
    time: timestampToIsoString(cloudEvent.time),
    type: String(cloudEvent.type ?? ""),
    params: Object.freeze({ ...(firestore.params ?? {}) }),
    project: String(firestore.project_id ?? ""),
    database: String(firestore.database_id ?? ""),
    document: documentPathToString(firestore.document_path),
    location: "",
    namespace: "(default)",
  });
}

function materializeFirebaseFirestoreEventData(rawEvent) {
  const eventType = String(rawEvent.cloud_event?.type ?? "");
  if (eventType === "google.cloud.firestore.document.v1.created") {
    return materializeQueryDocumentSnapshot(rawEvent.data?.value);
  }
  if (eventType === "google.cloud.firestore.document.v1.deleted") {
    return materializeQueryDocumentSnapshot(rawEvent.data?.oldValue);
  }
  if (eventType === "google.cloud.firestore.document.v1.updated") {
    return Object.freeze({
      before: materializeQueryDocumentSnapshot(rawEvent.data?.oldValue),
      after: materializeQueryDocumentSnapshot(rawEvent.data?.value),
    });
  }
  if (eventType === "google.cloud.firestore.document.v1.written") {
    return Object.freeze({
      before: materializeDocumentSnapshot(rawEvent.data?.oldValue),
      after: materializeDocumentSnapshot(rawEvent.data?.value),
    });
  }
  return rawEvent.data;
}

class FirestoreDocumentSnapshot {
  constructor(documentEventDocument) {
    this._documentEventDocument = documentEventDocument ?? null;
    this.exists = this._documentEventDocument !== null;
    this.ref = Object.freeze({
      path: documentPathToString(documentEventDocument?.path),
      id: documentPathLeafId(documentEventDocument?.path),
    });
    this.id = this.ref.id;
    this.metadata = Object.freeze({
      fromCache: false,
      hasPendingWrites: false,
    });
  }

  data() {
    if (!this.exists) {
      return undefined;
    }
    return cloneValue(this._documentEventDocument.document?.fields ?? {});
  }

  get(fieldPath) {
    if (typeof fieldPath !== "string" || fieldPath.trim().length === 0) {
      return undefined;
    }
    return readFieldPath(this.data(), fieldPath);
  }
}

class FirestoreQueryDocumentSnapshot extends FirestoreDocumentSnapshot {
  data() {
    const value = super.data();
    if (value === undefined) {
      throw new Error("QueryDocumentSnapshot data must be present.");
    }
    return value;
  }
}

function materializeDocumentSnapshot(documentEventDocument) {
  return new FirestoreDocumentSnapshot(documentEventDocument ?? null);
}

function materializeQueryDocumentSnapshot(documentEventDocument) {
  if (!documentEventDocument) {
    return undefined;
  }
  return new FirestoreQueryDocumentSnapshot(documentEventDocument);
}

function readFieldPath(value, fieldPath) {
  if (value === undefined) {
    return undefined;
  }
  return fieldPath
    .split(".")
    .reduce(
      (current, segment) => (
        current !== null
          && typeof current === "object"
          && !Array.isArray(current)
          && segment in current
      )
        ? current[segment]
        : undefined,
      value,
    );
}

function documentPathLeafId(documentPath) {
  const segments = documentPathSegments(documentPath);
  return segments.length === 0 ? "" : segments[segments.length - 1];
}

function documentPathToString(documentPath) {
  return documentPathSegments(documentPath).join("/");
}

function documentPathSegments(documentPath) {
  if (!documentPath || typeof documentPath !== "object") {
    return [];
  }
  const collectionPath = documentPath.collection_path;
  const segments = [];
  if (collectionPath && typeof collectionPath === "object") {
    if (collectionPath.root !== undefined) {
      segments.push(String(collectionPath.root));
    }
    for (const descendant of collectionPath.descendants ?? []) {
      if (!descendant || typeof descendant !== "object") {
        continue;
      }
      if (descendant.document_id !== undefined) {
        segments.push(String(descendant.document_id));
      }
      if (descendant.collection !== undefined) {
        segments.push(String(descendant.collection));
      }
    }
  }
  if (documentPath.document_id !== undefined) {
    segments.push(String(documentPath.document_id));
  }
  return segments;
}

function timestampToIsoString(value) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return new Date(value).toISOString();
  }
  if (typeof value === "string" && value.trim().length > 0) {
    return value;
  }
  return new Date(0).toISOString();
}

function cloneValue(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

function registerFrameworkTarget(signatureType, name, handler) {
  if (typeof name !== "string" || name.trim().length === 0) {
    invalidInput("@google-cloud/functions-framework targets require a non-empty name.");
  }
  if (typeof handler !== "function") {
    invalidInput(
      \`@google-cloud/functions-framework target "\${name}" requires a handler function.\`,
    );
  }
  const normalizedName = name.trim();
  if (globalState.frameworkTargets.some((target) => target.target.name === normalizedName)) {
    invalidInput(
      \`@google-cloud/functions-framework target "\${normalizedName}" is declared more than once.\`,
    );
  }
  const entrypoint = \`registry.\${normalizedName}\`;
  globalState.frameworkTargets.push({
    entrypoint,
    invoke: async (args, request) => {
      if (signatureType === "cloud_event") {
        return handler(materializeFrameworkCloudEvent(args), request);
      }
      const { req, res, state } = createFrameworkHttpSession(args);
      const result = await handler(req, res);
      return finalizeFrameworkHttpResponse(state, result);
    },
    target: {
      name: normalizedName,
      entrypoint,
      authoring_surface: "functions_framework",
      signature_type: signatureType,
    },
  });
}

function materializeFrameworkCloudEvent(rawEvent) {
  validatePlainObject(rawEvent, "Functions Framework CloudEvent");
  const cloudEvent = rawEvent.cloud_event ?? {};
  return Object.freeze({
    id: String(cloudEvent.id ?? ""),
    source: String(cloudEvent.source ?? ""),
    specversion: String(cloudEvent.specversion ?? "1.0"),
    subject: cloudEvent.subject === undefined ? undefined : String(cloudEvent.subject),
    time: timestampToIsoString(cloudEvent.time),
    type: String(cloudEvent.type ?? ""),
    data: cloneValue(rawEvent.data),
  });
}

function createFrameworkHttpSession(rawRequest) {
  validatePlainObject(rawRequest, "Functions Framework HTTP request");
  const headers = normalizeFrameworkHeaders(rawRequest.headers ?? {});
  const state = {
    statusCode: 200,
    headers: new Map(),
    bodyKind: null,
    body: undefined,
    sent: false,
  };
  const req = {
    method: String(rawRequest.method ?? "GET"),
    path: String(rawRequest.path ?? "/"),
    originalUrl: String(rawRequest.original_url ?? rawRequest.path ?? "/"),
    url: String(rawRequest.original_url ?? rawRequest.path ?? "/"),
    headers,
    query: cloneValue(rawRequest.query ?? {}),
    body: cloneValue(rawRequest.body),
    rawBody: String(rawRequest.raw_body ?? ""),
    get(name) {
      if (typeof name !== "string") {
        return undefined;
      }
      return headers[name.toLowerCase()];
    },
    header(name) {
      return this.get(name);
    },
  };
  const res = createFrameworkHttpResponseRecorder(state);
  return { req, res, state };
}

function normalizeFrameworkHeaders(rawHeaders) {
  validatePlainObject(rawHeaders, "Functions Framework HTTP request headers");
  return Object.freeze(Object.fromEntries(
    Object.entries(rawHeaders).map(([name, value]) => [name.toLowerCase(), String(value)]),
  ));
}

function createFrameworkHttpResponseRecorder(state) {
  return {
    status(code) {
      state.statusCode = normalizeFrameworkStatusCode(code);
      return this;
    },
    set(name, value) {
      if (typeof name !== "string" || name.trim().length === 0) {
        invalidInput("Functions Framework HTTP response headers require a non-empty name.");
      }
      state.headers.set(name.toLowerCase(), String(value));
      return this;
    },
    header(name, value) {
      return this.set(name, value);
    },
    get(name) {
      if (typeof name !== "string") {
        return undefined;
      }
      return state.headers.get(name.toLowerCase());
    },
    type(value) {
      return this.set("content-type", value);
    },
    json(value) {
      if (!state.headers.has("content-type")) {
        state.headers.set("content-type", "application/json");
      }
      state.bodyKind = "json";
      state.body = cloneValue(value);
      state.sent = true;
      return this;
    },
    send(value) {
      const payload = normalizeFrameworkHttpReturnValue(value);
      if (payload.bodyKind === "json" && !state.headers.has("content-type")) {
        state.headers.set("content-type", "application/json");
      }
      state.bodyKind = payload.bodyKind;
      state.body = payload.body;
      state.sent = true;
      return this;
    },
    end(value = "") {
      return this.send(value);
    },
    get headersSent() {
      return state.sent;
    },
    get statusCode() {
      return state.statusCode;
    },
    set statusCode(code) {
      state.statusCode = normalizeFrameworkStatusCode(code);
    },
  };
}

function normalizeFrameworkStatusCode(value) {
  const code = Number(value);
  if (!Number.isInteger(code) || code < 100 || code > 999) {
    invalidInput("Functions Framework HTTP responses require a valid numeric status code.");
  }
  return code;
}

function normalizeFrameworkHttpReturnValue(value) {
  if (value === undefined) {
    return {
      bodyKind: "text",
      body: "",
    };
  }
  if (
    value === null
    || Array.isArray(value)
    || typeof value === "object"
  ) {
    return {
      bodyKind: "json",
      body: cloneValue(value),
    };
  }
  return {
    bodyKind: "text",
    body: String(value),
  };
}

function finalizeFrameworkHttpResponse(state, handlerResult) {
  if (!state.sent) {
    const payload = normalizeFrameworkHttpReturnValue(handlerResult);
    if (payload.bodyKind === "json" && !state.headers.has("content-type")) {
      state.headers.set("content-type", "application/json");
    }
    state.bodyKind = payload.bodyKind;
    state.body = payload.body;
  }
  return {
    status: state.statusCode,
    headers: Object.fromEntries(state.headers),
    body_kind: state.bodyKind ?? "text",
    body: state.body ?? "",
  };
}

function collectExportedTargets(modules) {
  const seenNames = new Set();
  const seenEntrypoints = new Set();
  const collected = [];
  for (const module of modules) {
    for (const [exportName, exportedValue] of Object.entries(module.exports)) {
      if (!exportedValue || exportedValue[TARGET_MARKER] !== true) {
        continue;
      }
      const targetName = exportName;
      const entrypoint = \`exports.\${exportName}\`;
      if (seenNames.has(targetName)) {
        invalidInput(
          \`Cloud Functions export "\${targetName}" is duplicated across codebases; unique exported trigger names are required in the first slice\`,
        );
      }
      if (seenEntrypoints.has(entrypoint)) {
        invalidInput(\`Cloud Functions runtime entrypoint "\${entrypoint}" is duplicated.\`);
      }
      seenNames.add(targetName);
      seenEntrypoints.add(entrypoint);
      const binding = cloneValue(exportedValue.target.binding ?? {});
      if (
        exportedValue.target.authoring_surface === "firebase_v2"
        && exportedValue.target.signature_type === "http"
        && binding.binding_kind === "https"
        && (binding.exposure === "http" || binding.exposure === "callable")
      ) {
        binding.path = \`/\${targetName}\`;
      }
      collected.push({
        entrypoint,
        invoke: exportedValue.invoke,
        target: {
          name: targetName,
          entrypoint,
          authoring_surface: exportedValue.target.authoring_surface,
          signature_type: exportedValue.target.signature_type,
          binding,
        },
      });
    }
  }
  return collected;
}

function collectRegisteredFrameworkTargets() {
  return [...globalState.frameworkTargets];
}

function createInvocationDispatcher(targets) {
  const handlers = new Map(
    targets.map((target) => [target.entrypoint, target.invoke]),
  );
  return async function __neovexInvoke(request) {
    const handler = handlers.get(request.function_name);
    if (!handler) {
      throw new Error(\`unknown handler \${request.function_name}\`);
    }
    return handler(request.args, request);
  };
}

export {
  collectExportedTargets,
  collectRegisteredFrameworkTargets,
  createInvocationDispatcher,
  defineFirestoreDocumentTarget,
  defineFirebaseHttpsCallableTarget,
  defineFirebaseHttpsRequestTarget,
  HttpsError,
  onInit,
  registerFrameworkTarget,
  setGlobalOptions,
  unsupportedPhaseSurface,
};
`;
}

function cloudFunctionsAdminAppSource() {
  return `
const APPS = globalThis.__neovexAdminApps ??= [];

function initializeApp(options = {}, name = "[DEFAULT]") {
  const existing = APPS.find((app) => app.name === name);
  if (existing) {
    return existing;
  }
  const app = { name, options };
  APPS.push(app);
  return app;
}

function getApp(name = "[DEFAULT]") {
  const app = APPS.find((candidate) => candidate.name === name);
  if (!app) {
    throw new Error(\`firebase-admin/app app "\${name}" has not been initialized\`);
  }
  return app;
}

function getApps() {
  return [...APPS];
}

async function deleteApp(app) {
  const index = APPS.findIndex((candidate) => candidate === app);
  if (index >= 0) {
    APPS.splice(index, 1);
  }
}

export { deleteApp, getApp, getApps, initializeApp };
`;
}

function cloudFunctionsAdminFirestoreSource() {
  return `
import { initializeApp } from "__neovex_firebase_admin_app__";

class Timestamp {
  constructor(milliseconds) {
    if (!Number.isFinite(milliseconds)) {
      throw new Error("firebase-admin/firestore Timestamp requires a finite millisecond value.");
    }
    this._milliseconds = Number(milliseconds);
  }

  static fromMillis(milliseconds) {
    return new Timestamp(milliseconds);
  }

  static now() {
    return new Timestamp(Date.now());
  }

  toMillis() {
    return this._milliseconds;
  }

  toDate() {
    return new Date(this._milliseconds);
  }

  isEqual(other) {
    return other instanceof Timestamp && other.toMillis() === this.toMillis();
  }

  toJSON() {
    return this.toMillis();
  }
}

function cloneAdminValue(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

function readAdminFieldPath(value, fieldPath) {
  if (value === undefined) {
    return undefined;
  }
  return fieldPath
    .split(".")
    .reduce(
      (current, segment) => (
        current !== null
          && typeof current === "object"
          && !Array.isArray(current)
          && segment in current
      )
        ? current[segment]
        : undefined,
      value,
    );
}

function splitFirestorePath(path, label) {
  if (typeof path !== "string" || path.trim().length === 0) {
    throw new Error(\`\${label} requires a non-empty path string.\`);
  }
  const segments = path.split("/");
  if (segments.some((segment) => segment.trim().length === 0)) {
    throw new Error(\`\${label} must not contain empty path segments.\`);
  }
  return segments;
}

function ensureDocumentPath(path, label) {
  const segments = splitFirestorePath(path, label);
  if (!segments.length || segments.length % 2 !== 0) {
    throw new Error(\`\${label} requires a Firestore document path with an even number of segments.\`);
  }
  return segments;
}

function ensureCollectionPath(path, label) {
  const segments = splitFirestorePath(path, label);
  if (!segments.length || segments.length % 2 !== 1) {
    throw new Error(\`\${label} requires a Firestore collection path with an odd number of segments.\`);
  }
  return segments;
}

function joinFirestorePath(baseSegments, relativePath, label, kind) {
  const segments = [...baseSegments, ...splitFirestorePath(relativePath, label)];
  const expectsDocument = kind === "document";
  if (expectsDocument ? segments.length % 2 !== 0 : segments.length % 2 !== 1) {
    throw new Error(
      expectsDocument
        ? \`\${label} must resolve to a Firestore document path.\`
        : \`\${label} must resolve to a Firestore collection path.\`,
    );
  }
  return segments.join("/");
}

function normalizeAdminWriteValue(value) {
  if (value instanceof Timestamp) {
    return value.toMillis();
  }
  if (value instanceof Date) {
    return value.toISOString();
  }
  if (Array.isArray(value)) {
    return value.map((item) => normalizeAdminWriteValue(item));
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, normalizeAdminWriteValue(entry)]),
    );
  }
  return value;
}

function normalizeAdminWriteData(data, label) {
  if (data === null || typeof data !== "object" || Array.isArray(data)) {
    throw new Error(\`\${label} requires a plain object.\`);
  }
  return Object.fromEntries(
    Object.entries(data).map(([key, value]) => [key, normalizeAdminWriteValue(value)]),
  );
}

function materializeWriteResult(value) {
  return Object.freeze({
    writeTime: Timestamp.fromMillis(value?.write_time_ms ?? 0),
  });
}

async function callFirestoreAdminHost(operation, payload) {
  return globalThis.__neovexAsyncHostValue(
    "op_neovex_runtime_extension_call",
    {
      namespace: "cloud_functions",
      operation,
      payload,
    },
  );
}

class FirestoreDocumentSnapshot {
  constructor(reference, rawDocument) {
    this.ref = reference;
    this.id = reference.id;
    this.exists = rawDocument !== null;
    this.createTime = rawDocument
      && typeof rawDocument.create_time_ms === "number"
      ? Timestamp.fromMillis(rawDocument.create_time_ms)
      : undefined;
    this.updateTime = rawDocument
      && typeof rawDocument.update_time_ms === "number"
      ? Timestamp.fromMillis(rawDocument.update_time_ms)
      : undefined;
    this._rawDocument = rawDocument;
  }

  data() {
    if (!this.exists) {
      return undefined;
    }
    return cloneAdminValue(this._rawDocument.fields ?? {});
  }

  get(fieldPath) {
    if (typeof fieldPath !== "string" || fieldPath.trim().length === 0) {
      return undefined;
    }
    return readAdminFieldPath(this.data(), fieldPath);
  }
}

class FirestoreCollectionReference {
  constructor(firestore, path) {
    this.firestore = firestore;
    this.path = ensureCollectionPath(path, "firebase-admin/firestore collection()").join("/");
    const segments = this.path.split("/");
    this.id = segments[segments.length - 1];
    this.parent = segments.length > 1
      ? new FirestoreDocumentReference(firestore, segments.slice(0, -1).join("/"))
      : null;
  }

  doc(path) {
    if (arguments.length !== 1) {
      throw new Error("firebase-admin/firestore collection().doc() requires an explicit document path in the covered slice.");
    }
    return new FirestoreDocumentReference(
      this.firestore,
      joinFirestorePath(this.path.split("/"), path, "firebase-admin/firestore collection().doc()", "document"),
    );
  }
}

class FirestoreDocumentReference {
  constructor(firestore, path) {
    this.firestore = firestore;
    this.path = ensureDocumentPath(path, "firebase-admin/firestore doc()").join("/");
    const segments = this.path.split("/");
    this.id = segments[segments.length - 1];
    this.parent = new FirestoreCollectionReference(
      firestore,
      segments.slice(0, -1).join("/"),
    );
  }

  async get() {
    const rawDocument = await callFirestoreAdminHost(
      "firebase_admin.firestore.get_document",
      {
        database_id: this.firestore.databaseId,
        document_path: this.path,
      },
    );
    return new FirestoreDocumentSnapshot(this, rawDocument);
  }

  async set(data) {
    if (arguments.length !== 1) {
      throw new Error("firebase-admin/firestore DocumentReference.set() currently supports only set(data).");
    }
    const result = await callFirestoreAdminHost(
      "firebase_admin.firestore.set_document",
      {
        database_id: this.firestore.databaseId,
        document_path: this.path,
        fields: normalizeAdminWriteData(data, "firebase-admin/firestore DocumentReference.set()"),
      },
    );
    return materializeWriteResult(result);
  }

  async update(data) {
    if (arguments.length !== 1) {
      throw new Error("firebase-admin/firestore DocumentReference.update() currently supports only update(data).");
    }
    const result = await callFirestoreAdminHost(
      "firebase_admin.firestore.update_document",
      {
        database_id: this.firestore.databaseId,
        document_path: this.path,
        patch: normalizeAdminWriteData(data, "firebase-admin/firestore DocumentReference.update()"),
      },
    );
    return materializeWriteResult(result);
  }

  async delete() {
    if (arguments.length !== 0) {
      throw new Error("firebase-admin/firestore DocumentReference.delete() does not yet support delete options.");
    }
    const result = await callFirestoreAdminHost(
      "firebase_admin.firestore.delete_document",
      {
        database_id: this.firestore.databaseId,
        document_path: this.path,
      },
    );
    return materializeWriteResult(result);
  }

  collection(path) {
    if (arguments.length !== 1) {
      throw new Error("firebase-admin/firestore DocumentReference.collection() requires an explicit child collection path.");
    }
    return new FirestoreCollectionReference(
      this.firestore,
      joinFirestorePath(this.path.split("/"), path, "firebase-admin/firestore DocumentReference.collection()", "collection"),
    );
  }
}

class Firestore {
  constructor(app, databaseId) {
    this.app = app;
    this.databaseId = databaseId;
  }

  collection(path) {
    return new FirestoreCollectionReference(this, path);
  }

  doc(path) {
    return new FirestoreDocumentReference(this, path);
  }
}

function getFirestore(app = initializeApp(), databaseId = "(default)") {
  return new Firestore(app, databaseId);
}

export { Timestamp, getFirestore };
`;
}

function ensureRelativeImport(candidate) {
  if (candidate.startsWith(".")) {
    return candidate;
  }
  return `./${candidate}`;
}

export {
  cloudFunctionsAdminAppSource,
  cloudFunctionsAdminFirestoreSource,
  cloudFunctionsEntrySource,
  cloudFunctionsSharedSource,
};
