const __nimbusRuntimeEnvOverlaySymbol = Symbol.for("nimbus.runtimeEnvOverlay");
const __nimbusRuntimeEnvDeletedMarker = Symbol.for("nimbus.runtimeEnvDeleted");
const __nimbusProcessEnvProxyMarker = Symbol.for("nimbus.processEnvProxy");
if (globalThis[__nimbusRuntimeEnvOverlaySymbol] === undefined) {
  Object.defineProperty(globalThis, __nimbusRuntimeEnvOverlaySymbol, {
    value: Object.create(null),
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

const __nimbusRuntimeEnvOverlay = function __nimbusRuntimeEnvOverlay() {
  return globalThis[__nimbusRuntimeEnvOverlaySymbol];
};

// Node rejects accessor descriptors and partial data descriptors on
// `process.env` with an `ERR_INVALID_OBJECT_DEFINE_PROPERTY` TypeError. Build
// the same error shape (a TypeError carrying that `code`) for the proxy's
// defineProperty trap.
const __nimbusErrInvalidObjectDefineProperty = function __nimbusErrInvalidObjectDefineProperty(message) {
  const error = new TypeError(message);
  error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
  return error;
};

const __nimbusCreateProcessEnvProxy = function __nimbusCreateProcessEnvProxy() {
  const snapshot = __nimbusCoreOps.op_nimbus_runtime_env_snapshot();
  const target = Object.assign(Object.create(null), snapshot);
  Object.defineProperty(target, __nimbusProcessEnvProxyMarker, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  return new Proxy(target, {
    get(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.get(currentTarget, property);
      }
      const overlay = __nimbusRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        const value = overlay[property];
        return value === __nimbusRuntimeEnvDeletedMarker ? undefined : value;
      }
      const result = __nimbusCoreOps.op_nimbus_runtime_env_get(property);
      if (!result || typeof result !== "object") {
        return undefined;
      }
      if (result.status === "allowed") {
        currentTarget[property] = result.value;
        return result.value;
      }
      if (result.status === "missing" || result.status === "denied") {
        delete currentTarget[property];
        return undefined;
      }
      throw new Error(result.message ?? `runtime env capability denied for ${property}`);
    },
    has(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.has(currentTarget, property);
      }
      const overlay = __nimbusRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        return overlay[property] !== __nimbusRuntimeEnvDeletedMarker;
      }
      const result = __nimbusCoreOps.op_nimbus_runtime_env_get(property);
      return result?.status === "allowed";
    },
    ownKeys(currentTarget) {
      const keys = new Set(Reflect.ownKeys(currentTarget));
      for (const property of Reflect.ownKeys(__nimbusRuntimeEnvOverlay())) {
        if (
          typeof property === "string" &&
          __nimbusRuntimeEnvOverlay()[property] === __nimbusRuntimeEnvDeletedMarker
        ) {
          keys.delete(property);
          continue;
        }
        keys.add(property);
      }
      return [...keys];
    },
    getOwnPropertyDescriptor(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.getOwnPropertyDescriptor(currentTarget, property);
      }
      const overlay = __nimbusRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        if (overlay[property] === __nimbusRuntimeEnvDeletedMarker) {
          return undefined;
        }
        return {
          configurable: true,
          enumerable: true,
          writable: true,
          value: overlay[property],
        };
      }
      if (!Object.prototype.hasOwnProperty.call(currentTarget, property)) {
        return undefined;
      }
      return {
        configurable: true,
        enumerable: true,
        writable: true,
        value: currentTarget[property],
      };
    },
    set(_currentTarget, property, value) {
      if (typeof property === "symbol" || typeof value === "symbol") {
        throw new TypeError("Cannot convert a Symbol value to a string");
      }
      if (typeof value !== "string") {
        // Node emits the DEP0104 deprecation warning when a non-string value is
        // assigned to a process.env property; the value is still coerced to a
        // string. Mirror that so the warning fires before the coercion.
        const runtimeProcess = globalThis.process;
        if (runtimeProcess && typeof runtimeProcess.emitWarning === "function") {
          runtimeProcess.emitWarning(
            "Assigning any value other than a string, number, or boolean to a " +
              "process.env property is deprecated. Please make sure to convert the value " +
              "to a string before setting process.env with it.",
            "DeprecationWarning",
            "DEP0104",
          );
        }
      }
      const stringValue = String(value);
      const overlay = __nimbusRuntimeEnvOverlay();
      overlay[property] = stringValue;
      target[property] = stringValue;
      return true;
    },
    deleteProperty(currentTarget, property) {
      if (typeof property === "symbol") {
        return true;
      }
      const overlay = __nimbusRuntimeEnvOverlay();
      overlay[property] = __nimbusRuntimeEnvDeletedMarker;
      delete currentTarget[property];
      return true;
    },
    defineProperty(_currentTarget, property, descriptor) {
      // Node rejects accessor descriptors and any data descriptor that is not
      // fully configurable/writable/enumerable, then writes accepted values
      // through to the environment (matching the order in deno's process.env
      // polyfill).
      if (descriptor.get || descriptor.set) {
        throw __nimbusErrInvalidObjectDefineProperty(
          "'process.env' does not accept an accessor(getter/setter) descriptor",
        );
      }
      if (
        !descriptor.configurable ||
        !descriptor.enumerable ||
        !descriptor.writable
      ) {
        throw __nimbusErrInvalidObjectDefineProperty(
          "'process.env' only accepts a configurable, writable, and enumerable data descriptor",
        );
      }
      if (typeof property === "symbol") {
        return Reflect.defineProperty(target, property, descriptor);
      }
      const stringValue = String(descriptor.value);
      const overlay = __nimbusRuntimeEnvOverlay();
      overlay[property] = stringValue;
      target[property] = stringValue;
      return true;
    },
  });
};

