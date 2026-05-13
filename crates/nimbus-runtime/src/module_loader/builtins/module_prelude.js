
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
  throw new Error("Nimbus Node22 bootstrap expected process.getBuiltinModule('module') to be available");
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
const internalTestBindingState = globalThis.__nimbusInternalTestBindingState ??= {
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

function createNimbusInternalTestBindingModule() {
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

function createNimbusInternalDgramModule() {
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
      "Nimbus Node22 bootstrap expected dns, dgram, net.isIP, udp_wrap, util.guessHandleType, uv.UV_EINVAL, and ERR_SOCKET_BAD_TYPE to be available",
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
      "Nimbus Node22 bootstrap expected dgram sockets to expose an internal state symbol",
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

function createNimbusDgramModule(internalDgramModule) {
  const dgramBuiltin = denoGetBuiltinModule("dgram");
  if (typeof dgramBuiltin?.createSocket !== "function") {
    throw new Error(
      "Nimbus Node22 bootstrap expected the dgram builtin to be available",
    );
  }
  return cloneBuiltinModuleWithOverrides(dgramBuiltin, {
    _createSocketHandle: internalDgramModule._createSocketHandle,
  });
}

function createNimbusTlsModule() {
  const tlsBuiltin = denoGetBuiltinModule("tls");
  const ERR_TLS_INVALID_CONTEXT =
    internalErrors?.codes?.ERR_TLS_INVALID_CONTEXT ?? internalErrors?.ERR_TLS_INVALID_CONTEXT;
  if (
    typeof tlsBuiltin?.connect !== "function" ||
    typeof tlsBuiltin?.createSecurePair !== "function" ||
    typeof ERR_TLS_INVALID_CONTEXT !== "function"
  ) {
    throw new Error(
      "Nimbus Node22 bootstrap expected the tls builtin and ERR_TLS_INVALID_CONTEXT to be available",
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

function createNimbusTtyModule() {
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
      "Nimbus Node22 bootstrap expected tty builtin, net.Socket, and tty error constructors to be available",
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

function createNimbusOsModule() {
  const osBuiltin = denoGetBuiltinModule("os");
  if (!osBuiltin || typeof osBuiltin.homedir !== "function") {
    throw new Error(
      "Nimbus Node22 bootstrap expected os builtin and os.homedir() to be available",
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
  return globalThis.__nimbusNodeCompatTerm !== undefined &&
    globalThis.__nimbusNodeCompatTerm !== "dumb";
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

const NIMBUS_READLINE_PROMPT_PATCHED = Symbol.for("nimbus.readlinePromptPatched");
const NIMBUS_READLINE_TAB_COMPLETE_PATCHED = Symbol.for("nimbus.readlineTabCompletePatched");

function patchReadlineBuiltinPrototype(BuiltinInterface) {
  const prototype = BuiltinInterface?.prototype;
  if (!prototype || typeof prototype !== "object") {
    return;
  }

  const refreshLineSymbol = getReadlineSymbolByDescription(prototype, "kRefreshLine");
  const tabCompleteSymbol = getReadlineSymbolByDescription(prototype, "kTabComplete");

  if (
    prototype[NIMBUS_READLINE_PROMPT_PATCHED] !== true &&
    typeof prototype.prompt === "function" &&
    typeof refreshLineSymbol === "symbol"
  ) {
    const originalPrompt = prototype.prompt;
    Object.defineProperty(prototype, "prompt", {
      configurable: true,
      writable: true,
      value: function nimbusPrompt(preserveCursor) {
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
    Object.defineProperty(prototype, NIMBUS_READLINE_PROMPT_PATCHED, {
      configurable: true,
      enumerable: false,
      writable: false,
      value: true,
    });
  }

  if (
    prototype[NIMBUS_READLINE_TAB_COMPLETE_PATCHED] !== true &&
    typeof tabCompleteSymbol === "symbol" &&
    typeof prototype[tabCompleteSymbol] === "function"
  ) {
    const originalTabComplete = prototype[tabCompleteSymbol];
    Object.defineProperty(prototype, tabCompleteSymbol, {
      configurable: true,
      writable: true,
      value: async function nimbusTabComplete(...args) {
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
    Object.defineProperty(prototype, NIMBUS_READLINE_TAB_COMPLETE_PATCHED, {
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
      value: async function nimbusTabComplete(...args) {
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
  function NimbusReadlineInterface(...args) {
    const newTarget = new.target ?? NimbusReadlineInterface;
    const instance = Reflect.construct(BuiltinInterface, args, newTarget);
    return patchReadlineInterfaceInstance(instance, BuiltinInterface.prototype);
  }
  Object.setPrototypeOf(NimbusReadlineInterface, BuiltinInterface);
  Object.setPrototypeOf(NimbusReadlineInterface.prototype, BuiltinInterface.prototype);
  return NimbusReadlineInterface;
}

function createReadlineCreateInterfaceWrapper(createInterface, builtinInterfacePrototype) {
  return function nimbusCreateInterface(...args) {
    return patchReadlineInterfaceInstance(
      createInterface.apply(this, args),
      builtinInterfacePrototype,
    );
  };
}

function createNimbusReadlineModule() {
  const readlineBuiltin = denoGetBuiltinModule("readline");
  if (
    !readlineBuiltin ||
    typeof readlineBuiltin.Interface !== "function" ||
    typeof readlineBuiltin.createInterface !== "function"
  ) {
    throw new Error(
      "Nimbus Node22 bootstrap expected readline builtin to expose Interface and createInterface()",
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

function createNimbusReadlinePromisesModule() {
  const readlinePromisesBuiltin = denoGetBuiltinModule("readline/promises");
  if (
    !readlinePromisesBuiltin ||
    typeof readlinePromisesBuiltin.Interface !== "function" ||
    typeof readlinePromisesBuiltin.createInterface !== "function"
  ) {
    throw new Error(
      "Nimbus Node22 bootstrap expected readline/promises builtin to expose Interface and createInterface()",
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

