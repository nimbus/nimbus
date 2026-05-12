import path from "node:path";
import { spawnSync } from "node:child_process";

function cliName(baseName) {
  return process.platform === "win32" ? `${baseName}.cmd` : baseName;
}

function parseSmokeResult(text) {
  const trimmed = String(text ?? "").trim();
  if (!trimmed) {
    return null;
  }
  return JSON.parse(trimmed);
}

globalThis.__nimbusInvoke = function () {
  const appRoot = process.cwd();
  const hostNodeBin = path.join(
    appRoot,
    "node_modules",
    "nimbus-host-node",
    "bin",
    cliName("node"),
  );
  const nextEntrypoint = path.join(appRoot, "node_modules", "next", "dist", "bin", "next");
  const nextAppRoot = path.join(appRoot, "fixtures", "next-app");
  const smokeScript = path.join(appRoot, "fixtures", "next", "smoke.mjs");
  const env = {
    ...process.env,
    NEXT_TELEMETRY_DISABLED: "1",
  };

  const build = spawnSync(hostNodeBin, [nextEntrypoint, "build", nextAppRoot], {
    cwd: appRoot,
    env,
    encoding: "utf8",
  });
  const smoke = build.status === 0
    ? spawnSync(hostNodeBin, [smokeScript, nextAppRoot], {
        cwd: appRoot,
        env,
        encoding: "utf8",
      })
    : null;

  return {
    buildStatus: build.status ?? null,
    buildErrorCode: build.error?.code ?? null,
    buildStdout: String(build.stdout ?? "").trim(),
    buildStderr: String(build.stderr ?? "").trim(),
    smokeStatus: smoke?.status ?? null,
    smokeErrorCode: smoke?.error?.code ?? null,
    smokeStdout: String(smoke?.stdout ?? "").trim(),
    smokeStderr: String(smoke?.stderr ?? "").trim(),
    smokeResult: smoke?.status === 0
      ? parseSmokeResult(smoke.stdout ?? "")
      : null,
  };
};

export {};
