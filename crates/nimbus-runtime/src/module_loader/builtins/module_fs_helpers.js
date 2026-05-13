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

function openFlagsRequireExclusiveCreate(flags, fsConstants) {
  if (typeof flags === "number") {
    return (flags & fsConstants.O_EXCL) !== 0;
  }
  const normalizedFlags = typeof flags === "string" && flags.length > 0 ? flags : "r";
  return normalizedFlags.includes("x");
}

function normalizeInvalidOpenThrow(fsBuiltin, path, flags, error, callback) {
  if (!isInvalidOpenThrow(error)) {
    callback(error);
    return;
  }
  return fsBuiltin.stat(path, (statError) => {
    if (statError?.code === "ENOENT") {
      callback(createOpenEnoentError(path, statError));
      return;
    }
    if (!statError && openFlagsRequireExclusiveCreate(flags, fsBuiltin.constants)) {
      callback(createOpenEexistError(path, error));
      return;
    }
    callback(error);
  });
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
    normalizedOptions.__nimbusHasExplicitFlush === true ||
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
  delete sanitizedOptions.__nimbusHasExplicitFlush;
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
  const finishWriteFile = (error) => {
    if (!error) {
      callback(null);
      return;
    }
    normalizeInvalidOpenThrow(fsBuiltin, validatedPath, flag, error, callback);
  };

  if (!hasExplicitFlush) {
    try {
      return fsBuiltin.writeFile(validatedPath, data, sanitizedOptions, finishWriteFile);
    } catch (error) {
      return normalizeInvalidOpenThrow(fsBuiltin, validatedPath, flag, error, callback);
    }
  }

  if (!flush) {
    return fsBuiltin.writeFile(validatedPath, data, sanitizedOptions, (error) => {
      if (!error) {
        invokeFsCallbackAsync(callback, null);
        return;
      }
      normalizeInvalidOpenThrow(fsBuiltin, validatedPath, flag, error, (normalizedError) => {
        invokeFsCallbackAsync(callback, normalizedError);
      });
    });
  }

  return fsBuiltin.writeFile(validatedPath, data, sanitizedOptions, (writeError) => {
    if (writeError) {
      normalizeInvalidOpenThrow(fsBuiltin, validatedPath, flag, writeError, callback);
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
    globalThis.__nimbusSyncHostValue("op_nimbus_runtime_read_dir_sync", {
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
  if (!dir || typeof dir !== "object" || dir.__nimbusWrappedDir === true) {
    return dir;
  }

  const originalRead = dir.read.bind(dir);
  const originalReadSync = dir.readSync.bind(dir);
  const originalClose = dir.close.bind(dir);
  const originalCloseSync = dir.closeSync.bind(dir);
  let closed = false;
  let pendingAsyncReads = 0;

  Object.defineProperty(dir, "__nimbusWrappedDir", {
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

