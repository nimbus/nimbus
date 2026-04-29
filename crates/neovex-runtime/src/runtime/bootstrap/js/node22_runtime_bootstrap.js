import { core } from "ext:core/mod.js";
import { errors } from "ext:runtime/01_errors.js";
import { windowOrWorkerGlobalScope } from "ext:runtime/98_global_scope_shared.js";
import {
  denoGlobals as hiddenDenoGlobals,
  nodeGlobals as hiddenNodeGlobals,
} from "ext:neovex_node22/internal_bootstrap.js";

import "ext:deno_fetch/20_headers.js";
import "ext:deno_fetch/22_http_client.js";
import "ext:deno_fetch/23_request.js";
import "ext:deno_fetch/23_response.js";
import "ext:deno_fetch/26_fetch.js";
import "ext:deno_fetch/27_eventsource.js";
import "ext:deno_http/00_serve.ts";
import "ext:deno_http/01_http.js";
import "ext:deno_http/02_websocket.ts";
import "ext:deno_net/01_net.js";
import "ext:deno_net/02_tls.js";
import "ext:deno_os/40_signals.js";
import "ext:deno_web/01_urlpattern.js";
import "ext:deno_web/04_global_interfaces.js";
import "ext:deno_web/10_filereader.js";
import "ext:deno_web/12_location.js";
import "ext:deno_web/16_image_data.js";
import "ext:deno_websocket/01_websocket.js";
import "ext:deno_websocket/02_websocketstream.js";

Object.defineProperties(globalThis, windowOrWorkerGlobalScope);

function runtimeFsPathToString(path) {
  if (typeof path === "string") {
    return path;
  }
  if (path instanceof URL) {
    if (path.protocol !== "file:") {
      throw new TypeError(`Neovex only supports file: URLs for Deno fs APIs; received ${path.href}`);
    }
    return decodeURIComponent(path.pathname.replace(/^\/([A-Za-z]:)/, "$1"));
  }
  return String(path);
}

function runtimeFsMapThrownError(error) {
  const hostError = error?.neovexHostError;
  if (!hostError || typeof hostError !== "object") {
    return error;
  }
  const message =
    typeof hostError.message === "string" && hostError.message.length > 0
      ? hostError.message
      : String(error?.message ?? "unknown filesystem error");
  let mappedError;
  switch (hostError.code) {
    case "ENOENT":
      mappedError = new errors.NotFound(message);
      break;
    case "EEXIST":
      mappedError = new errors.AlreadyExists(message);
      break;
    case "EACCES":
    case "EPERM":
      mappedError = new errors.PermissionDenied(message);
      break;
    case "EINVAL":
      mappedError = new TypeError(message);
      break;
    default:
      mappedError = new Error(message);
      break;
  }
  mappedError.code = hostError.code;
  mappedError.neovexHostError = hostError;
  return mappedError;
}

function toFileInfo(value) {
  return {
    isFile: value?.isFile === true,
    isDirectory: value?.isDirectory === true,
    isSymlink: value?.isSymlink === true,
    size: Number(value?.size ?? 0),
    mtime: value?.mtimeMs == null ? null : new Date(value.mtimeMs),
    atime: value?.atimeMs == null ? null : new Date(value.atimeMs),
    birthtime: value?.birthtimeMs == null ? null : new Date(value.birthtimeMs),
    ctime: value?.ctimeMs == null ? null : new Date(value.ctimeMs),
    mode: value?.mode ?? null,
    dev: null,
    ino: null,
    nlink: null,
    uid: null,
    gid: null,
    rdev: null,
    blksize: null,
    blocks: null,
    isBlockDevice: false,
    isCharDevice: false,
    isFifo: false,
    isSocket: false,
  };
}

function toDirEntry(value) {
  return {
    name: String(value?.name ?? ""),
    isFile: value?.isFile === true,
    isDirectory: value?.isDirectory === true,
    isSymlink: value?.isSymlink === true,
  };
}

