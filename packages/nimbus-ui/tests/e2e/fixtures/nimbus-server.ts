import { test as base } from "@playwright/test";
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

export interface LocalAdminTokenRecord {
  version: number;
  token: string;
  generation: number;
  issuedAt: string;
  scope: string;
}

export interface NimbusServer {
  baseURL: string;
  port: number;
  scratchDir: string;
  tokenPath: string;
  discoveryPath: string;
  readToken: () => string;
  readTokenRecord: () => LocalAdminTokenRecord;
  hasExited: () => boolean;
  waitForExit: (timeoutMs: number) => Promise<boolean>;
  pid: number;
}

interface ScratchEnv {
  env: NodeJS.ProcessEnv;
  tokenPath: string;
  discoveryPath: string;
}

const DEFAULT_NIMBUS_BIN = "../../target/debug/nimbus";
const READINESS_TIMEOUT_MS = 60_000;
const READINESS_POLL_MS = 100;
const UNIX_GRACEFUL_SHUTDOWN_MS = 5_000;

function buildScratchEnv(scratchDir: string): ScratchEnv {
  if (process.platform === "darwin") {
    const home = scratchDir;
    const appSupport = join(home, "Library", "Application Support", "nimbus");
    const macTmp = join(scratchDir, "tmp");
    return {
      env: { HOME: home, TMPDIR: macTmp },
      tokenPath: join(appSupport, "auth", "token"),
      discoveryPath: join(macTmp, "nimbus", "server.json"),
    };
  }
  if (process.platform === "win32") {
    const localAppData = join(scratchDir, "AppData", "Local");
    return {
      env: { LOCALAPPDATA: localAppData, USERPROFILE: scratchDir },
      tokenPath: join(localAppData, "nimbus", "auth", "token.json"),
      discoveryPath: join(localAppData, "nimbus", "run", "server.json"),
    };
  }
  const xdgData = join(scratchDir, "xdg-data");
  const xdgState = join(scratchDir, "xdg-state");
  const xdgRuntime = join(scratchDir, "xdg-runtime");
  return {
    env: {
      HOME: scratchDir,
      XDG_DATA_HOME: xdgData,
      XDG_STATE_HOME: xdgState,
      XDG_RUNTIME_DIR: xdgRuntime,
    },
    tokenPath: join(xdgData, "nimbus", "auth", "token"),
    discoveryPath: join(xdgRuntime, "nimbus", "server.json"),
  };
}

function buildChildEnv(scratchEnv: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const inherited: NodeJS.ProcessEnv = { ...process.env };
  for (const key of [
    "HOME",
    "TMPDIR",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "LOCALAPPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
  ]) {
    delete inherited[key];
  }
  return { ...inherited, ...scratchEnv, NIMBUS_E2E: "1" };
}

async function allocateFreePort(): Promise<number> {
  return new Promise<number>((resolve, reject) => {
    const probe = createServer();
    probe.unref();
    probe.on("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const address = probe.address();
      if (address === null || typeof address === "string") {
        probe.close();
        reject(new Error("net.createServer did not return an AddressInfo"));
        return;
      }
      const { port } = address;
      probe.close(() => resolve(port));
    });
  });
}

async function waitForReady(url: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(url);
      if (res.status === 200) {
        return;
      }
      lastError = new Error(`status ${res.status}`);
    } catch (err) {
      lastError = err;
    }
    await new Promise((resolve) => setTimeout(resolve, READINESS_POLL_MS));
  }
  throw new Error(
    `server did not become ready at ${url} within ${timeoutMs}ms: ${
      lastError instanceof Error ? lastError.message : String(lastError)
    }`,
  );
}

async function terminateProcess(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  const pid = child.pid;
  if (pid === undefined) {
    return;
  }
  if (process.platform === "win32") {
    try {
      child.kill();
    } catch {
      // process may already be gone
    }
    await new Promise<void>((resolve) => {
      const taskkill = spawn("taskkill", ["/T", "/F", "/PID", String(pid)], {
        stdio: "ignore",
      });
      const done = () => resolve();
      taskkill.once("exit", done);
      taskkill.once("error", done);
      setTimeout(done, 3_000);
    });
    return;
  }
  try {
    process.kill(-pid, "SIGTERM");
  } catch {
    try {
      child.kill("SIGTERM");
    } catch {
      // process may already be gone
    }
  }
  const exited = await Promise.race([
    new Promise<boolean>((resolve) => {
      child.once("exit", () => resolve(true));
    }),
    new Promise<boolean>((resolve) =>
      setTimeout(() => resolve(false), UNIX_GRACEFUL_SHUTDOWN_MS),
    ),
  ]);
  if (exited) {
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    try {
      child.kill("SIGKILL");
    } catch {
      // process may already be gone
    }
  }
  await new Promise((resolve) => setTimeout(resolve, 250));
}

