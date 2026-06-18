import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";

import {
  NIMBUS_ROOT_SDK_CONTROL_PLANE_ROUTE_FRAGMENTS,
  NIMBUS_ROOT_SDK_ARTIFACT_PATHS,
  NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS,
  NIMBUS_ROOT_SDK_ROUTE_ARTIFACT_PATHS,
  assertNimbusRootSdkArtifactText,
  assertNimbusRootSdkRouteArtifactText,
} from "../../../scripts/nimbus-root-sdk-artifact-policy.mjs";

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));

const packageContracts = {
  "convex": {
    path: "packages/convex/package.json",
    exports: {
      "./react": "./src/react.ts",
      "./browser": "./src/browser.ts",
      "./server": "./src/server.ts",
      "./values": "./src/values.ts",
    },
  },
  "@nimbus/nimbus": {
    path: "packages/nimbus/package.json",
    exports: {
      ".": "./src/index.ts",
      "./react": "./src/react.ts",
      "./browser": "./src/browser.ts",
      "./server": "./src/server.ts",
      "./values": "./src/values.ts",
      "./internal/shared": "./src/internal/shared.ts",
      "./transports/rest": "./src/transports/rest.ts",
    },
  },
  firebase: {
    path: "packages/firebase/package.json",
    exports: {
      ".": "./src/index.ts",
      "./app": "./src/app.ts",
      "./firestore": "./src/firestore.ts",
    },
  },
  "@nimbus/mongodb": {
    path: "packages/mongodb/package.json",
    exports: {
      ".": "./src/index.ts",
    },
  },
  "@nimbus/dynamodb": {
    path: "packages/dynamodb/package.json",
    exports: {
      ".": "./src/index.ts",
    },
  },
};

