import { mkdir, readFile, writeFile } from "node:fs/promises";

async function capture(operation) {
  try {
    await operation();
    return null;
  } catch (error) {
    return error?.message ?? String(error);
  }
}

globalThis.__nimbusInvoke = async function () {
  const absoluteWriteDenied = await capture(() =>
    writeFile("/tmp/nimbus-host-heavy-persistent.txt", "unexpected", "utf8")
  );
  const parentEscapeDenied = await capture(() =>
    mkdir("../persistent-cache", { recursive: true })
  );
  const absoluteReadDenied = await capture(() =>
    readFile("/etc/passwd", "utf8")
  );
  return {
    surface: "persistent_filesystem",
    supportStatus: "service_microvm_required",
    diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
    denied: [absoluteWriteDenied, parentEscapeDenied, absoluteReadDenied]
      .filter(Boolean)
      .join(" | "),
    absoluteWriteDenied,
    parentEscapeDenied,
    absoluteReadDenied,
  };
};

export {};
