globalThis.__nimbusInvoke = async function () {
  try {
    const inspector = await import("node:inspector");
    inspector.open(0, undefined, false);
    inspector.close?.();
    return {
      surface: "inspector",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: null,
    };
  } catch (error) {
    return {
      surface: "inspector",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: error?.message ?? String(error),
    };
  }
};

export {};
