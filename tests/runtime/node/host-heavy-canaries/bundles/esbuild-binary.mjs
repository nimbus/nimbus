globalThis.__nimbusInvoke = async function () {
  try {
    const esbuild = await import("esbuild");
    await esbuild.transform("const value = 1 + 1", { loader: "js" });
    return {
      surface: "esbuild_binary",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: null,
    };
  } catch (error) {
    return {
      surface: "esbuild_binary",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: error?.message ?? String(error),
    };
  }
};

export {};