function readTokenRecordFile(tokenPath: string): LocalAdminTokenRecord {
  const raw = readFileSync(tokenPath, "utf8").trim();
  if (!raw.startsWith("{")) {
    throw new Error(
      `token file ${tokenPath} is not a JSON record (got ${raw.slice(0, 32)}...)`,
    );
  }
  const parsed = JSON.parse(raw) as Partial<LocalAdminTokenRecord>;
  if (typeof parsed.token !== "string" || !parsed.token) {
    throw new Error(`token file ${tokenPath} has no .token field`);
  }
  if (typeof parsed.generation !== "number") {
    throw new Error(`token file ${tokenPath} has no .generation field`);
  }
  return {
    version: parsed.version ?? 0,
    token: parsed.token,
    generation: parsed.generation,
    issuedAt: parsed.issuedAt ?? "",
    scope: parsed.scope ?? "",
  };
}

function readTokenFile(tokenPath: string): string {
  return readTokenRecordFile(tokenPath).token;
}

type NimbusServerFixtures = {
  nimbusServer: NimbusServer;
};

export const test = base.extend<NimbusServerFixtures>({
  baseURL: async ({ nimbusServer }, run) => {
    await run(nimbusServer.baseURL);
  },
  nimbusServer: async ({}, run) => {
    const nimbusBinRaw = process.env.NIMBUS_E2E_BIN ?? DEFAULT_NIMBUS_BIN;
    const nimbusBin = isAbsolute(nimbusBinRaw)
      ? nimbusBinRaw
      : resolve(process.cwd(), nimbusBinRaw);
    const scratchDir = mkdtempSync(join(tmpdir(), "nimbus-e2e-"));
    const { env: scratchEnv, tokenPath, discoveryPath } =
      buildScratchEnv(scratchDir);
    const port = await allocateFreePort();
    const baseURL = `http://127.0.0.1:${port}`;
    const childEnv = buildChildEnv(scratchEnv);

    const child = spawn(
      nimbusBin,
      ["start", "--host", "127.0.0.1", "--port", String(port)],
      {
        cwd: scratchDir,
        env: childEnv,
        stdio: ["ignore", "pipe", "pipe"],
        detached: process.platform !== "win32",
      },
    );

    const logChunks: string[] = [];
    child.stdout?.on("data", (chunk) => {
      logChunks.push(chunk.toString("utf8"));
    });
    child.stderr?.on("data", (chunk) => {
      logChunks.push(chunk.toString("utf8"));
    });

    let earlyExitCode: number | null = null;
    let earlyExitSignal: NodeJS.Signals | null = null;
    let exited = false;
    child.once("exit", (code, signal) => {
      earlyExitCode = code;
      earlyExitSignal = signal;
      exited = true;
    });

    try {
      await waitForReady(`${baseURL}/ui/auth`, READINESS_TIMEOUT_MS);
    } catch (err) {
      await terminateProcess(child);
      try {
        rmSync(scratchDir, { recursive: true, force: true });
      } catch {
        // best-effort cleanup
      }
      const earlyExit =
        earlyExitCode !== null || earlyExitSignal !== null
          ? ` (nimbus exited code=${earlyExitCode} signal=${earlyExitSignal})`
          : "";
      throw new Error(
        `disposable nimbus server failed to start at ${baseURL}${earlyExit}: ${
          err instanceof Error ? err.message : String(err)
        }\n--- nimbus logs ---\n${logChunks.join("")}`,
      );
    }

    const handle: NimbusServer = {
      baseURL,
      port,
      scratchDir,
      tokenPath,
      discoveryPath,
      readToken: () => readTokenFile(tokenPath),
      readTokenRecord: () => readTokenRecordFile(tokenPath),
      hasExited: () => exited,
      waitForExit: (timeoutMs: number) =>
        Promise.race<boolean>([
          new Promise<boolean>((resolveExit) => {
            if (exited) {
              resolveExit(true);
              return;
            }
            child.once("exit", () => resolveExit(true));
          }),
          new Promise<boolean>((resolveExit) =>
            setTimeout(() => resolveExit(exited), timeoutMs),
          ),
        ]),
      pid: child.pid ?? 0,
    };

    try {
      await run(handle);
    } finally {
      await terminateProcess(child);
      if (existsSync(scratchDir)) {
        try {
          rmSync(scratchDir, { recursive: true, force: true });
        } catch {
          // best-effort cleanup
        }
      }
    }
  },
});

export { expect } from "@playwright/test";