const __nimbusDefineNodeFeature = function __nimbusDefineNodeFeature(target, property, value) {
  Object.defineProperty(target, property, {
    value,
    configurable: true,
    enumerable: true,
    writable: true,
  });
};

const __nimbusDefineNodeFeatureGetter = function __nimbusDefineNodeFeatureGetter(target, property, value) {
  Object.defineProperty(target, property, {
    get() {
      return value;
    },
    configurable: true,
    enumerable: true,
  });
};

const __nimbusNodeFeatureBoolean = function __nimbusNodeFeatureBoolean(source, property) {
  return source && typeof source === "object" && source[property] === true;
};

const __nimbusCreateNodeProcessFeatures = function __nimbusCreateNodeProcessFeatures(source, nodeMajor) {
  const features = {};
  __nimbusDefineNodeFeature(
    features,
    "inspector",
    __nimbusNodeFeatureBoolean(source, "inspector"),
  );
  __nimbusDefineNodeFeature(
    features,
    "debug",
    __nimbusNodeFeatureBoolean(source, "debug"),
  );
  __nimbusDefineNodeFeature(features, "uv", __nimbusNodeFeatureBoolean(source, "uv"));
  __nimbusDefineNodeFeature(features, "ipv6", __nimbusNodeFeatureBoolean(source, "ipv6"));
  if (nodeMajor === "20") {
    __nimbusDefineNodeFeature(
      features,
      "require_module",
      __nimbusNodeFeatureBoolean(source, "require_module"),
    );
  } else {
    __nimbusDefineNodeFeature(
      features,
      "openssl_is_boringssl",
      __nimbusNodeFeatureBoolean(source, "openssl_is_boringssl"),
    );
    if (nodeMajor === "24" || nodeMajor === "26") {
      __nimbusDefineNodeFeature(
        features,
        "quic",
        source && typeof source === "object" ? source.quic : undefined,
      );
    }
  }
  __nimbusDefineNodeFeature(
    features,
    "tls_alpn",
    __nimbusNodeFeatureBoolean(source, "tls_alpn"),
  );
  __nimbusDefineNodeFeature(
    features,
    "tls_sni",
    __nimbusNodeFeatureBoolean(source, "tls_sni"),
  );
  __nimbusDefineNodeFeature(
    features,
    "tls_ocsp",
    __nimbusNodeFeatureBoolean(source, "tls_ocsp"),
  );
  __nimbusDefineNodeFeature(features, "tls", __nimbusNodeFeatureBoolean(source, "tls"));
  __nimbusDefineNodeFeature(
    features,
    "cached_builtins",
    __nimbusNodeFeatureBoolean(source, "cached_builtins"),
  );
  if (nodeMajor !== "20") {
    __nimbusDefineNodeFeature(
      features,
      "require_module",
      __nimbusNodeFeatureBoolean(source, "require_module"),
    );
    const sourceTypescript =
      source && typeof source === "object" ? source.typescript : undefined;
    __nimbusDefineNodeFeatureGetter(
      features,
      "typescript",
      typeof sourceTypescript === "string" ? sourceTypescript : sourceTypescript === true,
    );
  }
  return features;
};

