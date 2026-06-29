import { core, internals as coreInternals, primordials } from "ext:core/mod.js";
import { op_runtime_memory_usage, op_stream_base_register_state } from "ext:core/ops";
import { errors } from "ext:runtime/01_errors.js";
import { windowOrWorkerGlobalScope } from "ext:runtime/98_global_scope_shared.js";
import {
  denoGlobals as hiddenDenoGlobals,
  nodeGlobals as hiddenNodeGlobals,
} from "ext:nimbus_node22/internal_bootstrap.js";

import nimbusPerfHooksBuiltin from "ext:nimbus_node22/perf_hooks_impl.js";
import { createWritableStdioStream, initStdin } from "ext:deno_node/_process/streams.mjs";
import { FileHandle as nodeInternalFsFileHandle } from "ext:deno_node/internal/fs/handle.ts";
import {
  Dirent as nodeFsDirent,
  constants as nodeFsUtilConstants,
  getValidatedPathToString as nodeFsGetValidatedPathToString,
  getOptions as nodeFsGetOptions,
  toUnixTimestamp as nodeFsToUnixTimestamp,
} from "ext:deno_node/internal/fs/utils.mjs";
import { getBinding as getNodeInternalBinding } from "ext:deno_node/internal_binding/mod.ts";
import { Buffer as nodeBuffer } from "node:buffer";
import {
  isAbsolute as nodePathIsAbsolute,
  relative as nodePathRelative,
  resolve as nodePathResolve,
} from "node:path";
import nodeProcessBuiltin from "node:process";
import { StringDecoder as nodeStringDecoder } from "node:string_decoder";
import * as nodeTimersBuiltin from "node:timers";
import "ext:deno_websocket/01_websocket.js";
import "ext:deno_websocket/02_websocketstream.js";

core.loadExtScript("ext:deno_fetch/20_headers.js");
core.loadExtScript("ext:deno_fetch/22_http_client.js");
core.loadExtScript("ext:deno_fetch/23_request.js");
core.loadExtScript("ext:deno_fetch/23_response.js");
core.loadExtScript("ext:deno_telemetry/telemetry.ts");
core.loadExtScript("ext:deno_telemetry/util.ts");
core.loadExtScript("ext:deno_fetch/26_fetch.js");
core.loadExtScript("ext:deno_fetch/27_eventsource.js");
const { realPath: denoRealPath, realPathSync: denoRealPathSync } = core.loadExtScript(
  "ext:deno_fs/30_fs.js",
);
core.loadExtScript("ext:deno_http/00_serve.ts");
core.loadExtScript("ext:deno_http/01_http.js");
core.loadExtScript("ext:deno_http/02_websocket.ts");
core.loadExtScript("ext:deno_net/01_net.js");
core.loadExtScript("ext:deno_net/02_tls.js");
const {
  hostname: denoHostname,
  loadavg: denoLoadavg,
  networkInterfaces: denoNetworkInterfaces,
  osRelease: denoOsRelease,
  osUptime,
  systemMemoryInfo: denoSystemMemoryInfo,
} = core.loadExtScript("ext:deno_os/30_os.js");
core.loadExtScript("ext:deno_os/40_signals.js");
const io = core.loadExtScript("ext:deno_io/12_io.js");
core.loadExtScript("ext:deno_web/01_urlpattern.js");
const {
  defineEventHandler: defineWebEventHandler,
  PromiseRejectionEvent: WebPromiseRejectionEvent,
  reportException: reportWebException,
  saveGlobalThisReference: saveWebGlobalThisReference,
} = core.loadExtScript("ext:deno_web/02_event.js");
core.loadExtScript("ext:deno_web/04_global_interfaces.js");
const { atob: webAtob, btoa: webBtoa } = core.loadExtScript("ext:deno_web/05_base64.js");
const {
  ByteLengthQueuingStrategy: webByteLengthQueuingStrategy,
  CountQueuingStrategy: webCountQueuingStrategy,
  ReadableByteStreamController: webReadableByteStreamController,
  ReadableStream: webReadableStream,
  ReadableStreamBYOBReader: webReadableStreamBYOBReader,
  ReadableStreamBYOBRequest: webReadableStreamBYOBRequest,
  ReadableStreamDefaultController: webReadableStreamDefaultController,
  ReadableStreamDefaultReader: webReadableStreamDefaultReader,
  TransformStream: webTransformStream,
  TransformStreamDefaultController: webTransformStreamDefaultController,
  WritableStream: webWritableStream,
  WritableStreamDefaultController: webWritableStreamDefaultController,
  WritableStreamDefaultWriter: webWritableStreamDefaultWriter,
} = core.loadExtScript("ext:deno_web/06_streams.js");
// The encoding/compression web-stream globals come from the same ext modules
// that the `stream/web` Node polyfill re-exports, so seeding them here keeps
// `globalThis.TextEncoderStream === require("stream/web").TextEncoderStream`
// (and siblings) identity-equal. `loadExtScript` is cached, so these resolve to
// the same bindings the polyfill loads.
const {
  TextDecoderStream: webTextDecoderStream,
  TextEncoderStream: webTextEncoderStream,
} = core.loadExtScript("ext:deno_web/08_text_encoding.js");
const {
  CompressionStream: webCompressionStream,
  DecompressionStream: webDecompressionStream,
} = core.loadExtScript("ext:deno_web/14_compression.js");
core.loadExtScript("ext:deno_web/10_filereader.js");
core.loadExtScript("ext:deno_web/12_location.js");
const {
  deserializeJsMessageData: webDeserializeJsMessageData,
  MessageChannel: webMessageChannel,
  MessagePort: webMessagePort,
  MessagePortPrototype: webMessagePortPrototype,
  serializeJsMessageData: webSerializeJsMessageData,
  structuredClone: webStructuredClone,
  unrefParentPort: webUnrefParentPort,
} = core.loadExtScript("ext:deno_web/13_message_port.js");
const { performance: webPerformance } = core.loadExtScript(
  "ext:deno_web/15_performance.js",
);
core.loadExtScript("ext:deno_web/16_image_data.js");
const { enableNextTick } = core.loadExtScript("ext:deno_node/_next_tick.ts");
const { streamBaseState } = core.loadExtScript(
  "ext:deno_node/internal_binding/stream_wrap.ts",
);
const {
  bindStreamsLazy: bindNodeConsoleStreamsLazy,
  Console: NodeConsole,
  kBindProperties: nodeConsoleBindProperties,
} = core.loadExtScript("ext:deno_node/internal/console/constructor.mjs");
const { onWarning: nodeProcessOnWarning } = core.loadExtScript(
  "ext:deno_node/internal/process/warning.ts",
);
const {
  AbortError: nodeAbortError,
  ERR_FS_INVALID_SYMLINK_TYPE: nodeErrFsInvalidSymlinkType,
  ERR_FS_FILE_TOO_LARGE: nodeErrFsFileTooLarge,
  ERR_INVALID_ARG_VALUE: nodeErrInvalidArgValue,
} = core.loadExtScript("ext:deno_node/internal/errors.ts");
const { parseFileMode: nodeParseFileMode } = core.loadExtScript(
  "ext:deno_node/internal/validators.mjs",
);
const denoProcessModule = core.loadExtScript("ext:deno_process/40_process.js");

Object.defineProperties(globalThis, windowOrWorkerGlobalScope);
Object.defineProperty(globalThis, Symbol.toStringTag, {
  value: "global",
  configurable: true,
  enumerable: false,
  writable: false,
});
const nimbusInternalFsBinding = getNodeInternalBinding("fs");
const {
  ArrayIsArray,
  Float64Array,
  ObjectPrototypeIsPrototypeOf,
  PromiseResolve,
  SymbolAsyncIterator,
  SymbolDispose,
} = primordials;
const denoMemoryUsageBuffer = new Float64Array(4);

if (!Object.getOwnPropertyDescriptor(nodeFsDirent.prototype, "path")) {
  Object.defineProperty(nodeFsDirent.prototype, "path", {
    get() {
      return this.parentPath;
    },
    configurable: true,
    enumerable: true,
  });
}

