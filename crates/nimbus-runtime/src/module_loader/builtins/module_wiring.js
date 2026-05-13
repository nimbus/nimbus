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
    return globalThis.__nimbusPerfHooksBuiltin;
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
        return globalThis.__nimbusPerfHooksBuiltin;
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
