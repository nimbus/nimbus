// WebStandard bootstrap entry point. Mirrors the web-global half of the Node bootstrap
// (node22_runtime_bootstrap.js) WITHOUT any Node setup. The shared scope's `process` reads
// are all optional-chained or behind lazy getters, so they degrade benignly on a non-Node
// profile (no `process`).
import { core } from "ext:core/mod.js";
import { windowOrWorkerGlobalScope } from "ext:runtime/98_global_scope_shared.js";

Object.defineProperties(globalThis, windowOrWorkerGlobalScope);

// The shared scope is a SUBSET - it omits Streams, base64, structuredClone, and timers.
// The Node bootstrap loads these separately (05_base64 / 06_streams / 13_message_port /
// 02_timers) and seeds them onto the global; do the same here.
const base64 = core.loadExtScript("ext:deno_web/05_base64.js");
const streams = core.loadExtScript("ext:deno_web/06_streams.js");
const messagePort = core.loadExtScript("ext:deno_web/13_message_port.js");
const timers = core.loadExtScript("ext:deno_web/02_timers.js");
const performanceModule = core.loadExtScript("ext:deno_web/15_performance.js");

function seedGlobal(name, value) {
  if (value !== undefined && globalThis[name] === undefined) {
    Object.defineProperty(globalThis, name, {
      value,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  }
}

seedGlobal("atob", base64.atob);
seedGlobal("btoa", base64.btoa);
seedGlobal("ReadableStream", streams.ReadableStream);
seedGlobal("ReadableStreamBYOBReader", streams.ReadableStreamBYOBReader);
seedGlobal("ReadableStreamBYOBRequest", streams.ReadableStreamBYOBRequest);
seedGlobal("ReadableStreamDefaultController", streams.ReadableStreamDefaultController);
seedGlobal("ReadableStreamDefaultReader", streams.ReadableStreamDefaultReader);
seedGlobal("WritableStream", streams.WritableStream);
seedGlobal("WritableStreamDefaultController", streams.WritableStreamDefaultController);
seedGlobal("WritableStreamDefaultWriter", streams.WritableStreamDefaultWriter);
seedGlobal("TransformStream", streams.TransformStream);
seedGlobal("ByteLengthQueuingStrategy", streams.ByteLengthQueuingStrategy);
seedGlobal("CountQueuingStrategy", streams.CountQueuingStrategy);
seedGlobal("structuredClone", messagePort.structuredClone);
seedGlobal("MessageChannel", messagePort.MessageChannel);
seedGlobal("MessagePort", messagePort.MessagePort);
seedGlobal("setTimeout", timers.setTimeout);
seedGlobal("setInterval", timers.setInterval);
seedGlobal("clearTimeout", timers.clearTimeout);
seedGlobal("clearInterval", timers.clearInterval);
// Convex default-runtime timing surface: performance plus the Performance
// entry classes (the side-channel hardening pass coarsens performance.now, and
// the guest-semantics controller pins timeOrigin / freezes now per invocation
// kind on ConvexDefault lanes).
seedGlobal("performance", performanceModule.performance);
seedGlobal("Performance", performanceModule.Performance);
seedGlobal("PerformanceEntry", performanceModule.PerformanceEntry);
seedGlobal("PerformanceMark", performanceModule.PerformanceMark);
seedGlobal("PerformanceMeasure", performanceModule.PerformanceMeasure);
