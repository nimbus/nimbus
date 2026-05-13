function createNimbusFsPromisesModule() {
  const fsBuiltin = denoGetBuiltinModule("fs");
  const fsPromisesBuiltin = fsBuiltin?.promises;
  if (!fsPromisesBuiltin || typeof fsPromisesBuiltin.open !== "function") {
    throw new Error("Nimbus Node22 bootstrap expected process.getBuiltinModule('fs').promises to be available");
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

function createNimbusFsModule(fsPromisesModule) {
  const fsBuiltin = denoGetBuiltinModule("fs");
  if (!fsBuiltin || typeof fsBuiltin.rmdir !== "function" || typeof fsBuiltin.rm !== "function") {
    throw new Error("Nimbus Node22 bootstrap expected process.getBuiltinModule('fs') to expose rmdir and rm");
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
    return globalThis.__nimbusSyncHostValue("op_nimbus_runtime_validate_open_path", {
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
      if (!statError && openFlagsRequireExclusiveCreate(flags, fsBuiltin.constants)) {
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
    if (openFlagsRequireExclusiveCreate(flags, fsBuiltin.constants)) {
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
    options.__nimbusHasExplicitFlush = true;
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