async function runtimeFsStat(path, followSymlink) {
  try {
    const value = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_stat", {
      path: runtimeFsPathToString(path),
      follow_symlink: followSymlink,
    });
    return toFileInfo(value);
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsStatSync(path, followSymlink) {
  try {
    const value = globalThis.__neovexSyncHostValue("op_neovex_runtime_stat_sync", {
      path: runtimeFsPathToString(path),
      follow_symlink: followSymlink,
    });
    return toFileInfo(value);
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsMkdir(path, options = undefined) {
  const normalizedOptions = options && typeof options === "object" ? options : {};
  try {
    await globalThis.__neovexAsyncHostValue("op_neovex_runtime_mkdir", {
      path: runtimeFsPathToString(path),
      recursive: normalizedOptions.recursive === true,
      mode:
        typeof normalizedOptions.mode === "number" ? normalizedOptions.mode : null,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsMkdirSync(path, options = undefined) {
  const normalizedOptions = options && typeof options === "object" ? options : {};
  try {
    globalThis.__neovexSyncHostValue("op_neovex_runtime_mkdir_sync", {
      path: runtimeFsPathToString(path),
      recursive: normalizedOptions.recursive === true,
      mode:
        typeof normalizedOptions.mode === "number" ? normalizedOptions.mode : null,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function* runtimeFsReadDir(path) {
  let entries;
  try {
    entries = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_read_dir", {
      path: runtimeFsPathToString(path),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
  for (const entry of entries ?? []) {
    yield toDirEntry(entry);
  }
}

function runtimeFsReadDirSync(path) {
  try {
    const entries = globalThis.__neovexSyncHostValue("op_neovex_runtime_read_dir_sync", {
      path: runtimeFsPathToString(path),
    });
    return (entries ?? []).map(toDirEntry).values();
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeNodeArch() {
  const buildArch = typeof core.build?.arch === "string" && core.build.arch.length > 0
    ? core.build.arch
    : "";
  switch (buildArch) {
    case "x86_64":
      return "x64";
    case "aarch64":
      return "arm64";
    case "riscv64gc":
      return "riscv64";
    case "x86":
    case "i686":
      return "ia32";
    default:
      return buildArch;
  }
}

function runtimeNodePlatform() {
  const buildOs = typeof core.build?.os === "string" && core.build.os.length > 0
    ? core.build.os
    : "";
  switch (buildOs) {
    case "macos":
      return "darwin";
    case "windows":
      return "win32";
    default:
      return buildOs;
  }
}

function seedNodeProcessPlatformMetadata(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

  const nodePlatform = runtimeNodePlatform();
  if (nodePlatform.length > 0 && nodeProcess.platform !== nodePlatform) {
    nodeProcess.platform = nodePlatform;
  }

  const nodeArch = runtimeNodeArch();
  if (nodeArch.length > 0 && nodeProcess.arch !== nodeArch) {
    // Neovex does not run Deno's full nodeBootstrap() sequence because that
    // path assumes CLI-owned stdio, argv, and process wiring that the embedded
    // runtime does not expose. Seed the minimal platform metadata that Node
    // packages such as esbuild require instead of pretending the full CLI
    // bootstrap contract exists.
    Object.defineProperty(nodeProcess, "arch", {
      value: nodeArch,
      configurable: true,
      enumerable: true,
      writable: false,
    });
  }
}

const deno = hiddenDenoGlobals;
const internalSymbol = deno.internal ?? Symbol("Deno.internal");
const internals = deno[internalSymbol] ?? {};
if (internals.nodeGlobals === undefined) {
  internals.nodeGlobals = hiddenNodeGlobals;
}
Object.defineProperty(deno, "internal", {
  value: internalSymbol,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(deno, internalSymbol, {
  value: internals,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(deno, "core", {
  value: core,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(deno, "errors", {
  value: errors,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "args", {
  value: [],
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "build", {
  value: core.build,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "cwd", {
  value() {
    return globalThis.process?.cwd?.() ?? "/";
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "stat", {
  value(path) {
    return runtimeFsStat(path, true);
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "statSync", {
  value(path) {
    return runtimeFsStatSync(path, true);
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "lstat", {
  value(path) {
    return runtimeFsStat(path, false);
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "lstatSync", {
  value(path) {
    return runtimeFsStatSync(path, false);
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "mkdir", {
  value: runtimeFsMkdir,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "mkdirSync", {
  value: runtimeFsMkdirSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "readDir", {
  value: runtimeFsReadDir,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "readDirSync", {
  value: runtimeFsReadDirSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "execPath", {
  value() {
    return core.ops.op_neovex_runtime_exec_path();
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "env", {
  value: {
    get(name) {
      return globalThis.process?.env?.[name];
    },
    toObject() {
      return { ...(globalThis.process?.env ?? {}) };
    },
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "version", {
  value: {
    deno: "2.7.14-neovex",
    v8: "147.4.0-locker.1",
    typescript: "0.0.0-neovex",
  },
  configurable: true,
  enumerable: true,
  writable: false,
});

const runtimeTargetTriple = core.ops.op_neovex_runtime_target_triple();
if (typeof runtimeTargetTriple === "string" && runtimeTargetTriple.length > 0) {
  core.setBuildInfo(runtimeTargetTriple);
}

seedNodeProcessPlatformMetadata(internals.nodeGlobals?.process);
seedNodeProcessPlatformMetadata(globalThis.process);
if (
  internals.nodeGlobals?.process
  && typeof internals.nodeGlobals.process === "object"
  && globalThis.process !== internals.nodeGlobals.process
) {
  globalThis.process = internals.nodeGlobals.process;
}

if (typeof internals.requireImpl?.setUsesLocalNodeModulesDir === "function") {
  internals.requireImpl.setUsesLocalNodeModulesDir();
}
delete globalThis.nodeBootstrap;

export {};
