import assert from "node:assert/strict";

import {
  nimbusRemapHandlerError,
  nimbusWrapRuntimeInvoke,
} from "../emit/runtime_remap.mjs";

// Builds the same Function-constructor wrapper the runtime preamble uses, so the
// integration cases exercise the real V8 line numbering rather than a mock.
function compileHandler(source) {
  return new Function("ctx", "args", "return (" + source + ")(ctx, args);");
}

async function runRuntimeRemapFixtures() {
  // 1. Unit: a crafted V8 stack frame is remapped to the original module:line.
  // Handler starts at module line 10; the throw frame is reported at line 5, so
  // original = 10 + (5 - 2) - 1 = 12.
  {
    const error = new Error("boom");
    // The remap targets the topmost `<anonymous>:LINE:COL` body frame; the real
    // deno/node frame also carries an outer marker, exercised by case 3 below.
    error.stack = "Error: boom\n    at <anonymous>:5:9";
    const out = nimbusRemapHandlerError(error, {
      module: "messages",
      line: 10,
    });
    assert.ok(
      out.message.includes("messages:12"),
      `expected message to name messages:12, got: ${out.message}`,
    );
    assert.equal(out.nimbusOriginalLocation, "messages:12");
  }

  // 2. Unit: no resolvable origin leaves the error untouched (graceful degrade).
  {
    const error = new Error("plain");
    const out = nimbusRemapHandlerError(error, { module: "x", line: null });
    assert.equal(out.message, "plain");
    assert.equal(out.nimbusOriginalLocation, undefined);
  }

  // 3. Integration: a real handler whose throw is on source line 3. With the
  // handler at module line 1, original = 1 + (5 - 2) - 1 = 3. Verifies the +2
  // V8 offset against an actual synthesized-function stack, for both sync
  // detection and the async-rejection path.
  const throwingSource =
    "async (ctx, args) => {\n" +
    "  if (args.boom) {\n" +
    '    throw new Error("kaboom");\n' +
    "  }\n" +
    "  return args.value;\n" +
    "}";
  const invoke = compileHandler(throwingSource);
  const origin = { module: "messages", line: 1 };

  const requestProbe = compileHandler(
    "(_ctx, _args, request) => request === undefined",
  );
  assert.equal(
    nimbusWrapRuntimeInvoke(requestProbe, [], {}, {}, origin),
    true,
    "the private invocation request must not reach guest handler parameters",
  );

  // 3a. Success values pass through unchanged.
  const ok = await nimbusWrapRuntimeInvoke(
    invoke,
    [],
    {},
    { value: 42 },
    origin,
  );
  assert.equal(ok, 42, "success value must be preserved");

  // 3b. Async rejection is remapped to the original throw line.
  let caught;
  try {
    await nimbusWrapRuntimeInvoke(invoke, [], {}, { boom: true }, origin);
  } catch (error) {
    caught = error;
  }
  assert.ok(caught, "expected the handler to reject");
  assert.ok(
    caught.message.includes("messages:3"),
    `expected remapped location messages:3, got: ${caught.message}`,
  );
  assert.equal(caught.nimbusOriginalLocation, "messages:3");

  // 4. Integration: a synchronous throw (non-async handler) is also remapped.
  const syncSource =
    "(ctx, args) => {\n" + '  throw new Error("sync boom");\n' + "}";
  const syncInvoke = compileHandler(syncSource);
  let syncCaught;
  try {
    nimbusWrapRuntimeInvoke(
      syncInvoke,
      [],
      {},
      {},
      { module: "users", line: 5 },
    );
  } catch (error) {
    syncCaught = error;
  }
  assert.ok(syncCaught, "expected the sync handler to throw");
  // throw on source line 2 → original = 5 + (4 - 2) - 1 = 6.
  assert.ok(
    syncCaught.message.includes("users:6"),
    `expected remapped location users:6, got: ${syncCaught.message}`,
  );

  console.log("runtime remap fixtures: ok (5 cases)");
}

export { runRuntimeRemapFixtures };
