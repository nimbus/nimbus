import { WebSocketServer } from "ws";

// Production-profile negative: a raw `ws` WebSocketServer cannot bind a
// listening socket because the production Node profile grants no net_listen.
// The denial surfaces as the same "Requires net access" gate as a plain
// http.Server.listen (see raw-server-listen.mjs); a persistent WebSocket
// server belongs on the sandbox-backed service/microVM surface instead.
function listenDenied() {
  return new Promise((resolve) => {
    const timeout = setTimeout(() => resolve(null), 1000);
    try {
      const server = new WebSocketServer(
        { host: "127.0.0.1", port: 0 },
        () => {
          clearTimeout(timeout);
          try {
            server.close();
          } catch (_error) {
            // ignore
          }
          resolve(null);
        },
      );
      server.once("error", (error) => {
        clearTimeout(timeout);
        resolve(error?.message ?? String(error));
      });
    } catch (error) {
      clearTimeout(timeout);
      resolve(error?.message ?? String(error));
    }
  });
}

globalThis.__nimbusInvoke = async function () {
  let denied = null;
  try {
    denied = await listenDenied();
  } catch (error) {
    denied = error?.message ?? String(error);
  }
  return {
    surface: "ws_server_listen",
    supportStatus: "service_microvm_required",
    diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
    denied,
  };
};

export {};
