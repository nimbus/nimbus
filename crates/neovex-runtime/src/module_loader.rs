use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::backends::v8::embedder::{
    JsErrorBox, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, RequestedModuleType,
    ResolutionKind, SourceCodeCacheInfo, resolve_import,
};
use crate::limits::RuntimeCompatibilityTarget;
use crate::node_compat::{
    ResolvedNodeModuleKind, ResolvedNodeTarget, build_package_json_resolver,
    classify_resolved_module_kind, resolve_node_target, translate_commonjs_to_esm,
};
use crate::runtime_capabilities::RuntimePathPolicy;
use twox_hash::XxHash64;

const NODE_FS_SPECIFIER: &str = "node:fs";
const NEOVEX_NODE_FS_SPECIFIER: &str = "neovex:node/fs";
const NODE_FS_PROMISES_SPECIFIER: &str = "node:fs/promises";
const NEOVEX_NODE_FS_PROMISES_SPECIFIER: &str = "neovex:node/fs/promises";
const NODE_PERF_HOOKS_SPECIFIER: &str = "node:perf_hooks";
const NODE_TLS_SPECIFIER: &str = "node:tls";
const NODE_MODULE_SPECIFIER: &str = "node:module";
const NEOVEX_NODE_MODULE_SPECIFIER: &str = "node:neovex/module";
const INTERNAL_READLINE_UTILS_SPECIFIER: &str = "internal/readline/utils";
const NEOVEX_INTERNAL_READLINE_UTILS_SPECIFIER: &str = "neovex:internal/readline/utils";
const NODE_PERF_HOOKS_MODULE_SOURCE: &str = r#"
export * from "ext:neovex_node22/perf_hooks_impl.js";
import defaultExport from "ext:neovex_node22/perf_hooks_impl.js";
export default defaultExport;
"#;
const NODE_TLS_MODULE_SOURCE: &str = r#"
import Module, { getBuiltinModule as getNeovexBuiltinModule } from "node:neovex/module";
import { ERR_TLS_INVALID_CONTEXT } from "ext:deno_node/internal/errors.ts";

const tlsBuiltin = getNeovexBuiltinModule?.("tls") ?? Module?.getBuiltinModule?.("tls");
if (!tlsBuiltin || typeof tlsBuiltin.connect !== "function") {
  throw new Error(
    "Neovex Node22 bootstrap expected node:neovex/module to expose the tls builtin",
  );
}

const {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  CryptoStream,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecurePair,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createServer,
  getCiphers,
  rootCertificates,
  setDefaultCACertificates,
} = tlsBuiltin;

function createSecurePair(context, ...args) {
  if (!context || typeof context !== "object" || !("context" in context)) {
    throw new ERR_TLS_INVALID_CONTEXT("context");
  }
  return tlsBuiltin.createSecurePair(context, ...args);
}

const defaultExport = Object.create(tlsBuiltin);
Object.assign(defaultExport, {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  CryptoStream,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecurePair,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createSecurePair,
  createServer,
  getCiphers,
  rootCertificates,
  setDefaultCACertificates,
});

export {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  CryptoStream,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecurePair,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createSecurePair,
  createServer,
  getCiphers,
  rootCertificates,
  setDefaultCACertificates,
};
export default defaultExport;
"#;
const INTERNAL_READLINE_UTILS_MODULE_SOURCE: &str = r#"
import Module from "node:module";

const internalReadlineUtils = Module._load("internal/readline/utils", null, false);
if (!internalReadlineUtils || typeof internalReadlineUtils !== "object") {
  throw new Error(
    "Neovex Node22 bootstrap expected node:module to expose internal/readline/utils",
  );
}

const {
  CSI,
  charLengthAt,
  charLengthLeft,
  commonPrefix,
  emitKeys,
  kSubstringSearch,
} = internalReadlineUtils;

export {
  CSI,
  charLengthAt,
  charLengthLeft,
  commonPrefix,
  emitKeys,
  kSubstringSearch,
};
export default internalReadlineUtils;
"#;

const NODE_FS_MODULE_SOURCE: &str = r#"
import Module, { getBuiltinModule as getNeovexBuiltinModule } from "node:neovex/module";

const fsBuiltin = getNeovexBuiltinModule?.("fs") ?? Module?.getBuiltinModule?.("fs");
if (!fsBuiltin || typeof fsBuiltin.readFile !== "function") {
  throw new Error(
    "Neovex Node22 bootstrap expected node:neovex/module to expose a local fs builtin override",
  );
}

const {
  _toUnixTimestamp,
  access,
  accessSync,
  appendFile,
  appendFileSync,
  BigIntStats,
  CFISBIS,
  chmod,
  chmodSync,
  chown,
  chownSync,
  close,
  closeSync,
  constants,
  convertFileInfoToBigIntStats,
  convertFileInfoToStats,
  copyFile,
  copyFileSync,
  cp,
  cpSync,
  createReadStream,
  createWriteStream,
  Dir,
  Dirent,
  exists,
  existsSync,
  fchmod,
  fchmodSync,
  fchown,
  fchownSync,
  fdatasync,
  fdatasyncSync,
  fstat,
  fstatSync,
  fsync,
  fsyncSync,
  ftruncate,
  ftruncateSync,
  futimes,
  futimesSync,
  glob,
  globSync,
  lchmod,
  lchmodSync,
  lchown,
  lchownSync,
  link,
  linkSync,
  lstat,
  lstatSync,
  lutimes,
  lutimesSync,
  mkdir,
  mkdirSync,
  mkdtemp,
  mkdtempDisposableSync,
  mkdtempSync,
  open,
  openAsBlob,
  openSync,
  opendir,
  opendirSync,
  promises,
  read,
  readFile,
  readFileSync,
  readlink,
  readlinkSync,
  readSync,
  readdir,
  readdirSync,
  ReadStream,
  readv,
  readvSync,
  realpath,
  realpathSync,
  rename,
  renameSync,
  rmdir,
  rmdirSync,
  rm,
  rmSync,
  stat,
  statfs,
  statfsSync,
  Stats,
  statSync,
  symlink,
  symlinkSync,
  SyncWriteStream,
  truncate,
  truncateSync,
  unlink,
  unlinkSync,
  unwatchFile,
  Utf8Stream,
  utimes,
  utimesSync,
  watch,
  watchFile,
  write,
  writeFile,
  writeFileSync,
  writeSync,
  writev,
  writevSync,
  WriteStream,
} = fsBuiltin;

export {
  _toUnixTimestamp,
  access,
  accessSync,
  appendFile,
  appendFileSync,
  BigIntStats,
  CFISBIS,
  chmod,
  chmodSync,
  chown,
  chownSync,
  close,
  closeSync,
  constants,
  convertFileInfoToBigIntStats,
  convertFileInfoToStats,
  copyFile,
  copyFileSync,
  cp,
  cpSync,
  createReadStream,
  createWriteStream,
  Dir,
  Dirent,
  exists,
  existsSync,
  fchmod,
  fchmodSync,
  fchown,
  fchownSync,
  fdatasync,
  fdatasyncSync,
  fstat,
  fstatSync,
  fsync,
  fsyncSync,
  ftruncate,
  ftruncateSync,
  futimes,
  futimesSync,
  glob,
  globSync,
  lchmod,
  lchmodSync,
  lchown,
  lchownSync,
  link,
  linkSync,
  lstat,
  lstatSync,
  lutimes,
  lutimesSync,
  mkdir,
  mkdirSync,
  mkdtemp,
  mkdtempDisposableSync,
  mkdtempSync,
  open,
  openAsBlob,
  openSync,
  opendir,
  opendirSync,
  promises,
  read,
  readFile,
  readFileSync,
  readlink,
  readlinkSync,
  readSync,
  readdir,
  readdirSync,
  ReadStream,
  readv,
  readvSync,
  realpath,
  realpathSync,
  rename,
  renameSync,
  rmdir,
  rmdirSync,
  rm,
  rmSync,
  stat,
  statfs,
  statfsSync,
  Stats,
  statSync,
  symlink,
  symlinkSync,
  SyncWriteStream,
  truncate,
  truncateSync,
  unlink,
  unlinkSync,
  unwatchFile,
  Utf8Stream,
  utimes,
  utimesSync,
  watch,
  watchFile,
  write,
  writeFile,
  writeFileSync,
  writeSync,
  writev,
  writevSync,
  WriteStream,
};
export default fsBuiltin;
"#;

const NODE_FS_PROMISES_MODULE_SOURCE: &str = r#"
const processBuiltin = globalThis.process;
const fsBuiltin = processBuiltin?.getBuiltinModule?.("fs");
const fsPromisesBuiltin = fsBuiltin?.promises;
const moduleBuiltin = processBuiltin?.getBuiltinModule?.("module");
const internalFsUtils = moduleBuiltin?._load?.("internal/fs/utils", null, false);
if (
  !fsPromisesBuiltin ||
  typeof fsPromisesBuiltin.open !== "function" ||
  !internalFsUtils ||
  typeof internalFsUtils.emitRecursiveRmdirWarning !== "function" ||
  typeof internalFsUtils.validateRmdirOptions !== "function" ||
  typeof internalFsUtils.getValidatedPathToString !== "function"
) {
  throw new Error(
    "Neovex Node22 bootstrap expected process.getBuiltinModule('fs').promises and internal/fs/utils to be available",
  );
}

const {
  emitRecursiveRmdirWarning,
  getValidatedPathToString,
  validateRmdirOptions,
} = internalFsUtils;

function mapFsHostError(error, operation) {
  const hostError = error?.neovexHostError;
  if (!hostError || typeof hostError !== "object") {
    const message = typeof error?.message === "string" ? error.message : "";
    const code = typeof error?.code === "string" && error.code.length > 0
      ? error.code
      : message.match(/"code":"([A-Z0-9_]+)"/)?.[1] ?? null;
    if (code === null) {
      throw error;
    }
    const mappedError = new Error(message.length > 0 ? message : `${operation} failed`);
    mappedError.code = code;
    throw mappedError;
  }
  const message =
    typeof hostError.message === "string" && hostError.message.length > 0
      ? hostError.message
      : String(error?.message ?? `${operation} failed`);
  const mappedError = new Error(message);
  mappedError.code = hostError.code ?? null;
  mappedError.neovexHostError = hostError;
  throw mappedError;
}

function toStats(value) {
  const isFile = value?.isFile === true;
  const isDirectory = value?.isDirectory === true;
  const isSymlink = value?.isSymlink === true;
  const size = Number(value?.size ?? 0);
  const mtimeMs = value?.mtimeMs ?? null;
  const atimeMs = value?.atimeMs ?? null;
  const birthtimeMs = value?.birthtimeMs ?? null;
  const ctimeMs = value?.ctimeMs ?? null;
  return {
    isFile() {
      return isFile;
    },
    isDirectory() {
      return isDirectory;
    },
    isSymbolicLink() {
      return isSymlink;
    },
    isBlockDevice() {
      return false;
    },
    isCharacterDevice() {
      return false;
    },
    isFIFO() {
      return false;
    },
    isSocket() {
      return false;
    },
    size,
    mtimeMs,
    atimeMs,
    birthtimeMs,
    ctimeMs,
    mtime: mtimeMs == null ? null : new Date(mtimeMs),
    atime: atimeMs == null ? null : new Date(atimeMs),
    birthtime: birthtimeMs == null ? null : new Date(birthtimeMs),
    ctime: ctimeMs == null ? null : new Date(ctimeMs),
    mode: value?.mode ?? null,
  };
}

function toDirent(value) {
  const isFile = value?.isFile === true;
  const isDirectory = value?.isDirectory === true;
  const isSymlink = value?.isSymlink === true;
  return {
    name: String(value?.name ?? ""),
    isFile() {
      return isFile;
    },
    isDirectory() {
      return isDirectory;
    },
    isSymbolicLink() {
      return isSymlink;
    },
  };
}

function normalizeReadFileEncoding(options) {
  if (options === undefined || options === null) {
    return null;
  }
  if (typeof options === "string") {
    return options.toLowerCase();
  }
  if (typeof options === "object" && typeof options.encoding === "string") {
    return options.encoding.toLowerCase();
  }
  return null;
}

function normalizeMkdirOptions(options) {
  if (options === undefined || options === null) {
    return { recursive: false, mode: null };
  }
  if (typeof options === "boolean") {
    return { recursive: options, mode: null };
  }
  if (typeof options === "object") {
    return {
      recursive: options.recursive === true,
      mode: typeof options.mode === "number" ? options.mode : null,
    };
  }
  return { recursive: false, mode: null };
}

function createScandirNotDirectoryError(path) {
  const displayPath =
    typeof path === "string"
      ? path
      : path instanceof URL
      ? path.pathname
      : String(path);
  const error = new Error(
    `${processBuiltin?.platform === "win32" ? "ENOENT" : "ENOTDIR"}: not a directory, scandir '${displayPath}'`,
  );
  error.code = processBuiltin?.platform === "win32" ? "ENOENT" : "ENOTDIR";
  error.syscall = "scandir";
  error.path = displayPath;
  return error;
}

async function ensureDirectoryForReaddir(path) {
  const stats = await fsPromisesBuiltin.stat(path);
  if (!stats?.isDirectory?.()) {
    throw createScandirNotDirectoryError(path);
  }
}

function sortReaddirResults(result, options) {
  if (typeof options !== "object" || options === null || options.withFileTypes !== true) {
    return result;
  }
  if (!Array.isArray(result)) {
    return result;
  }
  return result.slice().sort((left, right) =>
    String(left?.name ?? left).localeCompare(String(right?.name ?? right))
  );
}

function createInvalidEncodingOptionError(encoding) {
  const error = new TypeError(`The value "${encoding}" is invalid for option "encoding"`);
  error.code = "ERR_INVALID_ARG_VALUE";
  return error;
}

function validateReaddirEncodingOption(encoding) {
  if (encoding === undefined || encoding === null || encoding === "buffer") {
    return;
  }
  if (typeof encoding === "string" && Buffer.isEncoding(encoding)) {
    return;
  }
  throw createInvalidEncodingOptionError(encoding);
}

function snapshotReaddirOptions(options) {
  if (options === null || options === undefined) {
    return {};
  }
  if (typeof options === "string") {
    validateReaddirEncodingOption(options);
    return { encoding: options };
  }
  if (typeof options !== "object") {
    return options;
  }
  validateReaddirEncodingOption(options.encoding);
  return { ...options };
}

function openFlagsNeedWrite(flags) {
  if (typeof flags === "number") {
    // Conservatively treat numeric flags as write-capable until the runtime
    // exposes a richer parsed open-flags contract.
    return true;
  }
  const normalizedFlags = typeof flags === "string" && flags.length > 0 ? flags : "r";
  return normalizedFlags.includes("w")
    || normalizedFlags.includes("a")
    || normalizedFlags.includes("+");
}

function validateOpenPath(path, flags) {
  return globalThis.__neovexSyncHostValue("op_neovex_runtime_validate_open_path", {
    path: String(path),
    write: openFlagsNeedWrite(flags),
  });
}

async function readFile(path, options) {
  const normalizedEncoding =
    normalizeReadFileEncoding(options);
  let result;
  try {
    result = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_fs_read_file", {
      path: String(path),
      encoding: normalizedEncoding,
    });
  } catch (error) {
    mapFsHostError(error, "readFile");
  }
  if (result?.kind === "text") {
    return result.value;
  }
  return Uint8Array.from(result?.value ?? []);
}

