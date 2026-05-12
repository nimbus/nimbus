import { core, internals as coreInternals, primordials } from "ext:core/mod.js";
import { op_runtime_memory_usage, op_stream_base_register_state } from "ext:core/ops";
import { errors } from "ext:runtime/01_errors.js";
import { windowOrWorkerGlobalScope } from "ext:runtime/98_global_scope_shared.js";
import {
  denoGlobals as hiddenDenoGlobals,
  nodeGlobals as hiddenNodeGlobals,
} from "ext:nimbus_node22/internal_bootstrap.js";

import "ext:deno_fetch/20_headers.js";
import "ext:deno_fetch/22_http_client.js";
import "ext:deno_fetch/23_request.js";
import "ext:deno_fetch/23_response.js";
import "ext:deno_fetch/26_fetch.js";
import "ext:deno_fetch/27_eventsource.js";
import { realPath as denoRealPath, realPathSync as denoRealPathSync } from "ext:deno_fs/30_fs.js";
import "ext:deno_http/00_serve.ts";
import "ext:deno_http/01_http.js";
import "ext:deno_http/02_websocket.ts";
import { enableNextTick } from "ext:deno_node/_next_tick.ts";
import { createWritableStdioStream, initStdin } from "ext:deno_node/_process/streams.mjs";
import { streamBaseState } from "ext:deno_node/internal_binding/stream_wrap.ts";
import {
  bindStreamsLazy as bindNodeConsoleStreamsLazy,
  Console as NodeConsole,
  kBindProperties as nodeConsoleBindProperties,
} from "ext:deno_node/internal/console/constructor.mjs";
import { onWarning as nodeProcessOnWarning } from "ext:deno_node/internal/process/warning.ts";
import "ext:deno_net/01_net.js";
import "ext:deno_net/02_tls.js";
import {
  hostname as denoHostname,
  loadavg as denoLoadavg,
  networkInterfaces as denoNetworkInterfaces,
  osRelease as denoOsRelease,
  osUptime,
  systemMemoryInfo as denoSystemMemoryInfo,
} from "ext:deno_os/30_os.js";
import "ext:deno_os/40_signals.js";
import * as io from "ext:deno_io/12_io.js";
import "ext:deno_web/01_urlpattern.js";
import {
  defineEventHandler as defineWebEventHandler,
  PromiseRejectionEvent as WebPromiseRejectionEvent,
  reportException as reportWebException,
  saveGlobalThisReference as saveWebGlobalThisReference,
} from "ext:deno_web/02_event.js";
import "ext:deno_web/04_global_interfaces.js";
import {
  ByteLengthQueuingStrategy as webByteLengthQueuingStrategy,
  CountQueuingStrategy as webCountQueuingStrategy,
  ReadableByteStreamController as webReadableByteStreamController,
  ReadableStream as webReadableStream,
  ReadableStreamBYOBReader as webReadableStreamBYOBReader,
  ReadableStreamBYOBRequest as webReadableStreamBYOBRequest,
  ReadableStreamDefaultController as webReadableStreamDefaultController,
  ReadableStreamDefaultReader as webReadableStreamDefaultReader,
  TransformStream as webTransformStream,
  TransformStreamDefaultController as webTransformStreamDefaultController,
  WritableStream as webWritableStream,
  WritableStreamDefaultController as webWritableStreamDefaultController,
  WritableStreamDefaultWriter as webWritableStreamDefaultWriter,
} from "ext:deno_web/06_streams.js";
import "ext:deno_web/10_filereader.js";
import "ext:deno_web/12_location.js";
import {
  deserializeJsMessageData as webDeserializeJsMessageData,
  MessageChannel as webMessageChannel,
  MessagePort as webMessagePort,
  MessagePortPrototype as webMessagePortPrototype,
  serializeJsMessageData as webSerializeJsMessageData,
  structuredClone as webStructuredClone,
  unrefParentPort as webUnrefParentPort,
} from "ext:deno_web/13_message_port.js";
import { performance as webPerformance } from "ext:deno_web/15_performance.js";
import "ext:deno_web/16_image_data.js";
import "ext:deno_websocket/01_websocket.js";
import "ext:deno_websocket/02_websocketstream.js";
import nimbusPerfHooksBuiltin from "ext:nimbus_node22/perf_hooks_impl.js";
import { Buffer as nodeBuffer } from "node:buffer";
import { readFileSync as nodeFsReadFileSync } from "node:fs";
import { relative as nodePathRelative, resolve as nodePathResolve } from "node:path";
import { fileURLToPath as nodeFileURLToPath } from "node:url";
import { parseEnv as nodeUtilParseEnv } from "node:util";
import "node:worker_threads";
import { FileHandle as nodeInternalFsFileHandle } from "ext:deno_node/internal/fs/handle.ts";
import {
  AbortError as nodeAbortError,
  ERR_FS_INVALID_SYMLINK_TYPE as nodeErrFsInvalidSymlinkType,
  ERR_FS_FILE_TOO_LARGE as nodeErrFsFileTooLarge,
} from "ext:deno_node/internal/errors.ts";
import {
  Dirent as nodeFsDirent,
  constants as nodeFsUtilConstants,
  getValidatedPathToString as nodeFsGetValidatedPathToString,
  getOptions as nodeFsGetOptions,
  toUnixTimestamp as nodeFsToUnixTimestamp,
} from "ext:deno_node/internal/fs/utils.mjs";
import { StringDecoder as nodeStringDecoder } from "node:string_decoder";
import { getBinding as getNodeInternalBinding } from "ext:deno_node/internal_binding/mod.ts";

