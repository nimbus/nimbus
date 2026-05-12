import path from "node:path";
import { spawnSync } from "node:child_process";

function cliName(baseName) {
  return process.platform === "win32" ? `${baseName}.cmd` : baseName;
}

function includesToken(result, token) {
  return String(result.stderr ?? "").includes(token)
    || String(result.stdout ?? "").includes(token);
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
  const tsxEntrypoint = path.join(appRoot, "node_modules", "tsx", "dist", "cli.mjs");
  const success = spawnSync(
    hostNodeBin,
    [tsxEntrypoint, path.join(appRoot, "fixtures", "tsx", "success.ts")],
    {
      cwd: appRoot,
      encoding: "utf8",
    },
  );
  const failure = spawnSync(
    hostNodeBin,
    [tsxEntrypoint, path.join(appRoot, "fixtures", "tsx", "failure.ts")],
    {
      cwd: appRoot,
      encoding: "utf8",
    },
  );

  return {
    successStatus: success.status ?? null,
    successStdout: String(success.stdout ?? "").trim(),
    successStderr: String(success.stderr ?? "").trim(),
    successErrorCode: success.error?.code ?? null,
    failureStatus: failure.status ?? null,
    failureStderr: String(failure.stderr ?? "").trim(),
    failureErrorCode: failure.error?.code ?? null,
    failureHasToken: includesToken(failure, "tsx-canary-boom"),
  };
};

export {};
