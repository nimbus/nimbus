import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";

globalThis.__nimbusInvoke = function () {
  mkdirSync("./node_modules/.prisma/client", { recursive: true });
  writeFileSync("./node_modules/.prisma/client/query_engine.node", "not a prisma engine");
  const require = createRequire(import.meta.url);
  try {
    require("./node_modules/.prisma/client/query_engine.node");
    return {
      surface: "prisma_engine",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: null,
    };
  } catch (error) {
    return {
      surface: "prisma_engine",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: error?.message ?? String(error),
    };
  }
};

export {};
