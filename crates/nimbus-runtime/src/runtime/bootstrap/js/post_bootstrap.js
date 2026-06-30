const __nimbusRuntimeContract =
  __nimbusCoreOps.op_nimbus_runtime_contract();
const __nimbusCompatibilityTarget =
  __nimbusRuntimeContract?.compatibility_target;
const __nimbusCompatibilityMatch =
  typeof __nimbusCompatibilityTarget === "string"
    ? /^node(\d+)$/.exec(__nimbusCompatibilityTarget)
    : null;
if (__nimbusCompatibilityMatch !== null) {
  if (typeof globalThis.__nimbusRefreshNodeRuntimeOpState === "function") {
    globalThis.__nimbusRefreshNodeRuntimeOpState();
  }
  const __nimbusWasmStreamingCore = Deno.core;
  const __nimbusWasmStreamingFetchModule =
    globalThis.__nimbusDenoFetchModule ??
      __nimbusWasmStreamingCore.loadExtScript("ext:deno_fetch/26_fetch.js");
  __nimbusWasmStreamingCore.setWasmStreamingCallback(
    function __nimbusWasmStreamingCallback(source, rid) {
      return __nimbusWasmStreamingFetchModule.handleWasmStreaming(source, rid);
    },
  );
}
delete globalThis.__nimbusRefreshNodeRuntimeOpState;
delete globalThis.__nimbusDenoFetchModule;
if (globalThis.__nimbusRetainDenoForNodeLazyScripts !== true) {
  delete globalThis.Deno;
}
delete globalThis.__nimbusRetainDenoForNodeLazyScripts;
delete globalThis.__bootstrap;
delete globalThis.bootstrap;
__nimbusInstallRuntimeContractGlobals(__nimbusRuntimeContract);
__nimbusInstallSideChannelHardening();
const __nimbusNodeVersion =
  __nimbusRuntimeContract?.node_api_contract?.version_number;
const __nimbusNodeRuntimeMajor = __nimbusCompatibilityMatch
  ? Number.parseInt(__nimbusCompatibilityMatch[1], 10)
  : typeof __nimbusNodeVersion === "string"
    ? Number.parseInt(__nimbusNodeVersion, 10)
    : undefined;
Object.defineProperty(globalThis, "__nimbusNodeRuntimeMajor", {
  value: __nimbusNodeRuntimeMajor,
  configurable: true,
  enumerable: false,
  writable: true,
});
if (globalThis.process && typeof globalThis.process === "object") {
  Object.defineProperty(globalThis.process, "__nimbusNodeRuntimeMajor", {
    value: __nimbusNodeRuntimeMajor,
    configurable: true,
    enumerable: false,
    writable: true,
  });
}
if (Promise.reject.__nimbusDomainAware !== true) {
  const __nimbusOriginalPromiseReject = Promise.reject;
  const __nimbusDomainAwarePromiseReject = function __nimbusDomainAwarePromiseReject(reason) {
    const promise = __nimbusOriginalPromiseReject.apply(this, arguments);
    const domain = globalThis.process?.domain;
    if (domain !== null && domain !== undefined) {
      Object.defineProperty(promise, "domain", {
        configurable: true,
        enumerable: false,
        value: domain,
        writable: true,
      });
      if (reason !== null && typeof reason === "object") {
        Object.defineProperty(reason, "domain", {
          configurable: true,
          enumerable: false,
          value: domain,
          writable: true,
        });
        if (reason.domainThrown === undefined) {
          reason.domainThrown = true;
        }
      }
    }
    return promise;
  };
  Object.defineProperty(__nimbusDomainAwarePromiseReject, "__nimbusDomainAware", {
    configurable: false,
    enumerable: false,
    value: true,
    writable: false,
  });
  Object.defineProperty(Promise, "reject", {
    configurable: true,
    enumerable: false,
    value: __nimbusDomainAwarePromiseReject,
    writable: true,
  });
}
{
  for (const __nimbusGlobalName of Object.keys(globalThis)) {
    if (!__nimbusGlobalName.startsWith("__nimbus")) {
      continue;
    }
    const __nimbusGlobalDescriptor =
      Object.getOwnPropertyDescriptor(globalThis, __nimbusGlobalName);
    if (
      __nimbusGlobalDescriptor &&
      __nimbusGlobalDescriptor.configurable === true &&
      __nimbusGlobalDescriptor.enumerable === true
    ) {
      Object.defineProperty(globalThis, __nimbusGlobalName, {
        ...__nimbusGlobalDescriptor,
        enumerable: false,
      });
    }
  }
}
