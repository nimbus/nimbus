const fsPromisesOverrideModule = createNimbusFsPromisesModule();
const fsOverrideModule = createNimbusFsModule(fsPromisesOverrideModule);
const internalDgramOverrideModule = createNimbusInternalDgramModule();
const dgramOverrideModule = createNimbusDgramModule(internalDgramOverrideModule);
const tlsOverrideModule = createNimbusTlsModule();
const internalTestBindingOverrideModule = createNimbusInternalTestBindingModule();
const ttyOverrideModule = createNimbusTtyModule();
const osOverrideModule = createNimbusOsModule();
const readlineOverrideModule = createNimbusReadlineModule();
const readlinePromisesOverrideModule = createNimbusReadlinePromisesModule();
const vmOverrideModule = createNimbusVmModule();
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
  vm: vmOverrideModule,
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

function isProcessSpecifier(specifier) {
  return normalizeBuiltinSpecifier(specifier) === "process";
}

function getBuiltinOverride(specifier) {
  const normalizedSpecifier = normalizeBuiltinSpecifier(specifier);
  if (!normalizedSpecifier) {
    return undefined;
  }
  if (normalizedSpecifier === "process") {
    return processModule;
  }
  return INTERNAL_MODULE_OVERRIDES[normalizedSpecifier];
}

function getBuiltinModule(specifier) {
  if (isPerfHooksSpecifier(specifier)) {
    return globalThis.__nimbusPerfHooksBuiltin;
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
    normalizedSpecifier === "process" ||
    (normalizedSpecifier !== null &&
      isPublicBuiltinOverrideSpecifier(normalizedSpecifier)) ||
    denoIsBuiltin(specifier)
  );
}

const NIMBUS_BUILTIN_ESM_EXPORT_SYNC_CALLBACKS =
  "__nimbusBuiltinEsmExportSyncCallbacks";
const NIMBUS_ORIGINAL_SYNC_BUILTIN_ESM_EXPORTS =
  "__nimbusOriginalSyncBuiltinESMExports";
const denoSyncBuiltinESMExports = Object.prototype.hasOwnProperty.call(
  Module,
  NIMBUS_ORIGINAL_SYNC_BUILTIN_ESM_EXPORTS,
)
  ? Module[NIMBUS_ORIGINAL_SYNC_BUILTIN_ESM_EXPORTS]
  : typeof Module.syncBuiltinESMExports === "function"
  ? Module.syncBuiltinESMExports.bind(Module)
  : null;
if (
  !Object.prototype.hasOwnProperty.call(
    Module,
    NIMBUS_ORIGINAL_SYNC_BUILTIN_ESM_EXPORTS,
  )
) {
  Object.defineProperty(Module, NIMBUS_ORIGINAL_SYNC_BUILTIN_ESM_EXPORTS, {
    value: denoSyncBuiltinESMExports,
    configurable: true,
    enumerable: false,
    writable: false,
  });
}
const nimbusBuiltinEsmExportSyncCallbacks = Object.prototype.hasOwnProperty.call(
  Module,
  NIMBUS_BUILTIN_ESM_EXPORT_SYNC_CALLBACKS,
)
  ? Module[NIMBUS_BUILTIN_ESM_EXPORT_SYNC_CALLBACKS]
  : new Set();
if (
  !Object.prototype.hasOwnProperty.call(
    Module,
    NIMBUS_BUILTIN_ESM_EXPORT_SYNC_CALLBACKS,
  )
) {
  Object.defineProperty(Module, NIMBUS_BUILTIN_ESM_EXPORT_SYNC_CALLBACKS, {
    value: nimbusBuiltinEsmExportSyncCallbacks,
    configurable: true,
    enumerable: false,
    writable: false,
  });
}

function __nimbusRegisterBuiltinEsmExportSync(callback) {
  if (typeof callback !== "function") {
    throw new TypeError("Nimbus builtin ESM export sync callback must be a function");
  }
  nimbusBuiltinEsmExportSyncCallbacks.add(callback);
  return function unregisterBuiltinEsmExportSync() {
    nimbusBuiltinEsmExportSyncCallbacks.delete(callback);
  };
}

function syncBuiltinESMExports() {
  denoSyncBuiltinESMExports?.();
  for (const callback of nimbusBuiltinEsmExportSyncCallbacks) {
    callback();
  }
}

Object.defineProperty(Module, "__nimbusRegisterBuiltinEsmExportSync", {
  value: __nimbusRegisterBuiltinEsmExportSync,
  configurable: true,
  enumerable: false,
  writable: true,
});

let activeHookRegistrations = 0;

function registerHooks(hooks) {
  const registration = denoRegisterHooks(hooks);
  activeHookRegistrations += 1;
  let registered = true;
  return {
    deregister() {
      if (registered) {
        registered = false;
        activeHookRegistrations = Math.max(0, activeHookRegistrations - 1);
      }
      return registration.deregister();
    },
  };
}

function loadHookedBuiltinWithOverrideFallback(request, parent, isMain, override) {
  const loaded = denoLoad(request, parent, isMain);
  const normalizedSpecifier = normalizeBuiltinSpecifier(request);
  const denoBuiltin = normalizedSpecifier
    ? denoGetBuiltinModule(normalizedSpecifier)
    : undefined;
  return loaded === denoBuiltin ? override : loaded;
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
  const nodeMajor = Number.parseInt(processModule?.versions?.node ?? "", 10);
  if (nodeMajor <= 22) {
    for (let index = builtinModules.length - 1; index >= 0; index -= 1) {
      if (String(builtinModules[index]).startsWith("node:")) {
        builtinModules.splice(index, 1);
      }
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
    return globalThis.__nimbusPerfHooksBuiltin;
  }
  if (isProcessSpecifier(request)) {
    return processModule;
  }
  const override = getBuiltinOverride(request);
  if (override !== undefined) {
    if (activeHookRegistrations > 0) {
      return loadHookedBuiltinWithOverrideFallback(request, parent, isMain, override);
    }
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
    if (activeHookRegistrations > 0) {
      const resolved = denoResolveFilename(request, parent, isMain, options);
      return normalizeBuiltinSpecifier(resolved) === normalizedSpecifier
        ? normalizedSpecifier
        : resolved;
    }
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

function createRequire(filenameOrUrl) {
  const require = denoCreateRequire(filenameOrUrl);
  return new Proxy(require, {
    apply(target, thisArg, args) {
      const request = args[0];
      if (isPerfHooksSpecifier(request)) {
        return globalThis.__nimbusPerfHooksBuiltin;
      }
      if (isProcessSpecifier(request)) {
        return processModule;
      }
      const override = getBuiltinOverride(request);
      if (override !== undefined) {
        if (activeHookRegistrations > 0) {
          return loadHookedBuiltinWithOverrideFallback(
            request,
            undefined,
            false,
            override,
          );
        }
        return override;
      }
      return Reflect.apply(target, thisArg, args);
    },
  });
}

Module.createRequire = createRequire;
Module.registerHooks = registerHooks;
Module.syncBuiltinESMExports = syncBuiltinESMExports;

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
  registerHooks,
  syncBuiltinESMExports,
};
export default Module;
