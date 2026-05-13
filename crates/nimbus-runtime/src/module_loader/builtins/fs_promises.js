
import Module, { getBuiltinModule as getNimbusBuiltinModule } from "node:nimbus/module";

const processBuiltin = globalThis.process;
const fsBuiltin = getNimbusBuiltinModule?.("fs") ?? processBuiltin?.getBuiltinModule?.("fs");
const fsPromisesBuiltin = fsBuiltin?.promises;
const moduleBuiltin = Module ?? processBuiltin?.getBuiltinModule?.("module");
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
    "Nimbus Node22 bootstrap expected process.getBuiltinModule('fs').promises and internal/fs/utils to be available",
  );
}

const {
  emitRecursiveRmdirWarning,
  getValidatedPathToString,
  validateRmdirOptions,
} = internalFsUtils;

function mapFsHostError(error, operation) {
  const hostError = error?.nimbusHostError;
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
  mappedError.nimbusHostError = hostError;
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
  return globalThis.__nimbusSyncHostValue("op_nimbus_runtime_validate_open_path", {
    path: String(path),
    write: openFlagsNeedWrite(flags),
  });
}

async function readFile(path, options) {
  const normalizedEncoding =
    normalizeReadFileEncoding(options);
  let result;
  try {
    result = await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_fs_read_file", {
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
    const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_stat", {
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
    const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_stat", {
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
    await globalThis.__nimbusAsyncHostValue("op_nimbus_runtime_mkdir", {
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
