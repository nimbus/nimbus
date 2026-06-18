//! Runtime-handler error remapping.
//!
//! Real Convex functions execute their *verbatim* original handler source via
//! the Function constructor, with `return (<source>)(ctx, args, request);` as
//! the body (see runtime_bundle_preamble.mjs). When such a handler throws, V8
//! reports the frame inside that synthesized function — its line numbers are
//! relative to that synthesized body, not the original module. This module turns
//! that back into a `module:line` in the developer's own source, so a failed
//! run names the real location.
//!
//! The two functions below are the single source of truth: they are embedded
//! into the generated bundle verbatim (via `Function.prototype.toString`, see
//! the preamble) AND imported directly by the codegen selftest. Keep them
//! self-contained (no module-scope references) so `.toString()` embedding works.

// V8 wraps the synthesized body as `function anonymous(p1,p2,...\n) {\n<body>\n}`.
// The parameter list stays on one line regardless of how many bindings there
// are, so the body always starts two lines below the reported function — a
// stable +2 offset. Calibrated empirically against V8 (see the FSV plan).
// Body line N (1-based) therefore appears at reported line N + 2, and because
// the wrapper prepends `return (` only on the first line, body line N maps 1:1
// to handler source line N. So: original module line =
// handler_origin_line + (reportedLine - 2) - 1.

function nimbusRemapHandlerError(error, origin) {
  if (!origin || typeof origin.line !== "number") {
    return error;
  }
  const err = error instanceof Error ? error : new Error(String(error));
  const stack = typeof err.stack === "string" ? err.stack : "";
  // The topmost `<anonymous>:LINE:COL` frame is the handler's throw site. The
  // outer `eval at <anonymous> (file:...)` marker has `<anonymous> (`, not
  // `<anonymous>:`, so this regex targets only the body frame.
  const match = stack.match(/<anonymous>:(\d+):(\d+)/);
  if (!match) {
    return err;
  }
  const reportedLine = Number(match[1]);
  const originalLine = origin.line + (reportedLine - 2) - 1;
  if (!Number.isFinite(originalLine) || originalLine < 1) {
    return err;
  }
  const location = (origin.module ? origin.module + ":" : "") + originalLine;
  const suffix = " (at " + location + ")";
  const baseMessage = String(err.message == null ? "" : err.message);
  if (baseMessage.endsWith(suffix)) {
    return err;
  }
  // Append ` (at module:line)` and throw a FRESH error. Two deno_core behaviors,
  // both verified live against the dev runtime, dictate this exact approach:
  // (1) deno_core derives the surfaced exception text from V8's `create_message`,
  //     which reflects the message captured at the *original* throw — so mutating
  //     the existing error's `.message` is ignored; a fresh error is required.
  // (2) Do NOT copy the original error's `.stack` onto the fresh error. Doing so
  //     re-associates it with the original's frames, and deno_core then collapses
  //     the surfaced text back to the original message, dropping the appended
  //     location. Letting the fresh error keep its own stack lets the location
  //     survive into the run record.
  const remapped = new Error(baseMessage + suffix);
  remapped.name = err.name;
  remapped.nimbusOriginalLocation = location;
  return remapped;
}

// Wraps a compiled runtime handler so both synchronous throws and asynchronous
// rejections are remapped, while success values pass through untouched. Returns
// whatever the handler returns (value or promise) so execution semantics are
// preserved exactly.
function nimbusWrapRuntimeInvoke(invoke, bindingValues, ctx, args, request, origin) {
  let result;
  try {
    result = invoke(...bindingValues, ctx, args, request);
  } catch (error) {
    throw nimbusRemapHandlerError(error, origin);
  }
  if (result !== null && typeof result === "object" && typeof result.then === "function") {
    return result.then(undefined, (error) => {
      throw nimbusRemapHandlerError(error, origin);
    });
  }
  return result;
}

export { nimbusRemapHandlerError, nimbusWrapRuntimeInvoke };
