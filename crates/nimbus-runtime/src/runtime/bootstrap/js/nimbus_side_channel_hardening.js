const __nimbusTimerResolutionMs = 10;
const __nimbusCoarsenTimerValue = function __nimbusCoarsenTimerValue(value) {
  return Number.isFinite(value)
    ? Math.floor(value / __nimbusTimerResolutionMs) * __nimbusTimerResolutionMs
    : value;
};

const __nimbusDefineFunctionMarker = function __nimbusDefineFunctionMarker(fn, marker) {
  Object.defineProperty(fn, marker, {
    configurable: false,
    enumerable: false,
    value: true,
    writable: false,
  });
  return fn;
};

const __nimbusInstallDateNowCoarsening = function __nimbusInstallDateNowCoarsening() {
  if (Date.now?.__nimbusCoarsenedTimer === true) {
    return;
  }
  const nativeDateNow = Date.now.bind(Date);
  const coarsenedDateNow = __nimbusDefineFunctionMarker(function now() {
    return __nimbusCoarsenTimerValue(nativeDateNow());
  }, "__nimbusCoarsenedTimer");
  Object.defineProperty(Date, "now", {
    configurable: true,
    enumerable: false,
    value: coarsenedDateNow,
    writable: true,
  });
};

const __nimbusInstallPerformanceNowCoarsening = function __nimbusInstallPerformanceNowCoarsening() {
  if (globalThis.performance === undefined) {
    const timeOrigin = Date.now();
    Object.defineProperty(globalThis, "performance", {
      configurable: true,
      enumerable: false,
      value: {
        now: __nimbusDefineFunctionMarker(function now() {
          return __nimbusCoarsenTimerValue(Date.now() - timeOrigin);
        }, "__nimbusCoarsenedTimer"),
        timeOrigin,
      },
      writable: true,
    });
    return;
  }
  if (
    globalThis.performance !== null &&
    typeof globalThis.performance === "object" &&
    typeof globalThis.performance.now === "function" &&
    globalThis.performance.now.__nimbusCoarsenedTimer !== true
  ) {
    const nativePerformanceNow = globalThis.performance.now.bind(globalThis.performance);
    const coarsenedPerformanceNow = __nimbusDefineFunctionMarker(function now() {
      return __nimbusCoarsenTimerValue(nativePerformanceNow());
    }, "__nimbusCoarsenedTimer");
    Object.defineProperty(globalThis.performance, "now", {
      configurable: true,
      enumerable: false,
      value: coarsenedPerformanceNow,
      writable: true,
    });
  }
};

const __nimbusDisableBlockingAtomicsWait = function __nimbusDisableBlockingAtomicsWait() {
  if (typeof Atomics !== "object" || Atomics === null) {
    return;
  }
  const disabledWait = __nimbusDefineFunctionMarker(function wait() {
    throw new TypeError("Nimbus disables Atomics.wait for in-process untrusted runtimes");
  }, "__nimbusDisabledAtomicsWait");
  Object.defineProperty(Atomics, "wait", {
    configurable: true,
    enumerable: false,
    value: disabledWait,
    writable: true,
  });
  if (typeof Atomics.waitAsync === "function") {
    const disabledWaitAsync = __nimbusDefineFunctionMarker(function waitAsync() {
      throw new TypeError("Nimbus disables Atomics.waitAsync for in-process untrusted runtimes");
    }, "__nimbusDisabledAtomicsWait");
    Object.defineProperty(Atomics, "waitAsync", {
      configurable: true,
      enumerable: false,
      value: disabledWaitAsync,
      writable: true,
    });
  }
};

const __nimbusHideSharedArrayBuffer = function __nimbusHideSharedArrayBuffer() {
  if (typeof globalThis.SharedArrayBuffer === "undefined") {
    return;
  }
  Reflect.deleteProperty(globalThis, "SharedArrayBuffer");
  if (typeof globalThis.SharedArrayBuffer !== "undefined") {
    Object.defineProperty(globalThis, "SharedArrayBuffer", {
      configurable: true,
      enumerable: false,
      value: undefined,
      writable: true,
    });
  }
};

const __nimbusDisableSharedWebAssemblyMemory = function __nimbusDisableSharedWebAssemblyMemory() {
  const webAssembly = globalThis.WebAssembly;
  if (
    typeof webAssembly !== "object" ||
    webAssembly === null ||
    typeof webAssembly.Memory !== "function"
  ) {
    return;
  }
  const NativeMemory = webAssembly.Memory;
  const HardenedMemory = function Memory(descriptor) {
    if (new.target === undefined) {
      throw new TypeError("WebAssembly.Memory must be invoked with new");
    }
    if (descriptor && descriptor.shared) {
      throw new TypeError("Nimbus disables shared WebAssembly memory");
    }
    return Reflect.construct(NativeMemory, arguments, new.target);
  };
  Object.setPrototypeOf(HardenedMemory, NativeMemory);
  HardenedMemory.prototype = NativeMemory.prototype;
  Object.defineProperty(webAssembly, "Memory", {
    configurable: true,
    enumerable: false,
    value: HardenedMemory,
    writable: true,
  });
};

const __nimbusInstallSideChannelHardening = function __nimbusInstallSideChannelHardening() {
  __nimbusInstallDateNowCoarsening();
  __nimbusInstallPerformanceNowCoarsening();
  __nimbusDisableBlockingAtomicsWait();
  __nimbusHideSharedArrayBuffer();
  __nimbusDisableSharedWebAssemblyMemory();
};

__nimbusInstallSideChannelHardening();
