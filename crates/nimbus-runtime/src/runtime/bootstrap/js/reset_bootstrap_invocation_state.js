__nimbusNextHostCallSessionId = 1;
__nimbusInvocationGeneration++;
__nimbusResetWaitUntil();
if (typeof globalThis.__nimbusRefreshNodeProcessCwd === "function") {
  globalThis.__nimbusRefreshNodeProcessCwd();
}
{
  const __nimbusRuntimeExecPath = __nimbusCoreOps.op_nimbus_runtime_exec_path();
  if (
    globalThis.process &&
    typeof globalThis.process === "object" &&
    typeof __nimbusRuntimeExecPath === "string" &&
    __nimbusRuntimeExecPath.length > 0
  ) {
    globalThis.process.execPath = __nimbusRuntimeExecPath;
    if (Array.isArray(globalThis.process.argv) && globalThis.process.argv.length > 0) {
      globalThis.process.argv[0] = __nimbusRuntimeExecPath;
    }
  }
}