const entryContracts = {
  "convex/browser": {
    path: "packages/convex/src/browser.ts",
    exports: [
      "AnyApi",
      "AuthTokenFetcher",
      "ConnectionState",
      "ConvexClient",
      "ConvexHttpClient",
      "ConvexReactClient",
      "Unsubscribe",
      "WebSocketConstructor",
      "anyApi",
      "defineAction",
      "defineMutation",
      "definePaginatedQuery",
      "defineQuery",
      "makeActionReference",
      "makeMutationReference",
      "makePaginatedQueryReference",
      "makeQueryReference",
    ],
  },
  "convex/react": {
    path: "packages/convex/src/react.ts",
    exports: [
      "ConnectionState",
      "ConvexAuthState",
      "ConvexProvider",
      "ConvexProviderWithAuth",
      "ConvexReactClient",
      "PaginationStatus",
      "UsePaginatedQueryResult",
      "UseQueriesRequest",
      "UseQueriesResults",
      "useAction",
      "useConvex",
      "useConvexAuth",
      "useConvexConnectionState",
      "useMutation",
      "usePaginatedQuery",
      "useQueries",
      "useQuery",
    ],
  },
  "convex/server": {
    path: "packages/convex/src/server.ts",
    exports: [
      "ActionCtx",
      "Auth",
      "AuthConfig",
      "AuthProvider",
      "Cursor",
      "DefaultFunctionArgs",
      "FilterExpressionBuilder",
      "FilterField",
      "FunctionReference",
      "GenericActionCtx",
      "GenericDatabaseReader",
      "GenericDatabaseWriter",
      "GenericMutationCtx",
      "GenericQueryCtx",
      "HttpRouteMethod",
      "HttpRouteSpec",
      "HttpRouter",
      "IndexRangeBuilder",
      "MutationCtx",
      "PaginationOptions",
      "PaginationResult",
      "PaginationStatus",
      "PublicHttpAction",
      "QueryBuilder",
      "QueryCtx",
      "QueryOrder",
      "RegisteredAction",
      "RegisteredMutation",
      "RegisteredPaginatedQuery",
      "RegisteredQuery",
      "Scheduler",
      "SchemaDefinition",
      "TableDefinition",
      "UserIdentity",
      "UserIdentityAttributes",
      "action",
      "defineSchema",
      "defineTable",
      "httpAction",
      "httpRouter",
      "internalAction",
      "internalMutation",
      "internalPaginatedQuery",
      "internalQuery",
      "mutation",
      "paginatedQuery",
      "paginationOptsValidator",
      "paginationResultValidator",
      "query",
    ],
  },
  "convex/values": {
    path: "packages/convex/src/values.ts",
    exports: ["GenericId", "Infer", "Validator", "v"],
  },
  "@nimbus/nimbus": {
    path: "packages/nimbus/src/index.ts",
    exports: [
      "Nimbus",
      "NimbusBuiltInProviderId",
      "NimbusClientOptions",
      "NimbusCondition",
      "NimbusCredential",
      "NimbusExternalAuthPolicy",
      "NimbusExternalEndpointPolicy",
      "NimbusHealthCheckPolicy",
      "NimbusRedactedValues",
      "NimbusSandboxBackendKind",
      "NimbusSandboxCollection",
      "NimbusSandboxCreateRequest",
      "NimbusSandboxListRequest",
      "NimbusSandboxOciImageReferenceSource",
      "NimbusSandboxOwnerSpec",
      "NimbusSandboxProcessResponse",
      "NimbusSandboxProcessSpec",
      "NimbusSandboxProfile",
      "NimbusSandboxResource",
      "NimbusSandboxRootResponse",
      "NimbusSandboxRootSpec",
      "NimbusSandboxSelector",
      "NimbusSandboxSpec",
      "NimbusSandboxSpecResponse",
      "NimbusService",
      "NimbusServiceActivationWaitCondition",
      "NimbusServiceBackendResponse",
      "NimbusServiceBackendSpec",
      "NimbusServiceCreateRequest",
      "NimbusServiceDefinition",
      "NimbusServiceDefinitionCollection",
      "NimbusServiceDeleteRequest",
      "NimbusServiceEndpoint",
      "NimbusServiceLifecycleRequest",
      "NimbusServiceListRequest",
      "NimbusServiceSelector",
      "NimbusServiceStartRequest",
      "NimbusServiceStopRequest",
      "NimbusServiceStopWaitCondition",
      "NimbusServiceUpdateRequest",
      "NimbusServiceWaitCondition",
      "NimbusServiceWaitRequest",
      "NimbusSessionChannel",
      "NimbusSessionCloseRequest",
      "NimbusSessionCollection",
      "NimbusSessionListRequest",
      "NimbusSessionOpenRequest",
      "NimbusSessionResource",
      "NimbusSessionSelector",
      "NimbusSessionTarget",
    ],
  },
  "@nimbus/nimbus/browser": {
    path: "packages/nimbus/src/browser.ts",
    exports: [
      "AuthTokenFetcher",
      "ConnectionState",
      "FunctionReference",
      "InferArgs",
      "InferResult",
      "NimbusClient",
      "NimbusHttpClient",
      "NimbusReactClient",
      "QueryEntry",
      "QueryReference",
      "Unsubscribe",
      "WebSocketConstructor",
      "WebSocketLike",
      "defineAction",
      "defineMutation",
      "definePaginatedQuery",
      "defineQuery",
      "makeActionReference",
      "makeMutationReference",
      "makePaginatedQueryReference",
      "makeQueryReference",
      "queryEntry",
    ],
  },
  "@nimbus/nimbus/react": {
    path: "packages/nimbus/src/react.ts",
    exports: [
      "ConnectionState",
      "NimbusAuthState",
      "NimbusProvider",
      "NimbusProviderWithAuth",
      "NimbusReactClient",
      "PaginationStatus",
      "UsePaginatedQueryResult",
      "UseQueriesRequest",
      "UseQueriesResults",
      "useAction",
      "useMutation",
      "useNimbus",
      "useNimbusAuth",
      "useNimbusConnectionState",
      "usePaginatedQuery",
      "useQueries",
      "useQuery",
    ],
  },
  "@nimbus/nimbus/server": {
    path: "packages/nimbus/src/server.ts",
    exports: [
      "Auth",
      "AuthConfig",
      "AuthProvider",
      "Cursor",
      "DefaultFunctionArgs",
      "FilterExpressionBuilder",
      "FilterField",
      "FunctionReference",
      "GenericActionCtx",
      "GenericDatabaseReader",
      "GenericDatabaseWriter",
      "GenericMutationCtx",
      "GenericQueryCtx",
      "HttpRouteMethod",
      "HttpRouteSpec",
      "HttpRouter",
      "IndexRangeBuilder",
      "PaginationOptions",
      "PaginationResult",
      "PaginationStatus",
      "PublicHttpAction",
      "QueryBuilder",
      "QueryOrder",
      "RegisteredAction",
      "RegisteredMutation",
      "RegisteredPaginatedQuery",
      "RegisteredQuery",
      "Scheduler",
      "SchemaDefinition",
      "TableDefinition",
      "UserIdentity",
      "UserIdentityAttributes",
      "VerifiedIdentity",
      "VerifiedIdentityAttributes",
      "VerifiedIdentityKind",
      "action",
      "defineSchema",
      "defineTable",
      "httpAction",
      "httpRouter",
      "internalAction",
      "internalMutation",
      "internalPaginatedQuery",
      "internalQuery",
      "mutation",
      "paginatedQuery",
      "paginationOptsValidator",
      "paginationResultValidator",
      "query",
    ],
  },
  "@nimbus/nimbus/values": {
    path: "packages/nimbus/src/values.ts",
    exports: ["GenericId", "Infer", "Validator", "v"],
  },
  "@nimbus/nimbus/transports/rest": {
    path: "packages/nimbus/src/transports/rest.ts",
    exports: [
      "CronJobRequest",
      "FetchLike",
      "NIMBUS_REST_ROUTES",
      "NimbusRestClient",
      "NimbusRestClientOptions",
      "NimbusRestRouteName",
      "NimbusSubscriptionClient",
      "PaginatedQueryRequest",
      "RequestOptions",
      "ScheduleMutationRequest",
      "SubscribeQuery",
      "Subscription",
      "SubscriptionClientOptions",
      "TableSchema",
    ],
  },
  firebase: {
    path: "packages/firebase/src/index.ts",
    exports: [
      "CollectionGroup",
      "CollectionReference",
      "DocumentData",
      "DocumentIdFieldPath",
      "DocumentReference",
      "DocumentSnapshot",
      "FetchLike",
      "FieldValue",
      "FirebaseApp",
      "FirebaseAppSettings",
      "FirebaseOptions",
      "Firestore",
      "FirestoreAuthTokenFetcher",
      "FirestoreDataConverter",
      "FirestoreEmulatorOptions",
      "FirestoreError",
      "FirestoreSettings",
      "FirestoreUnaryTransport",
      "OrderByDirection",
      "Query",
      "QueryConstraint",
      "QueryDocumentSnapshot",
      "QuerySnapshot",
      "SetOptions",
      "SnapshotMetadata",
      "SnapshotObserver",
      "Transaction",
      "TransactionOptions",
      "Unsubscribe",
      "WhereFilterOp",
      "WriteBatch",
      "addDoc",
      "arrayRemove",
      "arrayUnion",
      "collection",
      "collectionGroup",
      "connectFirestoreEmulator",
      "deleteApp",
      "deleteDoc",
      "deleteField",
      "doc",
      "documentId",
      "endAt",
      "endBefore",
      "getApp",
      "getApps",
      "getDoc",
      "getDocs",
      "getFirestore",
      "increment",
      "initializeApp",
      "initializeFirestore",
      "limit",
      "onSnapshot",
      "orderBy",
      "query",
      "queryEqual",
      "refEqual",
      "runTransaction",
      "serverTimestamp",
      "setDoc",
      "snapshotEqual",
      "startAfter",
      "startAt",
      "terminate",
      "updateDoc",
      "where",
      "writeBatch",
    ],
  },
  "firebase/app": {
    path: "packages/firebase/src/app.ts",
    exports: [
      "FirebaseApp",
      "FirebaseAppSettings",
      "FirebaseOptions",
      "deleteApp",
      "getApp",
      "getApps",
      "initializeApp",
    ],
  },
  "firebase/firestore": {
    path: "packages/firebase/src/firestore.ts",
    exports: [
      "CollectionGroup",
      "CollectionReference",
      "DocumentData",
      "DocumentIdFieldPath",
      "DocumentReference",
      "DocumentSnapshot",
      "FetchLike",
      "FieldValue",
      "Firestore",
      "FirestoreAuthTokenFetcher",
      "FirestoreDataConverter",
      "FirestoreEmulatorOptions",
      "FirestoreError",
      "FirestoreSettings",
      "FirestoreUnaryTransport",
      "OrderByDirection",
      "Query",
      "QueryConstraint",
      "QueryDocumentSnapshot",
      "QuerySnapshot",
      "SetOptions",
      "SnapshotMetadata",
      "SnapshotObserver",
      "Transaction",
      "TransactionOptions",
      "Unsubscribe",
      "WhereFilterOp",
      "WriteBatch",
      "addDoc",
      "arrayRemove",
      "arrayUnion",
      "collection",
      "collectionGroup",
      "connectFirestoreEmulator",
      "deleteDoc",
      "deleteField",
      "doc",
      "documentId",
      "endAt",
      "endBefore",
      "getDoc",
      "getDocs",
      "getFirestore",
      "increment",
      "initializeFirestore",
      "limit",
      "onSnapshot",
      "orderBy",
      "query",
      "queryEqual",
      "refEqual",
      "runTransaction",
      "serverTimestamp",
      "setDoc",
      "snapshotEqual",
      "startAfter",
      "startAt",
      "terminate",
      "updateDoc",
      "where",
      "writeBatch",
    ],
  },
  "@nimbus/mongodb": {
    path: "packages/mongodb/src/index.ts",
    exports: ["MongoUriOptions", "mongoUri"],
  },
  "@nimbus/dynamodb": {
    path: "packages/dynamodb/src/index.ts",
    exports: [
      "NimbusDynamoConfig",
      "NimbusDynamoOptions",
      "clientConfig",
      "endpoint",
    ],
  },
};