async function writeFile(path, data, options) {
  if (path && typeof path === "object" && typeof path.writeFile === "function") {
    return fsPromisesBuiltin.writeFile(path, data, options);
  }
  return await new Promise((resolve, reject) => {
    fsBuiltin.writeFile(path, data, options, (error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

async function appendFile(path, data, options) {
  if (path && typeof path === "object" && typeof path.appendFile === "function") {
    return fsPromisesBuiltin.appendFile(path, data, options);
  }
  return await new Promise((resolve, reject) => {
    fsBuiltin.appendFile(path, data, options, (error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

async function stat(path) {
  try {
    const value = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_stat", {
      path: String(path),
      follow_symlink: true,
    });
    return toStats(value);
  } catch (error) {
    mapFsHostError(error, "stat");
  }
}

async function lstat(path) {
  try {
    const value = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_stat", {
      path: String(path),
      follow_symlink: false,
    });
    return toStats(value);
  } catch (error) {
    mapFsHostError(error, "lstat");
  }
}

async function mkdir(path, options) {
  const normalizedOptions = normalizeMkdirOptions(options);
  try {
    await globalThis.__neovexAsyncHostValue("op_neovex_runtime_mkdir", {
      path: String(path),
      recursive: normalizedOptions.recursive,
      mode: normalizedOptions.mode,
    });
  } catch (error) {
    mapFsHostError(error, "mkdir");
  }
}

function open(path, flags = "r", mode = 0o666) {
  const normalizedPath = validateOpenPath(path, flags);
  return fsPromisesBuiltin.open(normalizedPath, flags, mode);
}

async function rmdir(path, options) {
  const normalizedOptions = validateRmdirOptions(options);
  if (!normalizedOptions.recursive) {
    try {
      return await fsPromisesBuiltin.rmdir(path, sanitizeRmdirOptions(options));
    } catch (error) {
      const normalizedError = new Error(
        typeof error?.message === "string" && error.message.length > 0
          ? error.message
          : "rmdir failed",
      );
      normalizedError.name = typeof error?.name === "string" ? error.name : "Error";
      normalizedError.code = error?.code;
      normalizedError.errno = error?.errno;
      normalizedError.syscall = "rmdir";
      normalizedError.path = path ?? error?.path;
      if (typeof error?.stack === "string" && error.stack.length > 0) {
        normalizedError.stack = error.stack;
      }
      throw normalizedError;
    }
  }

  emitRecursiveRmdirWarning();

  let stats = null;
  try {
    stats = await fsPromisesBuiltin.lstat(path);
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }

  if (stats?.isDirectory?.()) {
    return fsPromisesBuiltin.rm(path, {
      recursive: true,
      force: false,
      maxRetries: normalizedOptions.maxRetries,
      retryDelay: normalizedOptions.retryDelay,
    });
  }

  return fsPromisesBuiltin.rmdir(path);
}

async function readdir(path, options) {
  const optionsSnapshot = snapshotReaddirOptions(options);
  await ensureDirectoryForReaddir(path);
  return sortReaddirResults(
    await fsPromisesBuiltin.readdir(path, optionsSnapshot),
    optionsSnapshot,
  );
}

function createWatchTypeError(name, expected, value) {
  const receivedType = value === null ? "null" : typeof value;
  const error = new TypeError(
    `The "${name}" argument must be of type ${expected}. Received ${receivedType}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  return error;
}

function createAbortError(cause = undefined) {
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
    throw createWatchTypeError("options", "Object", options);
  }
  const optionsSnapshot = { ...options };
  if (
    optionsSnapshot.persistent !== undefined &&
    typeof optionsSnapshot.persistent !== "boolean"
  ) {
    throw createWatchTypeError("options.persistent", "boolean", optionsSnapshot.persistent);
  }
  if (
    optionsSnapshot.recursive !== undefined &&
    typeof optionsSnapshot.recursive !== "boolean"
  ) {
    throw createWatchTypeError("options.recursive", "boolean", optionsSnapshot.recursive);
  }
  if (optionsSnapshot.encoding !== undefined && typeof optionsSnapshot.encoding !== "string") {
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
    throw createWatchTypeError("options.signal", "AbortSignal", optionsSnapshot.signal);
  }
  const signal = optionsSnapshot.signal;
  delete optionsSnapshot.signal;
  return {
    __proto__: null,
    builtin: optionsSnapshot,
    signal,
  };
}

function watchPromise(path, options) {
  const normalizedPath = getValidatedPathToString(path);
  const { builtin, signal } = validateFsPromisesWatchOptions(options);
  const watcher = fsBuiltin.watch(normalizedPath, builtin);
  const watchPathBasename = getWatchPathBasename(normalizedPath);
  let watchingDirectory = false;
  try {
    watchingDirectory = fsBuiltin.statSync(normalizedPath)?.isDirectory?.() === true;
  } catch (_error) {
    watchingDirectory = false;
  }
  let closed = false;
  let pendingAbortError = null;
  const queue = [];
  const pending = [];

  const closeWatcher = () => {
    if (closed) {
      return;
    }
    closed = true;
    watcher.close();
  };

  const settleNext = (entry) => {
    const waiter = pending.shift();
    if (waiter) {
      waiter(entry);
      return;
    }
    queue.push(entry);
  };

  const onAbort = () => {
    pendingAbortError = createAbortError(signal?.reason);
    closeWatcher();
  };

  watcher.on("change", (eventType, filename) => {
    settleNext({
      kind: "value",
      value: {
        eventType,
        filename: normalizeWatchFilename(filename, watchPathBasename, watchingDirectory),
      },
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
      pendingAbortError = createAbortError(signal.reason);
      processBuiltin?.nextTick?.(() => closeWatcher());
    } else {
      signal.addEventListener("abort", onAbort, { once: true });
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
      return Promise.resolve({ value, done: true });
    },
    [Symbol.asyncIterator]() {
      return this;
    },
  };
}

const fsPromisesModule = {
  ...fsPromisesBuiltin,
  appendFile,
  lstat,
  mkdir,
  open,
  readFile,
  readdir,
  rmdir,
  stat,
  watch: watchPromise,
  writeFile,
};

const {
  access,
  appendFile: appendFileExport,
  chmod,
  chown,
  copyFile,
  cp,
  glob,
  lchmod,
  lchown,
  link,
  lstat: lstatExport,
  lutimes,
  mkdir: mkdirExport,
  mkdtemp,
  open: openExport,
  opendir,
  readFile: readFileExport,
  readdir: readdirExport,
  readlink,
  realpath,
  rename,
  rm,
  rmdir: rmdirExport,
  stat: statExport,
  statfs,
  symlink,
  truncate,
  unlink,
  utimes,
  watch,
  writeFile: writeFileExport,
} = fsPromisesModule;

export {
  access,
  appendFileExport as appendFile,
  chmod,
  chown,
  copyFile,
  cp,
  glob,
  lchmod,
  lchown,
  link,
  lstatExport as lstat,
  lutimes,
  mkdirExport as mkdir,
  mkdtemp,
  openExport as open,
  opendir,
  readFileExport as readFile,
  readdirExport as readdir,
  readlink,
  realpath,
  rename,
  rm,
  rmdirExport as rmdir,
  statExport as stat,
  statfs,
  symlink,
  truncate,
  unlink,
  utimes,
  watch,
  writeFileExport as writeFile,
};
export default fsPromisesModule;
"#;

#[allow(dead_code)]
const NODE_MODULE_MODULE_SOURCE: &str = r#"
import internalFsPromisesDefault from "ext:deno_node/internal/fs/promises.ts";
import { FileHandle as InternalFsPromisesFileHandle } from "ext:deno_node/internal/fs/handle.ts";
import { getBinding as getNodeInternalBinding } from "ext:deno_node/internal_binding/mod.ts";

const DEPRECATED_REQUIRE_WARNINGS = Object.freeze({
  punycode: Object.freeze({
    code: "DEP0040",
    message:
      "The `punycode` module is deprecated. Please use a userland alternative instead.",
  }),
});

const processModule = globalThis.process;
const Module = processModule?.getBuiltinModule?.("module");
if (!Module) {
  throw new Error("Neovex Node22 bootstrap expected process.getBuiltinModule('module') to be available");
}

const {
  _cache,
  _extensions,
  _findPath,
  _initPaths,
  _nodeModulePaths,
  _pathCache,
  _preloadModules,
  _resolveLookupPaths,
  builtinModules,
  findSourceMap,
  globalPaths,
  register,
} = Module;

const denoCreateRequire = Module.createRequire.bind(Module);
const denoLoad = Module._load.bind(Module);
const denoResolveFilename = Module._resolveFilename.bind(Module);
const denoIsBuiltin = Module.isBuiltin.bind(Module);
const denoGetBuiltinModule = processModule.getBuiltinModule.bind(processModule);
const pathModule = denoGetBuiltinModule("path");
const internalFsUtils = denoLoad("internal/fs/utils", null, false);
const internalConsoleConstructor = denoLoad("internal/console/constructor", null, false);
const internalErrors = denoLoad("internal/errors", null, false);
const utilModule = denoLoad("util", null, false);
const inspectCustomSymbol = Symbol.for("nodejs.util.inspect.custom");
const ERR_SOCKET_BUFFER_SIZE =
  internalErrors?.codes?.ERR_SOCKET_BUFFER_SIZE ?? internalErrors?.ERR_SOCKET_BUFFER_SIZE;
if (
  typeof ERR_SOCKET_BUFFER_SIZE === "function" &&
  typeof utilModule?.inspect === "function"
) {
  ERR_SOCKET_BUFFER_SIZE.prototype[inspectCustomSymbol] = function inspectSocketBufferError(
    _recurseTimes,
    ctx,
  ) {
    const inspected = utilModule.inspect(this, {
      ...ctx,
      getters: true,
      customInspect: false,
    });
    return inspected.replace(
      /^ERR_SOCKET_BUFFER_SIZE \[SystemError\]:/,
      `${this.name} [${this.code}]:`,
    );
  };
}
const internalFsBinding = getNodeInternalBinding("fs");
const internalTestBindingState = globalThis.__neovexInternalTestBindingState ??= {
  warningEmitted: false,
  overrides: new Map(),
  readlineBuiltin: undefined,
};
const {
  assertEncoding,
  constants: internalFsConstants,
  copyObject,
  emitRecursiveRmdirWarning,
  getOptions,
  getValidatedPathToString,
  stringToFlags,
  validateStringAfterArrayBufferView,
} = internalFsUtils;

function emitInternalTestBindingWarning() {
  if (internalTestBindingState.warningEmitted) {
    return;
  }
  internalTestBindingState.warningEmitted = true;
  globalThis.process?.emitWarning?.(
    "These APIs are for internal testing only. Do not use them.",
    "internal/test/binding",
  );
}

function cloneMutableInternalBinding(binding) {
  if (!binding || typeof binding !== "object") {
    return binding;
  }
  const clone = { ...binding };
  const prototype = Object.getPrototypeOf(binding);
  if (prototype !== null && prototype !== Object.prototype) {
    Object.setPrototypeOf(clone, prototype);
  }
  return clone;
}

function getMutableInternalBinding(name) {
  let override = internalTestBindingState.overrides.get(name);
  if (override === undefined) {
    override = cloneMutableInternalBinding(getNodeInternalBinding(name));
    if (!override || typeof override !== "object") {
      override = {};
    }
    internalTestBindingState.overrides.set(name, override);
  }
  return override;
}

function getInternalTestBinding(name) {
  emitInternalTestBindingWarning();
  if (String(name).startsWith("internal_only")) {
    const error = new Error(`No such binding: ${name}`);
    error.code = "ERR_INVALID_MODULE";
    throw error;
  }
  if (name === "tty_wrap" || name === "os") {
    return getMutableInternalBinding(name);
  }
  return getNodeInternalBinding(name);
}

function getMutableTtyWrapBinding() {
  return getMutableInternalBinding("tty_wrap");
}

function getMutableOsBinding() {
  return getMutableInternalBinding("os");
}

function loadReadlineBuiltin() {
  if (internalTestBindingState.readlineBuiltin === undefined) {
    internalTestBindingState.readlineBuiltin =
      Module._load("readline", null, false) ??
      denoGetBuiltinModule("readline");
  }
  return internalTestBindingState.readlineBuiltin;
}

function createNeovexInternalTestBindingModule() {
  const primordials = {};
  return {
    internalBinding: getInternalTestBinding,
    primordials,
    default: {
      internalBinding: getInternalTestBinding,
      primordials,
    },
  };
}

function cloneBuiltinModuleWithOverrides(builtinModule, overrides = {}) {
  const clone = {};
  Object.defineProperties(clone, Object.getOwnPropertyDescriptors(builtinModule));
  Object.defineProperties(clone, Object.getOwnPropertyDescriptors(overrides));
  const prototype = Object.getPrototypeOf(builtinModule);
  if (prototype !== null) {
    Object.setPrototypeOf(clone, prototype);
  }
  return Object.freeze(clone);
}

function createNeovexInternalDgramModule() {
  const dnsBuiltin = denoGetBuiltinModule("dns");
  const dgramBuiltin = denoGetBuiltinModule("dgram");
  const netBuiltin = denoGetBuiltinModule("net");
  const udpWrapBinding = getNodeInternalBinding("udp_wrap");
  const utilBinding = getNodeInternalBinding("util");
  const uvBinding = getNodeInternalBinding("uv");
  const UDP = udpWrapBinding?.UDP;
  const guessHandleType = utilBinding?.guessHandleType;
  const invalidArgumentErrno = uvBinding?.UV_EINVAL;
  const ERR_SOCKET_BAD_TYPE =
    internalErrors?.codes?.ERR_SOCKET_BAD_TYPE ?? internalErrors?.ERR_SOCKET_BAD_TYPE;
  if (
    typeof dnsBuiltin?.lookup !== "function" ||
    typeof dgramBuiltin?.createSocket !== "function" ||
    typeof netBuiltin?.isIP !== "function" ||
    typeof UDP !== "function" ||
    typeof guessHandleType !== "function" ||
    typeof invalidArgumentErrno !== "number" ||
    typeof ERR_SOCKET_BAD_TYPE !== "function"
  ) {
    throw new Error(
      "Neovex Node22 bootstrap expected dns, dgram, net.isIP, udp_wrap, util.guessHandleType, uv.UV_EINVAL, and ERR_SOCKET_BAD_TYPE to be available",
    );
  }
  const probeSocket = dgramBuiltin.createSocket("udp4");
  const kStateSymbol = (() => {
    try {
      return Object.getOwnPropertySymbols(probeSocket).find((symbol) => {
        const state = probeSocket[symbol];
        return state && typeof state === "object" && "handle" in state;
      });
    } finally {
      try {
        probeSocket.close();
      } catch (_error) {
        // Ignore close races for the bootstrap-only probe socket.
      }
    }
  })();
  if (typeof kStateSymbol !== "symbol") {
    throw new Error(
      "Neovex Node22 bootstrap expected dgram sockets to expose an internal state symbol",
    );
  }
  function lookup4(lookup, address, callback) {
    return lookup(address || "127.0.0.1", 4, callback);
  }
  function lookup6(lookup, address, callback) {
    return lookup(address || "::1", 6, callback);
  }
  function newHandle(type, lookup = dnsBuiltin.lookup) {
    const handle = new UDP();
    if (type === "udp4") {
      handle.lookup = lookup4.bind(handle, lookup);
      return handle;
    }
    if (type === "udp6") {
      handle.lookup = lookup6.bind(handle, lookup);
      handle.bind = handle.bind6;
      handle.connect = handle.connect6;
      handle.send = handle.send6;
      return handle;
    }
    throw new ERR_SOCKET_BAD_TYPE();
  }
  function _createSocketHandle(address, port, addressType, fd, flags) {
    const handle = newHandle(addressType);
    let err;
    if (Number.isInteger(fd) && fd > 0) {
      err = guessHandleType(fd) === "UDP"
        ? handle.open(fd)
        : invalidArgumentErrno;
    } else if (port || address) {
      if (address && netBuiltin.isIP(address) === 0) {
        err = invalidArgumentErrno;
      } else {
        err = handle.bind(address, port || 0, flags);
      }
    }
    if (err) {
      handle.close();
      return err;
    }
    return handle;
  }
  return Object.freeze({
    kStateSymbol,
    newHandle,
    _createSocketHandle,
  });
}

function createNeovexDgramModule(internalDgramModule) {
  const dgramBuiltin = denoGetBuiltinModule("dgram");
  if (typeof dgramBuiltin?.createSocket !== "function") {
    throw new Error(
      "Neovex Node22 bootstrap expected the dgram builtin to be available",
    );
  }
  return cloneBuiltinModuleWithOverrides(dgramBuiltin, {
    _createSocketHandle: internalDgramModule._createSocketHandle,
  });
}

function createNeovexTlsModule() {
  const tlsBuiltin = denoGetBuiltinModule("tls");
  const ERR_TLS_INVALID_CONTEXT =
    internalErrors?.codes?.ERR_TLS_INVALID_CONTEXT ?? internalErrors?.ERR_TLS_INVALID_CONTEXT;
  if (
    typeof tlsBuiltin?.connect !== "function" ||
    typeof tlsBuiltin?.createSecurePair !== "function" ||
    typeof ERR_TLS_INVALID_CONTEXT !== "function"
  ) {
    throw new Error(
      "Neovex Node22 bootstrap expected the tls builtin and ERR_TLS_INVALID_CONTEXT to be available",
    );
  }

  return cloneBuiltinModuleWithOverrides(tlsBuiltin, {
    createSecurePair(context, ...args) {
      if (!context || typeof context !== "object" || !("context" in context)) {
        throw new ERR_TLS_INVALID_CONTEXT("context");
      }
      return tlsBuiltin.createSecurePair(context, ...args);
    },
  });
}

function createNeovexTtyModule() {
  const ttyBuiltin = denoGetBuiltinModule("tty");
  const netModule = denoGetBuiltinModule("net");
  const Socket = netModule?.Socket;
  const BuiltinReadStream = ttyBuiltin?.ReadStream;
  const BuiltinWriteStream = ttyBuiltin?.WriteStream;
  const builtinIsatty = ttyBuiltin?.isatty;
  const ERR_INVALID_FD =
    internalErrors?.codes?.ERR_INVALID_FD ?? internalErrors?.ERR_INVALID_FD;
  const ERR_TTY_INIT_FAILED =
    internalErrors?.codes?.ERR_TTY_INIT_FAILED ?? internalErrors?.ERR_TTY_INIT_FAILED;

  if (
    !ttyBuiltin ||
    typeof Socket !== "function" ||
    typeof BuiltinWriteStream !== "function" ||
    typeof BuiltinReadStream !== "function" ||
    typeof builtinIsatty !== "function" ||
    typeof ERR_INVALID_FD !== "function"
  ) {
    throw new Error(
      "Neovex Node22 bootstrap expected tty builtin, net.Socket, and tty error constructors to be available",
    );
  }

  const defaultTtyWrapBinding = getNodeInternalBinding("tty_wrap");

  function createTtyInitFailedError(ctx) {
    const code = String(ctx?.code ?? "UNKNOWN");
    const detail = String(ctx?.message ?? "unknown error");
    const error = new Error(
      `TTY initialization failed: uv_tty_init returned ${code} (${detail})`,
    );
    error.name = "SystemError";
    error.code = "ERR_TTY_INIT_FAILED";
    error.info = ctx;
    if (ctx?.errno !== undefined) {
      error.errno = ctx.errno;
    }
    if (ctx?.syscall !== undefined) {
      error.syscall = ctx.syscall;
    }
    return error;
  }

  function maybeConvertInvalidFdError(fd, originalError) {
    const ttyWrapBinding = getMutableTtyWrapBinding();
    if (typeof ttyWrapBinding?.TTY !== "function") {
      return originalError;
    }
    const ctx = {};
    try {
      new ttyWrapBinding.TTY(fd, ctx);
    } catch (_error) {
      // Ignore direct constructor throws and prefer the populated ctx shape.
    }
    if (ctx.code !== undefined) {
      return createTtyInitFailedError(ctx);
    }
    return originalError;
  }

  function ReadStream(fd, options) {
    if (!(this instanceof ReadStream)) {
      return new ReadStream(fd, options);
    }
    if (fd >> 0 !== fd || fd < 0) {
      throw new ERR_INVALID_FD(fd);
    }

    const ttyWrapBinding = getMutableTtyWrapBinding();
    if (
      ttyWrapBinding === defaultTtyWrapBinding ||
      ttyWrapBinding?.TTY === defaultTtyWrapBinding?.TTY ||
      typeof ttyWrapBinding?.TTY !== "function"
    ) {
      try {
        return BuiltinReadStream(fd, options);
      } catch (error) {
        throw maybeConvertInvalidFdError(fd, error);
      }
    }

    const ctx = {};
    const tty = new ttyWrapBinding.TTY(fd, ctx);
    if (ctx.code !== undefined) {
      throw createTtyInitFailedError(ctx);
    }

    Socket.call(this, {
      readableHighWaterMark: 0,
      handle: tty,
      manualStart: true,
      ...(options ?? {}),
    });

    this.isRaw = false;
    this.isTTY = true;
  }

  function WriteStream(fd) {
    if (!(this instanceof WriteStream)) {
      return new WriteStream(fd);
    }
    if (fd >> 0 !== fd || fd < 0) {
      throw new ERR_INVALID_FD(fd);
    }

    const ttyWrapBinding = getMutableTtyWrapBinding();
    if (
      ttyWrapBinding === defaultTtyWrapBinding ||
      ttyWrapBinding?.TTY === defaultTtyWrapBinding?.TTY ||
      typeof ttyWrapBinding?.TTY !== "function"
    ) {
      try {
        return BuiltinWriteStream(fd);
      } catch (error) {
        throw maybeConvertInvalidFdError(fd, error);
      }
    }

    const ctx = {};
    const tty = new ttyWrapBinding.TTY(fd, ctx);
    if (ctx.code !== undefined) {
      throw createTtyInitFailedError(ctx);
    }

    Socket.call(this, {
      readableHighWaterMark: 0,
      handle: tty,
      manualStart: true,
    });

    if (typeof this._handle?.setBlocking === "function") {
      this._handle.setBlocking(true);
    }

    const winSize = [0, 0];
    const err = typeof tty?.getWindowSize === "function"
      ? tty.getWindowSize(winSize)
      : typeof this._handle?.getWindowSize === "function"
      ? this._handle.getWindowSize(winSize)
      : 0;
    if (!err) {
      this.columns = winSize[0];
      this.rows = winSize[1];
    }
  }

  Object.setPrototypeOf(ReadStream.prototype, BuiltinReadStream.prototype);
  Object.setPrototypeOf(ReadStream, BuiltinReadStream);
  ReadStream.prototype.setRawMode = function setRawMode(flag) {
    flag = !!flag;
    this._handle.setRawMode(flag);
    this.isRaw = flag;
    return this;
  };

  Object.setPrototypeOf(WriteStream.prototype, BuiltinWriteStream.prototype);
  Object.setPrototypeOf(WriteStream, BuiltinWriteStream);

  WriteStream.prototype.cursorTo = function cursorTo(x, y, callback) {
    return loadReadlineBuiltin().cursorTo(this, x, y, callback);
  };
  WriteStream.prototype.moveCursor = function moveCursor(dx, dy, callback) {
    return loadReadlineBuiltin().moveCursor(this, dx, dy, callback);
  };
  WriteStream.prototype.clearLine = function clearLine(dir, callback) {
    return loadReadlineBuiltin().clearLine(this, dir, callback);
  };
  WriteStream.prototype.clearScreenDown = function clearScreenDown(callback) {
    return loadReadlineBuiltin().clearScreenDown(this, callback);
  };

  return {
    ...ttyBuiltin,
    isatty: builtinIsatty,
    ReadStream,
    WriteStream,
    default: {
      ...ttyBuiltin,
      isatty: builtinIsatty,
      ReadStream,
      WriteStream,
    },
  };
}

function createNeovexOsModule() {
  const osBuiltin = denoGetBuiltinModule("os");
  if (!osBuiltin || typeof osBuiltin.homedir !== "function") {
    throw new Error(
      "Neovex Node22 bootstrap expected os builtin and os.homedir() to be available",
    );
  }

  const defaultOsBinding = getNodeInternalBinding("os");
  const defaultGetHomeDirectory = defaultOsBinding?.getHomeDirectory;

  function createCheckedFunctionError(ctx) {
    const syscall = String(ctx?.syscall ?? "unknown");
    const code = String(ctx?.code ?? "UNKNOWN");
    const message = String(ctx?.message ?? "unknown error");
    const error = new Error(
      `A system error occurred: ${syscall} returned ${code} (${message})`,
    );
    error.info = ctx;
    return error;
  }

  function homedir() {
    const osBinding = getMutableOsBinding();
    const overrideGetHomeDirectory = osBinding?.getHomeDirectory;
    if (
      typeof overrideGetHomeDirectory === "function" &&
      overrideGetHomeDirectory !== defaultGetHomeDirectory
    ) {
      const ctx = {};
      const result = overrideGetHomeDirectory(ctx);
      if (
        ctx.syscall !== undefined ||
        ctx.code !== undefined ||
        ctx.message !== undefined
      ) {
        throw createCheckedFunctionError(ctx);
      }
      return result ?? null;
    }
    return osBuiltin.homedir();
  }

  const osModule = Object.create(
    Object.getPrototypeOf(osBuiltin),
    Object.getOwnPropertyDescriptors(osBuiltin),
  );
  Object.defineProperty(osModule, "homedir", {
    configurable: true,
    enumerable: true,
    writable: true,
    value: homedir,
  });

  const defaultExport =
    osBuiltin.default && typeof osBuiltin.default === "object"
      ? Object.create(
          Object.getPrototypeOf(osBuiltin.default),
          Object.getOwnPropertyDescriptors(osBuiltin.default),
        )
      : Object.create(
          Object.getPrototypeOf(osBuiltin),
          Object.getOwnPropertyDescriptors(osBuiltin),
        );
  Object.defineProperty(defaultExport, "homedir", {
    configurable: true,
    enumerable: true,
    writable: true,
    value: homedir,
  });
  Object.defineProperty(osModule, "default", {
    configurable: true,
    enumerable: true,
    writable: true,
    value: defaultExport,
  });

  return osModule;
}

function shouldForceInteractiveTerminal() {
  return globalThis.__neovexNodeCompatTerm !== undefined &&
    globalThis.__neovexNodeCompatTerm !== "dumb";
}

function getReadlineSymbolByDescription(prototype, description) {
  let currentPrototype = prototype;
  while (
    currentPrototype &&
    currentPrototype !== Object.prototype &&
    currentPrototype !== Function.prototype
  ) {
    const symbol = Object.getOwnPropertySymbols(currentPrototype).find((candidate) =>
      candidate.description === description
    );
    if (symbol !== undefined) {
      return symbol;
    }
    currentPrototype = Object.getPrototypeOf(currentPrototype);
  }
  return undefined;
}

const NEOVEX_READLINE_PROMPT_PATCHED = Symbol.for("neovex.readlinePromptPatched");
const NEOVEX_READLINE_TAB_COMPLETE_PATCHED = Symbol.for("neovex.readlineTabCompletePatched");

function patchReadlineBuiltinPrototype(BuiltinInterface) {
  const prototype = BuiltinInterface?.prototype;
  if (!prototype || typeof prototype !== "object") {
    return;
  }

  const refreshLineSymbol = getReadlineSymbolByDescription(prototype, "kRefreshLine");
  const tabCompleteSymbol = getReadlineSymbolByDescription(prototype, "kTabComplete");

  if (
    prototype[NEOVEX_READLINE_PROMPT_PATCHED] !== true &&
    typeof prototype.prompt === "function" &&
    typeof refreshLineSymbol === "symbol"
  ) {
    const originalPrompt = prototype.prompt;
    Object.defineProperty(prototype, "prompt", {
      configurable: true,
      writable: true,
      value: function neovexPrompt(preserveCursor) {
        if (this.paused) {
          this.resume();
        }
        if (this.terminal && shouldForceInteractiveTerminal()) {
          if (!preserveCursor) {
            this.cursor = 0;
          }
          this[refreshLineSymbol]();
          return;
        }
        return originalPrompt.call(this, preserveCursor);
      },
    });
    Object.defineProperty(prototype, NEOVEX_READLINE_PROMPT_PATCHED, {
      configurable: true,
      enumerable: false,
      writable: false,
      value: true,
    });
  }

  if (
    prototype[NEOVEX_READLINE_TAB_COMPLETE_PATCHED] !== true &&
    typeof tabCompleteSymbol === "symbol" &&
    typeof prototype[tabCompleteSymbol] === "function"
  ) {
    const originalTabComplete = prototype[tabCompleteSymbol];
    Object.defineProperty(prototype, tabCompleteSymbol, {
      configurable: true,
      writable: true,
      value: async function neovexTabComplete(...args) {
        try {
          return await originalTabComplete.apply(this, args);
        } catch (error) {
          if (this.closed && error?.code === "ERR_USE_AFTER_CLOSE") {
            return;
          }
          throw error;
        }
      },
    });
    Object.defineProperty(prototype, NEOVEX_READLINE_TAB_COMPLETE_PATCHED, {
      configurable: true,
      enumerable: false,
      writable: false,
      value: true,
    });
  }
}

function patchReadlineInterfaceInstance(instance, builtinInterfacePrototype) {
  const tabCompleteSymbol = getReadlineSymbolByDescription(
    builtinInterfacePrototype,
    "_tabComplete",
  );

  if (
    shouldForceInteractiveTerminal() &&
    instance?.terminal === true &&
    typeof builtinInterfacePrototype?._ttyWrite === "function"
  ) {
    instance._ttyWrite = builtinInterfacePrototype._ttyWrite.bind(instance);
  }

  if (typeof tabCompleteSymbol === "symbol" && typeof instance?.[tabCompleteSymbol] === "function") {
    const originalTabComplete = instance[tabCompleteSymbol];
    Object.defineProperty(instance, tabCompleteSymbol, {
      configurable: true,
      enumerable: false,
      writable: true,
      value: async function neovexTabComplete(...args) {
        try {
          return await originalTabComplete.apply(this, args);
        } catch (error) {
          if (this.closed && error?.code === "ERR_USE_AFTER_CLOSE") {
            return;
          }
          throw error;
        }
      },
    });
  }
  return instance;
}

function createReadlineInterfaceWrapper(BuiltinInterface) {
  function NeovexReadlineInterface(...args) {
    const newTarget = new.target ?? NeovexReadlineInterface;
    const instance = Reflect.construct(BuiltinInterface, args, newTarget);
    return patchReadlineInterfaceInstance(instance, BuiltinInterface.prototype);
  }
  Object.setPrototypeOf(NeovexReadlineInterface, BuiltinInterface);
  Object.setPrototypeOf(NeovexReadlineInterface.prototype, BuiltinInterface.prototype);
  return NeovexReadlineInterface;
}

function createReadlineCreateInterfaceWrapper(createInterface, builtinInterfacePrototype) {
  return function neovexCreateInterface(...args) {
    return patchReadlineInterfaceInstance(
      createInterface.apply(this, args),
      builtinInterfacePrototype,
    );
  };
}

function createNeovexReadlineModule() {
  const readlineBuiltin = denoGetBuiltinModule("readline");
  if (
    !readlineBuiltin ||
    typeof readlineBuiltin.Interface !== "function" ||
    typeof readlineBuiltin.createInterface !== "function"
  ) {
    throw new Error(
      "Neovex Node22 bootstrap expected readline builtin to expose Interface and createInterface()",
    );
  }

  patchReadlineBuiltinPrototype(readlineBuiltin.Interface);

  const Interface = createReadlineInterfaceWrapper(readlineBuiltin.Interface);
  const createInterface = createReadlineCreateInterfaceWrapper(
    readlineBuiltin.createInterface,
    readlineBuiltin.Interface.prototype,
  );

  return {
    ...readlineBuiltin,
    Interface,
    createInterface,
    default: {
      ...readlineBuiltin,
      Interface,
      createInterface,
    },
  };
}

function createNeovexReadlinePromisesModule() {
  const readlinePromisesBuiltin = denoGetBuiltinModule("readline/promises");
  if (
    !readlinePromisesBuiltin ||
    typeof readlinePromisesBuiltin.Interface !== "function" ||
    typeof readlinePromisesBuiltin.createInterface !== "function"
  ) {
    throw new Error(
      "Neovex Node22 bootstrap expected readline/promises builtin to expose Interface and createInterface()",
    );
  }

  patchReadlineBuiltinPrototype(readlinePromisesBuiltin.Interface);

  const Interface = createReadlineInterfaceWrapper(
    readlinePromisesBuiltin.Interface,
  );
  const createInterface = createReadlineCreateInterfaceWrapper(
    readlinePromisesBuiltin.createInterface,
    readlinePromisesBuiltin.Interface.prototype,
  );

  return {
    ...readlinePromisesBuiltin,
    Interface,
    createInterface,
    default: {
      ...readlinePromisesBuiltin,
      Interface,
      createInterface,
    },
  };
}

function cloneModuleExports(moduleExports) {
  return Object.create(
    Object.getPrototypeOf(moduleExports),
    Object.getOwnPropertyDescriptors(moduleExports),
  );
}

function validateCallbackFunction(value, name) {
  if (typeof value === "function") {
    return;
  }
  const receivedType = value === null ? "null" : typeof value;
  const error = new TypeError(
    `ERR_INVALID_ARG_TYPE: The "${name}" argument must be of type function. Received ${receivedType}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
}

function formatLenTypeDetail(value) {
  if (value === null) {
    return " Received null";
  }
  if (Array.isArray(value)) {
    return " Received an instance of Array";
  }
  if (typeof value === "string") {
    return ` Received type string ('${value}')`;
  }
  if (typeof value === "boolean") {
    return ` Received type boolean (${value})`;
  }
  if (typeof value === "object") {
    const constructorName =
      typeof value?.constructor?.name === "string" && value.constructor.name.length > 0
        ? value.constructor.name
        : "Object";
    return ` Received an instance of ${constructorName}`;
  }
  return ` Received type ${typeof value} (${String(value)})`;
}

function normalizeTruncateLength(len) {
  if (len === undefined) {
    return 0;
  }
  if (typeof len !== "number") {
    const error = new TypeError(
      `The "len" argument must be of type number.${formatLenTypeDetail(len)}`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(len)) {
    const error = new RangeError(
      `The value of "len" is out of range. It must be an integer. Received ${len}`,
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return Math.max(0, len);
}

function createOpenEnoentError(path, originalError) {
  const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
  error.code = "ENOENT";
  error.path = path;
  error.syscall = "open";
  error.errno = originalError?.errno ?? null;
  if (typeof originalError?.stack === "string" && originalError.stack.length > 0) {
    error.stack = originalError.stack;
  }
  return error;
}

function createOpenEexistError(path, originalError) {
  const error = new Error(`EEXIST: file already exists, open '${path}'`);
  error.code = "EEXIST";
  error.path = path;
  error.syscall = "open";
  error.errno = originalError?.errno ?? null;
  if (typeof originalError?.stack === "string" && originalError.stack.length > 0) {
    error.stack = originalError.stack;
  }
  return error;
}

function isInvalidOpenThrow(error) {
  return error instanceof TypeError && error.message === "invalid_argument";
}

function sanitizeRmdirOptions(options) {
  if (options === undefined) {
    return undefined;
  }
  if (options === null || typeof options !== "object") {
    return options;
  }
  const sanitized = { ...options };
  if (sanitized.recursive === false) {
    delete sanitized.recursive;
  }
  return sanitized;
}

function createRecursiveRmdirTargetError(path) {
  const error = new Error("The target path is not a directory");
  error.code = processModule?.platform === "win32" ? "ENOENT" : "ENOTDIR";
  error.syscall = "rmdir";
  error.path = path;
  return error;
}

function createMissingRmdirError(path, originalError) {
  const error = new Error(
    typeof originalError?.message === "string" && originalError.message.length > 0
      ? originalError.message
      : "ENOENT: no such file or directory, rmdir",
  );
  error.name = typeof originalError?.name === "string" ? originalError.name : "Error";
  error.code = originalError?.code ?? "ENOENT";
  error.errno = originalError?.errno ?? null;
  error.syscall = "rmdir";
  error.path = path ?? originalError?.path;
  if (typeof originalError?.stack === "string" && originalError.stack.length > 0) {
    error.stack = originalError.stack;
  }
  return error;
}

function normalizeRmdirError(error, path) {
  if (!error || typeof error !== "object") {
    return error;
  }
  const normalizedError = new Error(
    typeof error.message === "string" && error.message.length > 0
      ? error.message
      : "rmdir failed",
  );
  normalizedError.name = typeof error.name === "string" ? error.name : "Error";
  normalizedError.code = error.code;
  normalizedError.errno = error.errno;
  normalizedError.syscall = "rmdir";
  normalizedError.path = path ?? error.path;
  if (typeof error.stack === "string" && error.stack.length > 0) {
    normalizedError.stack = error.stack;
  }
  return normalizedError;
}

function emitTruncateFdDeprecationWarning() {
  globalThis.process?.emitWarning?.(
    "Using fs.truncate with a file descriptor is deprecated. Please use fs.ftruncate with a file descriptor instead.",
    "DeprecationWarning",
    "DEP0081",
  );
}

function createScandirNotDirectoryError(path) {
  const displayPath =
    typeof path === "string"
      ? path
      : path instanceof URL
      ? path.pathname
      : String(path);
  const error = new Error(
    `${processModule?.platform === "win32" ? "ENOENT" : "ENOTDIR"}: not a directory, scandir '${displayPath}'`,
  );
  error.code = processModule?.platform === "win32" ? "ENOENT" : "ENOTDIR";
  error.syscall = "scandir";
  error.path = displayPath;
  return error;
}

function sortReaddirResults(result, options) {
  if (typeof options !== "object" || options === null || options.withFileTypes !== true) {
    return result;
  }
  if (!Array.isArray(result)) {
    return result;
  }
  return result.slice().sort((left, right) =>
    String(left?.name ?? left).localeCompare(String(right?.name ?? right))
  );
}

function createInvalidEncodingOptionError(encoding) {
  const error = new TypeError(`The value "${encoding}" is invalid for option "encoding"`);
  error.code = "ERR_INVALID_ARG_VALUE";
  return error;
}

function validateReaddirEncodingOption(encoding) {
  if (encoding === undefined || encoding === null || encoding === "buffer") {
    return;
  }
  if (typeof encoding === "string" && Buffer.isEncoding(encoding)) {
    return;
  }
  throw createInvalidEncodingOptionError(encoding);
}

function snapshotReaddirOptions(options) {
  if (options === null || options === undefined) {
    return {};
  }
  if (typeof options === "string") {
    validateReaddirEncodingOption(options);
    return { encoding: options };
  }
  if (typeof options !== "object") {
    return options;
  }
  validateReaddirEncodingOption(options.encoding);
  return { ...options };
}

function snapshotFsEncodingOptions(options) {
  if (options === null || options === undefined) {
    return options;
  }
  if (typeof options === "string") {
    validateReaddirEncodingOption(options);
    return { encoding: options };
  }
  if (typeof options !== "object") {
    return options;
  }
  validateReaddirEncodingOption(options.encoding);
  return { ...options };
}

function validateFsWatchSignal(signal) {
  if (signal === undefined) {
    return;
  }
  if (signal instanceof AbortSignal) {
    return;
  }
  const error = new TypeError(
    `The "options.signal" argument must be of type AbortSignal. Received ${signal === null ? "null" : typeof signal}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
}

function pathBasename(path) {
  if (typeof path !== "string" || path.length === 0) {
    return null;
  }
  const trimmedPath = path.replace(/[\\/]+$/, "");
  const separatorIndex = Math.max(
    trimmedPath.lastIndexOf("/"),
    trimmedPath.lastIndexOf("\\"),
  );
  return separatorIndex === -1 ? trimmedPath : trimmedPath.slice(separatorIndex + 1);
}

function getWatchPathBasename(path) {
  try {
    return pathBasename(String(fsBuiltin.realpathSync(path)));
  } catch (_error) {
    try {
      return pathBasename(String(path));
    } catch (_innerError) {
      return null;
    }
  }
}

function watchPathToErrorPath(path) {
  if (typeof path === "string") {
    return path;
  }
  if (Buffer.isBuffer(path)) {
    return path.toString();
  }
  if (path instanceof URL) {
    return path.pathname;
  }
  return String(path);
}

function normalizeWatchError(error, watchPath) {
  if (!error || typeof error !== "object") {
    return error;
  }
  if (error.path === undefined) {
    error.path = watchPath;
  }
  if (error.filename === undefined) {
    error.filename = watchPath;
  }
  if (error.syscall === undefined) {
    error.syscall = "watch";
  }
  if (error.code === "ENOENT" && error.errno === undefined) {
    error.errno = -2;
  }
  if (error.code === "ENODEV" && error.errno === undefined) {
    error.errno = -19;
  }
  if (
    error.code === "ENOENT" &&
    (typeof error.message !== "string" ||
      !error.message.startsWith("ENOENT: no such file or directory"))
  ) {
    error.message = `ENOENT: no such file or directory, watch '${watchPath}'`;
  }
  if (
    error.code === "ENODEV" &&
    error.message !== `ENODEV: no such device, watch '${watchPath}'`
  ) {
    error.message = `ENODEV: no such device, watch '${watchPath}'`;
  }
  return error;
}

function watchErrorCodeFromStatus(status) {
  switch (status) {
    case -2:
      return "ENOENT";
    case -19:
      return "ENODEV";
    default:
      return undefined;
  }
}

function watchFilenameMatchesWatchPath(filename, watchPathBasename) {
  if (watchPathBasename === null) {
    return false;
  }
  if (typeof filename === "string") {
    return filename === watchPathBasename;
  }
  if (Buffer.isBuffer(filename)) {
    return filename.toString("utf8") === watchPathBasename;
  }
  return false;
}

function normalizeWatchFilename(filename, watchPathBasename, watchingDirectory) {
  if (watchingDirectory) {
    return watchFilenameMatchesWatchPath(filename, watchPathBasename) ? null : filename;
  }
  if (watchPathBasename === null) {
    return filename;
  }
  if (filename === "") {
    return watchPathBasename;
  }
  if (Buffer.isBuffer(filename) && filename.length === 0) {
    return Buffer.from(watchPathBasename);
  }
  return filename;
}

function encodeWatchFilename(filename, encoding) {
  if (filename == null || typeof filename !== "string") {
    return filename;
  }
  if (encoding === undefined || encoding === null || encoding === "utf8" || encoding === "utf-8") {
    return filename;
  }
  if (encoding === "buffer") {
    return Buffer.from(filename);
  }
  return Buffer.from(filename).toString(encoding);
}

function toBigIntStatsField(value) {
  if (typeof value === "bigint") {
    return value;
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    return BigInt(Math.trunc(value));
  }
  return 0n;
}

function convertStatsToBigIntStats(stats) {
  if (!stats || typeof stats !== "object") {
    return stats;
  }
  if (
    typeof stats.atimeNs === "bigint" &&
    typeof stats.mtimeNs === "bigint" &&
    typeof stats.ctimeNs === "bigint" &&
    typeof stats.birthtimeNs === "bigint"
  ) {
    return stats;
  }
  const atimeMs = toBigIntStatsField(stats.atimeMs);
  const mtimeMs = toBigIntStatsField(stats.mtimeMs);
  const ctimeMs = toBigIntStatsField(stats.ctimeMs);
  const birthtimeMs = toBigIntStatsField(stats.birthtimeMs);
  const bigIntStatsCtor =
    globalThis.process?.getBuiltinModule?.("module")
      ?._load?.("internal/fs/utils", null, false)?.BigIntStats;
  if (typeof bigIntStatsCtor !== "function") {
    return stats;
  }
  return new bigIntStatsCtor(
    toBigIntStatsField(stats.dev),
    toBigIntStatsField(stats.mode),
    toBigIntStatsField(stats.nlink),
    toBigIntStatsField(stats.uid),
    toBigIntStatsField(stats.gid),
    toBigIntStatsField(stats.rdev),
    toBigIntStatsField(stats.blksize),
    toBigIntStatsField(stats.ino),
    toBigIntStatsField(stats.size),
    toBigIntStatsField(stats.blocks),
    atimeMs,
    mtimeMs,
    ctimeMs,
    birthtimeMs,
    typeof stats.atimeNs === "bigint" ? stats.atimeNs : atimeMs * 1000000n,
    typeof stats.mtimeNs === "bigint" ? stats.mtimeNs : mtimeMs * 1000000n,
    typeof stats.ctimeNs === "bigint" ? stats.ctimeNs : ctimeMs * 1000000n,
    typeof stats.birthtimeNs === "bigint"
      ? stats.birthtimeNs
      : birthtimeMs * 1000000n,
  );
}

function isNodeFd(value) {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

function validateBooleanOption(value, name) {
  if (typeof value === "boolean") {
    return;
  }
  const error = new TypeError(
    `The "${name}" argument must be of type boolean. Received ${typeof value}`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
}

function parseWriteFileMode(mode, defaultValue = 0o666) {
  if (mode === undefined || mode === null) {
    return defaultValue;
  }
  if (typeof mode === "number" && Number.isInteger(mode) && mode >= 0 && mode <= 0xFFFFFFFF) {
    return mode;
  }
  if (typeof mode === "string" && /^[0-7]+$/.test(mode)) {
    return Number.parseInt(mode, 8);
  }
  const error = new TypeError(
    'The "mode" argument must be a 32-bit unsigned integer or an octal string',
  );
  error.code = "ERR_INVALID_ARG_VALUE";
  throw error;
}

function normalizeWriteFileOptions(path, options, defaultFlag) {
  const normalizedOptions = copyObject(getOptions(options, {
    encoding: "utf8",
    mode: 0o666,
    flag: defaultFlag,
    flush: false,
  }));

  if (defaultFlag === "a" && (!normalizedOptions.flag || isNodeFd(path))) {
    normalizedOptions.flag = "a";
  }

  const flush = normalizedOptions.flush ?? false;
  validateBooleanOption(flush, "options.flush");

  const flag = normalizedOptions.flag || defaultFlag;
  const isUserFd = isNodeFd(path);
  const validatedPath = isUserFd ? path : getValidatedPathToString(path);
  const hasExplicitFlush =
    normalizedOptions.__neovexHasExplicitFlush === true ||
    (typeof options === "object" &&
      options !== null &&
      Object.prototype.hasOwnProperty.call(options, "flush"));
  return {
    flag,
    flush,
    hasExplicitFlush,
    isUserFd,
    normalizedOptions,
    validatedPath,
  };
}

function writeFileSyncWithCurrentFsBindings(fsModule, path, data, options, defaultFlag) {
  const {
    flag,
    flush,
    isUserFd,
    normalizedOptions,
    validatedPath,
  } = normalizeWriteFileOptions(path, options, defaultFlag);

  if (
    typeof data === "string" &&
    (normalizedOptions.encoding === "utf8" || normalizedOptions.encoding === "utf-8") &&
    typeof internalFsBinding?.writeFileUtf8 === "function" &&
    !isUserFd
  ) {
    return internalFsBinding.writeFileUtf8(
      validatedPath,
      data,
      stringToFlags(flag, "options.flag"),
      parseWriteFileMode(normalizedOptions.mode, 0o666),
    );
  }

  if (!ArrayBuffer.isView(data)) {
    validateStringAfterArrayBufferView(data, "data");
    data = Buffer.from(data, normalizedOptions.encoding || "utf8");
  }

  const fd = isUserFd ? path : fsModule.openSync(validatedPath, flag, normalizedOptions.mode);
  let offset = 0;
  let length = data.byteLength;
  try {
    while (length > 0) {
      const written = fsModule.writeSync(fd, data, offset, length);
      offset += written;
      length -= written;
    }
    if (flush) {
      fsModule.fsyncSync(fd);
    }
  } finally {
    if (!isUserFd) {
      fsModule.closeSync(fd);
    }
  }
}

function writeAllWithCurrentFsBindings(fsModule, fd, isUserFd, buffer, offset, length, flush, callback) {
  fsModule.write(fd, buffer, offset, length, null, (writeError, written) => {
    if (writeError) {
      if (isUserFd) {
        callback(writeError);
        return;
      }
      fsModule.close(fd, (closeError) => callback(writeError ?? closeError ?? null));
      return;
    }

    if (written !== length) {
      writeAllWithCurrentFsBindings(
        fsModule,
        fd,
        isUserFd,
        buffer,
        offset + written,
        length - written,
        flush,
        callback,
      );
      return;
    }

    const finish = (error) => {
      if (isUserFd) {
        callback(error ?? null);
        return;
      }
      fsModule.close(fd, (closeError) => callback(error ?? closeError ?? null));
    };

    if (!flush) {
      finish(null);
      return;
    }

    fsModule.fsync(fd, finish);
  });
}

function sanitizeWriteFileOptions(normalizedOptions) {
  const sanitizedOptions = copyObject(normalizedOptions);
  delete sanitizedOptions.flush;
  delete sanitizedOptions.__neovexHasExplicitFlush;
  return sanitizedOptions;
}

function invokeFsCallbackAsync(callback, error) {
  setTimeout(() => callback(error ?? null), 0);
}

function flushWrittenFile(fsModule, fsBuiltin, pathOrFd, isUserFd, callback) {
  if (isUserFd) {
    fsModule.fsync(pathOrFd, callback);
    return;
  }

  fsBuiltin.open(pathOrFd, "r", (openError, fd) => {
    if (openError) {
      callback(openError);
      return;
    }
    fsModule.fsync(fd, (syncError) => {
      fsModule.close(fd, (closeError) => callback(syncError ?? closeError ?? null));
    });
  });
}

function writeFileWithCurrentFsBindings(fsModule, fsBuiltin, path, data, options, defaultFlag, callback) {
  callback ||= options;
  validateCallbackFunction(callback, "cb");

  const {
    flag,
    flush,
    hasExplicitFlush,
    isUserFd,
    normalizedOptions,
    validatedPath,
  } = normalizeWriteFileOptions(path, options, defaultFlag);
  const sanitizedOptions = sanitizeWriteFileOptions(normalizedOptions);

  if (!hasExplicitFlush) {
    return fsBuiltin.writeFile(validatedPath, data, sanitizedOptions, callback);
  }

  if (!flush) {
    return fsBuiltin.writeFile(validatedPath, data, sanitizedOptions, (error) => {
      invokeFsCallbackAsync(callback, error);
    });
  }

  return fsBuiltin.writeFile(validatedPath, data, sanitizedOptions, (writeError) => {
    if (writeError) {
      callback(writeError);
      return;
    }
    flushWrittenFile(fsModule, fsBuiltin, validatedPath, isUserFd, callback);
  });
}

function shouldUseBindingReaddir(options) {
  return typeof options === "object" && options !== null && options.withFileTypes === true;
}

function bindingReaddirEncoding(options) {
  if (typeof options === "object" && options !== null && typeof options.encoding === "string") {
    return options.encoding;
  }
  return undefined;
}

function bindingReaddirErrorCode(error) {
  if (typeof error?.code === "string" && error.code.length > 0) {
    return error.code;
  }
  const message = typeof error?.message === "string" ? error.message : "";
  return message.match(/"code":"([A-Z0-9_]+)"/)?.[1] ?? null;
}

function normalizeBindingReaddirError(error, path) {
  if (bindingReaddirErrorCode(error) === "ENOTDIR") {
    return createScandirNotDirectoryError(path);
  }
  return error;
}

function bindingReaddirType(entry, constants) {
  const isDirectory = typeof entry?.isDirectory === "function"
    ? entry.isDirectory()
    : entry?.isDirectory === true;
  if (isDirectory) {
    return constants.UV_DIRENT_DIR;
  }
  const isFile = typeof entry?.isFile === "function" ? entry.isFile() : entry?.isFile === true;
  if (isFile) {
    return constants.UV_DIRENT_FILE;
  }
  const isBlockDevice = typeof entry?.isBlockDevice === "function"
    ? entry.isBlockDevice()
    : entry?.isBlockDevice === true;
  if (isBlockDevice) {
    return constants.UV_DIRENT_BLOCK;
  }
  const isCharacterDevice = typeof entry?.isCharacterDevice === "function"
    ? entry.isCharacterDevice()
    : entry?.isCharacterDevice === true;
  if (isCharacterDevice) {
    return constants.UV_DIRENT_CHAR;
  }
  const isSymbolicLink = typeof entry?.isSymbolicLink === "function"
    ? entry.isSymbolicLink()
    : entry?.isSymbolicLink === true;
  if (isSymbolicLink) {
    return constants.UV_DIRENT_LINK;
  }
  const isFifo = typeof entry?.isFIFO === "function" ? entry.isFIFO() : entry?.isFIFO === true;
  if (isFifo) {
    return constants.UV_DIRENT_FIFO;
  }
  const isSocket = typeof entry?.isSocket === "function" ? entry.isSocket() : entry?.isSocket === true;
  if (isSocket) {
    return constants.UV_DIRENT_SOCKET;
  }
  return constants.UV_DIRENT_UNKNOWN;
}

function bindingReaddirResult(fsBuiltin, path, options) {
  const entries = sortReaddirResults(
    globalThis.__neovexSyncHostValue("op_neovex_runtime_read_dir_sync", {
      path: String(path),
    }) ?? [],
    { withFileTypes: true },
  );
  const encoding = bindingReaddirEncoding(options);
  return [
    entries.map((entry) => encoding === "buffer" ? Buffer.from(String(entry?.name ?? "")) : String(entry?.name ?? "")),
    entries.map((entry) => bindingReaddirType(entry, fsBuiltin.constants)),
  ];
}

function direntsFromBindingResult(fsBuiltin, path, result) {
  const names = Array.isArray(result?.[0]) ? result[0] : [];
  const types = Array.isArray(result?.[1]) ? result[1] : [];
  const constants = fsBuiltin.constants;
  return names.map((name, index) => {
    let type = types[index];
    if (type === constants.UV_DIRENT_UNKNOWN) {
      try {
        const stats = fsBuiltin.lstatSync(pathModule.join(String(path), String(name)));
        type = bindingReaddirType(stats, constants);
      } catch {
        type = constants.UV_DIRENT_UNKNOWN;
      }
    }
    const dirent = new fsBuiltin.Dirent(name, type);
    dirent.parentPath = String(path);
    return dirent;
  });
}

function ensureInternalFsBindingReaddir(fsBuiltin) {
  if (typeof internalFsBinding?.readdir === "function") {
    return;
  }
  internalFsBinding.readdir = function readdir(path, encoding, _withFileTypes, req) {
    const options = { withFileTypes: true };
    if (encoding !== undefined) {
      options.encoding = encoding;
    }
    if (req && typeof req === "object") {
      queueMicrotask(() => {
        try {
          req.oncomplete?.(null, bindingReaddirResult(fsBuiltin, path, options));
        } catch (error) {
          req.oncomplete?.(normalizeBindingReaddirError(error, path));
        }
      });
      return;
    }
    try {
      return bindingReaddirResult(fsBuiltin, path, options);
    } catch (error) {
      throw normalizeBindingReaddirError(error, path);
    }
  };
}

function createDirClosedError() {
  const error = new Error("Directory handle was closed");
  error.code = "ERR_DIR_CLOSED";
  return error;
}

function createDirConcurrentOperationError() {
  const error = new Error("Cannot synchronously operate on a directory while an async read is pending");
  error.code = "ERR_DIR_CONCURRENT_OPERATION";
  return error;
}

function createFsFileTooLargeError(size) {
  const error = new RangeError(`File size (${size}) is greater than 2 GiB`);
  error.code = "ERR_FS_FILE_TOO_LARGE";
  return error;
}

function readFileMaybeDecode(data, encoding) {
  const buffer = Buffer.isBuffer(data)
    ? data
    : Buffer.from(data.buffer, data.byteOffset, data.byteLength);
  if (encoding === undefined || encoding === null) {
    return buffer;
  }
  return buffer.toString(encoding);
}

function readFileStatsRepresentRegularFile(stats) {
  if (!stats || typeof stats !== "object") {
    return false;
  }
  if (typeof stats.isFile === "function") {
    return stats.isFile();
  }
  return stats.isFile === true;
}

function readFileStatsSize(stats) {
  return Number(stats?.size ?? 0);
}

function readFileOptionsFromArgument(options) {
  const normalizedOptions = copyObject(getOptions(options, {
    flag: "r",
  }));
  if (normalizedOptions.encoding !== "buffer") {
    assertEncoding(normalizedOptions.encoding);
  }
  return normalizedOptions;
}

function createReadFileAbortError(cause = undefined) {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  error.code = "ABORT_ERR";
  if (cause !== undefined) {
    error.cause = cause;
  }
  return error;
}

function checkReadFileAborted(signal) {
  if (signal?.aborted) {
    throw createReadFileAbortError(signal.reason);
  }
}

function wrapDirHandle(dir) {
  if (!dir || typeof dir !== "object" || dir.__neovexWrappedDir === true) {
    return dir;
  }

  const originalRead = dir.read.bind(dir);
  const originalReadSync = dir.readSync.bind(dir);
  const originalClose = dir.close.bind(dir);
  const originalCloseSync = dir.closeSync.bind(dir);
  let closed = false;
  let pendingAsyncReads = 0;

  Object.defineProperty(dir, "__neovexWrappedDir", {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });

  dir.read = function read(callback) {
    if (callback !== undefined) {
      validateCallbackFunction(callback, "callback");
    }
    if (closed) {
      const error = createDirClosedError();
      if (callback) {
        callback(error);
        return;
      }
      return Promise.reject(error);
    }

    pendingAsyncReads += 1;
    const promise = Promise.resolve(originalRead()).finally(() => {
      pendingAsyncReads -= 1;
    });
    if (callback) {
      promise.then(
        (value) => callback(null, value),
        (error) => callback(error),
      );
      return;
    }
    return promise;
  };

  dir.readSync = function readSync() {
    if (pendingAsyncReads > 0) {
      throw createDirConcurrentOperationError();
    }
    if (closed) {
      throw createDirClosedError();
    }
    return originalReadSync();
  };

  dir.close = function close(callback) {
    if (callback !== undefined) {
      validateCallbackFunction(callback, "callback");
    }
    if (closed) {
      const error = createDirClosedError();
      if (callback) {
        callback(error);
        return;
      }
      return Promise.reject(error);
    }

    closed = true;
    const promise = Promise.resolve(originalClose()).then(() => undefined);
    if (callback) {
      promise.then(
        () => callback(null),
        (error) => callback(error),
      );
      return;
    }
    return promise;
  };

  dir.closeSync = function closeSync() {
    if (pendingAsyncReads > 0) {
      throw createDirConcurrentOperationError();
    }
    if (closed) {
      throw createDirClosedError();
    }
    closed = true;
    return originalCloseSync();
  };

  return dir;
}

function createNeovexFsPromisesModule() {
  const fsBuiltin = denoGetBuiltinModule("fs");
  const fsPromisesBuiltin = fsBuiltin?.promises;
  if (!fsPromisesBuiltin || typeof fsPromisesBuiltin.open !== "function") {
    throw new Error("Neovex Node22 bootstrap expected process.getBuiltinModule('fs').promises to be available");
  }

  const fsPromisesModule = cloneModuleExports(fsPromisesBuiltin);
  fsPromisesModule.rmdir = async function rmdir(path, options) {
    const normalizedOptions = internalFsUtils.validateRmdirOptions(options);
    if (!normalizedOptions.recursive) {
      try {
        return await fsPromisesBuiltin.rmdir(path, sanitizeRmdirOptions(options));
      } catch (error) {
        throw normalizeRmdirError(error, path);
      }
    }

    emitRecursiveRmdirWarning();

    try {
      const stats = await fsPromisesBuiltin.lstat(path);
      if (stats?.isDirectory?.()) {
        return fsPromisesBuiltin.rm(path, {
          recursive: true,
          force: false,
          maxRetries: normalizedOptions.maxRetries,
          retryDelay: normalizedOptions.retryDelay,
        });
      }
    } catch (error) {
      throw error;
    }

    throw createRecursiveRmdirTargetError(path);
  };
  Object.defineProperty(fsPromisesModule, "readdir", {
    value: async function readdir(path, options) {
      const optionsSnapshot = snapshotReaddirOptions(options);
      const stats = await fsPromisesBuiltin.stat(path);
      if (!stats?.isDirectory?.()) {
        throw createScandirNotDirectoryError(path);
      }
      return sortReaddirResults(
        await fsPromisesBuiltin.readdir(path, optionsSnapshot),
        optionsSnapshot,
      );
    },
    configurable: true,
    enumerable: true,
    writable: true,
  });
  fsPromisesModule.opendir = async function opendir(path, options) {
    return wrapDirHandle(await fsPromisesBuiltin.opendir(path, options));
  };
  return fsPromisesModule;
}

function createNeovexFsModule(fsPromisesModule) {
  const fsBuiltin = denoGetBuiltinModule("fs");
  if (!fsBuiltin || typeof fsBuiltin.rmdir !== "function" || typeof fsBuiltin.rm !== "function") {
    throw new Error("Neovex Node22 bootstrap expected process.getBuiltinModule('fs') to expose rmdir and rm");
  }

  const fsModule = cloneModuleExports(fsBuiltin);
  Object.defineProperty(fsModule, "promises", {
    value: fsPromisesModule,
    configurable: true,
    enumerable: true,
    writable: true,
  });
  const originalFsPromisesWriteFile = fsPromisesModule.writeFile.bind(fsPromisesModule);
  const originalFsPromisesAppendFile = fsPromisesModule.appendFile.bind(fsPromisesModule);
  function openFlagsNeedWrite(flags) {
    if (typeof flags === "number") {
      return true;
    }
    const normalizedFlags = typeof flags === "string" && flags.length > 0 ? flags : "r";
    return normalizedFlags.includes("w")
      || normalizedFlags.includes("a")
      || normalizedFlags.includes("+");
  }
  function validateOpenPath(path, flags) {
    const validatedPath = getValidatedPathToString(path);
    return globalThis.__neovexSyncHostValue("op_neovex_runtime_validate_open_path", {
      path: validatedPath,
      write: openFlagsNeedWrite(flags),
    });
  }
  function precheckReadFilePathSync(validatedPath, options) {
    const fd = fsModule.openSync(validatedPath, options?.flag ?? "r");
    try {
      const statFields = fsModule.fstatSync(fd);
      if (readFileStatsRepresentRegularFile(statFields)) {
        const size = readFileStatsSize(statFields);
        if (size > internalFsConstants.kIoMaxLength) {
          throw createFsFileTooLargeError(size);
        }
      }
    } finally {
      fsModule.closeSync(fd);
    }
  }
  function precheckReadFilePathAsync(validatedPath, options, callback) {
    const signal = options?.signal;
    try {
      checkReadFileAborted(signal);
    } catch (error) {
      callback(error);
      return;
    }
    fsModule.open(validatedPath, options?.flag ?? "r", (openError, fd) => {
      if (openError) {
        callback(openError);
        return;
      }
      fsModule.fstat(fd, (statError, statFields) => {
        const finish = (error) => {
          fsModule.close(fd, (_closeError) => callback(error ?? null));
        };
        if (statError) {
          finish(statError);
          return;
        }
        try {
          checkReadFileAborted(signal);
          if (readFileStatsRepresentRegularFile(statFields)) {
            const size = readFileStatsSize(statFields);
            if (size > internalFsConstants.kIoMaxLength) {
              finish(createFsFileTooLargeError(size));
              return;
            }
          }
        } catch (error) {
          finish(error);
          return;
        }
        finish(null);
      });
    });
  }
  function openFlagsRequireExclusiveCreate(flags) {
    if (typeof flags === "number") {
      return (flags & fsBuiltin.constants.O_EXCL) !== 0;
    }
    const normalizedFlags = typeof flags === "string" && flags.length > 0 ? flags : "r";
    return normalizedFlags.includes("x");
  }
  function normalizeOpenError(path, flags, error, callback) {
    if (!isInvalidOpenThrow(error)) {
      callback(error);
      return;
    }
    return fsBuiltin.stat(path, (statError) => {
      if (statError?.code === "ENOENT") {
        callback(createOpenEnoentError(path, statError));
        return;
      }
      if (!statError && openFlagsRequireExclusiveCreate(flags)) {
        callback(createOpenEexistError(path, error));
        return;
      }
      callback(error);
    });
  }
  function normalizeOpenSyncError(path, flags, error) {
    if (error == null) {
      try {
        fsBuiltin.statSync(path);
      } catch (statError) {
        if (statError?.code === "ENOENT") {
          throw createOpenEnoentError(path, statError);
        }
      }
      throw error;
    }
    if (!isInvalidOpenThrow(error)) {
      throw error;
    }
    try {
      fsBuiltin.statSync(path);
    } catch (statError) {
      if (statError?.code === "ENOENT") {
        throw createOpenEnoentError(path, statError);
      }
    }
    if (openFlagsRequireExclusiveCreate(flags)) {
      throw createOpenEexistError(path, error);
    }
    throw error;
  }
  fsPromisesModule.access = async function access(path, mode) {
    const validatedPath = getValidatedPathToString(path);
    return await Promise.prototype.then.call(
      fsBuiltin.promises.access(validatedPath, mode),
      undefined,
      (error) => {
        const message = typeof error?.message === "string" && error.message.length > 0
          ? error.message
          : "access failed";
        let wrappedError;
        try {
          const ErrorCtor =
            typeof error?.constructor === "function" ? error.constructor : Error;
          wrappedError = new ErrorCtor(message);
        } catch {
          wrappedError = new Error(message);
        }
        wrappedError.name = typeof error?.name === "string" && error.name.length > 0
          ? error.name
          : "Error";
        wrappedError.code = error?.code;
        wrappedError.errno = error?.errno;
        wrappedError.path = error?.path;
        wrappedError.syscall = error?.syscall;
        throw wrappedError;
      },
    );
  };
  fsModule.open = function open(path, flags, mode, callback) {
    if (typeof flags === "function") {
      callback = flags;
      flags = "r";
      mode = 0o666;
    } else if (typeof mode === "function") {
      callback = mode;
      mode = 0o666;
    }
    validateCallbackFunction(callback, "callback");
    const validatedPath = validateOpenPath(path, flags);
    try {
      return fsBuiltin.open(validatedPath, flags, mode, (error, fd) => {
        if (!error) {
          callback(null, fd);
          return;
        }
        normalizeOpenError(validatedPath, flags, error, (normalizedError) => callback(normalizedError));
      });
    } catch (error) {
      return normalizeOpenError(validatedPath, flags, error, (normalizedError) => callback(normalizedError));
    }
  };
  fsModule.openSync = function openSync(path, flags = "r", mode = 0o666) {
    const validatedPath = validateOpenPath(path, flags);
    try {
      return fsBuiltin.openSync(validatedPath, flags, mode);
    } catch (error) {
      normalizeOpenSyncError(validatedPath, flags, error);
    }
  };
  fsModule.readFile = function readFile(path, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    validateCallbackFunction(callback, "callback");
    if (typeof path === "number" || (path && typeof path === "object" && typeof path.read === "function")) {
      return fsBuiltin.readFile(path, options, callback);
    }
    const normalizedOptions = readFileOptionsFromArgument(options);
    const validatedPath = validateOpenPath(path, normalizedOptions.flag);
    precheckReadFilePathAsync(validatedPath, normalizedOptions, (precheckError) => {
      if (precheckError) {
        callback(precheckError);
        return;
      }
      try {
        checkReadFileAborted(normalizedOptions.signal);
      } catch (error) {
        callback(error);
        return;
      }
      fsBuiltin.readFile(validatedPath, normalizedOptions, callback);
    });
  };
  fsModule.readFileSync = function readFileSync(path, options) {
    if (
      typeof path === "number" ||
      (path && typeof path === "object" && typeof path.readSync === "function")
    ) {
      return fsBuiltin.readFileSync(path, options);
    }
    const normalizedOptions = readFileOptionsFromArgument(options);
    const validatedPath = validateOpenPath(path, normalizedOptions.flag);
    precheckReadFilePathSync(validatedPath, normalizedOptions);
    return fsBuiltin.readFileSync(validatedPath, normalizedOptions);
  };
  fsModule.writeFile = function writeFile(path, data, options, callback) {
    return writeFileWithCurrentFsBindings(fsModule, fsBuiltin, path, data, options, "w", callback);
  };
  fsModule.appendFile = function appendFile(path, data, options, callback) {
    const hasExplicitFlush =
      typeof options === "object" &&
      options !== null &&
      Object.prototype.hasOwnProperty.call(options, "flush");
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    options = copyObject(getOptions(options, {
      encoding: "utf8",
      mode: 0o666,
      flag: "a",
      flush: false,
    }));
    if (!options.flag || isNodeFd(path)) {
      options.flag = "a";
    }
    if (!hasExplicitFlush) {
      const validatedPath = isNodeFd(path) ? path : getValidatedPathToString(path);
      return fsBuiltin.appendFile(validatedPath, data, options, callback);
    }
    options.__neovexHasExplicitFlush = true;
    return fsModule.writeFile(path, data, options, callback);
  };
  fsModule.writeFileSync = function writeFileSync(path, data, options) {
    return writeFileSyncWithCurrentFsBindings(fsModule, path, data, options, "w");
  };
  fsModule.appendFileSync = function appendFileSync(path, data, options) {
    return writeFileSyncWithCurrentFsBindings(fsModule, path, data, options, "a");
  };
  fsPromisesModule.writeFile = async function writeFile(path, data, options) {
    const normalizedOptions = copyObject(getOptions(options, {
      encoding: "utf8",
      mode: 0o666,
      flag: "w",
      flush: false,
    }));
    const flush = normalizedOptions.flush ?? false;
    validateBooleanOption(flush, "options.flush");
    return originalFsPromisesWriteFile(path, data, sanitizeWriteFileOptions(normalizedOptions));
  };
  fsPromisesModule.appendFile = async function appendFile(path, data, options) {
    const normalizedOptions = copyObject(getOptions(options, {
      encoding: "utf8",
      mode: 0o666,
      flag: "a",
      flush: false,
    }));
    const flush = normalizedOptions.flush ?? false;
    validateBooleanOption(flush, "options.flush");
    if (!normalizedOptions.flag || isNodeFd(path)) {
      normalizedOptions.flag = "a";
    }
    return originalFsPromisesAppendFile(path, data, sanitizeWriteFileOptions(normalizedOptions));
  };
  const plainRmdirSyncTargets = new Set();

  function performPlainRmdirSync(path, options) {
    try {
      fsBuiltin.lstatSync(path);
    } catch (error) {
      throw createMissingRmdirError(path, error);
    }
    try {
      return fsBuiltin.rmdirSync(path, sanitizeRmdirOptions(options));
    } catch (error) {
      throw normalizeRmdirError(error, path);
    }
  }

  function recursiveRmdirSync(currentPath) {
    const entries = fsBuiltin.readdirSync(currentPath);
    for (const entry of entries) {
      const childPath = pathModule.join(currentPath, entry);
      let childStats;
      try {
        childStats = fsBuiltin.lstatSync(childPath);
      } catch (error) {
        if (error?.code === "ENOENT") {
          continue;
        }
        throw error;
      }
      if (childStats?.isDirectory?.()) {
        fsBuiltin.rmSync(childPath, { recursive: true, force: false });
      } else {
        try {
          fsBuiltin.unlinkSync(childPath);
        } catch (error) {
          if (error?.code !== "ENOENT") {
            throw error;
          }
        }
      }
    }
    plainRmdirSyncTargets.add(currentPath);
    try {
      return fsModule.rmdirSync(currentPath, {});
    } finally {
      plainRmdirSyncTargets.delete(currentPath);
    }
  }
  fsModule.rmdir = function rmdir(path, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }

    validateCallbackFunction(callback, "cb");

    if (!options?.recursive) {
      return fsBuiltin.rmdir(
        path,
        sanitizeRmdirOptions(options),
        (error) => callback(error ? normalizeRmdirError(error, path) : error),
      );
    }

    const normalizedOptions = internalFsUtils.validateRmdirOptions(options);
    emitRecursiveRmdirWarning();
    return fsBuiltin.lstat(path, (error, stats) => {
      if (error) {
        return callback(error);
      } else if (stats?.isDirectory?.()) {
        return fsBuiltin.rm(path, {
          recursive: true,
          force: false,
          maxRetries: normalizedOptions.maxRetries,
          retryDelay: normalizedOptions.retryDelay,
        }, callback);
      }
      return callback(createRecursiveRmdirTargetError(path));
    });
  };
  fsModule.rmdirSync = function rmdirSync(path, options) {
    if (plainRmdirSyncTargets.has(path)) {
      return performPlainRmdirSync(path, options);
    }

    if (!options?.recursive) {
      return performPlainRmdirSync(path, options);
    }

    const normalizedOptions = internalFsUtils.validateRmdirOptions(options);
    emitRecursiveRmdirWarning();
    try {
      const stats = fsBuiltin.lstatSync(path);
      if (stats?.isDirectory?.()) {
        return recursiveRmdirSync(path);
      }
    } catch (error) {
      if (error?.code !== "ENOENT") {
        throw error;
      }
      throw error;
    }

    throw createRecursiveRmdirTargetError(path);
  };
  Object.defineProperty(fsModule, "readdir", {
    value: function readdir(path, options, callback) {
      if (typeof options === "function") {
        callback = options;
        options = undefined;
      }
      const optionsSnapshot = snapshotReaddirOptions(options);

      validateCallbackFunction(callback, "callback");
      if (shouldUseBindingReaddir(optionsSnapshot)) {
        ensureInternalFsBindingReaddir(fsBuiltin);
        const request = {
          oncomplete(readdirError, result) {
            if (readdirError) {
              callback(readdirError);
              return;
            }
            callback(null, direntsFromBindingResult(fsBuiltin, path, result));
          },
        };
        internalFsBinding.readdir(
          path,
          bindingReaddirEncoding(optionsSnapshot),
          true,
          request,
        );
        return;
      }
      return fsBuiltin.stat(path, (error, stats) => {
        if (error) {
          callback(error);
          return;
        }
        if (!stats?.isDirectory?.()) {
          callback(createScandirNotDirectoryError(path));
          return;
        }
        fsBuiltin.readdir(path, optionsSnapshot, (readdirError, result) => {
          if (readdirError) {
            callback(readdirError);
            return;
          }
          callback(null, sortReaddirResults(result, optionsSnapshot));
        });
      });
    },
    configurable: true,
    enumerable: true,
    writable: true,
  });
  Object.defineProperty(fsModule, "readdirSync", {
    value: function readdirSync(path, options) {
      const optionsSnapshot = snapshotReaddirOptions(options);
      if (shouldUseBindingReaddir(optionsSnapshot)) {
        ensureInternalFsBindingReaddir(fsBuiltin);
        const result = internalFsBinding.readdir(
          path,
          bindingReaddirEncoding(optionsSnapshot),
          true,
        );
        return direntsFromBindingResult(fsBuiltin, path, result);
      }
      const stats = fsBuiltin.statSync(path);
      if (!stats?.isDirectory?.()) {
        throw createScandirNotDirectoryError(path);
      }
      return sortReaddirResults(fsBuiltin.readdirSync(path, optionsSnapshot), optionsSnapshot);
    },
    configurable: true,
    enumerable: true,
    writable: true,
  });
  fsModule.readlink = function readlink(path, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    return fsBuiltin.readlink(path, snapshotFsEncodingOptions(options), callback);
  };
  fsModule.readlinkSync = function readlinkSync(path, options) {
    return fsBuiltin.readlinkSync(path, snapshotFsEncodingOptions(options));
  };
  fsModule.realpath = function realpath(path, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    return fsBuiltin.realpath(path, snapshotFsEncodingOptions(options), callback);
  };
  if (typeof fsBuiltin.realpath?.native === "function") {
    fsModule.realpath.native = function realpathNative(path, options, callback) {
      if (typeof options === "function") {
        callback = options;
        options = undefined;
      }
      return fsBuiltin.realpath.native(path, snapshotFsEncodingOptions(options), callback);
    };
  }
  fsModule.realpathSync = function realpathSync(path, options) {
    return fsBuiltin.realpathSync(path, snapshotFsEncodingOptions(options));
  };
  if (typeof fsBuiltin.realpathSync?.native === "function") {
    fsModule.realpathSync.native = function realpathSyncNative(path, options) {
      return fsBuiltin.realpathSync.native(path, snapshotFsEncodingOptions(options));
    };
  }
  fsModule.mkdtemp = function mkdtemp(prefix, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    return fsBuiltin.mkdtemp(prefix, snapshotFsEncodingOptions(options), callback);
  };
  fsModule.mkdtempSync = function mkdtempSync(prefix, options) {
    return fsBuiltin.mkdtempSync(prefix, snapshotFsEncodingOptions(options));
  };
  fsModule.watch = function watch(path, options, listener) {
    if (typeof options === "function") {
      listener = options;
      options = undefined;
    }
    const optionsSnapshot = snapshotFsEncodingOptions(options);
    const signal = optionsSnapshot?.signal;
    validateFsWatchSignal(signal);
    const builtinOptions =
      optionsSnapshot && typeof optionsSnapshot === "object"
        ? (() => {
            const cloned = { ...optionsSnapshot };
            delete cloned.signal;
            return cloned;
          })()
        : optionsSnapshot;
    const watchErrorPath = watchPathToErrorPath(path);
    let watcher;
    try {
      watcher = fsBuiltin.watch(path, builtinOptions, listener);
    } catch (error) {
      throw normalizeWatchError(error, watchErrorPath);
    }
    const requestedEncoding =
      optionsSnapshot && typeof optionsSnapshot === "object"
        ? optionsSnapshot.encoding
        : undefined;
    const watchPathBasename = getWatchPathBasename(path);
    let watchingDirectory = false;
    try {
      watchingDirectory = fsBuiltin.statSync(path)?.isDirectory?.() === true;
    } catch (_error) {
      watchingDirectory = false;
    }
    const originalEmit = watcher.emit;
    if (typeof originalEmit === "function") {
      let watchErrored = false;
      const emitWatchChange = (eventType, filename) => {
        if (watchErrored) {
          return false;
        }
        const normalizedFilename = encodeWatchFilename(
          normalizeWatchFilename(filename, watchPathBasename, watchingDirectory),
          requestedEncoding,
        );
        return Reflect.apply(originalEmit, watcher, ["change", eventType, normalizedFilename]);
      };
      const emitWatchError = (error, errorPath = watchErrorPath) => {
        watchErrored = true;
        return Reflect.apply(originalEmit, watcher, [
          "error",
          normalizeWatchError(error, errorPath),
        ]);
      };
      watcher.emit = function emit(eventName, ...args) {
        if (eventName === "change") {
          const [eventType, filename] = args;
          return emitWatchChange(eventType, filename);
        }
        if (eventName === "error" && args.length > 0) {
          return emitWatchError(args[0]);
        }
        return Reflect.apply(originalEmit, this, [eventName, ...args]);
      };

      const handle =
        watcher._handle && typeof watcher._handle === "object" ? watcher._handle : {};
      if (typeof handle.onchange !== "function") {
        handle.onchange = function onchange(status, eventType, filename) {
          if (typeof status === "number" && status < 0) {
            const errorPath =
              filename === undefined || filename === null
                ? watchErrorPath
                : watchPathToErrorPath(filename);
            const error = new Error();
            error.code =
              typeof eventType === "string" && eventType.length > 0
                ? eventType
                : watchErrorCodeFromStatus(status);
            error.errno = status;
            error.path = errorPath;
            error.filename = errorPath;
            error.syscall = "watch";
            try {
              watcher.close();
            } catch (_error) {
              // Ignore close races; the synthetic error still needs to surface.
            }
            return emitWatchError(error, errorPath);
          }
          return emitWatchChange(eventType, filename);
        };
      }
      if (watcher._handle !== handle) {
        Object.defineProperty(watcher, "_handle", {
          value: handle,
          configurable: true,
          enumerable: false,
          writable: true,
        });
      }
    }
    if (signal !== undefined) {
      const closeWatcher = () => watcher.close();
      if (signal.aborted) {
        processModule?.nextTick?.(closeWatcher);
      } else {
        signal.addEventListener("abort", closeWatcher, { once: true });
      }
    }
    return watcher;
  };
  fsModule.watchFile = function watchFile(filename, options, listener) {
    if (typeof options === "function") {
      return fsBuiltin.watchFile(filename, options);
    }
    const wantsBigInt = options && typeof options === "object" && options.bigint === true;
    if (!wantsBigInt || typeof listener !== "function") {
      return fsBuiltin.watchFile(filename, options, listener);
    }
    const wrappedListener = function wrappedWatchFileListener(curr, prev) {
      return Reflect.apply(listener, this, [
        convertStatsToBigIntStats(curr),
        convertStatsToBigIntStats(prev),
      ]);
    };
    return fsBuiltin.watchFile(filename, options, wrappedListener);
  };
  function snapshotFsStreamOptions(options) {
    const optionsSnapshot = snapshotFsEncodingOptions(options);
    if (
      optionsSnapshot
      && typeof optionsSnapshot === "object"
      && "fd" in optionsSnapshot
      && optionsSnapshot.fd != null
    ) {
      return optionsSnapshot;
    }
    if (optionsSnapshot && typeof optionsSnapshot === "object") {
      return { ...optionsSnapshot, fs: fsModule };
    }
    return { fs: fsModule };
  }
  fsModule.createReadStream = function createReadStream(path, options) {
    return new fsModule.ReadStream(path, options);
  };
  fsModule.ReadStream = function ReadStream(path, options) {
    return fsBuiltin.ReadStream(path, snapshotFsStreamOptions(options));
  };
  fsModule.ReadStream.prototype = fsBuiltin.ReadStream.prototype;
  Object.setPrototypeOf(fsModule.ReadStream, fsBuiltin.ReadStream);
  fsModule.createWriteStream = function createWriteStream(path, options) {
    return new fsModule.WriteStream(path, options);
  };
  fsModule.WriteStream = function WriteStream(path, options) {
    return fsBuiltin.WriteStream(path, snapshotFsStreamOptions(options));
  };
  fsModule.WriteStream.prototype = fsBuiltin.WriteStream.prototype;
  Object.setPrototypeOf(fsModule.WriteStream, fsBuiltin.WriteStream);
  fsModule.truncate = function truncate(path, len, callback) {
    if (typeof len === "function") {
      callback = len;
      len = 0;
    }
    if (typeof path === "number") {
      emitTruncateFdDeprecationWarning();
      return callback === undefined
        ? fsBuiltin.ftruncate(path, len)
        : fsBuiltin.ftruncate(path, len, callback);
    }
    len = normalizeTruncateLength(len);
    validateCallbackFunction(callback, "callback");
    const validatedPath = getValidatedPathToString(path);
    try {
      return fsBuiltin.open(validatedPath, "r+", (openError, fd) => {
        if (openError) {
          callback(openError);
          return;
        }
        fsBuiltin.ftruncate(fd, len, (truncateError) => {
          fsBuiltin.close(fd, (closeError) => callback(truncateError || closeError || null));
        });
      });
    } catch (openError) {
      if (!isInvalidOpenThrow(openError)) {
        callback(openError);
        return;
      }
      return fsBuiltin.stat(validatedPath, (statError) => {
        if (statError?.code === "ENOENT") {
          callback(createOpenEnoentError(validatedPath, statError));
          return;
        }
        callback(openError);
      });
    }
  };
  fsModule.truncateSync = function truncateSync(path, len) {
    if (len === undefined) {
      len = 0;
    }
    if (typeof path === "number") {
      emitTruncateFdDeprecationWarning();
      return fsBuiltin.ftruncateSync(path, len);
    }
    const validatedPath = getValidatedPathToString(path);
    const fd = fsBuiltin.openSync(validatedPath, "r+");
    try {
      return fsBuiltin.ftruncateSync(fd, len);
    } finally {
      fsBuiltin.closeSync(fd);
    }
  };
  fsModule.lchmod = undefined;
  fsModule.lchmodSync = undefined;
  fsModule.symlink = function symlink(target, path, type, callback) {
    if (typeof type === "function") {
      callback = type;
      type = undefined;
    } else {
      internalFsUtils.stringToSymlinkType(type);
    }
    return fsBuiltin.symlink(target, path, type, callback);
  };
  fsModule.symlinkSync = function symlinkSync(target, path, type) {
    internalFsUtils.stringToSymlinkType(type);
    return fsBuiltin.symlinkSync(target, path, type);
  };
  fsModule.opendir = function opendir(path, options, callback) {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    if (callback === undefined) {
      return fsBuiltin.opendir(path, options, callback);
    }
    return fsBuiltin.opendir(path, options, (error, dir) => {
      if (error) {
        return callback(error);
      }
      callback(null, wrapDirHandle(dir));
    });
  };
  fsModule.opendirSync = function opendirSync(path, options) {
    return wrapDirHandle(fsBuiltin.opendirSync(path, options));
  };
  return fsModule;
}

const fsPromisesOverrideModule = createNeovexFsPromisesModule();
const fsOverrideModule = createNeovexFsModule(fsPromisesOverrideModule);
const internalDgramOverrideModule = createNeovexInternalDgramModule();
const dgramOverrideModule = createNeovexDgramModule(internalDgramOverrideModule);
const tlsOverrideModule = createNeovexTlsModule();
const internalTestBindingOverrideModule = createNeovexInternalTestBindingModule();
const ttyOverrideModule = createNeovexTtyModule();
const osOverrideModule = createNeovexOsModule();
const readlineOverrideModule = createNeovexReadlineModule();
const readlinePromisesOverrideModule = createNeovexReadlinePromisesModule();
const internalFsPromisesModule = Object.freeze({
  ...internalFsPromisesDefault,
  FileHandle: InternalFsPromisesFileHandle,
  default: internalFsPromisesDefault,
});

const INTERNAL_MODULE_OVERRIDES = Object.freeze({
  dgram: dgramOverrideModule,
  fs: fsOverrideModule,
  "fs/promises": fsPromisesOverrideModule,
  "internal/dgram": internalDgramOverrideModule,
  "internal/fs/promises": internalFsPromisesModule,
  "internal/test/binding": internalTestBindingOverrideModule,
  os: osOverrideModule,
  readline: readlineOverrideModule,
  "readline/promises": readlinePromisesOverrideModule,
  tls: tlsOverrideModule,
  tty: ttyOverrideModule,
  "internal/util/debuglog": Object.freeze({
    kNone: 1 << 0,
    kSkipLog: 1 << 1,
    kSkipTrace: 1 << 2,
    debuglog: utilModule?.debuglog,
    formatTime: internalConsoleConstructor?.formatTime,
    initializeDebugEnv() {},
  }),
});

function isPublicBuiltinOverrideSpecifier(specifier) {
  return (
    Object.prototype.hasOwnProperty.call(INTERNAL_MODULE_OVERRIDES, specifier) &&
    !specifier.startsWith("internal/")
  );
}

function normalizeBuiltinSpecifier(specifier) {
  if (typeof specifier !== "string") {
    return null;
  }
  return specifier.startsWith("node:")
    ? specifier.slice(5)
    : specifier;
}

function isPerfHooksSpecifier(specifier) {
  return normalizeBuiltinSpecifier(specifier) === "perf_hooks";
}

function getBuiltinOverride(specifier) {
  const normalizedSpecifier = normalizeBuiltinSpecifier(specifier);
  if (!normalizedSpecifier) {
    return undefined;
  }
  return INTERNAL_MODULE_OVERRIDES[normalizedSpecifier];
}

function getBuiltinModule(specifier) {
  if (isPerfHooksSpecifier(specifier)) {
    return globalThis.__neovexPerfHooksBuiltin;
  }
  const override = getBuiltinOverride(specifier);
  if (override !== undefined) {
    return override;
  }
  return denoGetBuiltinModule(specifier);
}

function isBuiltin(specifier) {
  const normalizedSpecifier = normalizeBuiltinSpecifier(specifier);
  return (
    (normalizedSpecifier !== null &&
      isPublicBuiltinOverrideSpecifier(normalizedSpecifier)) ||
    denoIsBuiltin(specifier)
  );
}

if (Array.isArray(builtinModules)) {
  for (const specifier of Object.keys(INTERNAL_MODULE_OVERRIDES)) {
    if (!isPublicBuiltinOverrideSpecifier(specifier)) {
      continue;
    }
    if (!builtinModules.includes(specifier)) {
      builtinModules.push(specifier);
    }
  }
}

const denoFsBuiltin = denoGetBuiltinModule("fs");
const MODULE_STAT_EXPERIMENTAL_WARNING =
  "Module._stat is an experimental feature and might change at any time";
let moduleStat = function moduleStat(filename) {
  if (typeof internalFsBinding?.internalModuleStat === "function") {
    return internalFsBinding.internalModuleStat(filename);
  }
  try {
    const stats = denoFsBuiltin?.statSync?.(filename);
    if (stats?.isFile?.() === true) {
      return 0;
    }
    if (stats) {
      return 1;
    }
  } catch (_error) {
    return -1;
  }
  return -1;
};

Object.defineProperty(Module, "_stat", {
  get() {
    return moduleStat;
  },
  set(stat) {
    processModule?.emitWarning?.(
      MODULE_STAT_EXPERIMENTAL_WARNING,
      "ExperimentalWarning",
    );
    moduleStat = stat;
    return true;
  },
  configurable: true,
});

function _stat(...args) {
  return Reflect.apply(moduleStat, Module, args);
}

Module._load = function (request, parent, isMain) {
  if (isPerfHooksSpecifier(request)) {
    return globalThis.__neovexPerfHooksBuiltin;
  }
  const override = getBuiltinOverride(request);
  if (override !== undefined) {
    return override;
  }
  return denoLoad(request, parent, isMain);
};

Module._resolveFilename = function (request, parent, isMain, options) {
  const normalizedSpecifier = normalizeBuiltinSpecifier(request);
  if (
    normalizedSpecifier &&
    Object.prototype.hasOwnProperty.call(INTERNAL_MODULE_OVERRIDES, normalizedSpecifier)
  ) {
    return normalizedSpecifier;
  }
  return denoResolveFilename(request, parent, isMain, options);
};

function _load(...args) {
  return Module._load(...args);
}

function _resolveFilename(...args) {
  return Module._resolveFilename(...args);
}

function maybeEmitDeprecatedBuiltinWarning(specifier) {
  if (typeof specifier !== "string") {
    return;
  }

  const normalizedSpecifier = specifier.startsWith("node:")
    ? specifier.slice(5)
    : specifier;
  const warning = DEPRECATED_REQUIRE_WARNINGS[normalizedSpecifier];
  if (!warning) {
    return;
  }

  globalThis.process?.emitWarning?.(
    warning.message,
    "DeprecationWarning",
    warning.code,
  );
}

function createRequire(filenameOrUrl) {
  const require = denoCreateRequire(filenameOrUrl);
  return new Proxy(require, {
    apply(target, thisArg, args) {
      const request = args[0];
      maybeEmitDeprecatedBuiltinWarning(request);
      if (isPerfHooksSpecifier(request)) {
        return globalThis.__neovexPerfHooksBuiltin;
      }
      const override = getBuiltinOverride(request);
      if (override !== undefined) {
        return override;
      }
      return Reflect.apply(target, thisArg, args);
    },
  });
}

Module.createRequire = createRequire;

export {
  _stat,
  _cache,
  _extensions,
  _findPath,
  _initPaths,
  _load,
  _nodeModulePaths,
  _pathCache,
  _preloadModules,
  _resolveFilename,
  _resolveLookupPaths,
  builtinModules,
  createRequire,
  findSourceMap,
  getBuiltinModule,
  globalPaths,
  isBuiltin,
  Module,
  register,
};
export default Module;
"#;

#[derive(Debug, Clone)]
struct BundleModuleCodeCacheEntry {
    hash: u64,
    data: Vec<u8>,
}

#[derive(Debug, Default)]
struct BundleModuleCodeCacheState {
    entries: HashMap<String, BundleModuleCodeCacheEntry>,
    latest_hashes: HashMap<String, u64>,
    prevented_hashes: HashMap<String, u64>,
    writes: usize,
}

#[derive(Debug, Default)]
pub struct BundleModuleCodeCache {
    state: Mutex<BundleModuleCodeCacheState>,
}

impl BundleModuleCodeCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn lookup(&self, specifier: &ModuleSpecifier, hash: u64) -> Option<Cow<'static, [u8]>> {
        let key = specifier.to_string();
        let mut state = self
            .state
            .lock()
            .expect("bundle code cache lock should not be poisoned");
        state.latest_hashes.insert(key.clone(), hash);
        match state.prevented_hashes.get(&key).copied() {
            Some(prevented_hash) if prevented_hash == hash => return None,
            Some(_) => {
                state.prevented_hashes.remove(&key);
            }
            None => {}
        }
        match state.entries.get(&key) {
            Some(entry) if entry.hash == hash => Some(Cow::Owned(entry.data.clone())),
            Some(_) => {
                state.entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn store(&self, specifier: ModuleSpecifier, hash: u64, code_cache: &[u8]) {
        let key = specifier.to_string();
        let mut state = self
            .state
            .lock()
            .expect("bundle code cache lock should not be poisoned");
        state.latest_hashes.insert(key.clone(), hash);
        if state.prevented_hashes.get(&key).copied() == Some(hash) {
            return;
        }
        state.entries.insert(
            key,
            BundleModuleCodeCacheEntry {
                hash,
                data: code_cache.to_vec(),
            },
        );
        state.writes = state.writes.saturating_add(1);
    }

    fn purge_and_prevent(&self, module_specifier: &str) {
        let mut state = self
            .state
            .lock()
            .expect("bundle code cache lock should not be poisoned");
        let removed = state.entries.remove(module_specifier);
        if let Some(hash) = state
            .latest_hashes
            .get(module_specifier)
            .copied()
            .or_else(|| removed.map(|entry| entry.hash))
        {
            state
                .prevented_hashes
                .insert(module_specifier.to_string(), hash);
        }
    }

    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.state
            .lock()
            .expect("bundle code cache lock should not be poisoned")
            .entries
            .len()
    }

    #[cfg(test)]
    pub(crate) fn write_count(&self) -> usize {
        self.state
            .lock()
            .expect("bundle code cache lock should not be poisoned")
            .writes
    }
}

#[derive(Debug, Clone)]
pub struct RestrictedModuleLoader {
    path_policy: RuntimePathPolicy,
    compatibility_target: RuntimeCompatibilityTarget,
    code_cache: Arc<BundleModuleCodeCache>,
}

impl RestrictedModuleLoader {
    pub fn new(
        path_policy: RuntimePathPolicy,
        compatibility_target: RuntimeCompatibilityTarget,
        code_cache: Arc<BundleModuleCodeCache>,
    ) -> Self {
        Self {
            path_policy,
            compatibility_target,
            code_cache,
        }
    }

    fn unsupported_node_builtin_error(&self, specifier: &str) -> JsErrorBox {
        let reason = match self.compatibility_target {
            RuntimeCompatibilityTarget::WebStandardIsolate => {
                "node: imports are unavailable under RuntimeCompatibilityTarget::WebStandardIsolate"
            }
            RuntimeCompatibilityTarget::Node22 => {
                "unsupported node: builtin for the current Node22 surface; the verified extension-backed lane currently includes core semantics builtins (node:assert/strict, node:buffer, node:console, node:events, node:path including posix/win32, node:punycode, node:querystring, node:string_decoder, node:url), process/timing builtins (node:process, node:timers, node:timers/promises, node:util, node:diagnostics_channel, node:perf_hooks), selected host/runtime builtins (node:fs, node:fs/promises, node:os, node:tty, node:stream including consumers/promises/web, node:child_process, node:crypto, node:worker_threads), and the in-progress networking family (node:dns, node:net, node:dgram, node:tls, node:http, node:https, node:http2), plus minimal Node globals"
            }
        };
        JsErrorBox::generic(format!(
            "unsupported runtime module import {specifier}: {reason}"
        ))
    }

    fn ensure_allowed_specifier(&self, specifier: &ModuleSpecifier) -> Result<(), JsErrorBox> {
        if self
            .supported_node_builtin_source(specifier.as_str())
            .is_some()
        {
            return Ok(());
        }
        if specifier.scheme() == "ext" {
            return Ok(());
        }
        if specifier.scheme() != "file" {
            return Err(JsErrorBox::generic(format!(
                "runtime bundle imports must stay within approved runtime roots, unsupported scheme: {}",
                specifier.scheme()
            )));
        }

        let path = specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {specifier}"))
        })?;
        self.path_policy
            .ensure_module_read_path(&path)
            .map(|_| ())
            .map_err(|error| JsErrorBox::generic(error.to_string()))
    }

    async fn load_module_source(
        &self,
        module_specifier: &ModuleSpecifier,
        options: ModuleLoadOptions,
    ) -> Result<ModuleSource, JsErrorBox> {
        if let Some(source) = self.supported_node_builtin_source(module_specifier.as_str()) {
            return Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::Bytes(source.as_bytes().to_vec().into_boxed_slice().into()),
                module_specifier,
                None,
            ));
        }
        let path = module_specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {module_specifier}"))
        })?;
        let module_type = module_type_from_path(&path, &options)?;
        let mut code = std::fs::read(&path).map_err(|source| {
            JsErrorBox::generic(format!(
                "failed to load runtime bundle module {}: {source}",
                path.display()
            ))
        })?;
        if module_type == ModuleType::JavaScript
            && matches!(
                self.compatibility_target,
                RuntimeCompatibilityTarget::Node22
            )
        {
            let package_json_resolver = build_package_json_resolver();
            if classify_resolved_module_kind(&path, package_json_resolver.as_ref())?
                == ResolvedNodeModuleKind::CommonJs
            {
                let source = String::from_utf8(code).map_err(|error| {
                    JsErrorBox::generic(format!(
                        "failed to decode runtime CommonJS module {} as utf8: {error}",
                        path.display()
                    ))
                })?;
                code = translate_commonjs_to_esm(&self.path_policy, module_specifier, &source)
                    .await?
                    .into_bytes();
            }
        }
        let hash = hash_module_source_bytes(&code);
        let code_cache = Some(SourceCodeCacheInfo {
            hash,
            data: self.code_cache.lookup(module_specifier, hash),
        });
        Ok(ModuleSource::new(
            module_type,
            ModuleSourceCode::Bytes(code.into_boxed_slice().into()),
            module_specifier,
            code_cache,
        ))
    }

    fn supported_node_builtin_source(&self, specifier: &str) -> Option<&'static str> {
        if !matches!(
            self.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        ) {
            return None;
        }
        match specifier {
            NODE_FS_SPECIFIER | NEOVEX_NODE_FS_SPECIFIER => Some(NODE_FS_MODULE_SOURCE),
            NODE_TLS_SPECIFIER => Some(NODE_TLS_MODULE_SOURCE),
            NODE_MODULE_SPECIFIER | NEOVEX_NODE_MODULE_SPECIFIER => Some(NODE_MODULE_MODULE_SOURCE),
            NODE_FS_PROMISES_SPECIFIER | NEOVEX_NODE_FS_PROMISES_SPECIFIER => {
                Some(NODE_FS_PROMISES_MODULE_SOURCE)
            }
            INTERNAL_READLINE_UTILS_SPECIFIER | NEOVEX_INTERNAL_READLINE_UTILS_SPECIFIER => {
                Some(INTERNAL_READLINE_UTILS_MODULE_SOURCE)
            }
            NODE_PERF_HOOKS_SPECIFIER => Some(NODE_PERF_HOOKS_MODULE_SOURCE),
            _ => None,
        }
    }

    fn supports_extension_backed_node_builtin(&self, specifier: &str) -> bool {
        if !matches!(
            self.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        ) {
            return false;
        }
        matches!(
            specifier,
            "node:assert"
                | "node:assert/strict"
                | "node:buffer"
                | "node:console"
                | "node:events"
                | "node:path"
                | "node:path/posix"
                | "node:path/win32"
                | "node:punycode"
                | "node:querystring"
                | "node:string_decoder"
                | "node:test"
                | "node:test/reporters"
                | "node:url"
                | "node:process"
                | "node:timers"
                | "node:timers/promises"
                | "node:util"
                | "node:diagnostics_channel"
                | "node:perf_hooks"
                | "node:os"
                | "node:tty"
                | "node:stream"
                | "node:stream/consumers"
                | "node:stream/promises"
                | "node:stream/web"
                | "node:dns"
                | "node:net"
                | "node:dgram"
                | "node:tls"
                | "node:http"
                | "node:https"
                | "node:http2"
                | "node:child_process"
                | "node:crypto"
                | "node:worker_threads"
        )
    }

    fn resolve_bare_package_specifier(
        &self,
        specifier: &str,
        referrer: &str,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        match resolve_node_target(
            &self.path_policy,
            specifier,
            referrer,
            node_resolver::ResolutionMode::Import,
        )? {
            ResolvedNodeTarget::BuiltIn { module_name } => {
                ModuleSpecifier::parse(&format!("node:{module_name}")).map_err(JsErrorBox::from_err)
            }
            ResolvedNodeTarget::Module { path, .. } => {
                let resolved = self
                    .path_policy
                    .ensure_module_read_path(&path)
                    .map_err(|error| JsErrorBox::generic(error.to_string()))?;
                ModuleSpecifier::from_file_path(&resolved).map_err(|_| {
                    JsErrorBox::generic(format!(
                        "resolved runtime package entry is not a valid file URL: {}",
                        resolved.display()
                    ))
                })
            }
        }
    }
}

impl ModuleLoader for RestrictedModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if specifier.starts_with("node:") {
            if specifier == NODE_FS_SPECIFIER {
                return ModuleSpecifier::parse(NEOVEX_NODE_FS_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if specifier == NODE_FS_PROMISES_SPECIFIER {
                return ModuleSpecifier::parse(NEOVEX_NODE_FS_PROMISES_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if specifier == NODE_MODULE_SPECIFIER {
                return ModuleSpecifier::parse(NEOVEX_NODE_MODULE_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if self.supported_node_builtin_source(specifier).is_some()
                || self.supports_extension_backed_node_builtin(specifier)
            {
                return ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err);
            }
            return Err(self.unsupported_node_builtin_error(specifier));
        }
        if specifier == INTERNAL_READLINE_UTILS_SPECIFIER {
            return ModuleSpecifier::parse(NEOVEX_INTERNAL_READLINE_UTILS_SPECIFIER)
                .map_err(JsErrorBox::from_err);
        }
        if is_bare_package_specifier(specifier) {
            return self.resolve_bare_package_specifier(specifier, referrer);
        }
        let resolved = resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)?;
        match kind {
            ResolutionKind::MainModule | ResolutionKind::Import | ResolutionKind::DynamicImport => {
                self.ensure_allowed_specifier(&resolved)?
            }
        }
        Ok(resolved)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        if let Err(error) = self.ensure_allowed_specifier(module_specifier) {
            return ModuleLoadResponse::Sync(Err(error));
        }
        ModuleLoadResponse::Async(Box::pin({
            let loader = self.clone();
            let module_specifier = module_specifier.clone();
            async move { loader.load_module_source(&module_specifier, options).await }
        }))
    }

    fn code_cache_ready(
        &self,
        module_specifier: ModuleSpecifier,
        hash: u64,
        code_cache: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = ()>>> {
        self.code_cache.store(module_specifier, hash, code_cache);
        Box::pin(async {})
    }

    fn purge_and_prevent_code_cache(&self, module_specifier: &str) {
        self.code_cache.purge_and_prevent(module_specifier);
    }
}

fn module_type_from_path(
    path: &Path,
    options: &ModuleLoadOptions,
) -> Result<ModuleType, JsErrorBox> {
    let module_type = if let Some(extension) = path.extension() {
        let ext = extension.to_string_lossy().to_ascii_lowercase();
        if ext == "json" {
            ModuleType::Json
        } else if ext == "wasm" {
            ModuleType::Wasm
        } else {
            match &options.requested_module_type {
                RequestedModuleType::Other(ty) => ModuleType::Other(ty.clone()),
                RequestedModuleType::Text => ModuleType::Text,
                RequestedModuleType::Bytes => ModuleType::Bytes,
                _ => ModuleType::JavaScript,
            }
        }
    } else {
        ModuleType::JavaScript
    };

    if module_type == ModuleType::Json && options.requested_module_type != RequestedModuleType::Json
    {
        return Err(JsErrorBox::generic(
            "Attempted to load JSON module without specifying \"type\": \"json\" attribute in the import statement.",
        ));
    }

    Ok(module_type)
}

fn hash_module_source_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = XxHash64::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn is_bare_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !has_url_like_scheme(specifier)
}

fn has_url_like_scheme(specifier: &str) -> bool {
    let Some((scheme, _)) = specifier.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::BundleModuleCodeCache;
    use super::*;
    use deno_core::ModuleSpecifier;

    #[test]
    fn bundle_code_cache_prevents_same_hash_after_purge() {
        let cache = BundleModuleCodeCache::new();
        let specifier =
            ModuleSpecifier::parse("file:///bundle/mod.js").expect("module specifier should parse");

        cache.store(specifier.clone(), 11, b"compiled");
        assert!(cache.lookup(&specifier, 11).is_some());

        cache.purge_and_prevent(specifier.as_str());
        assert!(cache.lookup(&specifier, 11).is_none());

        cache.store(specifier.clone(), 11, b"compiled-again");
        assert!(cache.lookup(&specifier, 11).is_none());

        cache.store(specifier.clone(), 12, b"compiled-new");
        let cached = cache
            .lookup(&specifier, 12)
            .expect("new hash should be allowed");
        assert_eq!(cached.as_ref(), b"compiled-new");
    }

    #[test]
    fn bare_package_detection_excludes_url_like_schemes() {
        assert!(!is_bare_package_specifier("ext:core/mod.js"));
        assert!(!is_bare_package_specifier("node:path"));
        assert!(!is_bare_package_specifier("file:///tmp/mod.js"));
        assert!(!is_bare_package_specifier("data:text/javascript,export{}"));
        assert!(is_bare_package_specifier("@scope/pkg/subpath"));
        assert!(is_bare_package_specifier("minimatch"));
    }
}