const __nimbusSyncNodeProcessFeatures = function __nimbusSyncNodeProcessFeatures(target, source) {
  if (!target || typeof target !== "object") {
    return source;
  }
  for (const property of Reflect.ownKeys(target)) {
    if (!Reflect.has(source, property)) {
      try {
        delete target[property];
      } catch (_error) {}
    }
  }
  for (const property of Reflect.ownKeys(source)) {
    const descriptor = Object.getOwnPropertyDescriptor(source, property);
    if (descriptor) {
      try {
        Object.defineProperty(target, property, descriptor);
      } catch (_error) {}
    }
  }
  return target;
};

const __nimbusInstallRuntimeContractGlobals = function __nimbusInstallRuntimeContractGlobals(contract) {
  if (!contract || typeof contract !== "object") {
    return;
  }
  const compatibilityTarget = contract.compatibility_target;
  const nodeApiContract =
    contract.node_api_contract && typeof contract.node_api_contract === "object"
      ? contract.node_api_contract
      : null;
  const nodeMajorMatch =
    typeof compatibilityTarget === "string"
      ? /^node(\d+)$/.exec(compatibilityTarget)
      : null;
  if (nodeApiContract || nodeMajorMatch) {
    const nodeMajor = nodeMajorMatch ? nodeMajorMatch[1] : null;
    const nodeVersion =
      typeof nodeApiContract?.version === "string"
        ? nodeApiContract.version
        : `v${nodeMajor ?? "0"}.0.0-nimbus`;
    const nodeVersionNumber =
      typeof nodeApiContract?.version_number === "string"
        ? nodeApiContract.version_number
        : nodeVersion.replace(/^v/, "");
    const nodeModuleVersion =
      typeof nodeApiContract?.module_version === "string"
        ? nodeApiContract.module_version
        : undefined;
    const nodeReleaseName =
      typeof nodeApiContract?.release_name === "string"
        ? nodeApiContract.release_name
        : "node";
    const nodeReleaseLts =
      typeof nodeApiContract?.release_lts === "string"
        ? nodeApiContract.release_lts
        : undefined;
    if (typeof globalThis.global === "undefined") {
      globalThis.global = globalThis;
    }
    const cwd = typeof contract.paths?.cwd === "string" ? contract.paths.cwd : "/";
    const env = __nimbusCreateProcessEnvProxy();
    const processBase = globalThis.process ?? {};
    const processValue =
      processBase && typeof processBase === "object"
        ? Object.create(processBase)
        : {};
    const existingVersions =
      processBase.versions && typeof processBase.versions === "object"
        ? processBase.versions
        : {};
    const nextVersions = {
      ...existingVersions,
      node: nodeVersionNumber,
    };
    if (nodeModuleVersion !== undefined) {
      nextVersions.modules = nodeModuleVersion;
    }
    const versions = Object.freeze(nextVersions);
    const existingRelease =
      processBase.release && typeof processBase.release === "object"
        ? processBase.release
        : {};
    const nextRelease = {
      ...existingRelease,
      name: nodeReleaseName,
    };
    if (nodeReleaseLts === undefined) {
      delete nextRelease.lts;
    } else {
      nextRelease.lts = nodeReleaseLts;
    }
    const release = Object.freeze(nextRelease);
    const desiredFeatures = __nimbusCreateNodeProcessFeatures(
      processBase.features,
      nodeMajor,
    );
    const features = __nimbusSyncNodeProcessFeatures(
      processBase.features,
      desiredFeatures,
    );
    if (processBase && typeof processBase === "object" && "emitWarning" in processBase) {
      Object.defineProperty(processValue, "emitWarning", {
        get() {
          return processBase.emitWarning;
        },
        set(value) {
          try {
            processBase.emitWarning = value;
          } catch (_error) {
            Object.defineProperty(processBase, "emitWarning", {
              value,
              configurable: true,
              enumerable: false,
              writable: true,
            });
          }
        },
        configurable: true,
        enumerable: false,
      });
    }
    Object.defineProperty(processValue, "cwd", {
      value() {
        return cwd;
      },
      configurable: true,
      enumerable: false,
      writable: false,
    });
    Object.defineProperty(processValue, "env", {
      value: env,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "features", {
      value: features,
      configurable: false,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "version", {
      value: nodeVersion,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "versions", {
      value: versions,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "release", {
      value: release,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, Symbol.toStringTag, {
      value: "process",
      configurable: false,
      enumerable: false,
      writable: true,
    });
    let globalProcessValue = processValue;
    Object.defineProperty(globalThis, "process", {
      get() {
        return globalProcessValue;
      },
      set(value) {
        globalProcessValue = value;
      },
      configurable: true,
      enumerable: false,
    });
    return;
  }
  delete globalThis.Buffer;
  delete globalThis.global;
  delete globalThis.process;
};

Object.freeze(__nimbusInstallRuntimeContractGlobals);
