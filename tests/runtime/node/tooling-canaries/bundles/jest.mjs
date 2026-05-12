import path from "node:path";
import { spawnSync } from "node:child_process";

function cliName(baseName) {
  return process.platform === "win32" ? `${baseName}.cmd` : baseName;
}

function outputText(result) {
  return `${String(result.stdout ?? "")}\n${String(result.stderr ?? "")}`;
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
  const jestEntrypoint = path.join(appRoot, "node_modules", "jest", "bin", "jest.js");
  const config = path.join(appRoot, "fixtures", "jest", "jest.config.cjs");
  const success = spawnSync(
    hostNodeBin,
    [
      jestEntrypoint,
      "--runInBand",
      "--colors=false",
      "--config",
      config,
      "--runTestsByPath",
      path.join(appRoot, "fixtures", "jest", "pass.test.cjs"),
    ],
    {
      cwd: appRoot,
      encoding: "utf8",
    },
  );
  const failure = spawnSync(
    hostNodeBin,
    [
      jestEntrypoint,
      "--runInBand",
      "--colors=false",
      "--config",
      config,
      "--runTestsByPath",
      path.join(appRoot, "fixtures", "jest", "fail.test.cjs"),
    ],
    {
      cwd: appRoot,
      encoding: "utf8",
    },
  );
  const successOutput = outputText(success);
  const failureOutput = outputText(failure);

  return {
    successStatus: success.status ?? null,
    successErrorCode: success.error?.code ?? null,
    successHasPassToken: successOutput.includes("PASS"),
    successHasTestName: successOutput.includes("jest canary pass"),
    successOutput,
    failureStatus: failure.status ?? null,
    failureErrorCode: failure.error?.code ?? null,
    failureHasFailToken: failureOutput.includes("FAIL"),
    failureHasTestName: failureOutput.includes("jest canary fail"),
    failureOutput,
  };
};

export {};
