globalThis.__nimbusInvoke = async function () {
  try {
    const sharp = await import("sharp");
    await sharp.default(Buffer.from([0x89, 0x50, 0x4e, 0x47])).metadata();
    return {
      surface: "sharp_native",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: null,
    };
  } catch (error) {
    return {
      surface: "sharp_native",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: error?.message ?? String(error),
    };
  }
};

export {};
