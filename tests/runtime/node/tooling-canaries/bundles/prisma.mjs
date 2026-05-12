import path from "node:path";
import { spawnSync } from "node:child_process";

function cliName(baseName) {
  return process.platform === "win32" ? `${baseName}.cmd` : baseName;
}

function run(command, args, options) {
  return spawnSync(command, args, {
    encoding: "utf8",
    ...options,
  });
}

function outputText(result) {
  return `${String(result.stdout ?? "")}\n${String(result.stderr ?? "")}`;
}

function prismaBoundaryToken(text) {
  const tokens = [
    'Using engine type "client" requires either "adapter" or "accelerateUrl"',
    "Prisma Client could not locate the Query Engine",
    "Query engine library for current platform",
    "Unable to require",
    "Node-API library",
    "native addon",
  ];
  for (const token of tokens) {
    if (text.includes(token)) {
      return token;
    }
  }
  return null;
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
  const prismaEntrypoint = path.join(appRoot, "node_modules", "prisma", "build", "index.js");
  const prismaConfigPath = path.join(appRoot, "fixtures", "prisma", "prisma.config.ts");
  const smokeScript = path.join(appRoot, "fixtures", "prisma", "smoke.mjs");
  const databasePath = path.join(appRoot, ".nimbus", "tmp", "prisma-canary.db");
  const env = {
    ...process.env,
    DATABASE_URL: `file:${databasePath}`,
  };

  const validate = run(hostNodeBin, [prismaEntrypoint, "validate", "--config", prismaConfigPath], {
    cwd: appRoot,
    env,
  });
  const generate = run(hostNodeBin, [prismaEntrypoint, "generate", "--config", prismaConfigPath], {
    cwd: appRoot,
    env,
  });
  const push = generate.status === 0
    ? run(hostNodeBin, [prismaEntrypoint, "db", "push", "--config", prismaConfigPath], {
        cwd: appRoot,
        env,
      })
    : null;
  const smoke = push && push.status === 0
    ? run(hostNodeBin, [smokeScript], {
        cwd: appRoot,
        env,
      })
    : null;

  if (
    validate.status === 0
    && generate.status === 0
    && push?.status === 0
    && smoke?.status === 0
  ) {
    return {
      mode: "success",
      validateStatus: validate.status ?? null,
      generateStatus: generate.status ?? null,
      pushStatus: push.status ?? null,
      smokeStatus: smoke.status ?? null,
      smokeResult: JSON.parse(String(smoke.stdout ?? "").trim()),
    };
  }

  const steps = [
    ["validate", validate],
    ["generate", generate],
    ["push", push],
    ["smoke", smoke],
  ];
  for (const [step, result] of steps) {
    if (!result || result.status === 0) {
      continue;
    }
    const boundaryToken = prismaBoundaryToken(outputText(result));
    return {
      mode: "documented-boundary",
      step,
      status: result.status ?? null,
      errorCode: result.error?.code ?? null,
      output: outputText(result),
      boundaryToken,
    };
  }

  return {
    mode: "documented-boundary",
    step: "unknown",
    status: null,
    errorCode: null,
    output: "",
    boundaryToken: null,
  };
};

export {};