function runtimeFsAssertExistingCwd(cwd) {
  try {
    const value = globalThis.__nimbusSyncHostValue("op_nimbus_runtime_stat_sync", {
      path: cwd,
      follow_symlink: true,
    });
    return toFileInfo(value);
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsCurrentCwd() {
  const policyCwd = typeof core.ops.op_nimbus_runtime_cwd === "function"
    ? core.ops.op_nimbus_runtime_cwd()
    : null;
  if (typeof policyCwd === "string" && policyCwd.length > 0) {
    nimbusRuntimeCurrentCwd = policyCwd;
    return policyCwd;
  }
  return nimbusRuntimeCurrentCwd ??
    nodeProcessBuiltin?.cwd?.() ??
    globalThis.process?.cwd?.() ??
    ".";
}

function runtimeFsPathToString(path) {
  if (typeof path === "string") {
    if (nodePathIsAbsolute(path)) {
      return path;
    }
    const cwd = runtimeFsCurrentCwd();
    if (nodePathIsAbsolute(cwd)) {
      runtimeFsAssertExistingCwd(cwd);
    }
    return nodePathResolve(cwd, path);
  }
  if (path instanceof URL) {
    if (path.protocol !== "file:") {
      throw new TypeError(`Nimbus only supports file: URLs for Deno fs APIs; received ${path.href}`);
    }
    return decodeURIComponent(path.pathname.replace(/^\/([A-Za-z]:)/, "$1"));
  }
  return String(path);
}

function runtimeFsSymlinkTargetToString(path) {
  if (typeof path === "string") {
    return path;
  }
  if (path instanceof URL) {
    if (path.protocol !== "file:") {
      throw new TypeError(`Nimbus only supports file: URLs for symlink targets; received ${path.href}`);
    }
    return decodeURIComponent(path.pathname.replace(/^\/([A-Za-z]:)/, "$1"));
  }
  return String(path);
}

function runtimeFsToUnixTimeFromEpoch(value) {
  let unixSeconds;
  try {
    unixSeconds = Date.prototype.getTime.call(value) / 1e3;
  } catch {
    unixSeconds = typeof value === "number" && Number.isFinite(value)
      ? value
      : nodeFsToUnixTimestamp(value);
  }
  const seconds = Math.floor(unixSeconds);
  const nanoseconds = Math.trunc((unixSeconds - seconds) * 1e9);
  return {
    seconds,
    nanoseconds,
  };
}

function runtimeFsMapThrownError(error) {
  const hostError = error?.nimbusHostError;
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
    case "ENOTDIR":
    case "EISDIR":
      mappedError = new Error(message);
      break;
    case "EINVAL":
      mappedError = new TypeError(message);
      break;
    default:
      mappedError = new Error(message);
      break;
  }
  mappedError.code = hostError.code;
  mappedError.nimbusHostError = hostError;
  return mappedError;
}

function runtimeFsWatchInfoSignature(fileInfo) {
  if (!fileInfo) {
    return "missing";
  }
  return JSON.stringify({
    isFile: fileInfo.isFile === true,
    isDirectory: fileInfo.isDirectory === true,
    isSymlink: fileInfo.isSymlink === true,
    size: Number(fileInfo.size ?? 0),
    mtimeMs: fileInfo.mtime instanceof Date ? fileInfo.mtime.getTime() : null,
    ctimeMs: fileInfo.ctime instanceof Date ? fileInfo.ctime.getTime() : null,
    birthtimeMs: fileInfo.birthtime instanceof Date ? fileInfo.birthtime.getTime() : null,
    ino: fileInfo.ino ?? null,
    mode: fileInfo.mode ?? null,
  });
}

function runtimeFsWatchPathDepth(relativePath) {
  if (typeof relativePath !== "string" || relativePath.length === 0) {
    return 0;
  }
  return relativePath.split(/[\\/]+/).filter((segment) => segment.length > 0).length;
}

function runtimeFsSelectMostSpecificWatchEntry(entries) {
  if (!ArrayIsArray(entries) || entries.length === 0) {
    return null;
  }
  return [...entries].sort((left, right) => {
    const depthDelta =
      runtimeFsWatchPathDepth(right.relativePath) - runtimeFsWatchPathDepth(left.relativePath);
    if (depthDelta !== 0) {
      return depthDelta;
    }
    return String(left.relativePath).localeCompare(String(right.relativePath));
  })[0];
}

function runtimeFsCollectDirectoryWatchChildren(
  rootPath,
  currentPath,
  recursive,
  children,
) {
  for (const entry of runtimeFsReadDirSync(currentPath)) {
    const childPath = nodePathResolve(currentPath, entry.name);
    const relativePath = nodePathRelative(rootPath, childPath);
    let childInfo = null;
    try {
      childInfo = runtimeFsStatSync(childPath, false);
    } catch (_error) {
      childInfo = null;
    }
    const normalizedChild = childInfo ?? entry;
    const childRecord = {
      path: childPath,
      relativePath,
      signature: runtimeFsWatchInfoSignature(normalizedChild),
      isDirectory: normalizedChild?.isDirectory === true,
      isSymlink: normalizedChild?.isSymlink === true,
    };
    children.set(relativePath, childRecord);
    if (recursive && childRecord.isDirectory && !childRecord.isSymlink) {
      runtimeFsCollectDirectoryWatchChildren(rootPath, childPath, recursive, children);
    }
  }
}

function runtimeFsCreateWatchSnapshot(path, recursive = false) {
  const watchPath = runtimeFsPathToString(path);
  const fileInfo = runtimeFsStatSync(watchPath, true);
  if (!fileInfo.isDirectory) {
    return {
      kind: "file",
      path: watchPath,
      signature: runtimeFsWatchInfoSignature(fileInfo),
    };
  }

  const children = new Map();
  runtimeFsCollectDirectoryWatchChildren(watchPath, watchPath, recursive, children);

  return {
    kind: "directory",
    path: watchPath,
    signature: runtimeFsWatchInfoSignature(fileInfo),
    children,
  };
}

function runtimeFsDiffWatchSnapshots(previousSnapshot, nextSnapshot, recursive = false) {
  if (
    previousSnapshot.kind !== nextSnapshot.kind ||
    previousSnapshot.path !== nextSnapshot.path
  ) {
    return { kind: "modify", paths: [nextSnapshot.path], flag: null };
  }

  if (nextSnapshot.kind === "file") {
    if (previousSnapshot.signature !== nextSnapshot.signature) {
      return { kind: "modify", paths: [nextSnapshot.path], flag: null };
    }
    return null;
  }

  const previousChildren = previousSnapshot.children;
  const nextChildren = nextSnapshot.children;

  const removals = [];
  const additions = [];
  let directoryMetadataChange = null;

  for (const [name, previousChild] of previousChildren.entries()) {
    if (!nextChildren.has(name)) {
      removals.push(previousChild);
    }
  }

  for (const [name, nextChild] of nextChildren.entries()) {
    if (!previousChildren.has(name)) {
      additions.push(nextChild);
    }
  }

  if (additions.length > 0) {
    const addedEntry = runtimeFsSelectMostSpecificWatchEntry(additions);
    return { kind: "create", paths: [addedEntry.path], flag: null };
  }

  if (removals.length > 0) {
    const removedEntry = runtimeFsSelectMostSpecificWatchEntry(removals);
    return { kind: "remove", paths: [removedEntry.path], flag: null };
  }

  for (const [name, previousChild] of previousChildren.entries()) {
    const nextChild = nextChildren.get(name);
    if (!nextChild || previousChild.signature === nextChild.signature) {
      continue;
    }
    if (
      recursive &&
      previousChild.isDirectory &&
      nextChild.isDirectory &&
      !previousChild.isSymlink &&
      !nextChild.isSymlink
    ) {
      directoryMetadataChange ??= nextChild;
      continue;
    }
    return { kind: "modify", paths: [nextChild.path], flag: null };
  }

  if (directoryMetadataChange !== null) {
    return { kind: "modify", paths: [directoryMetadataChange.path], flag: null };
  }

  if (previousSnapshot.signature !== nextSnapshot.signature) {
    return { kind: "modify", paths: [null], flag: null };
  }
  return null;
}

class RuntimeFsWatcher {
  #closed = false;
  #paths = [];
  #queue = [];
  #recursive = false;
  #snapshots = new Map();
  #timer = null;
  #waiters = [];

  constructor(paths, options = { __proto__: null, recursive: true }) {
    this.#paths = ArrayIsArray(paths) ? [...paths] : [paths];
    this.#recursive = options?.recursive === true;
    for (const path of this.#paths) {
      const normalizedPath = runtimeFsPathToString(path);
      this.#snapshots.set(
        normalizedPath,
        runtimeFsCreateWatchSnapshot(normalizedPath, this.#recursive),
      );
    }
    this.#timer = setInterval(() => this.#poll(), 50);
  }

  #emit(event) {
    if (this.#closed) {
      return;
    }
    const waiter = this.#waiters.shift();
    if (waiter) {
      waiter({ value: event, done: false });
      return;
    }
    this.#queue.push(event);
  }

  #finish() {
    while (this.#waiters.length > 0) {
      const waiter = this.#waiters.shift();
      waiter?.({ value: undefined, done: true });
    }
  }

  #poll() {
    if (this.#closed) {
      return;
    }
    for (const path of this.#paths) {
      const normalizedPath = runtimeFsPathToString(path);
      const previousSnapshot = this.#snapshots.get(normalizedPath);
      let nextSnapshot = null;
      try {
        nextSnapshot = runtimeFsCreateWatchSnapshot(normalizedPath, this.#recursive);
      } catch (error) {
        const hostErrorCode = error?.code;
        if ((hostErrorCode === "ENOENT" || hostErrorCode === "ENOTDIR") && previousSnapshot) {
          this.#snapshots.delete(normalizedPath);
          this.#emit({ kind: "remove", paths: [normalizedPath], flag: null });
          return;
        }
        continue;
      }
      this.#snapshots.set(normalizedPath, nextSnapshot);
      if (!previousSnapshot) {
        this.#emit({ kind: "create", paths: [nextSnapshot.path], flag: null });
        return;
      }
      const event = runtimeFsDiffWatchSnapshots(previousSnapshot, nextSnapshot, this.#recursive);
      if (event) {
        this.#emit(event);
        return;
      }
    }
  }

  unref() {
    this.#timer?.unref?.();
  }

  ref() {
    this.#timer?.ref?.();
  }

  async next() {
    if (this.#queue.length > 0) {
      return { value: this.#queue.shift(), done: false };
    }
    if (this.#closed) {
      return { value: undefined, done: true };
    }
    return await new Promise((resolve) => {
      this.#waiters.push(resolve);
    });
  }

  return(value) {
    this.close();
    return PromiseResolve({ value, done: true });
  }

  close() {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    if (this.#timer !== null) {
      clearInterval(this.#timer);
      this.#timer = null;
    }
    this.#finish();
  }

  [SymbolAsyncIterator]() {
    return this;
  }

  [SymbolDispose]() {
    this.close();
  }
}

function denoWatchFs(
  paths,
  options = { __proto__: null, recursive: true },
) {
  return new RuntimeFsWatcher(ArrayIsArray(paths) ? paths : [paths], options);
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
    dev: value?.dev ?? null,
    ino: value?.ino ?? null,
    nlink: value?.nlink ?? null,
    uid: value?.uid ?? null,
    gid: value?.gid ?? null,
    rdev: value?.rdev ?? null,
    blksize: value?.blksize ?? null,
    blocks: value?.blocks ?? null,
    isBlockDevice: value?.isBlockDevice === true,
    isCharDevice: value?.isCharDevice === true,
    isFifo: value?.isFifo === true,
    isSocket: value?.isSocket === true,
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
    const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_stat", {
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
    const value = globalThis.__nimbusSyncHostValue("op_nimbus_runtime_stat_sync", {
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
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_mkdir", {
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
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_mkdir_sync", {
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
    entries = await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_read_dir", {
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
    const entries = globalThis.__nimbusSyncHostValue("op_nimbus_runtime_read_dir_sync", {
      path: runtimeFsPathToString(path),
    });
    return (entries ?? []).map(toDirEntry).values();
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsRemove(path, options = undefined) {
  const normalizedOptions = options && typeof options === "object" ? options : {};
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_remove", {
      path: runtimeFsPathToString(path),
      recursive: normalizedOptions.recursive === true,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsRemoveSync(path, options = undefined) {
  const normalizedOptions = options && typeof options === "object" ? options : {};
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_remove_sync", {
      path: runtimeFsPathToString(path),
      recursive: normalizedOptions.recursive === true,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsRmdir(path) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_remove", {
      path: runtimeFsPathToString(path),
      recursive: false,
      directory_only: true,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsRmdirSync(path) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_remove_sync", {
      path: runtimeFsPathToString(path),
      recursive: false,
      directory_only: true,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsChmod(path, mode) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_chmod", {
      path: runtimeFsPathToString(path),
      mode,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsLchmod(path, mode) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_lchmod", {
      path: runtimeFsPathToString(path),
      mode,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsChown(path, uid, gid) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_chown", {
      path: runtimeFsPathToString(path),
      uid,
      gid,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsCopyFile(from, to) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_copy_file", {
      from: runtimeFsPathToString(from),
      to: runtimeFsPathToString(to),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsCopyFileSync(from, to) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_copy_file_sync", {
      from: runtimeFsPathToString(from),
      to: runtimeFsPathToString(to),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsLink(oldpath, newpath) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_link", {
      oldpath: runtimeFsPathToString(oldpath),
      newpath: runtimeFsPathToString(newpath),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsLinkSync(oldpath, newpath) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_link_sync", {
      oldpath: runtimeFsPathToString(oldpath),
      newpath: runtimeFsPathToString(newpath),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsSymlinkFileType(options) {
  const fileType = typeof options === "string"
    ? options
    : options && typeof options === "object" && typeof options.type === "string"
    ? options.type
    : null;
  if (
    fileType !== null &&
    fileType !== "dir" &&
    fileType !== "file" &&
    fileType !== "junction"
  ) {
    throw new nodeErrFsInvalidSymlinkType(fileType);
  }
  return fileType;
}

async function runtimeFsSymlink(oldpath, newpath, options = undefined) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_symlink", {
      oldpath: runtimeFsSymlinkTargetToString(oldpath),
      newpath: runtimeFsPathToString(newpath),
      file_type: runtimeFsSymlinkFileType(options),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsSymlinkSync(oldpath, newpath, options = undefined) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_symlink_sync", {
      oldpath: runtimeFsSymlinkTargetToString(oldpath),
      newpath: runtimeFsPathToString(newpath),
      file_type: runtimeFsSymlinkFileType(options),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsReadLink(path) {
  try {
    return await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_read_link", {
      path: runtimeFsPathToString(path),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsReadLinkSync(path) {
  try {
    return globalThis.__nimbusSyncHostValue("op_nimbus_runtime_read_link_sync", {
      path: runtimeFsPathToString(path),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsRename(oldpath, newpath) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_rename", {
      oldpath: runtimeFsPathToString(oldpath),
      newpath: runtimeFsPathToString(newpath),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsRenameSync(oldpath, newpath) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_rename_sync", {
      oldpath: runtimeFsPathToString(oldpath),
      newpath: runtimeFsPathToString(newpath),
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsChmodSync(path, mode) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_chmod_sync", {
      path: runtimeFsPathToString(path),
      mode,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsLchmodSync(path, mode) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_lchmod_sync", {
      path: runtimeFsPathToString(path),
      mode,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsChownSync(path, uid, gid) {
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_chown_sync", {
      path: runtimeFsPathToString(path),
      uid,
      gid,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

async function runtimeFsUtime(path, atime, mtime) {
  const normalizedAtime = runtimeFsToUnixTimeFromEpoch(atime);
  const normalizedMtime = runtimeFsToUnixTimeFromEpoch(mtime);
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_utime", {
      path: runtimeFsPathToString(path),
      atime_secs: normalizedAtime.seconds,
      atime_nanos: normalizedAtime.nanoseconds,
      mtime_secs: normalizedMtime.seconds,
      mtime_nanos: normalizedMtime.nanoseconds,
    });
  } catch (error) {
    throw runtimeFsMapThrownError(error);
  }
}

function runtimeFsUtimeSync(path, atime, mtime) {
  const normalizedAtime = runtimeFsToUnixTimeFromEpoch(atime);
  const normalizedMtime = runtimeFsToUnixTimeFromEpoch(mtime);
  try {
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_utime_sync", {
      path: runtimeFsPathToString(path),
      atime_secs: normalizedAtime.seconds,
      atime_nanos: normalizedAtime.nanoseconds,
      mtime_secs: normalizedMtime.seconds,
      mtime_nanos: normalizedMtime.nanoseconds,
    });
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

const nimbusProcessCwdPatched = Symbol("nimbus.processCwdPatched");
const nimbusProcessStdioPatched = Symbol("nimbus.processStdioPatched");
let nimbusRuntimeCurrentCwd = null;

function seedNodeProcessCwd(nodeProcess) {
  if (
    !nodeProcess ||
    typeof nodeProcess !== "object"
  ) {
    return;
  }

  const alreadyPatched = nodeProcess[nimbusProcessCwdPatched] === true;
  const originalCwd = typeof nodeProcess.cwd === "function"
    ? nodeProcess.cwd.bind(nodeProcess)
    : null;
  const policyCwd = typeof core.ops.op_nimbus_runtime_cwd === "function"
    ? core.ops.op_nimbus_runtime_cwd()
    : null;
  if (
    alreadyPatched &&
    !(typeof policyCwd === "string" && policyCwd.length > 0)
  ) {
    return;
  }
  let currentCwd = typeof policyCwd === "string" && policyCwd.length > 0
    ? policyCwd
    : nimbusRuntimeCurrentCwd ??
      (nodeProcess !== globalThis.process &&
          globalThis.process?.[nimbusProcessCwdPatched] === true &&
          typeof globalThis.process?.cwd === "function"
        ? globalThis.process.cwd()
        : null) ??
      (originalCwd !== null &&
          nodeProcess === globalThis.process &&
          nodeProcess[nimbusProcessCwdPatched] === true
        ? originalCwd()
        : null) ??
      "/";
  nimbusRuntimeCurrentCwd = currentCwd;

  Object.defineProperty(nodeProcess, "cwd", {
    value() {
      return currentCwd;
    },
    configurable: true,
    enumerable: false,
    writable: true,
  });

  Object.defineProperty(nodeProcess, "chdir", {
    value(directory) {
      const nextCwd = nodePathResolve(currentCwd, String(directory));
      const fileInfo = runtimeFsAssertExistingCwd(nextCwd);
      if (!fileInfo.isDirectory) {
        const error = new Error(`ENOTDIR: not a directory, chdir '${currentCwd}' -> '${nextCwd}'`);
        error.code = "ENOTDIR";
        error.errno = -20;
        error.syscall = "chdir";
        error.path = nextCwd;
        throw error;
      }
      currentCwd = nextCwd;
      nimbusRuntimeCurrentCwd = currentCwd;
    },
    configurable: true,
    enumerable: false,
    writable: true,
  });

  Object.defineProperty(nodeProcess, nimbusProcessCwdPatched, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

function seedNodeProcessPlatformMetadata(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

  Object.defineProperty(nodeProcess, Symbol.toStringTag, {
    value: "process",
    configurable: false,
    enumerable: false,
    writable: true,
  });

  const nodePlatform = runtimeNodePlatform();
  if (nodePlatform.length > 0 && nodeProcess.platform !== nodePlatform) {
    Object.defineProperty(nodeProcess, "platform", {
      value: nodePlatform,
      configurable: true,
      enumerable: true,
      writable: false,
    });
  }

  const nodeArch = runtimeNodeArch();
  if (nodeArch.length > 0 && nodeProcess.arch !== nodeArch) {
    // Nimbus does not run Deno's full nodeBootstrap() sequence because that
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

function seedNodeProcessStdio(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }
  if (nodeProcess[nimbusProcessStdioPatched] === true) {
    return;
  }

  // The deno_node `node:process` polyfill installs self-delegating Proxy
  // placeholders for stdin/stdout/stderr (`makeStreamDelegate`) as plain data
  // properties at module top-level, and expects
  // `internals.__bootstrapNodeProcess` to replace them with real lazy-stream
  // accessors (`defineLazyStream`). Nimbus's FaaS bootstrap does not run
  // `__bootstrapNodeProcess`, so we materialize the real streams here,
  // mirroring that bootstrap's warmup branch.
  //
  // The overwrite MUST be unconditional. The placeholder is a data property
  // holding the Proxy (never `undefined`, so the previous `=== undefined`
  // guard never fired), and the Proxy's `get` trap reads `process[name]` and
  // `ReflectGet`s on it. While the property still points at the Proxy, any
  // `process.stdout.write(...)` re-enters the trap forever (`RangeError:
  // Maximum call stack size exceeded`). Pointing the property at the real
  // stream fixes both `process.stdout` and the exported `stdout` binding (the
  // latter delegates through `process[name]` in a single, terminating hop).
  // The marker keeps the two call sites (nodeGlobals + globalThis) idempotent
  // so we construct each stream once.
  nodeProcess.stdin = initStdin(false);
  nodeProcess.stdout = createWritableStdioStream(io.stdout, "stdout");
  nodeProcess.stderr = createWritableStdioStream(io.stderr, "stderr");

  Object.defineProperty(nodeProcess, nimbusProcessStdioPatched, {
    value: true,
    enumerable: false,
    configurable: true,
  });
}

function seedNodeProcessExecPath(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

  const execPath = core.ops.op_nimbus_runtime_exec_path();
  if (typeof execPath === "string" && execPath.length > 0) {
    nodeProcess.execPath = execPath;
    if (Array.isArray(nodeProcess.argv) && nodeProcess.argv.length > 0) {
      nodeProcess.argv[0] = execPath;
    }
  }
}

function seedNodeProcessFeatures(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

  const features = nodeProcess.features;
  if (!features || typeof features !== "object") {
    return;
  }

  features.inspector = features.inspector === true;
  features.debug = features.debug === true;
  features.uv = features.uv === true;
  features.ipv6 = features.ipv6 === true;
  features.tls_alpn = features.tls_alpn === true;
  features.tls_sni = features.tls_sni === true;
  features.tls_ocsp = features.tls_ocsp === true;
  features.tls = features.tls === true;
  features.openssl_is_boringssl = features.openssl_is_boringssl === true;
  features.cached_builtins = features.cached_builtins === true;
  features.require_module = features.require_module === true;
  if (!Object.prototype.hasOwnProperty.call(features, "typescript")) {
    features.typescript = false;
  }
  delete features.quic;
}

const nimbusBuiltinModuleLoaderInstalled = Symbol("nimbus.builtinModuleLoaderInstalled");

function seedNodeProcessBuiltinModuleLoader(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

  const getBuiltinModule = nodeProcessBuiltin?.getBuiltinModule;
  if (typeof getBuiltinModule !== "function") {
    return;
  }
  if (nodeProcess[nimbusBuiltinModuleLoaderInstalled] === true) {
    return;
  }
  const nodeProcessBuiltinModule = nodeProcess;
  function nimbusGetBuiltinModule(specifier) {
    const normalizedSpecifier = typeof specifier === "string" &&
        specifier.startsWith("node:")
      ? specifier.slice(5)
      : specifier;
    if (normalizedSpecifier === "process") {
      return nodeProcessBuiltinModule;
    }
    const moduleBuiltin = getBuiltinModule.call(nodeProcessBuiltin, specifier);
    if (normalizedSpecifier === "module") {
      installNimbusNativeExtensionErrorMapping(moduleBuiltin);
    }
    return moduleBuiltin;
  }
  Object.defineProperty(nodeProcess, nimbusBuiltinModuleLoaderInstalled, {
    value: true,
    configurable: true,
  });
  Object.defineProperty(nodeProcess, "getBuiltinModule", {
    value: nimbusGetBuiltinModule,
    configurable: true,
    enumerable: true,
    writable: true,
  });
}

const nimbusProcessFinalizationInstalled = Symbol("nimbus.processFinalizationInstalled");
const nimbusProcessFatalGuardsInstalled = Symbol("nimbus.processFatalGuardsInstalled");

function validateProcessFinalizationRegistration(ref, callback) {
  if ((typeof ref !== "object" && typeof ref !== "function") || ref === null) {
    throw new TypeError('The "ref" argument must be of type object');
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
}

function seedNodeProcessFinalization(nodeProcess) {
  if (
    !nodeProcess ||
    typeof nodeProcess !== "object" ||
    typeof nodeProcess.on !== "function" ||
    nodeProcess[nimbusProcessFinalizationInstalled] === true
  ) {
    return;
  }

  const exitRegistrations = [];
  const beforeExitRegistrations = [];

  function registerIn(registrations, ref, callback) {
    validateProcessFinalizationRegistration(ref, callback);
    registrations.push({
      callback,
      ref: new WeakRef(ref),
    });
  }

  function runRegistrations(registrations, eventName) {
    for (const registration of [...registrations]) {
      const ref = registration.ref.deref();
      if (ref !== undefined) {
        registration.callback(ref, eventName);
      }
    }
  }

  const finalization = {
    register(ref, callback) {
      registerIn(exitRegistrations, ref, callback);
    },
    registerBeforeExit(ref, callback) {
      registerIn(beforeExitRegistrations, ref, callback);
    },
    unregister(ref) {
      for (const registrations of [exitRegistrations, beforeExitRegistrations]) {
        for (let index = registrations.length - 1; index >= 0; index -= 1) {
          const registeredRef = registrations[index].ref.deref();
          if (registeredRef === undefined || registeredRef === ref) {
            registrations.splice(index, 1);
          }
        }
      }
    },
  };

  Object.defineProperty(nodeProcess, "finalization", {
    value: finalization,
    configurable: true,
    enumerable: true,
    writable: false,
  });
  nodeProcess.on("beforeExit", () => {
    runRegistrations(beforeExitRegistrations, "beforeExit");
  });
  nodeProcess.on("exit", () => {
    runRegistrations(exitRegistrations, "exit");
  });
  Object.defineProperty(nodeProcess, nimbusProcessFinalizationInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

function createNimbusDeniedProcessFatalOperation(name) {
  function deniedProcessFatalOperation() {
    throw new Error(
      `Nimbus denies process.${name}() in embedded Node runtime; use process or microVM isolation for fatal-capable workloads`,
    );
  }
  Object.defineProperty(deniedProcessFatalOperation, "__nimbusDeniedProcessFatalOperation", {
    value: name,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  return deniedProcessFatalOperation;
}

function seedNodeProcessFatalGuards(nodeProcess) {
  if (
    !nodeProcess ||
    typeof nodeProcess !== "object" ||
    nodeProcess[nimbusProcessFatalGuardsInstalled] === true
  ) {
    return;
  }

  for (const name of ["abort", "kill"]) {
    Object.defineProperty(nodeProcess, name, {
      value: createNimbusDeniedProcessFatalOperation(name),
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }
  Object.defineProperty(nodeProcess, nimbusProcessFatalGuardsInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

const nimbusProcessDlopenPatched = Symbol("nimbus.processDlopenPatched");
const nimbusNativeExtensionPatched = Symbol("nimbus.nativeExtensionPatched");

function isDlopenTypeError(error) {
  return error?.name === "TypeError" &&
    typeof error?.message === "string" &&
    error.message.startsWith("dlopen(");
}

function createNodeDlopenError(error) {
  const mapped = new Error(error.message);
  mapped.code = "ERR_DLOPEN_FAILED";
  if (typeof error.stack === "string") {
    mapped.stack = error.stack.replace(/^TypeError:/, "Error:");
  }
  return mapped;
}

function installNimbusProcessDlopenErrorMapping(nodeProcess) {
  if (
    !nodeProcess ||
    typeof nodeProcess !== "object" ||
    typeof nodeProcess.dlopen !== "function" ||
    nodeProcess[nimbusProcessDlopenPatched] === true
  ) {
    return;
  }

  const originalDlopen = nodeProcess.dlopen;
  function nimbusDlopen(...args) {
    try {
      return Reflect.apply(originalDlopen, this, args);
    } catch (error) {
      if (isDlopenTypeError(error)) {
        throw createNodeDlopenError(error);
      }
      throw error;
    }
  }
  Object.defineProperty(nimbusDlopen, nimbusProcessDlopenPatched, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  Object.defineProperty(nodeProcess, nimbusProcessDlopenPatched, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  Object.defineProperty(nodeProcess, "dlopen", {
    value: nimbusDlopen,
    configurable: true,
    enumerable: true,
    writable: true,
  });
}

function installNimbusNativeExtensionErrorMapping(moduleBuiltin) {
  const extensions = moduleBuiltin?._extensions;
  if (
    !extensions ||
    typeof extensions !== "object" ||
    typeof extensions[".node"] !== "function" ||
    extensions[".node"][nimbusNativeExtensionPatched] === true
  ) {
    return;
  }

  const originalNativeExtension = extensions[".node"];
  function nimbusNativeExtension(...args) {
    try {
      return Reflect.apply(originalNativeExtension, this, args);
    } catch (error) {
      if (isDlopenTypeError(error)) {
        throw createNodeDlopenError(error);
      }
      throw error;
    }
  }
  Object.defineProperty(nimbusNativeExtension, nimbusNativeExtensionPatched, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  Object.defineProperty(extensions, ".node", {
    value: nimbusNativeExtension,
    configurable: true,
    enumerable: true,
    writable: true,
  });
}

const nimbusScopedNodeSpawnSyncInstalled = Symbol.for(
  "nimbus.scopedNodeSpawnSyncInstalled",
);
function installScopedNodeSpawnSyncChild() {
  if (
    denoProcessModule?.[nimbusScopedNodeSpawnSyncInstalled] === true ||
    typeof denoProcessModule?.nodeSpawnSyncChild !== "function"
  ) {
    return;
  }

  const nodeSpawnSyncChild = denoProcessModule.nodeSpawnSyncChild;
  Object.defineProperty(denoProcessModule, "nodeSpawnSyncChild", {
    value(options = {}) {
      return nodeSpawnSyncChild({
        ...options,
        clearEnv: true,
      });
    },
    configurable: true,
    enumerable: true,
    writable: false,
  });
  Object.defineProperty(denoProcessModule, nimbusScopedNodeSpawnSyncInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

const nimbusLoadEnvFilePatched = Symbol("nimbus.loadEnvFilePatched");
const nimbusLoadEnvOverlaySymbol = Symbol.for("nimbus.runtimeEnvOverlay");

function normalizeLoadEnvFilePath(path) {
  if (path === undefined) {
    return ".env";
  }
  if (typeof path === "string" || path instanceof URL) {
    return path;
  }
  if (
    typeof Buffer !== "undefined"
    && typeof Buffer.isBuffer === "function"
    && Buffer.isBuffer(path)
  ) {
    return path.toString();
  }
  return path;
}

function displayLoadEnvFilePath(path) {
  const normalizedPath = normalizeLoadEnvFilePath(path);
  if (normalizedPath instanceof URL) {
    return runtimeFileURLToPath(normalizedPath);
  }
  return typeof normalizedPath === "string" ? normalizedPath : String(normalizedPath);
}

function resolveLoadEnvFilePath(nodeProcess, path) {
  const normalizedPath = normalizeLoadEnvFilePath(path);
  if (normalizedPath instanceof URL) {
    return runtimeFileURLToPath(normalizedPath);
  }
  const pathString = typeof normalizedPath === "string"
    ? normalizedPath
    : String(normalizedPath);
  if (
    pathString.startsWith("/")
    || /^[A-Za-z]:[\\/]/.test(pathString)
  ) {
    return pathString;
  }
  return nodePathResolve(nodeProcess.cwd(), pathString);
}

function runtimeReadTextFileSync(path) {
  return String(
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_require_read_file", {
      path: runtimeFsPathToString(path),
    }),
  );
}

function runtimeFileURLToPath(fileUrl) {
  const url = fileUrl instanceof URL ? fileUrl : new URL(fileUrl);
  if (url.protocol !== "file:") {
    throw new TypeError(`Nimbus only supports file: URLs for process.loadEnvFile(); received ${url.href}`);
  }
  if (url.hostname && url.hostname !== "localhost") {
    throw new TypeError(`Nimbus only supports local file: URLs for process.loadEnvFile(); received ${url.href}`);
  }
  return decodeURIComponent(url.pathname.replace(/^\/([A-Za-z]:)/, "$1"));
}

function createLoadEnvFileNotFoundError(path) {
  const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
  error.code = "ENOENT";
  error.errno = -2;
  error.syscall = "open";
  error.path = path;
  return error;
}

function isLoadEnvFileAccessDeniedError(error) {
  return error?.name === "NotCapable"
    || error?.code === "ERR_ACCESS_DENIED"
    || error?.permission === "FileSystemRead"
    || (
      typeof error?.message === "string"
      && (
        error.message.includes("Requires read access to") ||
        error.message.includes("Access to this API has been restricted") ||
        error.message.includes("runtime read capability denied")
      )
    );
}

function isLoadEnvFileNotFoundError(error) {
  const message = typeof error?.message === "string" ? error.message : "";
  return error?.name === "NotFound"
    || error?.code === "ENOENT"
    || error?.nimbusHostError?.code === "ENOENT"
    || message.includes("No such file or directory")
    || message.includes("os error 2");
}

function isLoadEnvFileInvalidDataError(error) {
  return error?.name === "InvalidData"
    || (
      typeof error?.message === "string"
      && error.message.includes("stream did not contain valid UTF-8")
    );
}

function createLoadEnvFileAccessDeniedError(resource, originalError = undefined) {
  const error = new Error("Access to this API has been restricted");
  error.code = "ERR_ACCESS_DENIED";
  error.permission = "FileSystemRead";
  error.resource = resource;

  const originalFrames = typeof originalError?.stack === "string"
    ? originalError.stack
      .split("\n")
      .filter((line) => line.trimStart().startsWith("at "))
    : [];
  error.stack = [
    "Error: Access to this API has been restricted",
    "  code: 'ERR_ACCESS_DENIED'",
    "  permission: 'FileSystemRead'",
    `  resource: ${JSON.stringify(resource)}`,
    ...originalFrames,
  ].join("\n");
  return error;
}

function seedNodeProcessEnvOverlay(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return null;
  }
  if (globalThis[nimbusLoadEnvOverlaySymbol] === undefined) {
    Object.defineProperty(globalThis, nimbusLoadEnvOverlaySymbol, {
      value: Object.create(null),
      configurable: false,
      enumerable: false,
      writable: false,
    });
  }
  return globalThis[nimbusLoadEnvOverlaySymbol];
}

function applyLoadedEnvFileEntries(nodeProcess, source) {
  const overlayEntries = seedNodeProcessEnvOverlay(nodeProcess);
  if (!overlayEntries) {
    return;
  }

  for (const [key, value] of Object.entries(runtimeParseEnv(source))) {
    try {
      if (nodeProcess.env[key] !== undefined) {
        continue;
      }
    } catch (_error) {
      if (Object.prototype.hasOwnProperty.call(overlayEntries, key)) {
        continue;
      }
    }
    try {
      nodeProcess.env[key] = value;
    } catch (_error) {
      // Keep the internal overlay in sync for embedders that expose a
      // read-only env object while still allowing loadEnvFile visibility.
    }
    overlayEntries[key] = value;
  }
}

function loadEnvFileThroughNimbusHost(nodeProcess, resolvedPath, displayPath) {
  try {
    const source = runtimeReadTextFileSync(resolvedPath);
    applyLoadedEnvFileEntries(nodeProcess, source);
    return undefined;
  } catch (fallbackError) {
    if (isLoadEnvFileAccessDeniedError(fallbackError)) {
      throw createLoadEnvFileAccessDeniedError(displayPath, fallbackError);
    }
    if (isLoadEnvFileNotFoundError(fallbackError)) {
      throw createLoadEnvFileNotFoundError(displayPath);
    }
    if (isLoadEnvFileInvalidDataError(fallbackError)) {
      throw new TypeError(`Contents of '${displayPath}' should be a valid string.`);
    }
    throw fallbackError;
  }
}

function runtimeParseEnv(source) {
  const entries = Object.create(null);
  const lines = String(source).split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    let line = lines[index];
    const trimmed = line.trim();
    if (trimmed.length === 0 || trimmed.startsWith("#")) {
      continue;
    }
    const match = /^(?:\s*export\s+)?\s*([A-Za-z_][A-Za-z0-9_]*?)\s*=\s*(.*)$/.exec(line);
    if (!match) {
      continue;
    }
    const key = match[1];
    let rawValue = match[2] ?? "";
    const leadingTrimmed = rawValue.trimStart();
    const quote = leadingTrimmed[0];
    if (quote === "\"" || quote === "'" || quote === "`") {
      rawValue = leadingTrimmed;
      while (!runtimeDotenvQuotedValueIsClosed(rawValue, quote) && index + 1 < lines.length) {
        index += 1;
        rawValue += `\n${lines[index]}`;
      }
    }
    entries[key] = runtimeParseEnvValue(rawValue);
  }
  return entries;
}

function runtimeDotenvQuotedValueIsClosed(value, quote) {
  if (!value.startsWith(quote)) {
    return false;
  }
  let escaped = false;
  for (let index = 1; index < value.length; index += 1) {
    const char = value[index];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (quote === "\"" && char === "\\") {
      escaped = true;
      continue;
    }
    if (char === quote) {
      return true;
    }
  }
  return false;
}

function runtimeParseEnvValue(rawValue) {
  let value = String(rawValue ?? "").trimStart();
  const quote = value[0];
  if (quote === "\"" || quote === "'" || quote === "`") {
    let escaped = false;
    for (let index = 1; index < value.length; index += 1) {
      const char = value[index];
      if (escaped) {
        escaped = false;
        continue;
      }
      if (quote === "\"" && char === "\\") {
        escaped = true;
        continue;
      }
      if (char === quote) {
        value = value.slice(1, index);
        return quote === "\"" ? value.replace(/\\n/g, "\n") : value;
      }
    }
    return value.slice(1);
  }

  const commentIndex = value.search(/\s#/);
  if (commentIndex !== -1) {
    value = value.slice(0, commentIndex);
  }
  return value.trim();
}

function normalizeFsReadLength(buffer, offset, length) {
  if (!ArrayBuffer.isView(buffer) || typeof offset !== "number") {
    return length;
  }
  if (length !== undefined && length !== null) {
    return length;
  }
  return buffer.byteLength - offset;
}

const nimbusFileHandleGcPatched = Symbol("nimbus.fileHandleGcPatched");
const nimbusFsPromisesLifecyclePatched = Symbol("nimbus.fsPromisesLifecyclePatched");
const nimbusFsWatchPatched = Symbol("nimbus.fsWatchPatched");
const nimbusFsPromisesWatchPatched = Symbol("nimbus.fsPromisesWatchPatched");
const nimbusOriginalFileHandleFdGetter =
  Object.getOwnPropertyDescriptor(nodeInternalFsFileHandle?.prototype ?? {}, "fd")?.get;

function isNimbusFileHandle(value) {
  return !!(
    value &&
    typeof value === "object" &&
    nodeInternalFsFileHandle?.prototype &&
    nodeInternalFsFileHandle.prototype.isPrototypeOf(value)
  );
}

function getNodeFsPromiseTargets(nodeFs) {
  const targets = [];
  if (
    nodeFs?.promises &&
    typeof nodeFs.promises === "object" &&
    !targets.includes(nodeFs.promises)
  ) {
    targets.push(nodeFs.promises);
  }
  return targets;
}

function getFsPromisesFlag(options, fallbackFlag) {
  if (options && typeof options === "object" && options.flag !== undefined) {
    return options.flag;
  }
  return fallbackFlag;
}

function hasRemovedRmdirRecursiveOption(options) {
  return options !== null &&
    typeof options === "object" &&
    options.recursive !== undefined;
}

function createRemovedRmdirRecursiveError(recursive) {
  return new nodeErrInvalidArgValue(
    "options.recursive",
    recursive,
    "is no longer supported",
  );
}

function createFsPromisesWatchTypeError(name, expected, value) {
  const receivedType = value === null ? "null" : typeof value;
  const error = new TypeError(
    `The "${name}" argument must be of type ${expected}. Received ${receivedType}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  return error;
}

function createFsPromisesWatchRangeError(name, range, value) {
  const error = new RangeError(
    `The value of "${name}" is out of range. It must be ${range}. Received ${value}`,
  );
  error.code = "ERR_OUT_OF_RANGE";
  return error;
}

function createFsPromisesWatchValueError(name, value, reason) {
  const error = new TypeError(`The "${name}" argument ${reason}. Received ${value}`);
  error.code = "ERR_INVALID_ARG_VALUE";
  return error;
}

function createFsPromisesWatchQueueOverflowError(maxQueue) {
  const error = new Error(`fs.watch maxQueue exceeded: ${maxQueue}`);
  error.code = "ERR_FS_WATCH_QUEUE_OVERFLOW";
  return error;
}

function createFsPromisesWatchAbortError(cause = undefined) {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  if (cause !== undefined) {
    error.cause = cause;
  }
  return error;
}

function createFsWatchNotFoundError(path) {
  const error = new Error(`ENOENT: no such file or directory, watch '${path}'`);
  error.errno = -2;
  error.code = "ENOENT";
  error.syscall = "watch";
  error.path = path;
  error.filename = path;
  return error;
}

function validateFsPromisesWatchOptions(options) {
  if (options === undefined) {
    return {
      __proto__: null,
      builtin: undefined,
      signal: undefined,
    };
  }
  if (options === null || typeof options !== "object") {
    throw createFsPromisesWatchTypeError("options", "Object", options);
  }
  const optionsSnapshot = { ...options };
  if (
    optionsSnapshot.persistent !== undefined &&
    typeof optionsSnapshot.persistent !== "boolean"
  ) {
    throw createFsPromisesWatchTypeError(
      "options.persistent",
      "boolean",
      optionsSnapshot.persistent,
    );
  }
  if (
    optionsSnapshot.recursive !== undefined &&
    typeof optionsSnapshot.recursive !== "boolean"
  ) {
    throw createFsPromisesWatchTypeError(
      "options.recursive",
      "boolean",
      optionsSnapshot.recursive,
    );
  }
  if (
    optionsSnapshot.encoding !== undefined &&
    typeof optionsSnapshot.encoding !== "string"
  ) {
    const error = new TypeError(
      `The value "${optionsSnapshot.encoding}" is invalid for option "encoding"`,
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (
    optionsSnapshot.signal !== undefined &&
    !(optionsSnapshot.signal instanceof AbortSignal)
  ) {
    throw createFsPromisesWatchTypeError(
      "options.signal",
      "AbortSignal",
      optionsSnapshot.signal,
    );
  }
  const maxQueue = optionsSnapshot.maxQueue ?? 2048;
  if (typeof maxQueue !== "number") {
    throw createFsPromisesWatchTypeError(
      "options.maxQueue",
      "number",
      maxQueue,
    );
  }
  if (!Number.isInteger(maxQueue)) {
    throw createFsPromisesWatchRangeError(
      "options.maxQueue",
      "an integer",
      maxQueue,
    );
  }
  const overflow = optionsSnapshot.overflow ?? "ignore";
  if (overflow !== "ignore" && overflow !== "error") {
    throw createFsPromisesWatchValueError(
      "options.overflow",
      overflow,
      "must be one of: 'ignore', 'error'",
    );
  }
  const signal = optionsSnapshot.signal;
  delete optionsSnapshot.signal;
  delete optionsSnapshot.maxQueue;
  delete optionsSnapshot.overflow;
  return {
    __proto__: null,
    builtin: optionsSnapshot,
    maxQueue,
    overflow,
    signal,
  };
}

function aggregateFsCloseErrors(closeError, opError) {
  if (closeError && opError && closeError !== opError) {
    if (Array.isArray(opError.errors)) {
      opError.errors.push(closeError);
      return opError;
    }
    const error = new AggregateError([opError, closeError], opError.message);
    error.code = opError.code;
    return error;
  }
  return closeError || opError;
}

async function handleFsPromisePathClose(fileOpPromise, closeFn) {
  let result;
  try {
    result = await fileOpPromise;
  } catch (opError) {
    try {
      await closeFn();
    } catch (closeError) {
      throw aggregateFsCloseErrors(closeError, opError);
    }
    throw opError;
  }
  await closeFn();
  return result;
}

async function closeNimbusFileHandle(handle) {
  const closeMethod = handle?.close;
  if (typeof closeMethod !== "function") {
    return undefined;
  }
  if (
    !isNimbusFileHandle(handle) ||
    typeof nimbusOriginalFileHandleFdGetter !== "function"
  ) {
    return await Reflect.apply(closeMethod, handle, []);
  }
  const rawFd = Reflect.apply(nimbusOriginalFileHandleFdGetter, handle, []);
  if (!Number.isInteger(rawFd) || rawFd < 0) {
    return await Reflect.apply(closeMethod, handle, []);
  }
  const hadOwnFd = Object.prototype.hasOwnProperty.call(handle, "fd");
  const ownFdDescriptor = hadOwnFd
    ? Object.getOwnPropertyDescriptor(handle, "fd")
    : undefined;
  Object.defineProperty(handle, "fd", {
    value: rawFd,
    configurable: true,
    enumerable: false,
    writable: true,
  });
  try {
    return await Reflect.apply(closeMethod, handle, []);
  } catch (error) {
    if (
      error?.name === "AggregateError" &&
      Array.isArray(error.errors) &&
      error.errors.length === 2 &&
      fsErrorsMatch(error.errors[0], error.errors[1])
    ) {
      throw error.errors[0];
    }
    throw error;
  } finally {
    if (hadOwnFd && ownFdDescriptor) {
      Object.defineProperty(handle, "fd", ownFdDescriptor);
    } else {
      delete handle.fd;
    }
  }
}

function checkFsReadFileAborted(signal) {
  if (signal?.aborted) {
    throw new nodeAbortError(undefined, { cause: signal.reason });
  }
}

async function statFsReadFileHandle(handle) {
  const bindingFstat = nimbusInternalFsBinding?.fstat;
  if (typeof bindingFstat === "function") {
    return await bindingFstat(handle.fd, false);
  }
  return await handle.stat();
}

function statFieldsRepresentRegularFile(statFields, nodeFs) {
  if (Array.isArray(statFields)) {
    const sIfmt = nodeFs?.constants?.S_IFMT;
    const sIfreg = nodeFs?.constants?.S_IFREG;
    if (typeof sIfmt === "number" && typeof sIfreg === "number") {
      return (Number(statFields[1] ?? 0) & sIfmt) === sIfreg;
    }
    return false;
  }
  if (typeof statFields?.isFile === "function") {
    return statFields.isFile();
  }
  return statFields?.isFile === true;
}

function statFieldsSize(statFields) {
  if (Array.isArray(statFields)) {
    return Number(statFields[8] ?? 0);
  }
  return Number(statFields?.size ?? 0);
}

function fsErrorsMatch(left, right) {
  return !!(
    left &&
    right &&
    left !== right &&
    left.name === right.name &&
    left.message === right.message &&
    left.code === right.code
  );
}

async function readFsPromisePathHandle(handle, options, nodeFs) {
  const normalizedOptions = nodeFsGetOptions(options, { flag: "r" });
  const signal = normalizedOptions?.signal;
  const encoding = normalizedOptions?.encoding;
  const decoder = encoding ? new nodeStringDecoder(encoding) : null;

  checkFsReadFileAborted(signal);

  const statFields = await statFsReadFileHandle(handle);

  checkFsReadFileAborted(signal);

  let size = 0;
  let length = 0;
  if (statFieldsRepresentRegularFile(statFields, nodeFs)) {
    size = statFieldsSize(statFields);
    length = encoding ? Math.min(size, nodeFsUtilConstants.kReadFileBufferLength) : size;
  }
  if (length === 0) {
    length = nodeFsUtilConstants.kReadFileUnknownBufferLength;
  }

  if (size > nodeFsUtilConstants.kIoMaxLength) {
    throw new nodeErrFsFileTooLarge(size);
  }

  let totalRead = 0;
  const noSize = size === 0;
  let buffer = nodeBuffer.allocUnsafeSlow(length);
  let result = "";
  let offset = 0;
  let buffers;
  const chunkedRead = length > nodeFsUtilConstants.kReadFileBufferLength;

  while (true) {
    checkFsReadFileAborted(signal);

    if (chunkedRead) {
      length = Math.min(size - totalRead, nodeFsUtilConstants.kReadFileBufferLength);
    }

    const readResult = await handle.read(buffer, offset, length, -1);
    const bytesRead = readResult?.bytesRead ?? 0;
    totalRead += bytesRead;

    if (
      bytesRead === 0 ||
      totalRead === size ||
      (bytesRead !== buffer.length && !chunkedRead && !noSize)
    ) {
      const singleRead = bytesRead === totalRead;
      const bytesToCheck = chunkedRead ? totalRead : bytesRead;

      if (bytesToCheck !== buffer.length) {
        buffer = buffer.subarray(0, bytesToCheck);
      }

      if (!encoding) {
        if (noSize && !singleRead) {
          buffers.push(buffer);
          return nodeBuffer.concat(buffers, totalRead);
        }
        return buffer;
      }

      if (singleRead) {
        return buffer.toString(encoding);
      }
      result += decoder.end(buffer);
      return result;
    }

    const readBuffer = bytesRead !== buffer.length
      ? buffer.subarray(0, bytesRead)
      : buffer;
    if (encoding) {
      result += decoder.write(readBuffer);
    } else if (size !== 0) {
      offset = totalRead;
    } else {
      buffers ??= [];
      buffers.push(readBuffer);
      buffer = nodeBuffer.allocUnsafeSlow(nodeFsUtilConstants.kReadFileUnknownBufferLength);
    }
  }
}

function patchNodeFsReadSemantics(nodeProcess, nodeFs = undefined) {
  nodeFs ??= nodeProcess?.getBuiltinModule?.("fs");
  if (!nodeFs || typeof nodeFs !== "object") {
    return;
  }

  const originalRead = nodeFs.read;
  if (typeof originalRead === "function" && originalRead.__nimbusNormalizedLength !== true) {
    const patchedRead = function (
      fd,
      bufferOrOptionsOrCallback,
      offsetOrOptionsOrCallback,
      lengthOrCallback,
      position,
      callback,
    ) {
      if (
        arguments.length >= 5 &&
        ArrayBuffer.isView(bufferOrOptionsOrCallback) &&
        typeof offsetOrOptionsOrCallback === "number"
      ) {
        const normalizedLength = normalizeFsReadLength(
          bufferOrOptionsOrCallback,
          offsetOrOptionsOrCallback,
          lengthOrCallback,
        );
        if (normalizedLength !== lengthOrCallback) {
          return Reflect.apply(originalRead, this, [
            fd,
            bufferOrOptionsOrCallback,
            offsetOrOptionsOrCallback,
            normalizedLength,
            position,
            callback,
          ]);
        }
      }
      return Reflect.apply(originalRead, this, arguments);
    };
    Object.defineProperties(patchedRead, Object.getOwnPropertyDescriptors(originalRead));
    Object.defineProperty(patchedRead, "__nimbusNormalizedLength", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    nodeFs.read = patchedRead;
  }

  const originalReadSync = nodeFs.readSync;
  if (typeof originalReadSync === "function" && originalReadSync.__nimbusNormalizedLength !== true) {
    const patchedReadSync = function (
      fd,
      buffer,
      offsetOrOptions,
      length,
      position,
    ) {
      if (
        arguments.length >= 4 &&
        ArrayBuffer.isView(buffer) &&
        typeof offsetOrOptions === "number"
      ) {
        const normalizedLength = normalizeFsReadLength(buffer, offsetOrOptions, length);
        if (normalizedLength !== length) {
          return Reflect.apply(originalReadSync, this, [
            fd,
            buffer,
            offsetOrOptions,
            normalizedLength,
            position,
          ]);
        }
      }
      return Reflect.apply(originalReadSync, this, arguments);
    };
    Object.defineProperties(patchedReadSync, Object.getOwnPropertyDescriptors(originalReadSync));
    Object.defineProperty(patchedReadSync, "__nimbusNormalizedLength", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    nodeFs.readSync = patchedReadSync;
  }

  const originalFileHandleRead = nodeInternalFsFileHandle?.prototype?.read;
  if (
    typeof originalFileHandleRead === "function" &&
    originalFileHandleRead.__nimbusNormalizedLength !== true
  ) {
    const patchedFileHandleRead = function (
      bufferOrOptions,
      offsetOrOptions,
      length,
      position,
    ) {
      if (
        bufferOrOptions &&
        typeof bufferOrOptions === "object" &&
        ArrayBuffer.isView(bufferOrOptions.buffer) &&
        (bufferOrOptions.length === undefined || bufferOrOptions.length === null)
      ) {
        return Reflect.apply(originalFileHandleRead, this, [{
          ...bufferOrOptions,
          length: bufferOrOptions.buffer.byteLength - (bufferOrOptions.offset ?? 0),
        }]);
      }
      if (
        ArrayBuffer.isView(bufferOrOptions) &&
        typeof offsetOrOptions === "number" &&
        (length === undefined || length === null)
      ) {
        return Reflect.apply(originalFileHandleRead, this, [{
          buffer: bufferOrOptions,
          offset: offsetOrOptions,
          length: bufferOrOptions.byteLength - offsetOrOptions,
          position: position ?? null,
        }]);
      }
      return Reflect.apply(originalFileHandleRead, this, arguments);
    };
    Object.defineProperty(patchedFileHandleRead, "__nimbusNormalizedLength", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    nodeInternalFsFileHandle.prototype.read = patchedFileHandleRead;
  }

  const originalTruncate = nodeFs.truncate;
  if (typeof originalTruncate === "function" && originalTruncate.__nimbusOpenErrorPath !== true) {
    const patchedTruncate = function (path, lenOrCallback = 0, maybeCallback = undefined) {
      const callback = typeof lenOrCallback === "function" ? lenOrCallback : maybeCallback;
      if (typeof callback !== "function") {
        return Reflect.apply(originalTruncate, this, arguments);
      }
      const wrappedCallback = function (error, ...rest) {
        if (error && error.path === undefined) {
          const message = String(error.message ?? "");
          if (
            error.code === "ENOENT" ||
            message.includes("ENOENT") ||
            message.includes("os error 2")
          ) {
            const normalizedError = new Error(
              `ENOENT: no such file or directory, open '${path}'`,
            );
            normalizedError.code = "ENOENT";
            normalizedError.errno = -2;
            normalizedError.syscall = "open";
            normalizedError.path = path;
            return callback.call(this, normalizedError, ...rest);
          }
        }
        return callback.call(this, error, ...rest);
      };
      if (typeof lenOrCallback === "function") {
        return Reflect.apply(originalTruncate, this, [path, wrappedCallback]);
      }
      return Reflect.apply(originalTruncate, this, [path, lenOrCallback, wrappedCallback]);
    };
    Object.defineProperties(patchedTruncate, Object.getOwnPropertyDescriptors(originalTruncate));
    Object.defineProperty(patchedTruncate, "__nimbusOpenErrorPath", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    nodeFs.truncate = patchedTruncate;
  }

  const originalRmdir = nodeFs.rmdir;
  if (typeof originalRmdir === "function" && originalRmdir.__nimbusRemovedRecursive !== true) {
    const patchedRmdir = function (path, options = undefined, callback = undefined) {
      const normalizedOptions = typeof options === "function" ? undefined : options;
      const normalizedCallback = typeof options === "function" ? options : callback;
      if (hasRemovedRmdirRecursiveOption(normalizedOptions)) {
        throw createRemovedRmdirRecursiveError(normalizedOptions.recursive);
      }
      if (typeof normalizedCallback !== "function") {
        return Reflect.apply(originalRmdir, this, arguments);
      }
      PromiseResolve(runtimeFsRmdir(path)).then(
        () => normalizedCallback(),
        (error) => normalizedCallback(error),
      );
    };
    Object.defineProperties(patchedRmdir, Object.getOwnPropertyDescriptors(originalRmdir));
    Object.defineProperty(patchedRmdir, "__nimbusRemovedRecursive", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    nodeFs.rmdir = patchedRmdir;
  }

  const originalRmdirSync = nodeFs.rmdirSync;
  if (
    typeof originalRmdirSync === "function" &&
    originalRmdirSync.__nimbusRemovedRecursive !== true
  ) {
    const patchedRmdirSync = function (path, options = undefined) {
      if (hasRemovedRmdirRecursiveOption(options)) {
        throw createRemovedRmdirRecursiveError(options.recursive);
      }
      return runtimeFsRmdirSync(path);
    };
    Object.defineProperties(patchedRmdirSync, Object.getOwnPropertyDescriptors(originalRmdirSync));
    Object.defineProperty(patchedRmdirSync, "__nimbusRemovedRecursive", {
      value: true,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    nodeFs.rmdirSync = patchedRmdirSync;
  }

  const originalWatch = nodeFs.watch;
  if (typeof originalWatch === "function" && originalWatch[nimbusFsWatchPatched] !== true) {
    const patchedWatch = function (filename, optionsOrListener = undefined, listener = undefined) {
      const options = optionsOrListener !== null && typeof optionsOrListener === "object"
        ? optionsOrListener
        : listener !== null && typeof listener === "object"
        ? listener
        : undefined;
      if (options?.throwIfNoEntry !== false) {
        const watchPath = nodeFsGetValidatedPathToString(filename);
        try {
          nodeFs.statSync(watchPath);
        } catch (error) {
          if (error?.code === "ENOENT" || error?.code === "ENOTDIR") {
            throw createFsWatchNotFoundError(watchPath);
          }
          throw error;
        }
      }
      return Reflect.apply(originalWatch, this, arguments);
    };
    Object.defineProperties(patchedWatch, Object.getOwnPropertyDescriptors(originalWatch));
    Object.defineProperty(patchedWatch, nimbusFsWatchPatched, {
      value: true,
      configurable: false,
      enumerable: false,
      writable: false,
    });
    nodeFs.watch = patchedWatch;
  }

  const nodeFsPromiseTargets = getNodeFsPromiseTargets(nodeFs);
  const nodeFsCloseSync = nodeFs.closeSync;
  if (
    typeof nodeFsCloseSync === "function" &&
    nodeInternalFsFileHandle?.prototype &&
    nodeInternalFsFileHandle.prototype[nimbusFileHandleGcPatched] !== true
  ) {
    const originalFileHandleClose = nodeInternalFsFileHandle.prototype.close;
    const fileHandleGcRegistry = new FinalizationRegistry(({ fd }) => {
      if (!Number.isInteger(fd) || fd < 0) {
        return;
      }
      try {
        Reflect.apply(nodeFsCloseSync, nodeFs, [fd]);
      } catch (_error) {
        // The watchpoint only requires the warning delivery path; double-close
        // or already-closed descriptors are tolerated here.
      }
      const scheduleWarning = typeof globalThis.setImmediate === "function"
        ? globalThis.setImmediate.bind(globalThis)
        : queueMicrotask;
      scheduleWarning(() => {
        nodeProcess?.emitWarning?.(
          `Closing file descriptor ${fd} on garbage collection`,
          "Warning",
        );
        nodeProcess?.emitWarning?.(
          "Closing a FileHandle object on garbage collection is deprecated. " +
            "Please close FileHandle objects explicitly using " +
            "FileHandle.prototype.close(). In the future, an error will be " +
            "thrown if a file descriptor is closed during garbage collection.",
          "DeprecationWarning",
          "DEP0137",
        );
      });
    });

    if (typeof originalFileHandleClose === "function") {
      const patchedFileHandleClose = function () {
        fileHandleGcRegistry.unregister(this);
        return Reflect.apply(originalFileHandleClose, this, arguments);
      };
      Object.defineProperties(
        patchedFileHandleClose,
        Object.getOwnPropertyDescriptors(originalFileHandleClose),
      );
      nodeInternalFsFileHandle.prototype.close = patchedFileHandleClose;
    }

    for (const nodeFsPromises of nodeFsPromiseTargets) {
      const originalOpen = nodeFsPromises.open;
      if (
        typeof originalOpen === "function" &&
        originalOpen[nimbusFileHandleGcPatched] !== true
      ) {
        const patchedOpen = async function () {
          const handle = await Reflect.apply(originalOpen, this, arguments);
          const handleFd = isNimbusFileHandle(handle) &&
              typeof nimbusOriginalFileHandleFdGetter === "function"
            ? Reflect.apply(nimbusOriginalFileHandleFdGetter, handle, [])
            : undefined;
          if (Number.isInteger(handleFd) && handleFd >= 0) {
            fileHandleGcRegistry.register(handle, { fd: handleFd }, handle);
          }
          return handle;
        };
        Object.defineProperties(patchedOpen, Object.getOwnPropertyDescriptors(originalOpen));
        Object.defineProperty(patchedOpen, nimbusFileHandleGcPatched, {
          value: true,
          configurable: false,
          enumerable: false,
          writable: false,
        });
        nodeFsPromises.open = patchedOpen;
      }
    }

    Object.defineProperty(nodeInternalFsFileHandle.prototype, nimbusFileHandleGcPatched, {
      value: true,
      configurable: false,
      enumerable: false,
      writable: false,
    });
  }

  for (const nodeFsPromises of nodeFsPromiseTargets) {
    if (
      !nodeFsPromises ||
      typeof nodeFsPromises !== "object" ||
      nodeFsPromises[nimbusFsPromisesLifecyclePatched] === true
    ) {
      continue;
    }

    const originalReadFile = nodeFsPromises.readFile;
    if (typeof originalReadFile === "function") {
      const patchedReadFile = function (path, options = undefined) {
        if (typeof path === "number" || isNimbusFileHandle(path)) {
          return Reflect.apply(originalReadFile, this, arguments);
        }
        return Promise.resolve().then(() => {
          const normalizedOptions = nodeFsGetOptions(options, { flag: "r" });
          checkFsReadFileAborted(normalizedOptions?.signal);
          return nodeFsPromises
            .open(path, getFsPromisesFlag(normalizedOptions, "r"))
            .then((handle) =>
              handleFsPromisePathClose(
                readFsPromisePathHandle(handle, normalizedOptions, nodeFs),
                () => closeNimbusFileHandle(handle),
              )
            );
        });
      };
      Object.defineProperties(patchedReadFile, Object.getOwnPropertyDescriptors(originalReadFile));
      nodeFsPromises.readFile = patchedReadFile;
    }

    const originalWriteFile = nodeFsPromises.writeFile;
    if (typeof originalWriteFile === "function") {
      const patchedWriteFile = function (path, data, options = undefined) {
        if (typeof path === "number" || isNimbusFileHandle(path)) {
          return Reflect.apply(originalWriteFile, this, arguments);
        }
        return nodeFsPromises
          .open(path, getFsPromisesFlag(options, "w"))
          .then((handle) =>
            handleFsPromisePathClose(
              handle.writeFile(data, options),
              () => closeNimbusFileHandle(handle),
            )
          );
      };
      Object.defineProperties(
        patchedWriteFile,
        Object.getOwnPropertyDescriptors(originalWriteFile),
      );
      nodeFsPromises.writeFile = patchedWriteFile;
    }

    const originalTruncate = nodeFsPromises.truncate;
    if (typeof originalTruncate === "function") {
      const patchedTruncate = function (path, len = 0) {
        if (typeof path === "number" || isNimbusFileHandle(path)) {
          return Reflect.apply(originalTruncate, this, arguments);
        }
        return nodeFsPromises
          .open(path, "r+")
          .then((handle) =>
            handleFsPromisePathClose(
              handle.truncate(len),
              () => closeNimbusFileHandle(handle),
            )
          );
      };
      Object.defineProperties(
        patchedTruncate,
        Object.getOwnPropertyDescriptors(originalTruncate),
      );
      nodeFsPromises.truncate = patchedTruncate;
    }

    const originalLchmod = nodeFsPromises.lchmod;
    if (typeof originalLchmod === "function") {
      const patchedLchmod = async function (path, mode) {
        if (typeof path === "number" || isNimbusFileHandle(path)) {
          return await Reflect.apply(originalLchmod, this, arguments);
        }
        const validatedPath = nodeFsGetValidatedPathToString(path);
        const validatedMode = nodeParseFileMode(mode, "mode");
        return await deno.lchmod(validatedPath, validatedMode);
      };
      Object.defineProperties(patchedLchmod, Object.getOwnPropertyDescriptors(originalLchmod));
      nodeFsPromises.lchmod = patchedLchmod;
    }

    const originalRmdir = nodeFsPromises.rmdir;
    if (
      typeof originalRmdir === "function" &&
      originalRmdir.__nimbusRemovedRecursive !== true
    ) {
      const patchedRmdir = async function (path, options = undefined) {
        if (hasRemovedRmdirRecursiveOption(options)) {
          throw createRemovedRmdirRecursiveError(options.recursive);
        }
        return await runtimeFsRmdir(path);
      };
      Object.defineProperties(patchedRmdir, Object.getOwnPropertyDescriptors(originalRmdir));
      Object.defineProperty(patchedRmdir, "__nimbusRemovedRecursive", {
        value: true,
        configurable: true,
        enumerable: false,
        writable: false,
      });
      nodeFsPromises.rmdir = patchedRmdir;
    }

    const originalWatch = nodeFsPromises.watch;
    if (
      typeof originalWatch === "function" &&
      originalWatch[nimbusFsPromisesWatchPatched] !== true
    ) {
      const patchedWatch = function (path, options) {
        const normalizedPath = nodeFsGetValidatedPathToString(path);
        const { builtin, maxQueue, overflow, signal } =
          validateFsPromisesWatchOptions(options);
        const watcher = nodeFs.watch(normalizedPath, builtin);
        let closed = false;
        let pendingAbortError = null;
        const queue = [];
        const pending = [];

        const settleNext = (entry) => {
          const waiter = pending.shift();
          if (waiter) {
            waiter(entry);
            return;
          }
          if (entry.kind === "value" && queue.length >= maxQueue) {
            if (overflow === "error") {
              queue.length = 0;
              queue.push({
                kind: "error",
                value: createFsPromisesWatchQueueOverflowError(maxQueue),
              });
            } else {
              nodeProcess?.emitWarning?.("fs.watch maxQueue exceeded");
            }
            return;
          }
          queue.push(entry);
        };

        const closeWatcher = () => {
          if (closed) {
            return;
          }
          closed = true;
          watcher.close();
        };

        watcher.on("change", (eventType, filename) => {
          settleNext({
            kind: "value",
            value: { eventType, filename },
          });
        });
        watcher.on("error", (error) => {
          settleNext({ kind: "error", value: error });
        });
        watcher.on("close", () => {
          if (pendingAbortError !== null) {
            settleNext({ kind: "error", value: pendingAbortError });
            pendingAbortError = null;
            return;
          }
          settleNext({ kind: "done", value: undefined });
        });

        if (signal !== undefined) {
          if (signal.aborted) {
            pendingAbortError = createFsPromisesWatchAbortError(signal.reason);
            nodeProcess?.nextTick?.(() => closeWatcher());
          } else {
            signal.addEventListener("abort", () => {
              pendingAbortError = createFsPromisesWatchAbortError(signal.reason);
              closeWatcher();
            }, { once: true });
          }
        }

        return {
          async next() {
            if (queue.length > 0) {
              const entry = queue.shift();
              if (entry.kind === "value") {
                return { value: entry.value, done: false };
              }
              if (entry.kind === "done") {
                return { value: undefined, done: true };
              }
              throw entry.value;
            }
            return await new Promise((resolve, reject) => {
              pending.push((entry) => {
                if (entry.kind === "value") {
                  resolve({ value: entry.value, done: false });
                  return;
                }
                if (entry.kind === "done") {
                  resolve({ value: undefined, done: true });
                  return;
                }
                reject(entry.value);
              });
            });
          },
          return(value) {
            closeWatcher();
            return PromiseResolve({ value, done: true });
          },
          [SymbolAsyncIterator]() {
            return this;
          },
        };
      };
      Object.defineProperties(patchedWatch, Object.getOwnPropertyDescriptors(originalWatch));
      Object.defineProperty(patchedWatch, nimbusFsPromisesWatchPatched, {
        value: true,
        configurable: false,
        enumerable: false,
        writable: false,
      });
      nodeFsPromises.watch = patchedWatch;
    }

    Object.defineProperty(nodeFsPromises, nimbusFsPromisesLifecyclePatched, {
      value: true,
      configurable: false,
      enumerable: false,
      writable: false,
    });
  }
}

function seedNodeProcessLoadEnvFile(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

  const originalLoadEnvFile = nodeProcess.loadEnvFile;
  if (typeof originalLoadEnvFile !== "function") {
    return;
  }

  seedNodeProcessEnvOverlay(nodeProcess);

  function patchedLoadEnvFile(path = undefined) {
    const resolvedPath = resolveLoadEnvFilePath(nodeProcess, path);
    const displayPath = displayLoadEnvFilePath(path);
    return loadEnvFileThroughNimbusHost(nodeProcess, resolvedPath, displayPath);
  }

  Object.defineProperty(patchedLoadEnvFile, nimbusLoadEnvFilePatched, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  Object.defineProperty(nodeProcess, "loadEnvFile", {
    value: patchedLoadEnvFile,
    configurable: true,
    enumerable: true,
    writable: true,
  });
}

function createNodeCompatibleSetImmediate(setImmediateImpl) {
  function nimbusSetImmediate(callback, ...args) {
    if (typeof callback !== "function") {
      return setImmediateImpl(callback, ...args);
    }

    let handle;
    handle = setImmediateImpl(function (...callbackArgs) {
      return Reflect.apply(callback, handle, callbackArgs);
    }, ...args);
    return handle;
  }

  for (const property of Reflect.ownKeys(setImmediateImpl)) {
    const descriptor = Object.getOwnPropertyDescriptor(setImmediateImpl, property);
    if (descriptor) {
      Object.defineProperty(nimbusSetImmediate, property, descriptor);
    }
  }
  Object.defineProperty(nimbusSetImmediate, "name", {
    value: "setImmediate",
    configurable: true,
  });
  return nimbusSetImmediate;
}

function seedNodeGlobalTimers(nodeGlobals) {
  if (!nodeGlobals || typeof nodeGlobals !== "object") {
    return;
  }

  // Node22 compatibility must prefer the Node-family timer globals even when
  // the embedded runtime already has web timer functions. Leaving the web
  // versions in place breaks callback `this` binding and other Node timer
  // semantics across the whole timers family.
  for (const property of [
    "setImmediate",
    "clearImmediate",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
  ]) {
    const timer = nodeGlobals[property] ?? nodeTimersBuiltin[property];
    if (timer === undefined) {
      continue;
    }
    const value = property === "setImmediate"
      ? createNodeCompatibleSetImmediate(timer)
      : timer;
    Object.defineProperty(globalThis, property, {
      value,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }

  if (typeof globalThis.global === "undefined" && nodeGlobals.global !== undefined) {
    Object.defineProperty(globalThis, "global", {
      value: nodeGlobals.global,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }
}

const nimbusGlobalEventTarget = new EventTarget();

function seedGlobalEventTargetSurface() {
  const bindings = {
    addEventListener: nimbusGlobalEventTarget.addEventListener.bind(nimbusGlobalEventTarget),
    removeEventListener: nimbusGlobalEventTarget.removeEventListener.bind(nimbusGlobalEventTarget),
    dispatchEvent: nimbusGlobalEventTarget.dispatchEvent.bind(nimbusGlobalEventTarget),
  };

  for (const [property, value] of Object.entries(bindings)) {
    if (typeof globalThis[property] === "function") {
      continue;
    }
    Object.defineProperty(globalThis, property, {
      value,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }
}

function nimbusRejectionDomain(value) {
  if (value === null || (typeof value !== "object" && typeof value !== "function")) {
    return undefined;
  }
  try {
    return value.domain;
  } catch {
    return undefined;
  }
}

function processUnhandledPromiseRejection(promise, reason) {
  const rejectionEvent = new WebPromiseRejectionEvent("unhandledrejection", {
    cancelable: true,
    promise,
    reason,
  });
  const hasNodeDomain = nimbusRejectionDomain(promise) ||
    nimbusRejectionDomain(reason);

  if (
    hasNodeDomain &&
    typeof internals.nodeProcessUnhandledRejectionCallback !== "undefined"
  ) {
    internals.nodeProcessUnhandledRejectionCallback(rejectionEvent);
    if (rejectionEvent.defaultPrevented) {
      return true;
    }
  }

  globalThis.dispatchEvent(rejectionEvent);

  if (
    !rejectionEvent.defaultPrevented &&
    !hasNodeDomain &&
    typeof internals.nodeProcessUnhandledRejectionCallback !== "undefined"
  ) {
    internals.nodeProcessUnhandledRejectionCallback(rejectionEvent);
  }

  return rejectionEvent.defaultPrevented;
}

function processRejectionHandled(promise, reason) {
  const rejectionHandledEvent = new WebPromiseRejectionEvent(
    "rejectionhandled",
    { promise, reason },
  );

  globalThis.dispatchEvent(rejectionHandledEvent);

  if (typeof internals.nodeProcessRejectionHandledCallback !== "undefined") {
    internals.nodeProcessRejectionHandledCallback(rejectionHandledEvent);
  }
}

function installNimbusDomainPromiseRejectPatch() {
  if (Promise.reject.__nimbusDomainAware === true) {
    return;
  }
  const originalReject = Promise.reject;
  function reject(reason) {
    const promise = originalReject.apply(this, arguments);
    const domain = globalThis.process?.domain ?? nodeProcessBuiltin?.domain;
    if (domain !== null && domain !== undefined) {
      Object.defineProperty(promise, "domain", {
        configurable: true,
        enumerable: false,
        value: domain,
        writable: true,
      });
      if (reason !== null && typeof reason === "object") {
        Object.defineProperty(reason, "domain", {
          configurable: true,
          enumerable: false,
          value: domain,
          writable: true,
        });
        if (reason.domainThrown === undefined) {
          reason.domainThrown = true;
        }
      }
    }
    return promise;
  }
  Object.defineProperty(reject, "__nimbusDomainAware", {
    configurable: false,
    enumerable: false,
    value: true,
    writable: false,
  });
  Object.defineProperty(Promise, "reject", {
    configurable: true,
    enumerable: false,
    value: reject,
    writable: true,
  });
}

function runtimeWorkerNormalizeTransferList(transferOrOptions) {
  if (transferOrOptions === undefined || transferOrOptions === null) {
    return [];
  }
  if (ArrayIsArray(transferOrOptions)) {
    return transferOrOptions;
  }
  if (
    typeof transferOrOptions === "object" &&
    transferOrOptions !== null &&
    ArrayIsArray(transferOrOptions.transfer)
  ) {
    return transferOrOptions.transfer;
  }
  return [];
}

function runtimeWorkerExtractMessagePorts(transferables) {
  if (!ArrayIsArray(transferables) || transferables.length === 0) {
    return [];
  }
  return transferables.filter((candidate) =>
    ObjectPrototypeIsPrototypeOf(webMessagePortPrototype, candidate)
  );
}

function seedWorkerThreadHostSurface(workerBootstrapState) {
  if (!workerBootstrapState || workerBootstrapState.runningOnMainThread !== false) {
    return;
  }

  let messageListenerCount = 0;
  const queuedIncomingMessages = [];
  const trackedMessageListeners = new WeakSet();
  const trackedOnceMessageListeners = new WeakMap();
  const nativeAddEventListener = globalThis.addEventListener.bind(globalThis);
  const nativeRemoveEventListener = globalThis.removeEventListener.bind(globalThis);
  const hasMessageConsumer = () => messageListenerCount > 0;
  const messageListenerWantsOnce = (options) =>
    options !== null &&
    typeof options === "object" &&
    options.once === true;
  const dispatchIncomingMessage = (data) => {
    const [message, transferables] = webDeserializeJsMessageData(data);
    const event = new MessageEvent("message", {
      cancelable: false,
      data: message,
      ports: runtimeWorkerExtractMessagePorts(transferables),
    });
    globalThis.dispatchEvent(event);
  };
  const drainQueuedIncomingMessages = () => {
    if (!hasMessageConsumer()) {
      return;
    }
    while (queuedIncomingMessages.length > 0) {
      dispatchIncomingMessage(queuedIncomingMessages.shift());
    }
  };

  globalThis.addEventListener = function addEventListener(name, listener, options) {
    let targetListener = listener;
    if (
      name === "message" &&
      listener &&
      !trackedMessageListeners.has(listener) &&
      messageListenerWantsOnce(options)
    ) {
      targetListener = (event) => {
        try {
          if (typeof listener === "function") {
            listener(event);
          } else {
            listener.handleEvent?.(event);
          }
        } finally {
          if (trackedMessageListeners.has(listener)) {
            trackedMessageListeners.delete(listener);
            trackedOnceMessageListeners.delete(listener);
            messageListenerCount = Math.max(0, messageListenerCount - 1);
          }
        }
      };
      trackedOnceMessageListeners.set(listener, targetListener);
    }
    nativeAddEventListener(name, targetListener, options);
    if (name === "message" && listener && !trackedMessageListeners.has(listener)) {
      trackedMessageListeners.add(listener);
      messageListenerCount += 1;
      drainQueuedIncomingMessages();
    }
  };

  globalThis.removeEventListener = function removeEventListener(name, listener, options) {
    const targetListener = trackedOnceMessageListeners.get(listener) ?? listener;
    nativeRemoveEventListener(name, targetListener, options);
    if (name === "message" && listener && trackedMessageListeners.has(listener)) {
      trackedMessageListeners.delete(listener);
      trackedOnceMessageListeners.delete(listener);
      messageListenerCount = Math.max(0, messageListenerCount - 1);
    }
  };

  Object.defineProperty(globalThis, "postMessage", {
    value(message, transferOrOptions = undefined) {
      const transferList = runtimeWorkerNormalizeTransferList(transferOrOptions);
      if (transferList.length === 0) {
        core.ops.op_nimbus_worker_parent_post_message_raw(core.serialize(message));
        return;
      }
      const data = webSerializeJsMessageData(message, transferList);
      core.ops.op_nimbus_worker_parent_post_message(data);
    },
    configurable: true,
    enumerable: false,
    writable: true,
  });

  let pumpStarted = false;
  Object.defineProperty(globalThis, "__nimbusStartWorkerMessagePump", {
    value() {
      if (pumpStarted) {
        return;
      }
      pumpStarted = true;
      const closeOnIdle = workerBootstrapState.closeOnIdle === true;
      let startupTurnPending = true;
      let currentRecvMessage = null;
      const maybeUnrefCurrentRecvMessage = () => {
        if (
          closeOnIdle &&
          currentRecvMessage &&
          !hasRefedMessageListener()
        ) {
          core.unrefOpPromise(currentRecvMessage);
        }
      };
      setTimeout(() => {
        startupTurnPending = false;
        maybeUnrefCurrentRecvMessage();
      }, 0);
      const hasRefedMessageListener = () =>
        startupTurnPending ||
        (messageListenerCount > 0 && globalThis[webUnrefParentPort] !== true);

      PromiseResolve().then(async () => {
        while (true) {
          currentRecvMessage = core.ops.op_nimbus_worker_parent_recv_message();
          maybeUnrefCurrentRecvMessage();
          const data = await currentRecvMessage;
          currentRecvMessage = null;
          if (data === null) {
            break;
          }
          if (!hasMessageConsumer()) {
            queuedIncomingMessages.push(data);
          } else {
            dispatchIncomingMessage(data);
          }
          for (let index = 0; index < 1000; index += 1) {
            const syncData = core.ops.op_nimbus_worker_parent_recv_message_sync();
            if (syncData === null) {
              break;
            }
            if (!hasMessageConsumer()) {
              queuedIncomingMessages.push(syncData);
            } else {
              dispatchIncomingMessage(syncData);
            }
          }
        }
      });
    },
    configurable: true,
    enumerable: false,
    writable: false,
  });
}

function seedGlobalPerformance() {
  if (typeof globalThis.performance !== "undefined") {
    return;
  }
  Object.defineProperty(globalThis, "performance", {
    value: webPerformance,
    configurable: true,
    enumerable: false,
    writable: true,
  });
}

const nimbusNodeConsoleUpgraded = Symbol("nimbus.nodeConsoleUpgraded");

function upgradeGlobalConsole(nodeProcess) {
  const runtimeConsole = globalThis.console;
  if (
    !runtimeConsole ||
    typeof runtimeConsole !== "object" ||
    !nodeProcess ||
    typeof nodeProcess !== "object"
  ) {
    return;
  }

  if (runtimeConsole[nimbusNodeConsoleUpgraded] === true) {
    return;
  }

  for (const propertyKey of Reflect.ownKeys(NodeConsole.prototype)) {
    if (propertyKey === "constructor") {
      continue;
    }
    const descriptor = Object.getOwnPropertyDescriptor(
      NodeConsole.prototype,
      propertyKey,
    );
    if (descriptor) {
      Object.defineProperty(runtimeConsole, propertyKey, descriptor);
    }
  }

  bindNodeConsoleStreamsLazy(runtimeConsole, nodeProcess);
  runtimeConsole[nodeConsoleBindProperties](true, "auto");

  for (const methodName of Object.keys(NodeConsole.prototype)) {
    const boundMethod = NodeConsole.prototype[methodName].bind(runtimeConsole);
    Object.defineProperty(boundMethod, "name", {
      value: methodName,
      configurable: true,
    });
    Object.defineProperty(runtimeConsole, methodName, {
      value: boundMethod,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }

  const clearMethod = {
    clear() {
      const stream = nodeProcess?.stdout;
      if (
        stream?.isTTY &&
        nodeProcess?.env?.TERM !== "dumb" &&
        typeof stream.write === "function"
      ) {
        stream.write("\x1b[1;1H");
        stream.write("\x1b[0J");
      }
    },
  }.clear;
  Object.defineProperty(runtimeConsole, "clear", {
    value: clearMethod,
    configurable: true,
    enumerable: false,
    writable: true,
  });

  Object.defineProperty(runtimeConsole, "Console", {
    value: NodeConsole,
    configurable: true,
    enumerable: true,
    writable: true,
  });
  Object.defineProperty(runtimeConsole, nimbusNodeConsoleUpgraded, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

const nimbusWarningHandlerInstalled = Symbol("nimbus.warningHandlerInstalled");

function safeNodeProcessOnWarning(nodeProcess, warning) {
  try {
    nodeProcessOnWarning(warning);
  } catch (error) {
    if (!(warning instanceof Error) || typeof nodeProcess?.stderr?.write !== "function") {
      throw error;
    }

    let message = `(${nodeProcess.release?.name ?? "node"}:${nodeProcess.pid ?? 0}) `;
    if (typeof warning.code === "string" && warning.code.length > 0) {
      message += `[${warning.code}] `;
    }
    const name =
      typeof warning.name === "string" && warning.name.length > 0
        ? warning.name
        : "Warning";
    const detail =
      typeof warning.message === "string" && warning.message.length > 0
        ? warning.message
        : "";
    message += detail.length > 0 ? `${name}: ${detail}` : name;
    if (typeof warning.detail === "string" && warning.detail.length > 0) {
      message += `\n${warning.detail}`;
    }
    nodeProcess.stderr.write(`${message}\n`);
  }
}

function seedNodeProcessWarnings(nodeProcess) {
  if (
    !nodeProcess ||
    typeof nodeProcess !== "object" ||
    typeof nodeProcess.on !== "function"
  ) {
    return;
  }

  if (nodeProcess[nimbusWarningHandlerInstalled] === true) {
    return;
  }

  nodeProcess.on("warning", (warning) => safeNodeProcessOnWarning(nodeProcess, warning));
  Object.defineProperty(nodeProcess, nimbusWarningHandlerInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

const embeddedDenoTests = [];

function normalizeEmbeddedDenoTestDefinition(definition, maybeFn = undefined) {
  if (typeof definition === "function") {
    return {
      name: definition.name || "<anonymous>",
      fn: definition,
      ignore: false,
    };
  }

  if (typeof definition === "string") {
    return {
      name: definition,
      fn: typeof maybeFn === "function" ? maybeFn : async () => undefined,
      ignore: false,
    };
  }

  if (definition && typeof definition === "object") {
    return {
      name:
        typeof definition.name === "string" && definition.name.length > 0
          ? definition.name
          : (typeof maybeFn === "function" && maybeFn.name.length > 0
            ? maybeFn.name
            : "<anonymous>"),
      fn:
        typeof definition.fn === "function"
          ? definition.fn
          : (typeof maybeFn === "function" ? maybeFn : async () => undefined),
      ignore: definition.ignore === true,
    };
  }

  return {
    name: typeof maybeFn === "function" && maybeFn.name.length > 0
      ? maybeFn.name
      : "<anonymous>",
    fn: typeof maybeFn === "function" ? maybeFn : async () => undefined,
    ignore: false,
  };
}

function createEmbeddedDenoTestContext(name) {
  return {
    name,
    async step(stepDefinition, maybeFn = undefined) {
      const normalized = normalizeEmbeddedDenoTestDefinition(stepDefinition, maybeFn);
      if (normalized.ignore) {
        return false;
      }

      await normalized.fn(createEmbeddedDenoTestContext(normalized.name));
      return true;
    },
  };
}

function createEmbeddedDenoTestRandom(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

function shuffleEmbeddedDenoTestsInPlace(definitions, seed) {
  const random = createEmbeddedDenoTestRandom(seed);
  for (let index = definitions.length - 1; index > 0; index -= 1) {
    const swapIndex = Math.floor(random() * (index + 1));
    const current = definitions[index];
    definitions[index] = definitions[swapIndex];
    definitions[swapIndex] = current;
  }
}

async function flushEmbeddedDenoTests(options = undefined) {
  const continueOnError = options?.continueOnError === true;
  const requestedRandomization = options?.randomize === true
    ? options
    : globalThis.__nimbusEmbeddedTestRandomization;
  while (embeddedDenoTests.length > 0) {
    const pending = embeddedDenoTests.splice(0);
    if (requestedRandomization?.enabled === true && pending.length > 1) {
      const seed = typeof requestedRandomization.seed === "number"
        ? requestedRandomization.seed >>> 0
        : 0;
      shuffleEmbeddedDenoTestsInPlace(pending, seed);
    }
    for (const definition of pending) {
      try {
        await definition.fn(createEmbeddedDenoTestContext(definition.name));
      } catch (err) {
        if (!continueOnError) {
          throw err;
        }
      }
    }
  }
}

const deno = hiddenDenoGlobals;
const internalSymbol = deno.internal ?? Symbol("Deno.internal");
const internals = coreInternals;
Object.defineProperty(globalThis, "__nimbusHiddenDenoGlobals", {
  value: deno,
  configurable: false,
  enumerable: false,
  writable: false,
});
Object.defineProperty(globalThis, "__nimbusHiddenNodeGlobals", {
  value: hiddenNodeGlobals,
  configurable: false,
  enumerable: false,
  writable: false,
});
if (internals.nodeGlobals === undefined) {
  internals.nodeGlobals = hiddenNodeGlobals;
}
installScopedNodeSpawnSyncChild();
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

function seedHiddenDenoMethod(methodName, method) {
  if (typeof deno[methodName] === "function" || typeof method !== "function") {
    return;
  }
  Object.defineProperty(deno, methodName, {
    value: method,
    configurable: true,
    enumerable: true,
    writable: false,
  });
}

function denoMemoryUsage() {
  op_runtime_memory_usage(denoMemoryUsageBuffer);
  const {
    0: rss,
    1: heapTotal,
    2: heapUsed,
    3: external,
  } = denoMemoryUsageBuffer;
  return {
    rss,
    heapTotal,
    heapUsed,
    external,
  };
}

function createNimbusSharedWorkerEnvProxy() {
  return new Proxy(Object.create(null), {
    get(_target, prop) {
      if (typeof prop === "symbol") {
        return undefined;
      }
      return core.ops.op_nimbus_runtime_shared_env_get(String(prop));
    },
    ownKeys() {
      return Reflect.ownKeys(core.ops.op_nimbus_runtime_shared_env_snapshot());
    },
    getOwnPropertyDescriptor(_target, prop) {
      if (typeof prop === "symbol") {
        return undefined;
      }
      const value = core.ops.op_nimbus_runtime_shared_env_get(String(prop));
      if (value === undefined) {
        return undefined;
      }
      return {
        configurable: true,
        enumerable: true,
        value,
        writable: true,
      };
    },
    has(_target, prop) {
      if (typeof prop === "symbol") {
        return false;
      }
      return core.ops.op_nimbus_runtime_shared_env_get(String(prop)) !== undefined;
    },
    set(_target, prop, value) {
      if (typeof prop === "symbol" || typeof value === "symbol") {
        throw new TypeError("Cannot convert a Symbol value to a string");
      }
      core.ops.op_nimbus_runtime_shared_env_set(String(prop), String(value));
      return true;
    },
    deleteProperty(_target, prop) {
      if (typeof prop === "symbol") {
        return true;
      }
      core.ops.op_nimbus_runtime_shared_env_delete(String(prop));
      return true;
    },
    defineProperty(_target, prop, attributes) {
      if (typeof prop === "symbol") {
        return true;
      }
      core.ops.op_nimbus_runtime_shared_env_set(
        String(prop),
        String(attributes?.value),
      );
      return true;
    },
  });
}

function seedNodeClusterWorkerIfNeeded(workerBootstrapState, workerMetadataObject) {
  if (workerBootstrapState?.runningOnMainThread !== false) {
    return;
  }

  const processEnv = globalThis.process?.env;
  const uniqueId = workerMetadataObject?.env?.NODE_UNIQUE_ID ??
    processEnv?.NODE_UNIQUE_ID;
  if (typeof uniqueId !== "string" || uniqueId.length === 0) {
    return;
  }

  const schedulingPolicy = workerMetadataObject?.env?.NODE_CLUSTER_SCHED_POLICY ??
    processEnv?.NODE_CLUSTER_SCHED_POLICY;
  try {
    const clusterModule = core.loadExtScript("ext:deno_node/cluster.ts");
    if (clusterModule?.default?.isWorker === true) {
      return;
    }
    if (typeof internals.__initCluster === "function") {
      internals.__initCluster(uniqueId, schedulingPolicy);
    }
  } catch {
    // Cluster is optional for most embedded workers; user code will surface
    // any real load error if it later imports node:cluster.
  }
}

Object.defineProperty(globalThis, "__nimbusCloseWorker", {
  value: () => {
    if (typeof core.ops.op_worker_close === "function") {
      core.ops.op_worker_close();
      return;
    }
    if (typeof globalThis.close === "function") {
      globalThis.close();
    }
  },
  configurable: true,
  enumerable: false,
  writable: true,
});

function installNimbusSharedWorkerEnvProxy() {
  const snapshot = Object.create(null);
  const currentEnv =
    globalThis.process && typeof globalThis.process === "object"
      ? globalThis.process.env
      : undefined;
  if (currentEnv && typeof currentEnv === "object") {
    for (const key of Object.keys(currentEnv)) {
      snapshot[key] = String(currentEnv[key]);
    }
  }
  core.ops.op_nimbus_runtime_shared_env_seed(snapshot);
  const sharedEnv = createNimbusSharedWorkerEnvProxy();
  if (globalThis.process && typeof globalThis.process === "object") {
    Object.defineProperty(globalThis.process, "env", {
      value: sharedEnv,
      configurable: true,
      enumerable: true,
      writable: true,
    });
  }
  if (
    internals.nodeGlobals?.process &&
    typeof internals.nodeGlobals.process === "object"
  ) {
    Object.defineProperty(internals.nodeGlobals.process, "env", {
      value: sharedEnv,
      configurable: true,
      enumerable: true,
      writable: true,
    });
  }
  return sharedEnv;
}

Object.defineProperty(globalThis, "__nimbusInstallSharedWorkerEnvProxy", {
  value: installNimbusSharedWorkerEnvProxy,
  configurable: true,
  enumerable: false,
  writable: true,
});

seedHiddenDenoMethod("hostname", denoHostname);
seedHiddenDenoMethod("loadavg", denoLoadavg);
seedHiddenDenoMethod("memoryUsage", denoMemoryUsage);
seedHiddenDenoMethod("networkInterfaces", denoNetworkInterfaces);
seedHiddenDenoMethod("osRelease", denoOsRelease);
seedHiddenDenoMethod("systemMemoryInfo", denoSystemMemoryInfo);

Object.defineProperty(deno, "cwd", {
  value() {
    return globalThis.process?.cwd?.() ?? "/";
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
if (typeof core.ops.op_uid === "function") {
  Object.defineProperty(deno, "uid", {
    value() {
      return core.ops.op_uid();
    },
    configurable: true,
    enumerable: true,
    writable: false,
  });
}
if (typeof core.ops.op_gid === "function") {
  Object.defineProperty(deno, "gid", {
    value() {
      return core.ops.op_gid();
    },
    configurable: true,
    enumerable: true,
    writable: false,
  });
}
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
Object.defineProperty(deno, "remove", {
  value: runtimeFsRemove,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "removeSync", {
  value: runtimeFsRemoveSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "chmod", {
  value: runtimeFsChmod,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "chmodSync", {
  value: runtimeFsChmodSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "lchmod", {
  value: runtimeFsLchmod,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "lchmodSync", {
  value: runtimeFsLchmodSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "chown", {
  value: runtimeFsChown,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "chownSync", {
  value: runtimeFsChownSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "copyFile", {
  value: runtimeFsCopyFile,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "copyFileSync", {
  value: runtimeFsCopyFileSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "link", {
  value: runtimeFsLink,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "linkSync", {
  value: runtimeFsLinkSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "symlink", {
  value: runtimeFsSymlink,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "symlinkSync", {
  value: runtimeFsSymlinkSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "readLink", {
  value: runtimeFsReadLink,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "readLinkSync", {
  value: runtimeFsReadLinkSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "utime", {
  value: runtimeFsUtime,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "utimeSync", {
  value: runtimeFsUtimeSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "watchFs", {
  value: denoWatchFs,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "realPath", {
  value: denoRealPath,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "realPathSync", {
  value: denoRealPathSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "rename", {
  value: runtimeFsRename,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "renameSync", {
  value: runtimeFsRenameSync,
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "execPath", {
  value() {
    return core.ops.op_nimbus_runtime_exec_path();
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "env", {
  value: {
    get(name) {
      const operation = core.ops?.op_nimbus_runtime_env_get;
      if (typeof operation !== "function") {
        return undefined;
      }
      const result = operation(String(name));
      if (result?.status === "allowed") {
        return result.value;
      }
      if (result?.status === "missing" || result?.status === "denied") {
        return undefined;
      }
      throw new Error(result?.message ?? `runtime env capability denied for ${name}`);
    },
    toObject() {
      const operation = core.ops?.op_nimbus_runtime_env_snapshot;
      return typeof operation === "function" ? operation() : {};
    },
    set(name, value) {
      const processEnv = globalThis.process?.env;
      const marker = globalThis.Symbol.for("nimbus.processEnvProxy");
      if (processEnv?.[marker] === true) {
        processEnv[String(name)] = String(value);
      }
    },
    delete(name) {
      const processEnv = globalThis.process?.env;
      const marker = globalThis.Symbol.for("nimbus.processEnvProxy");
      if (processEnv?.[marker] === true) {
        delete globalThis.process.env[String(name)];
      }
    },
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "version", {
  value: {
    deno: "2.9.0-nimbus.1",
    v8: "149.4.0-nimbus.10",
    typescript: "0.0.0-nimbus",
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(deno, "test", {
  value(testDefinition, maybeFn = undefined) {
    const normalized = normalizeEmbeddedDenoTestDefinition(
      testDefinition,
      maybeFn,
    );
    if (normalized.ignore) {
      return undefined;
    }

    embeddedDenoTests.push(normalized);
    return undefined;
  },
  configurable: true,
  enumerable: true,
  writable: false,
});
Object.defineProperty(globalThis, "Deno", {
  value: deno,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(globalThis, "__nimbusRetainDenoForNodeLazyScripts", {
  value: true,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(globalThis, "__nimbusFlushEmbeddedTests", {
  value: flushEmbeddedDenoTests,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(globalThis, "__nimbusProcessTicksAndRejections", {
  value: core.processTicksAndRejections,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(globalThis, "__nimbusEventLoopHasMoreWork", {
  value: core.eventLoopHasMoreWork,
  configurable: true,
  enumerable: false,
  writable: false,
});
Object.defineProperty(globalThis, "__nimbusPerfHooksBuiltin", {
  value: nimbusPerfHooksBuiltin,
  configurable: true,
  enumerable: false,
  writable: false,
});

function seedGlobalIfMissing(name, value) {
  if (typeof globalThis[name] === "undefined") {
    Object.defineProperty(globalThis, name, {
      value,
      configurable: true,
      enumerable: false,
      writable: false,
    });
  }
}

function setGlobalEnumerable(name, enumerable) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  if (!descriptor || descriptor.enumerable === enumerable || !descriptor.configurable) {
    return;
  }
  Object.defineProperty(globalThis, name, {
    ...descriptor,
    enumerable,
  });
}

function alignNodeGlobalEnumerableSurface() {
  for (const name of Object.keys(globalThis)) {
    if (name.startsWith("__nimbus")) {
      setGlobalEnumerable(name, false);
    }
  }

  for (const name of [
    "EventSource",
    "gc",
    "onunhandledrejection",
    "reportError",
  ]) {
    setGlobalEnumerable(name, false);
  }

  for (const name of [
    "global",
    "queueMicrotask",
    "clearImmediate",
    "clearInterval",
    "clearTimeout",
    "atob",
    "btoa",
    "performance",
    "setImmediate",
    "setInterval",
    "setTimeout",
    "structuredClone",
    "fetch",
    "crypto",
    "navigator",
  ]) {
    setGlobalEnumerable(name, true);
  }
}

const runtimeTargetTriple = core.ops.op_nimbus_runtime_target_triple();
if (typeof runtimeTargetTriple === "string" && runtimeTargetTriple.length > 0) {
  core.setBuildInfo(runtimeTargetTriple);
}

enableNextTick();
function refreshNodeRuntimeOpState() {
  op_stream_base_register_state(streamBaseState);
}
Object.defineProperty(globalThis, "__nimbusRefreshNodeRuntimeOpState", {
  value: refreshNodeRuntimeOpState,
  configurable: true,
  enumerable: false,
  writable: true,
});
refreshNodeRuntimeOpState();
seedGlobalEventTargetSurface();
saveWebGlobalThisReference(globalThis);
defineWebEventHandler(globalThis, "unhandledrejection");
core.setUnhandledPromiseRejectionHandler(processUnhandledPromiseRejection);
core.setHandledPromiseRejectionHandler(processRejectionHandled);
core.setReportExceptionCallback(reportWebException);
seedGlobalPerformance();
if (
  internals.nodeGlobals?.process
  && typeof internals.nodeGlobals.process === "object"
  && globalThis.process !== internals.nodeGlobals.process
) {
  globalThis.process = internals.nodeGlobals.process;
}
if (
  internals.nodeGlobals &&
  (internals.nodeGlobals.process === undefined || internals.nodeGlobals.process === null) &&
  nodeProcessBuiltin &&
  typeof nodeProcessBuiltin === "object"
) {
  internals.nodeGlobals.process = nodeProcessBuiltin;
}
if (
  (globalThis.process === undefined || globalThis.process === null) &&
  nodeProcessBuiltin &&
  typeof nodeProcessBuiltin === "object"
) {
  globalThis.process = nodeProcessBuiltin;
}
seedNodeProcessCwd(nodeProcessBuiltin);
seedNodeProcessCwd(internals.nodeGlobals?.process);
seedNodeProcessPlatformMetadata(internals.nodeGlobals?.process);
seedNodeProcessStdio(internals.nodeGlobals?.process);
seedNodeProcessExecPath(internals.nodeGlobals?.process);
seedNodeProcessFeatures(internals.nodeGlobals?.process);
seedNodeProcessBuiltinModuleLoader(internals.nodeGlobals?.process);
seedNodeProcessFinalization(internals.nodeGlobals?.process);
seedNodeProcessFatalGuards(internals.nodeGlobals?.process);
installNimbusProcessDlopenErrorMapping(internals.nodeGlobals?.process);
seedNodeProcessCwd(globalThis.process);
seedNodeProcessPlatformMetadata(globalThis.process);
seedNodeProcessStdio(globalThis.process);
seedNodeProcessExecPath(globalThis.process);
seedNodeProcessFeatures(globalThis.process);
seedNodeProcessBuiltinModuleLoader(globalThis.process);
seedNodeProcessFinalization(globalThis.process);
seedNodeProcessFatalGuards(globalThis.process);
installNimbusProcessDlopenErrorMapping(globalThis.process);
installNimbusDomainPromiseRejectPatch();
const workerBootstrapState =
  typeof core.ops.op_nimbus_worker_bootstrap_state === "function"
    ? core.ops.op_nimbus_worker_bootstrap_state()
    : null;
seedWorkerThreadHostSurface(workerBootstrapState);
if (typeof internals.__initWorkerThreads === "function") {
  const deserializedWorkerMetadata =
    workerBootstrapState?.workerMetadata
      ? webDeserializeJsMessageData(workerBootstrapState.workerMetadata)
      : undefined;
  internals.__initWorkerThreads(
    workerBootstrapState?.runningOnMainThread ?? true,
    workerBootstrapState?.workerId ?? 0,
    deserializedWorkerMetadata,
    workerBootstrapState?.moduleSpecifier ?? null,
  );
  const workerMetadataObject = ArrayIsArray(deserializedWorkerMetadata)
    ? deserializedWorkerMetadata[0]
    : undefined;
  const shouldShareWorkerEnv =
    workerMetadataObject &&
    typeof workerMetadataObject === "object" &&
    workerMetadataObject.shareEnv === true;
  const workerEnv =
    shouldShareWorkerEnv
      ? createNimbusSharedWorkerEnvProxy()
      : workerMetadataObject?.env;
  if (
    workerBootstrapState?.runningOnMainThread === false &&
    workerEnv &&
    globalThis.process &&
    typeof globalThis.process === "object"
  ) {
    Object.defineProperty(globalThis.process, "env", {
      value: workerEnv,
      configurable: true,
      enumerable: true,
      writable: true,
    });
    if (
      internals.nodeGlobals?.process &&
      typeof internals.nodeGlobals.process === "object"
    ) {
      try {
        Object.defineProperty(internals.nodeGlobals.process, "env", {
          value: workerEnv,
          configurable: true,
          enumerable: true,
          writable: true,
        });
      } catch (_error) {
        internals.nodeGlobals.process.env = workerEnv;
      }
    }
    Object.defineProperty(globalThis, "__nimbusWorkerThreadEnv", {
      value: workerEnv,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }
  seedNodeClusterWorkerIfNeeded(workerBootstrapState, workerMetadataObject);
}
patchNodeFsReadSemantics(globalThis.process, core.loadExtScript("ext:deno_node/fs.ts"));
seedNodeProcessLoadEnvFile(globalThis.process);
seedNodeGlobalTimers(internals.nodeGlobals);
seedNodeProcessWarnings(globalThis.process);
if (
  internals.nodeGlobals
  && typeof internals.nodeGlobals === "object"
  && internals.nodeGlobals.Buffer === undefined
) {
  Object.defineProperty(internals.nodeGlobals, "Buffer", {
    value: nodeBuffer,
    configurable: true,
    enumerable: false,
    writable: false,
  });
}
if (
  typeof globalThis.Buffer === "undefined"
  && (internals.nodeGlobals?.Buffer !== undefined || nodeBuffer !== undefined)
) {
  let globalBufferValue = internals.nodeGlobals?.Buffer ?? nodeBuffer;
  Object.defineProperty(globalThis, "Buffer", {
    get() {
      return globalBufferValue;
    },
    set(value) {
      globalBufferValue = value;
    },
    configurable: true,
    enumerable: false,
  });
}
seedGlobalIfMissing("structuredClone", webStructuredClone);
seedGlobalIfMissing("atob", webAtob);
seedGlobalIfMissing("btoa", webBtoa);
seedGlobalIfMissing("ByteLengthQueuingStrategy", webByteLengthQueuingStrategy);
seedGlobalIfMissing("CountQueuingStrategy", webCountQueuingStrategy);
seedGlobalIfMissing("ReadableByteStreamController", webReadableByteStreamController);
seedGlobalIfMissing("ReadableStream", webReadableStream);
seedGlobalIfMissing("ReadableStreamBYOBReader", webReadableStreamBYOBReader);
seedGlobalIfMissing("ReadableStreamBYOBRequest", webReadableStreamBYOBRequest);
seedGlobalIfMissing("ReadableStreamDefaultController", webReadableStreamDefaultController);
seedGlobalIfMissing("ReadableStreamDefaultReader", webReadableStreamDefaultReader);
seedGlobalIfMissing("TransformStream", webTransformStream);
seedGlobalIfMissing("TransformStreamDefaultController", webTransformStreamDefaultController);
seedGlobalIfMissing("WritableStream", webWritableStream);
seedGlobalIfMissing("WritableStreamDefaultController", webWritableStreamDefaultController);
seedGlobalIfMissing("WritableStreamDefaultWriter", webWritableStreamDefaultWriter);
seedGlobalIfMissing("TextEncoderStream", webTextEncoderStream);
seedGlobalIfMissing("TextDecoderStream", webTextDecoderStream);
seedGlobalIfMissing("CompressionStream", webCompressionStream);
seedGlobalIfMissing("DecompressionStream", webDecompressionStream);
seedGlobalIfMissing("MessageChannel", webMessageChannel);
seedGlobalIfMissing("MessagePort", webMessagePort);
upgradeGlobalConsole(globalThis.process);
alignNodeGlobalEnumerableSurface();

if (typeof internals.requireImpl?.setUsesLocalNodeModulesDir === "function") {
  internals.requireImpl.setUsesLocalNodeModulesDir();
}
delete globalThis.nodeBootstrap;

export {};
