import type {
  ActionReference,
  MutationReference,
  PaginatedQueryReference,
  QueryReference,
  InferArgs,
} from "./internal/shared.ts";
import {
  defineAction,
  defineMutation,
  definePaginatedQuery,
  defineQuery,
  makeActionReference,
  makeMutationReference,
  makePaginatedQueryReference,
  makeQueryReference,
  normalizeArgs,
  stripTrailingSlash,
  validateDeploymentUrl,
  websocketUrlFromBase,
} from "./internal/shared.ts";

export {
  defineAction,
  defineMutation,
  definePaginatedQuery,
  defineQuery,
  makeActionReference,
  makeMutationReference,
  makePaginatedQueryReference,
  makeQueryReference,
} from "./internal/shared.ts";
export { NimbusHttpClient } from "./http-client.ts";
export type { AuthTokenFetcher } from "./http-client.ts";

import { NimbusHttpClient } from "./http-client.ts";
import type {
  AuthChangeListener,
  AuthTokenFetcher,
  FetchLike,
} from "./http-client.ts";
import {
  areSubscriptionValuesEqual,
  attachSocketListener,
  buildSubscribeMessage,
  decodeJwtPayload,
} from "./browser-utils.ts";
import type {
  InferLiveResult,
  LiveQueryReference,
  SubscriptionEntry,
} from "./browser-utils.ts";

export type ConnectionState = {
  hasInflightRequests: boolean;
  isWebSocketConnected: boolean;
  timeOfOldestInflightRequest: Date | null;
  hasEverConnected: boolean;
  connectionCount: number;
  connectionRetries: number;
  inflightMutations: number;
  inflightActions: number;
};

export type Unsubscribe<T> = {
  (): void;
  unsubscribe(): void;
  getCurrentValue(): T | undefined;
  getQueryLogs(): string[] | undefined;
};

export type WebSocketLike = {
  addEventListener?: (type: string, listener: (event: any) => void) => void;
  on?: (type: string, listener: (event: any) => void) => void;
  send(data: string): void;
  close(): void;
};
export type WebSocketConstructor = new (
  url: string,
  protocols?: string | string[],
) => WebSocketLike;

const MAXIMUM_REFRESH_DELAY = 20 * 24 * 60 * 60 * 1000;
const DEFAULT_AUTH_REFRESH_TOKEN_LEEWAY_SECONDS = 10;
const NIMBUS_WEBSOCKET_PROTOCOL = "nimbus.v2";
const NIMBUS_CLIENT_CAPABILITIES = ["queries.v1", "subscriptions.v1"] as const;

export class NimbusClient {
  private readonly httpClient: NimbusHttpClient;
  private readonly address: string;
  private readonly authRefreshTokenLeewaySeconds: number;
  private readonly webSocketImpl?: WebSocketConstructor;
  private readonly connectionListeners = new Set<() => void>();
  private readonly pendingSubscriptions = new Map<
    string,
    SubscriptionEntry<unknown>
  >();
  private readonly activeSubscriptions = new Map<number, SubscriptionEntry<unknown>>();
  private socket: WebSocketLike | null = null;
  private socketPromise: Promise<void> | null = null;
  private socketAuthentication:
    | {
        socket: WebSocketLike;
        token: string;
        resolve: () => void;
        reject: (error: Error) => void;
      }
    | null = null;
  private authRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private requestCounter = 0;
  private closed = false;
  private state: ConnectionState = {
    hasInflightRequests: false,
    isWebSocketConnected: false,
    timeOfOldestInflightRequest: null,
    hasEverConnected: false,
    connectionCount: 0,
    connectionRetries: 0,
    inflightMutations: 0,
    inflightActions: 0,
  };

  constructor(
    address: string,
    options: {
      skipDeploymentUrlCheck?: boolean;
      auth?: string;
      fetch?: FetchLike;
      disabled?: boolean;
      authRefreshTokenLeewaySeconds?: number;
      webSocket?: WebSocketConstructor;
    } = {},
  ) {
    if (options.skipDeploymentUrlCheck !== true) {
      validateDeploymentUrl(address);
    }
    this.address = stripTrailingSlash(address);
    this.httpClient = new NimbusHttpClient(address, options);
    this.authRefreshTokenLeewaySeconds =
      options.authRefreshTokenLeewaySeconds ?? DEFAULT_AUTH_REFRESH_TOKEN_LEEWAY_SECONDS;
    this.webSocketImpl = options.webSocket;
    if (options.disabled) {
      this.closed = true;
    }
  }