const compatSpecifiers = Object.keys(entryContracts).filter(
  (specifier) => !specifier.startsWith("@nimbus/nimbus"),
);
const nimbusUnprivilegedSpecifiers = [
  "@nimbus/nimbus/browser",
  "@nimbus/nimbus/react",
  "@nimbus/nimbus/server",
  "@nimbus/nimbus/values",
];
const forbiddenControlPlaneNames = [
  "Nimbus",
  "NimbusRestClient",
  "NimbusSubscriptionClient",
  "services",
  "sandboxes",
  "sessions",
  "models",
  "audio",
  "video",
  "content",
];

export async function assertCapabilitySurfaceContract() {
  await assertPackageExportMaps();
  await assertEntryExportSurfaces();
  await assertNimbusRootSdkBoundary();
  await assertNimbusRootSdkArtifacts();
  console.log("  ✓ capability segregation package surface contract verified");
}

async function assertPackageExportMaps() {
  for (const [packageName, contract] of Object.entries(packageContracts)) {
    const packageJson = JSON.parse(
      await fs.readFile(repoPath(contract.path), "utf8"),
    );
    assert.equal(packageJson.name, packageName, `${contract.path} name drifted`);
    assert.deepEqual(
      packageJson.exports,
      contract.exports,
      `${packageName} package exports changed without updating the CB0 surface contract`,
    );
  }
}

