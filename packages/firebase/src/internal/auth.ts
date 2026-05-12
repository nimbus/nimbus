import type { Firestore } from "../firestore";
import type { FirestoreGrpcWebContext } from "./grpc-web";

const FIRESTORE_LISTEN_AUTH_SUBPROTOCOL_PREFIX = "nimbus.firebase.auth.";
const FIRESTORE_LISTEN_WEBSOCKET_PROTOCOL = "nimbus.firebase.listen.v1";

export interface FirestoreAuthDependencies {
  mockUserToken(firestore: Firestore): string | null;
}

export async function resolveAuthToken(
  firestore: Firestore,
  forceRefresh: boolean,
  dependencies: FirestoreAuthDependencies,
): Promise<string | null> {
  const authToken = firestore.settings.experimentalAuthToken;
  if (typeof authToken === "string") {
    return authToken;
  }
  if (typeof authToken === "function") {
    return (await authToken({ forceRefresh })) ?? null;
  }
  return dependencies.mockUserToken(firestore);
}

export function canRefreshAuthToken(firestore: Firestore): boolean {
  return typeof firestore.settings.experimentalAuthToken === "function";
}

export function grpcWebContext(
  firestore: Firestore,
  dependencies: FirestoreAuthDependencies,
): FirestoreGrpcWebContext {
  return {
    baseUrl: `${firestore.settings.ssl ? "https" : "http"}://${firestore.settings.host}`,
    fetch: firestore.settings.experimentalFetch,
    apiKey: firestore.app.options.apiKey,
    appId: firestore.app.options.appId,
    headers: firestore.settings.experimentalHeaders,
    canRefreshAuthToken: canRefreshAuthToken(firestore),
    resolveAuthToken: (forceRefresh) =>
      resolveAuthToken(firestore, forceRefresh, dependencies),
  };
}

export async function resolveListenWebSocketSubprotocols(
  firestore: Firestore,
  forceRefresh: boolean,
  dependencies: FirestoreAuthDependencies,
): Promise<readonly string[]> {
  const protocols = [FIRESTORE_LISTEN_WEBSOCKET_PROTOCOL];
  const token = await resolveAuthToken(firestore, forceRefresh, dependencies);
  if (!token) {
    return protocols;
  }
  return [
    ...protocols,
    `${FIRESTORE_LISTEN_AUTH_SUBPROTOCOL_PREFIX}${encodeBase64UrlUtf8(token)}`,
  ];
}

function encodeBase64UrlUtf8(value: string): string {
  const bytes = new TextEncoder().encode(value);
  const bufferCtor = (globalThis as {
    Buffer?: {
      from(bytes: Uint8Array): {
        toString(encoding: "base64url"): string;
      };
    };
  }).Buffer;
  if (bufferCtor) {
    return bufferCtor.from(bytes).toString("base64url");
  }
  const encode = globalThis.btoa;
  if (typeof encode !== "function") {
    throw new Error("No base64 encoder is available for Firestore Listen auth.");
  }
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return encode(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/u, "");
}