  get url() {
    return this.address;
  }

  setAuth(value: string | AuthTokenFetcher, onChange?: AuthChangeListener) {
    this.httpClient.setAuth(value, onChange);
    this.clearScheduledAuthRefresh();
    this.restartSocketForAuthChange();
  }

  clearAuth() {
    this.httpClient.clearAuth();
    this.clearScheduledAuthRefresh();
    this.restartSocketForAuthChange();
  }

  connectionState() {
    return this.state;
  }

  subscribeToConnectionState(callback: () => void) {
    this.connectionListeners.add(callback);
    return () => {
      this.connectionListeners.delete(callback);
    };
  }

  async query<Query extends QueryReference<any, any>>(
    query: Query,
    args?: InferArgs<Query>,
  ) {
    return this.httpClient.query(query, args);
  }

  async mutation<Mutation extends MutationReference<any, any>>(
    mutation: Mutation,
    args?: InferArgs<Mutation>,
  ) {
    this.bumpInflight("mutation", 1);
    try {
      return await this.httpClient.mutation(mutation, args);
    } finally {
      this.bumpInflight("mutation", -1);
    }
  }

  async action<Action extends ActionReference<any, any>>(
    action: Action,
    args?: InferArgs<Action>,
  ) {
    this.bumpInflight("action", 1);
    try {
      return await this.httpClient.action(action, args);
    } finally {
      this.bumpInflight("action", -1);
    }
  }