async function assertEntryExportSurfaces() {
  for (const [specifier, contract] of Object.entries(entryContracts)) {
    const actual = await collectExportNames(repoPath(contract.path));
    assert.deepEqual(
      actual,
      contract.exports,
      `${specifier} named export surface changed without updating the CB0 surface contract`,
    );
  }

  for (const specifier of compatSpecifiers) {
    assertNoForbiddenExports(specifier, entryContracts[specifier].exports);
  }
  for (const specifier of nimbusUnprivilegedSpecifiers) {
    assertNoForbiddenExports(specifier, entryContracts[specifier].exports);
  }
}

async function assertNimbusRootSdkBoundary() {
  const source = await fs.readFile(repoPath("packages/nimbus/src/index.ts"), "utf8");
  const routes = await fs.readFile(repoPath("packages/nimbus/src/control_plane_routes.ts"), "utf8");
  for (const fragment of NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS) {
    assert.equal(
      source.includes(fragment),
      false,
      `packages/nimbus/src/index.ts contains forbidden root SDK fragment: ${fragment}`,
    );
  }
  for (const fragment of NIMBUS_ROOT_SDK_CONTROL_PLANE_ROUTE_FRAGMENTS) {
    assert.equal(
      routes.includes(fragment),
      true,
      `packages/nimbus/src/control_plane_routes.ts is missing root SDK route fragment: ${fragment}`,
    );
  }
  for (const fragment of ["/api/tenants/", "/api/sessions"]) {
    assert.equal(
      source.includes(fragment),
      false,
      `packages/nimbus/src/index.ts must use control_plane_routes.ts instead of embedding ${fragment}`,
    );
  }
  assert.equal(
    source.includes("async #controlPlaneRequest"),
    true,
    "root SDK control-plane transport must stay an ECMAScript-private implementation detail",
  );
  assert.equal(
    source.includes("async #controlPlaneRouteRequest"),
    true,
    "root SDK route expansion must stay an ECMAScript-private implementation detail",
  );
  assert.equal(
    source.includes("async #resolveRestClient"),
    true,
    "root SDK low-level client resolution must stay an ECMAScript-private implementation detail",
  );
  assert.equal(
    source.includes("new NimbusRestClient"),
    true,
    "root SDK must keep default authenticated control-plane transport selection internal",
  );
}

