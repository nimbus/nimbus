import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";

globalThis.__nimbusInvoke = function () {
  writeFileSync("./native-addon.node", "not a native addon");
  const require = createRequire(import.meta.url);
  try {
    require("./native-addon.node");
    return {
      surface: "native_addon",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: null,
    };
  } catch (error) {
    return {
      surface: "native_addon",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: error?.message ?? String(error),
      deniedCode: error?.code ?? null,
    };
  }
};

export {};