Object.defineProperties(globalThis, windowOrWorkerGlobalScope);
globalThis.__nimbusPerfHooksBuiltin = nimbusPerfHooksBuiltin;
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

function runtimeFsPathToString(path) {
  if (typeof path === "string") {
    return path;
  }
  if (path instanceof URL) {
    if (path.protocol !== "file:") {
      throw new TypeError(`Nimbus only supports file: URLs for Deno fs APIs; received ${path.href}`);
    }
    return decodeURIComponent(path.pathname.replace(/^\/([A-Za-z]:)/, "$1"));
  }
  return String(path);
}

function runtimeFsToUnixTimeFromEpoch(value) {
  const unixSeconds = nodeFsToUnixTimestamp(value);
  const seconds = Math.trunc(unixSeconds);
  const nanoseconds = Math.trunc((unixSeconds * 1e3) - (seconds * 1e3)) * 1e6;
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
    return { kind: "modify", paths: [nextSnapshot.path], flag: null };
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
      oldpath: runtimeFsPathToString(oldpath),
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
      oldpath: runtimeFsPathToString(oldpath),
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

function seedNodeProcessPlatformMetadata(nodeProcess) {
  if (!nodeProcess || typeof nodeProcess !== "object") {
    return;
  }

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

  if (nodeProcess.stdin === undefined) {
    nodeProcess.stdin = initStdin(false);
  }

  if (nodeProcess.stdout === undefined) {
    nodeProcess.stdout = createWritableStdioStream(io.stdout, "stdout");
  }

  if (nodeProcess.stderr === undefined) {
    nodeProcess.stderr = createWritableStdioStream(io.stderr, "stderr");
  }
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
  features.cached_builtins = features.cached_builtins === true;
  features.require_module = features.require_module === true;
  if (!Object.prototype.hasOwnProperty.call(features, "typescript")) {
    features.typescript = false;
  }
  delete features.openssl_is_boringssl;
  delete features.quic;
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
    return nodeFileURLToPath(normalizedPath);
  }
  return typeof normalizedPath === "string" ? normalizedPath : String(normalizedPath);
}