  async paginatedQuery<Query extends PaginatedQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query> | undefined,
    pageSize: number,
    cursor: string | null,
  ) {
    return this.httpClient.paginatedQuery(query, args, pageSize, cursor);
  }

  async scheduleAfter<Mutation extends MutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAfterMs: number,
  ) {
    return this.httpClient.scheduleAfter(mutation, args, runAfterMs);
  }

  async scheduleAt<Mutation extends MutationReference<any, any>>(
    mutation: Mutation,
    args: InferArgs<Mutation> | undefined,
    runAtMs: number,
  ) {
    return this.httpClient.scheduleAt(mutation, args, runAtMs);
  }

  async cancelScheduledFunction(jobId: string) {
    return this.httpClient.cancelScheduledFunction(jobId);
  }

  onUpdate<Query extends LiveQueryReference<any, any>>(
    query: Query,
    args: InferArgs<Query>,
    callback: (result: InferLiveResult<Query>) => unknown,
    onError?: (error: Error) => unknown,
    options?: { pageSize?: number; cursor?: string | null },
  ): Unsubscribe<InferLiveResult<Query>> {
    const entry: SubscriptionEntry<InferLiveResult<Query>> = {
      query,
      args: normalizeArgs(args),
      livePageSize: options?.pageSize,
      liveCursor: options?.cursor ?? null,
      callback,
      onError,
      unsubscribed: false,
    };
    this.queueSubscription(entry as SubscriptionEntry<unknown>);
    this.scheduleReconnect();

    const unsubscribe = (() => {
      if (entry.unsubscribed) {
        return;
      }
      entry.unsubscribed = true;
      if (entry.pendingRequestId) {
        this.pendingSubscriptions.delete(entry.pendingRequestId);
      }
      if (
        entry.subscriptionId !== undefined &&
        this.socket &&
        this.state.isWebSocketConnected
      ) {
        this.socket.send(
          JSON.stringify({
            type: "unsubscribe",
            subscription_id: entry.subscriptionId,
          }),
        );
        this.activeSubscriptions.delete(entry.subscriptionId);
      }
    }) as Unsubscribe<InferLiveResult<Query>>;

    unsubscribe.unsubscribe = unsubscribe;
    unsubscribe.getCurrentValue = () => entry.currentValue;
    unsubscribe.getQueryLogs = () => undefined;

    return unsubscribe;
  }

  close() {
    this.closed = true;
    this.clearScheduledAuthRefresh();
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.pendingSubscriptions.clear();
    this.activeSubscriptions.clear();
    if (this.socket) {
      this.socket.close();
      this.socket = null;
    }
  }

  private emitConnectionState() {
    for (const listener of this.connectionListeners) {
      listener();
    }
  }

  private bumpInflight(kind: "mutation" | "action", delta: number) {
    if (kind === "mutation") {
      this.state = {
        ...this.state,
        inflightMutations: Math.max(0, this.state.inflightMutations + delta),
      };
    } else {
      this.state = {
        ...this.state,
        inflightActions: Math.max(0, this.state.inflightActions + delta),
      };
    }
    this.state = {
      ...this.state,
      hasInflightRequests:
        this.state.inflightMutations + this.state.inflightActions > 0,
      timeOfOldestInflightRequest:
        this.state.inflightMutations + this.state.inflightActions > 0
          ? this.state.timeOfOldestInflightRequest ?? new Date()
          : null,
    };
    this.emitConnectionState();
  }

  private ensureSocket() {
    if (this.closed) {
      return Promise.reject(new Error("Client is closed."));
    }
    if (this.socket && this.state.isWebSocketConnected) {
      return Promise.resolve();
    }
    if (this.socketPromise) {
      return this.socketPromise;
    }

    this.socketPromise = new Promise<void>((resolve, reject) => {
      const SocketImpl = this.webSocketImpl ?? (globalThis.WebSocket as WebSocketConstructor);
      if (!SocketImpl) {
        reject(new Error("No WebSocket implementation is available for this environment."));
        return;
      }
      const socket = new SocketImpl(websocketUrlFromBase(this.address), [
        NIMBUS_WEBSOCKET_PROTOCOL,
      ]);
      this.socket = socket;

      attachSocketListener(socket, "open", () => {
        void this.finishSocketOpen(socket, resolve, reject);
      });

      attachSocketListener(socket, "message", (event) => {
        this.handleSocketMessage(event.data);
      });

      attachSocketListener(socket, "close", () => {
        this.clearScheduledAuthRefresh();
        if (this.socketAuthentication?.socket === socket) {
          this.socketAuthentication.reject(
            new Error("convex websocket closed during authentication"),
          );
          this.socketAuthentication = null;
        }
        this.state = {
          ...this.state,
          isWebSocketConnected: false,
        };
        this.emitConnectionState();
        this.socket = null;
        this.socketPromise = null;
        this.requeueActiveSubscriptions();
        this.scheduleReconnect();
      });

      attachSocketListener(socket, "error", () => {
        this.state = {
          ...this.state,
          isWebSocketConnected: false,
          connectionRetries: this.state.connectionRetries + 1,
        };
        this.emitConnectionState();
        this.socketPromise = null;
        reject(new Error("convex websocket connection failed"));
      });
    });

    return this.socketPromise;
  }

  private handleSocketMessage(raw: string) {
    const message = JSON.parse(raw) as
      | { type: "hello"; protocol?: string }
      | { type: "fatal_error"; error?: { message?: string } }
      | { type: "authenticated"; is_authenticated: boolean }
      | {
          type: "subscription_result";
          subscription_id: number;
          request_id?: string;
          data: unknown;
        }
      | { type: "error"; error?: { message?: string } }
      | { type: "op.error"; id?: string; error?: { message?: string } };

    if (message.type === "hello") {
      return;
    }

    if (message.type === "fatal_error") {
      const error = new Error(
        message.error?.message ?? "nimbus websocket protocol negotiation failed",
      );
      if (this.socketAuthentication) {
        this.socketAuthentication.reject(error);
        this.socketAuthentication = null;
      }
      for (const active of this.activeSubscriptions.values()) {
        active.onError?.(error);
      }
      this.socket?.close();
      return;
    }

    if (message.type === "authenticated") {
      const authenticatedToken = this.socketAuthentication?.token;
      this.httpClient.notifyAuthState(message.is_authenticated);
      if (message.is_authenticated === false) {
        this.clearScheduledAuthRefresh();
        this.socketAuthentication?.resolve();
        this.socketAuthentication = null;
        return;
      }
      if (authenticatedToken) {
        this.scheduleAuthTokenRefresh(authenticatedToken);
      }
      this.socketAuthentication?.resolve();
      this.socketAuthentication = null;
      return;
    }

    if (message.type === "subscription_result") {
      if (message.request_id) {
        const pending = this.pendingSubscriptions.get(message.request_id);
        if (!pending || pending.unsubscribed) {
          return;
        }
        pending.subscriptionId = message.subscription_id;
        pending.pendingRequestId = undefined;
        const shouldNotify = !areSubscriptionValuesEqual(
          pending.currentValue,
          message.data,
        );
        pending.currentValue = message.data;
        this.pendingSubscriptions.delete(message.request_id);
        this.activeSubscriptions.set(message.subscription_id, pending);
        if (shouldNotify) {
          pending.callback(message.data);
        }
        return;
      }

      const active = this.activeSubscriptions.get(message.subscription_id);
      if (!active || active.unsubscribed) {
        return;
      }
      if (areSubscriptionValuesEqual(active.currentValue, message.data)) {
        active.currentValue = message.data;
        return;
      }
      active.currentValue = message.data;
      active.callback(message.data);
      return;
    }

    const requestId =
      message.type === "op.error" && typeof message.id === "string"
        ? message.id
        : undefined;
    const errorMessage =
      "error" in message && typeof message.error?.message === "string"
        ? message.error.message
        : null;

    if (message.type === "op.error" && requestId) {
      const pending = this.pendingSubscriptions.get(requestId);
      if (!pending || pending.unsubscribed) {
        return;
      }
      this.pendingSubscriptions.delete(requestId);
      pending.pendingRequestId = undefined;
      const error = new Error(errorMessage ?? "websocket request failed");
      pending.onError?.(error);
      return;
    }

    if (message.type === "error" || message.type === "op.error") {
      const error = new Error(errorMessage ?? "websocket request failed");
      if (this.socketAuthentication) {
        this.clearScheduledAuthRefresh();
        this.httpClient.notifyAuthState(false);
        this.socketAuthentication.reject(error);
        this.socketAuthentication = null;
        return;
      }
      for (const active of this.activeSubscriptions.values()) {
        active.onError?.(error);
      }
    }
  }

  private queueSubscription(entry: SubscriptionEntry<unknown>) {
    const requestId = `convex-${++this.requestCounter}`;
    entry.pendingRequestId = requestId;
    entry.subscriptionId = undefined;
    this.pendingSubscriptions.set(requestId, entry);
  }

  private flushPendingSubscriptions() {
    if (!this.socket || !this.state.isWebSocketConnected) {
      return;
    }

    for (const [requestId, entry] of this.pendingSubscriptions) {
      if (entry.unsubscribed || entry.pendingRequestId !== requestId) {
        continue;
      }
      this.socket.send(
          JSON.stringify(
          buildSubscribeMessage(entry.query, requestId, entry.args, {
            pageSize: entry.livePageSize,
            cursor: entry.liveCursor,
          }),
        ),
      );
    }
  }

  private requeueActiveSubscriptions() {
    if (this.activeSubscriptions.size === 0) {
      return;
    }

    const activeEntries = Array.from(this.activeSubscriptions.values());
    this.activeSubscriptions.clear();
    for (const entry of activeEntries) {
      if (entry.unsubscribed) {
        continue;
      }
      this.queueSubscription(entry);
    }
  }

  private scheduleReconnect() {
    if (
      this.closed ||
      this.reconnectTimer !== null ||
      this.socketPromise !== null ||
      (this.socket !== null && this.state.isWebSocketConnected) ||
      this.pendingSubscriptions.size === 0
    ) {
      return;
    }

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.ensureSocket().catch(() => {
        this.scheduleReconnect();
      });
    }, 50);
  }

  private async finishSocketOpen(
    socket: WebSocketLike,
    resolve: () => void,
    reject: (error: Error) => void,
  ) {
    try {
      this.sendClientHello(socket);
      await this.authenticateSocket(socket);
      if (this.socket !== socket) {
        return;
      }
      this.state = {
        ...this.state,
        isWebSocketConnected: true,
        hasEverConnected: true,
        connectionCount: this.state.connectionCount + 1,
      };
      this.emitConnectionState();
      this.socketPromise = null;
      this.flushPendingSubscriptions();
      resolve();
    } catch (error) {
      this.socketPromise = null;
      reject(error instanceof Error ? error : new Error(String(error)));
      socket.close();
    }
  }

  private sendClientHello(socket: WebSocketLike) {
    socket.send(
      JSON.stringify({
        type: "client_hello",
        protocol: NIMBUS_WEBSOCKET_PROTOCOL,
        client: {
          kind: "browser",
          version: "unknown",
        },
        capabilities: [...NIMBUS_CLIENT_CAPABILITIES],
      }),
    );
  }

  private async authenticateSocket(socket: WebSocketLike) {
    const token = await this.httpClient.getAuthToken(false);
    if (token === null) {
      this.httpClient.notifyAuthState(false);
      return;
    }

    try {
      await this.sendAuthenticate(socket, token);
    } catch (_error) {
      const refreshedToken = await this.httpClient.getAuthToken(true);
      if (refreshedToken === null) {
        this.httpClient.notifyAuthState(false);
        return;
      }
      try {
        await this.sendAuthenticate(socket, refreshedToken);
      } catch (_retryError) {
        this.httpClient.notifyAuthState(false);
      }
    }
  }

  private sendAuthenticate(socket: WebSocketLike, token: string) {
    return new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        if (this.socketAuthentication?.socket === socket) {
          this.socketAuthentication = null;
        }
        reject(new Error("convex websocket authentication timed out"));
      }, 2000);
      this.socketAuthentication = { socket, token, resolve, reject };
      socket.send(
        JSON.stringify({
          type: "authenticate",
          token,
        }),
      );
      this.socketAuthentication = {
        socket,
        token,
        resolve: () => {
          clearTimeout(timeout);
          resolve();
        },
        reject: (error) => {
          clearTimeout(timeout);
          reject(error);
        },
      };
    });
  }

  private clearScheduledAuthRefresh() {
    if (this.authRefreshTimer !== null) {
      clearTimeout(this.authRefreshTimer);
      this.authRefreshTimer = null;
    }
  }

  private scheduleAuthTokenRefresh(token: string) {
    this.clearScheduledAuthRefresh();
    if (!this.httpClient.canRefreshAuthToken()) {
      return;
    }
    const decoded = decodeJwtPayload(token);
    if (!decoded) {
      return;
    }
    const iat = typeof decoded.iat === "number" ? decoded.iat : null;
    const exp = typeof decoded.exp === "number" ? decoded.exp : null;
    if (iat === null || exp === null) {
      return;
    }
    const tokenValiditySeconds = exp - iat;
    if (tokenValiditySeconds <= 2) {
      return;
    }
    let delay = Math.min(
      MAXIMUM_REFRESH_DELAY,
      (tokenValiditySeconds - this.authRefreshTokenLeewaySeconds) * 1000,
    );
    if (delay <= 0) {
      delay = 0;
    }
    this.authRefreshTimer = setTimeout(() => {
      this.authRefreshTimer = null;
      void this.refetchSocketAuthToken();
    }, delay);
  }

  private async refetchSocketAuthToken() {
    if (!this.httpClient.canRefreshAuthToken()) {
      return;
    }
    const socket = this.socket;
    if (!socket || !this.state.isWebSocketConnected) {
      return;
    }
    const token = await this.httpClient.getAuthToken(true);
    if (token === null) {
      this.httpClient.notifyAuthState(false);
      this.restartSocketForAuthChange();
      return;
    }
    try {
      await this.sendAuthenticate(socket, token);
    } catch {
      this.httpClient.notifyAuthState(false);
      this.restartSocketForAuthChange();
    }
  }

  private restartSocketForAuthChange() {
    this.clearScheduledAuthRefresh();
    if (this.socket) {
      this.socket.close();
      return;
    }
    this.scheduleReconnect();
  }
}

export class NimbusReactClient extends NimbusClient {}
