import http from "node:http";

function listenDenied(server) {
  return new Promise((resolve) => {
    const timeout = setTimeout(() => resolve(null), 1000);
    server.once("error", (error) => {
      clearTimeout(timeout);
      resolve(error?.message ?? String(error));
    });
    try {
      server.listen(0, "127.0.0.1", () => {
        clearTimeout(timeout);
        resolve(null);
      });
    } catch (error) {
      clearTimeout(timeout);
      resolve(error?.message ?? String(error));
    }
  });
}

globalThis.__nimbusInvoke = async function () {
  const server = http.createServer((_request, response) => {
    response.end("unexpected");
  });
  let denied = null;
  try {
    denied = await listenDenied(server);
  } catch (error) {
    denied = error?.message ?? String(error);
  }
  try {
    server.close();
  } catch (_error) {
  }
  return {
    surface: "raw_server_listen",
    supportStatus: "service_microvm_required",
    diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
    denied,
  };
};

export {};