async function assertNimbusRootSdkArtifacts() {
  const artifacts = [];
  for (const artifactPath of NIMBUS_ROOT_SDK_ARTIFACT_PATHS) {
    const artifact = await readIfExists(repoPath(artifactPath));
    if (artifact === null) continue;
    artifacts.push({ path: artifactPath, content: artifact });
  }
  if (artifacts.length === 0) return;
  assert.equal(
    artifacts.length,
    NIMBUS_ROOT_SDK_ARTIFACT_PATHS.length,
    "root SDK generated artifacts are partially present; rerun `npm run build:embedded-packages`",
  );
  for (const { path: artifactPath, content: artifact } of artifacts) {
    try {
      assertNimbusRootSdkArtifactText(artifactPath, artifact);
    } catch (error) {
      if (error instanceof Error) {
        assert.fail(error.message);
      }
      throw error;
    }
  }

  const routeArtifacts = [];
  for (const artifactPath of NIMBUS_ROOT_SDK_ROUTE_ARTIFACT_PATHS) {
    const artifact = await readIfExists(repoPath(artifactPath));
    if (artifact === null) continue;
    routeArtifacts.push({ path: artifactPath, content: artifact });
  }
  if (routeArtifacts.length === 0) return;
  assert.equal(
    routeArtifacts.length,
    NIMBUS_ROOT_SDK_ROUTE_ARTIFACT_PATHS.length,
    "root SDK route artifacts are partially present; rerun `npm run build:embedded-packages`",
  );
  for (const { path: artifactPath, content: artifact } of routeArtifacts) {
    try {
      assertNimbusRootSdkRouteArtifactText(artifactPath, artifact);
    } catch (error) {
      if (error instanceof Error) {
        assert.fail(error.message);
      }
      throw error;
    }
  }
}

async function readIfExists(filePath) {
  try {
    return await fs.readFile(filePath, "utf8");
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return null;
    }
    throw error;
  }
}

function assertNoForbiddenExports(specifier, exports) {
  const forbidden = exports.filter((name) =>
    forbiddenControlPlaneNames.includes(name),
  );
  assert.deepEqual(
    forbidden,
    [],
    `${specifier} exposes Nimbus-specific control-plane names: ${forbidden.join(", ")}`,
  );
}

async function collectExportNames(filePath, seen = new Set()) {
  const normalizedPath = path.normalize(filePath);
  if (seen.has(normalizedPath)) {
    return [];
  }
  seen.add(normalizedPath);

  const source = await fs.readFile(normalizedPath, "utf8");
  const sourceFile = ts.createSourceFile(
    normalizedPath,
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const names = new Set();

  for (const node of sourceFile.statements) {
    if (ts.isExportDeclaration(node)) {
      await collectExportDeclarationNames(node, normalizedPath, seen, names);
      continue;
    }
    if (!hasExportModifier(node)) {
      continue;
    }
    collectExportedDeclarationNames(node, names);
  }

  return [...names].sort();
}

async function collectExportDeclarationNames(node, filePath, seen, names) {
  const exportClause = node.exportClause;
  if (exportClause && ts.isNamedExports(exportClause)) {
    for (const specifier of exportClause.elements) {
      names.add(specifier.name.text);
    }
    return;
  }

  const moduleSpecifier =
    node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)
      ? node.moduleSpecifier.text
      : null;
  if (!exportClause && moduleSpecifier?.startsWith(".")) {
    const reexportPath = resolveLocalTypeScriptModule(filePath, moduleSpecifier);
    for (const name of await collectExportNames(reexportPath, seen)) {
      names.add(name);
    }
  }
}

function collectExportedDeclarationNames(node, names) {
  if (
    (ts.isFunctionDeclaration(node) ||
      ts.isClassDeclaration(node) ||
      ts.isInterfaceDeclaration(node) ||
      ts.isTypeAliasDeclaration(node) ||
      ts.isEnumDeclaration(node)) &&
    node.name
  ) {
    names.add(node.name.text);
    return;
  }

  if (ts.isVariableStatement(node)) {
    for (const declaration of node.declarationList.declarations) {
      collectBindingName(declaration.name, names);
    }
  }
}

function collectBindingName(name, names) {
  if (ts.isIdentifier(name)) {
    names.add(name.text);
    return;
  }
  if (ts.isObjectBindingPattern(name) || ts.isArrayBindingPattern(name)) {
    for (const element of name.elements) {
      if (ts.isBindingElement(element)) {
        collectBindingName(element.name, names);
      }
    }
  }
}

function hasExportModifier(node) {
  return (node.modifiers ?? []).some(
    (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword,
  );
}

function resolveLocalTypeScriptModule(fromFile, specifier) {
  const base = path.resolve(path.dirname(fromFile), specifier);
  return path.extname(base) ? base : `${base}.ts`;
}

function repoPath(relativePath) {
  return path.join(repoRoot, relativePath);
}