function resolveLoadEnvFilePath(nodeProcess, path) {
  const normalizedPath = normalizeLoadEnvFilePath(path);
  if (normalizedPath instanceof URL) {
    return nodeFileURLToPath(normalizedPath);
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
    || (
      typeof error?.message === "string"
      && error.message.includes("Requires read access to")
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

function rememberLoadedEnvFileEntries(nodeProcess, path) {
  const overlayEntries = seedNodeProcessEnvOverlay(nodeProcess);
  if (!overlayEntries) {
    return;
  }

  const source = nodeFsReadFileSync(resolveLoadEnvFilePath(nodeProcess, path), "utf8");

  for (const [key, value] of Object.entries(nodeUtilParseEnv(source))) {
    try {
      if (nodeProcess.env[key] !== undefined) {
        continue;
      }
    } catch (_error) {
      if (Object.prototype.hasOwnProperty.call(overlayEntries, key)) {
        continue;
      }
    }
    overlayEntries[key] = value;
  }
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

function getNodeFsPromiseTargets(nodeFs, nodeProcess) {
  const targets = [];
  const builtinPromises = nodeProcess?.getBuiltinModule?.("fs/promises");
  if (builtinPromises && typeof builtinPromises === "object") {
    targets.push(builtinPromises);
  }
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

function createFsPromisesWatchTypeError(name, expected, value) {
  const receivedType = value === null ? "null" : typeof value;
  const error = new TypeError(
    `The "${name}" argument must be of type ${expected}. Received ${receivedType}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
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
  const signal = optionsSnapshot.signal;
  delete optionsSnapshot.signal;
  return {
    __proto__: null,
    builtin: optionsSnapshot,
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

function patchNodeFsReadSemantics(nodeProcess) {
  const nodeFs = nodeProcess?.getBuiltinModule?.("fs");
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

  const nodeFsPromiseTargets = getNodeFsPromiseTargets(nodeFs, nodeProcess);
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
      const patchedLchmod = function (path, mode) {
        if (typeof path === "number" || isNimbusFileHandle(path)) {
          return Reflect.apply(originalLchmod, this, arguments);
        }
        const symlinkFlags = (nodeFs.constants?.O_WRONLY ?? 1) | (nodeFs.constants?.O_SYMLINK ?? 0);
        return nodeFsPromises
          .open(path, symlinkFlags, mode)
          .then((handle) =>
            handleFsPromisePathClose(
              handle.chmod(mode),
              () => closeNimbusFileHandle(handle),
            )
          );
      };
      Object.defineProperties(patchedLchmod, Object.getOwnPropertyDescriptors(originalLchmod));
      nodeFsPromises.lchmod = patchedLchmod;
    }

    const originalWatch = nodeFsPromises.watch;
    if (
      typeof originalWatch === "function" &&
      originalWatch[nimbusFsPromisesWatchPatched] !== true
    ) {
      const patchedWatch = function (path, options) {
        const normalizedPath = nodeFsGetValidatedPathToString(path);
        const { builtin, signal } = validateFsPromisesWatchOptions(options);
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

  if (originalLoadEnvFile[nimbusLoadEnvFilePatched] === true) {
    return;
  }

  seedNodeProcessEnvOverlay(nodeProcess);

  function patchedLoadEnvFile(path = undefined) {
    const resolvedPath = resolveLoadEnvFilePath(nodeProcess, path);
    const displayPath = displayLoadEnvFilePath(path);
    try {
      const result = originalLoadEnvFile.call(nodeProcess, resolvedPath);
      rememberLoadedEnvFileEntries(nodeProcess, resolvedPath);
      return result;
    } catch (error) {
      if (error !== undefined) {
        if (isLoadEnvFileAccessDeniedError(error)) {
          throw createLoadEnvFileAccessDeniedError(displayPath, error);
        }
        throw error;
      }

      try {
        hiddenDenoGlobals.statSync(resolvedPath);
      } catch (statError) {
        if (statError?.name === "NotFound") {
          throw createLoadEnvFileNotFoundError(displayPath);
        }
      }

      throw error;
    }
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
      try {
        return Reflect.apply(callback, handle, callbackArgs);
      } catch (error) {
        const processObject = globalThis.process;
        if (
          processObject &&
          typeof processObject._fatalException === "function" &&
          processObject._fatalException(error) === true
        ) {
          return;
        }
        throw error;
      }
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
    if (nodeGlobals[property] === undefined) {
      continue;
    }
    const value = property === "setImmediate"
      ? createNodeCompatibleSetImmediate(nodeGlobals[property])
      : nodeGlobals[property];
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

function processUnhandledPromiseRejection(promise, reason) {
  const rejectionEvent = new WebPromiseRejectionEvent("unhandledrejection", {
    cancelable: true,
    promise,
    reason,
  });

  globalThis.dispatchEvent(rejectionEvent);

  if (
    !rejectionEvent.defaultPrevented &&
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
    writable: false,
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
    deno: "2.7.14-nimbus",
    v8: "147.4.0-locker.1",
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

const runtimeTargetTriple = core.ops.op_nimbus_runtime_target_triple();
if (typeof runtimeTargetTriple === "string" && runtimeTargetTriple.length > 0) {
  core.setBuildInfo(runtimeTargetTriple);
}

enableNextTick();
op_stream_base_register_state(streamBaseState);
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
seedNodeProcessPlatformMetadata(internals.nodeGlobals?.process);
seedNodeProcessStdio(internals.nodeGlobals?.process);
seedNodeProcessExecPath(internals.nodeGlobals?.process);
seedNodeProcessFeatures(internals.nodeGlobals?.process);
seedNodeProcessPlatformMetadata(globalThis.process);
seedNodeProcessStdio(globalThis.process);
seedNodeProcessExecPath(globalThis.process);
seedNodeProcessFeatures(globalThis.process);
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
    globalThis.process.env = workerEnv;
    if (
      internals.nodeGlobals?.process &&
      typeof internals.nodeGlobals.process === "object"
    ) {
      internals.nodeGlobals.process.env = workerEnv;
    }
    Object.defineProperty(globalThis, "__nimbusWorkerThreadEnv", {
      value: workerEnv,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }
}
patchNodeFsReadSemantics(globalThis.process);
seedNodeProcessLoadEnvFile(globalThis.process);
seedNodeGlobalTimers(internals.nodeGlobals);
seedNodeProcessWarnings(globalThis.process);
if (
  typeof globalThis.Buffer === "undefined"
  && internals.nodeGlobals?.Buffer !== undefined
) {
  Object.defineProperty(globalThis, "Buffer", {
    value: internals.nodeGlobals.Buffer,
    configurable: true,
    enumerable: false,
    writable: false,
  });
}
seedGlobalIfMissing("structuredClone", webStructuredClone);
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
seedGlobalIfMissing("MessageChannel", webMessageChannel);
seedGlobalIfMissing("MessagePort", webMessagePort);
upgradeGlobalConsole(globalThis.process);

if (typeof internals.requireImpl?.setUsesLocalNodeModulesDir === "function") {
  internals.requireImpl.setUsesLocalNodeModulesDir();
}
delete globalThis.nodeBootstrap;

export {};
