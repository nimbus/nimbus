import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

globalThis.__nimbusInvoke = function () {
  const bundleDir = dirname(fileURLToPath(import.meta.url));
  const engineDir = join(bundleDir, "node_modules/.prisma/client");
  const enginePath = join(engineDir, "query_engine.node");
  const require = createRequire(import.meta.url);
  try {
    mkdirSync(engineDir, { recursive: true });
    writeFileSync(enginePath, "not a prisma engine");
    require(enginePath);
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
