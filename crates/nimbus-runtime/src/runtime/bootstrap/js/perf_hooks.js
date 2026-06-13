// Copyright 2018-2026 the Deno authors. MIT license.
//
// This is served through Nimbus's node:perf_hooks builtin override path in the
// restricted module loader so we can keep compatibility-specific module deltas
// local until they are worth promoting into the shared fork.

// TODO(petamoriken): enable prefer-primordials for node polyfills
// deno-lint-ignore-file prefer-primordials

import { core } from "ext:core/mod.js";
import { EldHistogram } from "ext:core/ops";

const {
  performance,
  PerformanceEntry,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceObserver: WebPerformanceObserver,
  PerformanceObserverEntryList,
} = core.loadExtScript("ext:deno_web/15_performance.js");

const {
  ERR_ILLEGAL_CONSTRUCTOR,
  ERR_INVALID_ARG_TYPE,
  ERR_INVALID_ARG_VALUE,
  ERR_INVALID_THIS,
  ERR_MISSING_ARGS,
  ERR_OUT_OF_RANGE,
} = core.loadExtScript("ext:deno_node/internal/errors.ts");
const {
  validateInteger,
  validateNumber,
  validateObject,
} = core.loadExtScript("ext:deno_node/internal/validators.mjs");
const {
  customInspectSymbol,
  kEmptyObject,
} = core.loadExtScript("ext:deno_node/internal/util.mjs");
const { inspect } =
  core.loadExtScript("ext:deno_node/internal/util/inspect.mjs");

const constants = {
  NODE_PERFORMANCE_ENTRY_TYPE_NODE: 0,
  NODE_PERFORMANCE_ENTRY_TYPE_MARK: 1,
  NODE_PERFORMANCE_ENTRY_TYPE_MEASURE: 2,
  NODE_PERFORMANCE_ENTRY_TYPE_GC: 3,
  NODE_PERFORMANCE_ENTRY_TYPE_FUNCTION: 4,
  NODE_PERFORMANCE_ENTRY_TYPE_HTTP2: 5,
  NODE_PERFORMANCE_ENTRY_TYPE_HTTP: 6,
  NODE_PERFORMANCE_ENTRY_TYPE_DNS: 7,
  NODE_PERFORMANCE_ENTRY_TYPE_NET: 8,
  NODE_PERFORMANCE_GC_MAJOR: 4,
  NODE_PERFORMANCE_GC_MINOR: 1,
  NODE_PERFORMANCE_GC_INCREMENTAL: 8,
  NODE_PERFORMANCE_GC_WEAKCB: 16,
  NODE_PERFORMANCE_GC_FLAGS_NO: 0,
  NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED: 2,
  NODE_PERFORMANCE_GC_FLAGS_FORCED: 4,
  NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING: 8,
  NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE: 16,
  NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY: 32,
  NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE: 64,
};

const EMPTY_HISTOGRAM_MIN_BIGINT = 9_223_372_036_854_775_807n;
const EMPTY_HISTOGRAM_MIN_NUMBER = Number(EMPTY_HISTOGRAM_MIN_BIGINT);
const INTERVAL_HISTOGRAM_CLONE_TYPE = "NimbusIntervalHistogramSnapshot";
const RECORDABLE_HISTOGRAM_CLONE_TYPE = "NimbusRecordableHistogram";
const NODE_ENTRY_TYPES = ["function", "gc", "http2", "http", "dns", "net"];
const kHistogramStateId = Symbol("kHistogramStateId");
const kHistogramRecordable = Symbol("kHistogramRecordable");
const kHistogramSkipThrow = Symbol("kHistogramSkipThrow");
const histogramStateRegistry = new Map();
const nodePerformanceObservers = [];
let nextHistogramStateId = 1;
const histogramStateFinalizer = new FinalizationRegistry((stateId) => {
  const state = histogramStateRegistry.get(stateId);
  if (state === undefined) {
    return;
  }
  state.wrapperCount -= 1;
  if (state.wrapperCount <= 0) {
    histogramStateRegistry.delete(stateId);
  }
});

function sortHistogramValues(values) {
  return [...values].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}

function percentileFromSortedValues(values, percentile) {
  if (values.length === 0) {
    return 0n;
  }
  const index = Math.ceil((percentile / 100) * values.length) - 1;
  return values[Math.max(0, Math.min(index, values.length - 1))];
}

function stateForHistogram(histogram, expectedType) {
  const stateId = histogram?.[kHistogramStateId];
  if (typeof stateId !== "number") {
    throw new ERR_INVALID_THIS(expectedType);
  }
  const state = histogramStateRegistry.get(stateId);
  if (state === undefined) {
    throw new ERR_INVALID_THIS(expectedType);
  }
  return state;
}

function validateHistogramPercentile(percentile) {
  validateNumber(percentile, "percentile");
  if (Number.isNaN(percentile) || percentile <= 0 || percentile > 100) {
    throw new ERR_OUT_OF_RANGE("percentile", "> 0 && <= 100", percentile);
  }
}

function histogramPercentilesMap(histogram) {
  const values = sortHistogramValues(stateForHistogram(histogram, "Histogram").values);
  const map = new Map();
  if (values.length === 0) {
    return map;
  }
  if (values.length === 1) {
    const value = Number(values[0]);
    map.set(0, value);
    map.set(100, value);
    return map;
  }
  for (const percentile of [0, 50, 75, 90, 95, 99, 100]) {
    const normalizedPercentile = percentile === 0 ? 1 : percentile;
    const value = percentileFromSortedValues(values, normalizedPercentile);
    map.set(percentile, Number(value));
  }
  return map;
}

function histogramPercentilesBigIntMap(histogram) {
  const values = sortHistogramValues(stateForHistogram(histogram, "Histogram").values);
  const map = new Map();
  if (values.length === 0) {
    return map;
  }
  if (values.length === 1) {
    map.set(0, values[0]);
    map.set(100, values[0]);
    return map;
  }
  for (const percentile of [0, 50, 75, 90, 95, 99, 100]) {
    const normalizedPercentile = percentile === 0 ? 1 : percentile;
    map.set(percentile, percentileFromSortedValues(values, normalizedPercentile));
  }
  return map;
}

function createHistogramState({
  lowest = 1,
  highest = Number.MAX_SAFE_INTEGER,
  figures = 3,
} = kEmptyObject) {
  if (typeof lowest !== "bigint") {
    validateInteger(lowest, "options.lowest", 1, Number.MAX_SAFE_INTEGER);
  }
  if (typeof highest !== "bigint") {
    validateInteger(
      highest,
      "options.highest",
      2 * Number(lowest),
      Number.MAX_SAFE_INTEGER,
    );
  } else if (highest < 2n * BigInt(lowest)) {
    throw new ERR_INVALID_ARG_VALUE.RangeError("options.highest", highest);
  }
  validateInteger(figures, "options.figures", 1, 5);

  const state = {
    id: nextHistogramStateId++,
    lowest,
    highest,
    figures,
    values: [],
    wrapperCount: 0,
  };
  histogramStateRegistry.set(state.id, state);
  return state;
}

function createRecordableHistogramFromState(state) {
  const histogram = new RecordableHistogram(kHistogramSkipThrow);
  histogram[kHistogramStateId] = state.id;
  histogram[kHistogramRecordable] = true;
  state.wrapperCount += 1;
  histogramStateFinalizer.register(histogram, state.id);
  Object.defineProperty(histogram, core.hostObjectBrand, {
    __proto__: null,
    value: () => ({
      type: RECORDABLE_HISTOGRAM_CLONE_TYPE,
      id: state.id,
    }),
    enumerable: false,
    configurable: false,
    writable: false,
  });
  return histogram;
}

core.registerCloneableResource(RECORDABLE_HISTOGRAM_CLONE_TYPE, (data) => {
  const state = histogramStateRegistry.get(data.id);
  if (state === undefined) {
    throw new Error(
      `Unable to restore cloned RecordableHistogram for state ${data.id}`,
    );
  }
  return createRecordableHistogramFromState(state);
});
core.registerCloneableResource(INTERVAL_HISTOGRAM_CLONE_TYPE, (data) => data.snapshot);

class Histogram {
  constructor(skipThrowSymbol = undefined) {
    if (skipThrowSymbol !== kHistogramSkipThrow) {
      throw new ERR_ILLEGAL_CONSTRUCTOR();
    }
  }

  [customInspectSymbol](depth, options) {
    if (depth < 0) {
      return "[RecordableHistogram]";
    }

    const inspectOptions = {
      ...options,
      depth: options?.depth == null ? null : options.depth - 1,
    };
    return `Histogram ${inspect({
      min: this.min,
      max: this.max,
      mean: this.mean,
      exceeds: this.exceeds,
      stddev: this.stddev,
      count: this.count,
      percentiles: this.percentiles,
    }, inspectOptions)}`;
  }

  get count() {
    return stateForHistogram(this, "Histogram").values.length;
  }

  get countBigInt() {
    return BigInt(this.count);
  }

  get min() {
    const { values } = stateForHistogram(this, "Histogram");
    if (values.length === 0) {
      return EMPTY_HISTOGRAM_MIN_NUMBER;
    }
    return Number(sortHistogramValues(values)[0]);
  }

  get minBigInt() {
    const { values } = stateForHistogram(this, "Histogram");
    if (values.length === 0) {
      return EMPTY_HISTOGRAM_MIN_BIGINT;
    }
    return sortHistogramValues(values)[0];
  }

  get max() {
    const { values } = stateForHistogram(this, "Histogram");
    if (values.length === 0) {
      return 0;
    }
    return Number(sortHistogramValues(values).at(-1));
  }

  get maxBigInt() {
    const { values } = stateForHistogram(this, "Histogram");
    if (values.length === 0) {
      return 0n;
    }
    return sortHistogramValues(values).at(-1);
  }

  get mean() {
    const { values } = stateForHistogram(this, "Histogram");
    if (values.length === 0) {
      return Number.NaN;
    }
    const total = values.reduce((sum, value) => sum + Number(value), 0);
    return total / values.length;
  }

  get exceeds() {
    return 0;
  }

  get exceedsBigInt() {
    return 0n;
  }

  get stddev() {
    const { values } = stateForHistogram(this, "Histogram");
    if (values.length === 0) {
      return Number.NaN;
    }
    const mean = this.mean;
    const variance = values.reduce((sum, value) => {
      const delta = Number(value) - mean;
      return sum + delta * delta;
    }, 0) / values.length;
    return Math.sqrt(variance);
  }

  percentile(percentile) {
    validateHistogramPercentile(percentile);
    const values = sortHistogramValues(stateForHistogram(this, "Histogram").values);
    return Number(percentileFromSortedValues(values, percentile));
  }

  percentileBigInt(percentile) {
    validateHistogramPercentile(percentile);
    const values = sortHistogramValues(stateForHistogram(this, "Histogram").values);
    return percentileFromSortedValues(values, percentile);
  }

  get percentiles() {
    return histogramPercentilesMap(this);
  }

  get percentilesBigInt() {
    return histogramPercentilesBigIntMap(this);
  }

  reset() {
    stateForHistogram(this, "Histogram").values.length = 0;
  }

  toJSON() {
    return {
      count: this.count,
      min: this.min,
      max: this.max,
      mean: this.mean,
      exceeds: this.exceeds,
      stddev: this.stddev,
      percentiles: Object.fromEntries(this.percentiles),
    };
  }
}

class RecordableHistogram extends Histogram {
  constructor(skipThrowSymbol = undefined) {
    if (skipThrowSymbol !== kHistogramSkipThrow) {
      throw new ERR_ILLEGAL_CONSTRUCTOR();
    }
    super(skipThrowSymbol);
  }

  record(value) {
    if (this[kHistogramRecordable] === undefined) {
      throw new ERR_INVALID_THIS("RecordableHistogram");
    }
    if (typeof value === "bigint") {
      if (value < 1n) {
        throw new ERR_OUT_OF_RANGE("val", ">= 1", value);
      }
      stateForHistogram(this, "RecordableHistogram").values.push(value);
      return;
    }

    validateInteger(value, "val", 1);
    stateForHistogram(this, "RecordableHistogram").values.push(BigInt(value));
  }

  recordDelta() {
    if (this[kHistogramRecordable] === undefined) {
      throw new ERR_INVALID_THIS("RecordableHistogram");
    }
  }

  add(other) {
    if (this[kHistogramRecordable] === undefined) {
      throw new ERR_INVALID_THIS("RecordableHistogram");
    }
    if (other?.[kHistogramRecordable] === undefined) {
      throw new ERR_INVALID_ARG_TYPE("other", "RecordableHistogram", other);
    }
    const state = stateForHistogram(this, "RecordableHistogram");
    const otherState = stateForHistogram(other, "RecordableHistogram");
    state.values.push(...otherState.values);
  }
}

class IntervalHistogram {
  #eldHistogram;

  constructor(resolution) {
    this.#eldHistogram = new EldHistogram(resolution);
    Object.defineProperty(this, core.hostObjectBrand, {
      __proto__: null,
      value: () => ({
        type: INTERVAL_HISTOGRAM_CLONE_TYPE,
        snapshot: {
          count: this.count,
          countBigInt: this.countBigInt,
          min: this.min,
          minBigInt: this.minBigInt,
          max: this.max,
          maxBigInt: this.maxBigInt,
          mean: this.mean,
          stddev: this.stddev,
          exceeds: 0,
          exceedsBigInt: 0n,
        },
      }),
      enumerable: false,
      configurable: false,
      writable: false,
    });
  }

  enable() {
    return this.#eldHistogram.enable();
  }

  disable() {
    return this.#eldHistogram.disable();
  }

  [Symbol.dispose]() {
    this.disable();
  }

  reset() {
    return this.#eldHistogram.reset();
  }

  percentile(percentile) {
    return this.#eldHistogram.percentile(percentile);
  }

  percentileBigInt(percentile) {
    return this.#eldHistogram.percentileBigInt(percentile);
  }

  get count() {
    return this.#eldHistogram.count;
  }

  get countBigInt() {
    return this.#eldHistogram.countBigInt;
  }

  get max() {
    return this.#eldHistogram.max;
  }

  get maxBigInt() {
    return this.#eldHistogram.maxBigInt;
  }

  get mean() {
    return this.#eldHistogram.mean;
  }

  get min() {
    return this.#eldHistogram.min;
  }

  get minBigInt() {
    return this.#eldHistogram.minBigInt;
  }

  get stddev() {
    return this.#eldHistogram.stddev;
  }

  get exceeds() {
    return 0;
  }

  get exceedsBigInt() {
    return 0n;
  }
}

function createHistogram(options = kEmptyObject) {
  validateObject(options, "options");
  return createRecordableHistogramFromState(createHistogramState(options));
}

class NodePerformanceEntry {
  constructor({
    name,
    entryType,
    startTime,
    duration,
    detail = null,
    args = [],
    kind = undefined,
    flags = undefined,
  }) {
    this.name = name;
    this.entryType = entryType;
    this.startTime = startTime;
    this.duration = duration;
    this.detail = detail;
    if (kind !== undefined) {
      this.kind = kind;
    }
    if (flags !== undefined) {
      this.flags = flags;
    }
    for (let index = 0; index < args.length; index += 1) {
      this[index] = args[index];
    }
  }

  toJSON() {
    return {
      name: this.name,
      entryType: this.entryType,
      startTime: this.startTime,
      duration: this.duration,
      detail: this.detail,
    };
  }
}

function enqueueNodePerformanceEntry(entry) {
  for (const observer of nodePerformanceObservers) {
    observer[Symbol.for("nimbus.perf_hooks.enqueueNodeEntry")](entry);
  }
}

// Node-compatible PerformanceObserver that throws proper Node.js errors
class PerformanceObserver extends WebPerformanceObserver {
  #nodeCallback = null;
  #nodeTypes = [];
  #nodeBuffer = [];
  #nodeScheduled = false;
  #resourceCallback = null;
  #resourceBuffer = [];
  #resourceScheduled = false;
  #observesResource = false;

  constructor(callback) {
    if (typeof callback !== "function") {
      throw new ERR_INVALID_ARG_TYPE("callback", "Function", callback);
    }
    super(callback);
    this.#nodeCallback = callback;
    this.#resourceCallback = callback;
  }

  observe(options) {
    if (typeof options !== "object" || options === null) {
      throw new ERR_INVALID_ARG_TYPE("options", "Object", options);
    }
    if (
      options.entryTypes !== undefined && !Array.isArray(options.entryTypes)
    ) {
      throw new ERR_INVALID_ARG_TYPE(
        "options.entryTypes",
        "string[]",
        options.entryTypes,
      );
    }
    const requestedTypes = options.entryTypes !== undefined
      ? options.entryTypes
      : options.type !== undefined
      ? [options.type]
      : [];
    const nodeEntryTypes = requestedTypes.filter((entryType) =>
      NODE_ENTRY_TYPES.includes(entryType)
    );
    const observesResource = requestedTypes.includes("resource");
    const webEntryTypes = options.entryTypes !== undefined
      ? options.entryTypes.filter((entryType) =>
        entryType !== "resource" && !NODE_ENTRY_TYPES.includes(entryType)
      )
      : requestedTypes.filter((entryType) =>
        entryType !== "resource" && !NODE_ENTRY_TYPES.includes(entryType)
      );

    if (webEntryTypes.length > 0) {
      if (options.entryTypes !== undefined) {
        super.observe({ entryTypes: webEntryTypes, buffered: options.buffered });
      } else {
        super.observe({
          type: webEntryTypes[0],
          buffered: options.buffered,
        });
      }
    }

    if (nodeEntryTypes.length > 0) {
      if (nodeEntryTypes.includes("gc")) {
        installGcPerformanceHook();
      }
      this.#nodeTypes = nodeEntryTypes;
      this.#nodeBuffer = [];
      if (!nodePerformanceObservers.includes(this)) {
        nodePerformanceObservers.push(this);
      }
    }

    if (observesResource) {
      this.#observesResource = true;
      if (!resourceTimingObservers.includes(this)) {
        resourceTimingObservers.push(this);
      }
    }
  }

  disconnect() {
    super.disconnect();
    const nodeIndex = nodePerformanceObservers.indexOf(this);
    if (nodeIndex !== -1) {
      nodePerformanceObservers.splice(nodeIndex, 1);
    }
    const index = resourceTimingObservers.indexOf(this);
    if (index !== -1) {
      resourceTimingObservers.splice(index, 1);
    }
    this.#nodeBuffer = [];
    this.#nodeTypes = [];
    this.#resourceBuffer = [];
    this.#observesResource = false;
  }

  takeRecords() {
    const records = super.takeRecords();
    const nodeRecords = this.#nodeBuffer;
    this.#nodeBuffer = [];
    if (this.#resourceBuffer.length === 0 && nodeRecords.length === 0) {
      return records;
    }
    const resourceRecords = this.#resourceBuffer;
    this.#resourceBuffer = [];
    return records.concat(nodeRecords, resourceRecords);
  }

  [Symbol.for("nimbus.perf_hooks.enqueueNodeEntry")](entry) {
    if (!this.#nodeTypes.includes(entry.entryType)) {
      return;
    }
    this.#nodeBuffer.push(entry);
    if (this.#nodeScheduled) {
      return;
    }
    this.#nodeScheduled = true;
    queueMicrotask(() => {
      this.#nodeScheduled = false;
      const entries = this.#nodeBuffer;
      this.#nodeBuffer = [];
      if (entries.length === 0) {
        return;
      }
      this.#nodeCallback(createPerformanceEntryList(entries), this);
    });
  }

  [Symbol.for("nimbus.perf_hooks.enqueueResourceTiming")](entry) {
    if (!this.#observesResource) {
      return;
    }
    this.#resourceBuffer.push(entry);
    if (this.#resourceScheduled) {
      return;
    }
    this.#resourceScheduled = true;
    queueMicrotask(() => {
      this.#resourceScheduled = false;
      const entries = this.#resourceBuffer;
      this.#resourceBuffer = [];
      if (entries.length === 0) {
        return;
      }
      this.#resourceCallback(createPerformanceEntryList(entries), this);
    });
  }

  static get supportedEntryTypes() {
    const entryTypes = [...WebPerformanceObserver.supportedEntryTypes];
    for (const entryType of [...NODE_ENTRY_TYPES, "resource"]) {
      if (!entryTypes.includes(entryType)) {
        entryTypes.push(entryType);
      }
    }
    return entryTypes;
  }
}

const eventLoopUtilization = () => {
  // TODO(@marvinhagemeister): Return actual non-stubbed values
  return { idle: 0, active: 0, utilization: 0 };
};

performance.eventLoopUtilization = eventLoopUtilization;

const bootstrapTime = performance.now();
let nodeTimingLoopStart = -1;
let nodeTimingLoopStartArmed = false;
let nodeTimingLoopExit = -1;
const nodeTiming = {
  name: "node",
  entryType: "node",
  startTime: 0,
  nodeStart: 0,
  v8Start: Math.max(1, bootstrapTime / 4),
  environment: Math.max(2, bootstrapTime / 2),
  bootstrapComplete: Math.max(3, bootstrapTime),
  idleTime: 0,
  get loopStart() {
    if (nodeTimingLoopStart < 0 && nodeTimingLoopStartArmed) {
      nodeTimingLoopStart = Math.max(this.bootstrapComplete, performance.now());
    }
    nodeTimingLoopStartArmed = true;
    return nodeTimingLoopStart;
  },
  get loopExit() {
    if (nodeTimingLoopStart < 0) {
      return -1;
    }
    if (nodeTimingLoopExit < 0) {
      nodeTimingLoopExit = performance.now();
    }
    return nodeTimingLoopExit;
  },
  get duration() {
    return Math.max(performance.now(), this.loopExit, this.bootstrapComplete);
  },
  toJSON() {
    return {
      name: this.name,
      entryType: this.entryType,
      startTime: this.startTime,
      duration: this.duration,
      nodeStart: this.nodeStart,
      v8Start: this.v8Start,
      environment: this.environment,
      bootstrapComplete: this.bootstrapComplete,
      loopStart: this.loopStart,
      loopExit: this.loopExit,
      idleTime: this.idleTime,
    };
  },
};

const seedNodeTimingMarks = () => {
  performance.mark("nodeStart", { startTime: nodeTiming.nodeStart });
  performance.mark("bootstrapComplete", {
    startTime: nodeTiming.bootstrapComplete,
  });
};

performance.nodeTiming = nodeTiming;
seedNodeTimingMarks();

function installGcPerformanceHook() {
  const originalGc = globalThis.gc;
  if (
    typeof originalGc !== "function" ||
    originalGc[Symbol.for("nimbus.perf_hooks.gcWrapped")] === true
  ) {
    return;
  }
  function performanceGc(...args) {
    const startTime = performance.now();
    const result = originalGc.apply(this, args);
    const duration = Math.max(0, performance.now() - startTime);
    enqueueNodePerformanceEntry(new NodePerformanceEntry({
      name: "gc",
      entryType: "gc",
      startTime,
      duration,
      detail: {
        kind: constants.NODE_PERFORMANCE_GC_MAJOR,
        flags: constants.NODE_PERFORMANCE_GC_FLAGS_FORCED,
      },
      kind: constants.NODE_PERFORMANCE_GC_MAJOR,
      flags: constants.NODE_PERFORMANCE_GC_FLAGS_FORCED,
    }));
    return result;
  }
  Object.defineProperty(performanceGc, Symbol.for("nimbus.perf_hooks.gcWrapped"), {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
  Object.defineProperty(globalThis, "gc", {
    value: performanceGc,
    configurable: true,
    enumerable: false,
    writable: true,
  });
  if (globalThis.global && globalThis.global.gc === originalGc) {
    Object.defineProperty(globalThis.global, "gc", {
      value: performanceGc,
      configurable: true,
      enumerable: false,
      writable: true,
    });
  }
}

installGcPerformanceHook();

const kResourceTiming = Symbol("kResourceTiming");
const enqueueResourceTimingSymbol = Symbol.for("nimbus.perf_hooks.enqueueResourceTiming");
const denoPrivateCustomInspectSymbol = Symbol.for("Deno.privateCustomInspect");
const resourceTimingEntries = [];
const resourceTimingObservers = [];

// Node mirrors the W3C Resource Timing buffer semantics
// (lib/internal/perf/observe.js): a primary buffer capped at
// resourceTimingBufferSizeLimit, a secondary overflow buffer, and a
// 'resourcetimingbufferfull' event dispatched on the global performance object
// once the primary buffer is full. deno_web implements
// setResourceTimingBufferSize as a no-op and has no markResourceTiming, so
// Nimbus owns the entire buffer here.
let resourceTimingBufferSizeLimit = 250;
let resourceTimingSecondaryBuffer = [];
let resourceTimingBufferFullPending = false;

// WebIDL `unsigned long` coercion (ToUint32): NaN / +/-Infinity coerce to 0,
// otherwise the truncated value modulo 2^32. Matches Node's
// converters['unsigned long'] used by Performance#setResourceTimingBufferSize.
function toResourceTimingBufferSize(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number === 0) {
    return 0;
  }
  let wrapped = Math.trunc(number) % 4294967296;
  if (wrapped < 0) {
    wrapped += 4294967296;
  }
  return wrapped;
}

// https://www.w3.org/TR/resource-timing-2/#dfn-add-a-performanceresourcetiming-entry
function bufferResourceTiming(entry) {
  if (
    resourceTimingEntries.length < resourceTimingBufferSizeLimit &&
    !resourceTimingBufferFullPending
  ) {
    resourceTimingEntries.push(entry);
    return;
  }

  if (!resourceTimingBufferFullPending) {
    resourceTimingBufferFullPending = true;
    globalThis.setImmediate(() => {
      while (resourceTimingSecondaryBuffer.length > 0) {
        const excessNumberBefore = resourceTimingSecondaryBuffer.length;
        performance.dispatchEvent(
          new globalThis.Event("resourcetimingbufferfull"),
        );

        // Number of secondary entries that now fit in the primary buffer.
        const numbersToPreserve = Math.max(
          Math.min(
            resourceTimingBufferSizeLimit - resourceTimingEntries.length,
            resourceTimingSecondaryBuffer.length,
          ),
          0,
        );
        const excessNumberAfter =
          resourceTimingSecondaryBuffer.length - numbersToPreserve;
        for (let idx = 0; idx < numbersToPreserve; idx++) {
          resourceTimingEntries.push(resourceTimingSecondaryBuffer[idx]);
        }

        if (excessNumberBefore <= excessNumberAfter) {
          resourceTimingSecondaryBuffer = [];
        }
      }
      resourceTimingBufferFullPending = false;
    });
  }

  resourceTimingSecondaryBuffer.push(entry);
}

function createPerformanceEntryList(entries) {
  return {
    getEntries() {
      return entries.slice();
    },
    getEntriesByType(type) {
      return entries.filter((entry) => entry.entryType === type);
    },
    getEntriesByName(name, type = undefined) {
      return entries.filter((entry) =>
        entry.name === name && (type === undefined || entry.entryType === type)
      );
    },
  };
}

function resourceTimingFields(entry) {
  return {
    name: entry.name,
    entryType: entry.entryType,
    startTime: entry.startTime,
    duration: entry.duration,
    initiatorType: entry.initiatorType,
    nextHopProtocol: entry.nextHopProtocol,
    workerStart: entry.workerStart,
    redirectStart: entry.redirectStart,
    redirectEnd: entry.redirectEnd,
    fetchStart: entry.fetchStart,
    domainLookupStart: entry.domainLookupStart,
    domainLookupEnd: entry.domainLookupEnd,
    connectStart: entry.connectStart,
    connectEnd: entry.connectEnd,
    secureConnectionStart: entry.secureConnectionStart,
    requestStart: entry.requestStart,
    responseStart: entry.responseStart,
    responseEnd: entry.responseEnd,
    transferSize: entry.transferSize,
    encodedBodySize: entry.encodedBodySize,
    decodedBodySize: entry.decodedBodySize,
    deliveryType: entry.deliveryType,
    responseStatus: entry.responseStatus,
  };
}

function normalizeResourceConnectionTiming(connection) {
  return {
    domainLookupStartTime: connection?.domainLookupStartTime ?? 0,
    domainLookupEndTime: connection?.domainLookupEndTime ?? 0,
    connectionStartTime: connection?.connectionStartTime ?? 0,
    connectionEndTime: connection?.connectionEndTime ?? 0,
    secureConnectionStartTime: connection?.secureConnectionStartTime ?? 0,
    ALPNNegotiatedProtocol: connection?.ALPNNegotiatedProtocol ?? [],
  };
}

class PerformanceResourceTiming {
  constructor(resourceTiming = undefined) {
    if (resourceTiming === undefined) {
      throw new ERR_ILLEGAL_CONSTRUCTOR();
    }
    this[kResourceTiming] = resourceTiming;
  }

  get name() {
    return this[kResourceTiming].name;
  }

  get entryType() {
    return "resource";
  }

  get startTime() {
    return this[kResourceTiming].startTime;
  }

  get duration() {
    return Math.max(0, this.responseEnd - this.startTime);
  }

  get initiatorType() {
    return this[kResourceTiming].initiatorType;
  }

  get workerStart() {
    return this[kResourceTiming].finalServiceWorkerStartTime;
  }

  get redirectStart() {
    return this[kResourceTiming].redirectStartTime;
  }

  get redirectEnd() {
    return this[kResourceTiming].redirectEndTime;
  }

  get fetchStart() {
    return this[kResourceTiming].postRedirectStartTime;
  }

  get domainLookupStart() {
    return this[kResourceTiming].connection.domainLookupStartTime;
  }

  get domainLookupEnd() {
    return this[kResourceTiming].connection.domainLookupEndTime;
  }

  get connectStart() {
    return this[kResourceTiming].connection.connectionStartTime;
  }

  get connectEnd() {
    return this[kResourceTiming].connection.connectionEndTime;
  }

  get secureConnectionStart() {
    return this[kResourceTiming].connection.secureConnectionStartTime;
  }

  get nextHopProtocol() {
    return this[kResourceTiming].connection.ALPNNegotiatedProtocol;
  }

  get requestStart() {
    return this[kResourceTiming].finalNetworkRequestStartTime;
  }

  get responseStart() {
    return this[kResourceTiming].finalNetworkResponseStartTime;
  }

  get responseEnd() {
    return this[kResourceTiming].endTime;
  }

  get encodedBodySize() {
    return this[kResourceTiming].encodedBodySize;
  }

  get decodedBodySize() {
    return this[kResourceTiming].decodedBodySize;
  }

  get transferSize() {
    if (this[kResourceTiming].cacheMode === "local") {
      return 0;
    }
    return this.encodedBodySize === 0 ? 0 : this.encodedBodySize + 300;
  }

  get deliveryType() {
    return this[kResourceTiming].deliveryType;
  }

  get responseStatus() {
    return this[kResourceTiming].responseStatus;
  }

  toJSON() {
    return resourceTimingFields(this);
  }

  [customInspectSymbol](depth, options) {
    if (depth < 0) {
      return "[PerformanceResourceTiming]";
    }
    const inspectOptions = {
      ...options,
      depth: options?.depth == null ? null : options.depth - 1,
    };
    return `PerformanceResourceTiming ${inspect(resourceTimingFields(this), inspectOptions)}`;
  }

  [denoPrivateCustomInspectSymbol](inspectValue, inspectOptions) {
    return `PerformanceResourceTiming ${inspectValue(resourceTimingFields(this), inspectOptions)}`;
  }
}

Object.setPrototypeOf(PerformanceResourceTiming.prototype, PerformanceEntry.prototype);
Object.defineProperty(PerformanceResourceTiming.prototype, Symbol.toStringTag, {
  value: "PerformanceResourceTiming",
  configurable: true,
});

const nodeTimingMarkNames = new Set(["nodeStart", "bootstrapComplete"]);
const isVisiblePerformanceEntry = (entry) =>
  entry.entryType !== "mark" || !nodeTimingMarkNames.has(entry.name);
const filterVisiblePerformanceEntries = (entries) =>
  entries.filter(isVisiblePerformanceEntry);

// Node sorts every getEntries* result ascending by startTime through a single
// shared comparator (upstream lib/internal/perf/observe.js performanceObserverSorter).
// Array.prototype.sort is stable, so ties preserve insertion order.
const performanceEntrySorter = (first, second) => first.startTime - second.startTime;
const sortPerformanceEntriesByStartTime = (entries) =>
  entries.sort(performanceEntrySorter);

const coerceNodeMarkName = (markName) => {
  if (typeof markName === "symbol") {
    `${markName}`;
  }
  return markName;
};

const validateNodeMarkOptions = (markOptions) => {
  if (markOptions === undefined) {
    return;
  }
  if (markOptions === null || typeof markOptions !== "object") {
    throw new ERR_INVALID_ARG_TYPE("options", "Object", markOptions);
  }
  if (
    "startTime" in markOptions && typeof markOptions.startTime !== "number"
  ) {
    throw new ERR_INVALID_ARG_TYPE(
      "startTime",
      "number",
      markOptions.startTime,
    );
  }
};

const originalMark = performance.mark.bind(performance);
performance.mark = (markName, markOptions = { __proto__: null }) => {
  validateNodeMarkOptions(markOptions);
  return originalMark(coerceNodeMarkName(markName), markOptions);
};

const originalClearMarks = performance.clearMarks.bind(performance);
performance.clearMarks = (markName = undefined) => {
  const coercedMarkName = markName === undefined ? undefined : coerceNodeMarkName(markName);
  if (coercedMarkName !== undefined && nodeTimingMarkNames.has(coercedMarkName)) {
    return;
  }
  originalClearMarks(coercedMarkName);
  if (markName === undefined) {
    seedNodeTimingMarks();
  }
};

const originalGetEntries = performance.getEntries.bind(performance);
performance.getEntries = () =>
  sortPerformanceEntriesByStartTime(
    filterVisiblePerformanceEntries(originalGetEntries()).concat(resourceTimingEntries),
  );

const originalGetEntriesByType = performance.getEntriesByType.bind(performance);
performance.getEntriesByType = function getEntriesByType(type) {
  if (arguments.length === 0) {
    throw new ERR_MISSING_ARGS("type");
  }
  if (type === "resource") {
    return sortPerformanceEntriesByStartTime(resourceTimingEntries.slice());
  }
  return sortPerformanceEntriesByStartTime(
    filterVisiblePerformanceEntries(originalGetEntriesByType(type)),
  );
};

const originalGetEntriesByName = performance.getEntriesByName.bind(performance);
performance.getEntriesByName = function getEntriesByName(name, type = undefined) {
  if (arguments.length === 0) {
    throw new ERR_MISSING_ARGS("name");
  }
  const webEntries = filterVisiblePerformanceEntries(originalGetEntriesByName(name, type));
  if (type !== undefined && type !== "resource") {
    return sortPerformanceEntriesByStartTime(webEntries);
  }
  return sortPerformanceEntriesByStartTime(
    webEntries.concat(resourceTimingEntries.filter((entry) => entry.name === name)),
  );
};

performance.clearResourceTimings = () => {
  resourceTimingEntries.length = 0;
};

// deno_web's setResourceTimingBufferSize is a no-op. Node coerces maxSize via
// the WebIDL `unsigned long` converter and updates the primary-buffer limit; the
// buffer itself is not trimmed when the limit is lowered
// (test/parallel/test-performance-resourcetimingbuffersize.js).
performance.setResourceTimingBufferSize = function setResourceTimingBufferSize(
  maxSize,
) {
  if (arguments.length === 0) {
    throw new ERR_MISSING_ARGS("maxSize");
  }
  resourceTimingBufferSizeLimit = toResourceTimingBufferSize(maxSize);
};

// The deno_web global performance.toJSON() only emits { timeOrigin }. Node's
// Performance#toJSON (lib/internal/perf/performance.js) additionally exposes
// nodeTiming and eventLoopUtilization, so wrap it to match Node's shape.
performance.toJSON = () => ({
  nodeTiming,
  timeOrigin: performance.timeOrigin,
  eventLoopUtilization: eventLoopUtilization(),
});

performance.markResourceTiming = (
  timingInfo,
  requestedUrl,
  initiatorType,
  _global,
  cacheMode,
  _bodyInfo,
  responseStatus = 0,
  deliveryType = "",
) => {
  const resourceTiming = new PerformanceResourceTiming({
    name: `${requestedUrl}`,
    startTime: timingInfo?.startTime ?? 0,
    redirectStartTime: timingInfo?.redirectStartTime ?? 0,
    redirectEndTime: timingInfo?.redirectEndTime ?? 0,
    postRedirectStartTime: timingInfo?.postRedirectStartTime ?? 0,
    finalServiceWorkerStartTime: timingInfo?.finalServiceWorkerStartTime ?? 0,
    finalNetworkRequestStartTime: timingInfo?.finalNetworkRequestStartTime ?? 0,
    finalNetworkResponseStartTime: timingInfo?.finalNetworkResponseStartTime ?? 0,
    endTime: timingInfo?.endTime ?? 0,
    encodedBodySize: timingInfo?.encodedBodySize ?? 0,
    decodedBodySize: timingInfo?.decodedBodySize ?? 0,
    connection: normalizeResourceConnectionTiming(timingInfo?.finalConnectionTimingInfo),
    initiatorType: `${initiatorType}`,
    cacheMode,
    responseStatus,
    deliveryType,
  });
  // Node calls enqueue() (notify observers) before bufferResourceTiming()
  // (lib/internal/perf/resource_timing.js markResourceTiming). Observers receive
  // every entry; the global buffer applies the size limit / overflow event.
  for (const observer of resourceTimingObservers) {
    observer[enqueueResourceTimingSymbol](resourceTiming);
  }
  bufferResourceTiming(resourceTiming);
  return resourceTiming;
};

const recordHistogramDuration = (histogram, startTime) => {
  if (!histogram || typeof histogram.record !== "function") {
    return;
  }
  const durationNanoseconds = Math.max(
    1,
    Math.round((performance.now() - startTime) * 1_000_000),
  );
  histogram.record(durationNanoseconds);
};

const timerify = (fn, options = {}) => {
  if (typeof fn !== "function") {
    throw new ERR_INVALID_ARG_TYPE("fn", "function", fn);
  }

  if (
    options !== undefined && (typeof options !== "object" || options === null)
  ) {
    throw new ERR_INVALID_ARG_TYPE("options", "Object", options);
  }

  if (options?.histogram !== undefined) {
    if (
      typeof options.histogram !== "object" ||
      options.histogram === null ||
      typeof options.histogram.record !== "function"
    ) {
      throw new ERR_INVALID_ARG_TYPE(
        "options.histogram",
        "RecordableHistogram",
        options.histogram,
      );
    }
  }

  function timerified(...args) {
    const startTime = performance.now();
    if (new.target) {
      try {
        const result = new fn(...args);
        enqueueNodePerformanceEntry(new NodePerformanceEntry({
          name: fn.name,
          entryType: "function",
          startTime,
          duration: Math.max(0, performance.now() - startTime),
          args,
        }));
        return result;
      } finally {
        recordHistogramDuration(options?.histogram, startTime);
      }
    }

    try {
      const result = fn.apply(this, args);
      if (result && typeof result.then === "function") {
        return Promise.resolve(result).finally(() => {
          recordHistogramDuration(options?.histogram, startTime);
          enqueueNodePerformanceEntry(new NodePerformanceEntry({
            name: fn.name,
            entryType: "function",
            startTime,
            duration: Math.max(0, performance.now() - startTime),
            args,
          }));
        });
      }
      recordHistogramDuration(options?.histogram, startTime);
      enqueueNodePerformanceEntry(new NodePerformanceEntry({
        name: fn.name,
        entryType: "function",
        startTime,
        duration: Math.max(0, performance.now() - startTime),
        args,
      }));
      return result;
    } catch (error) {
      recordHistogramDuration(options?.histogram, startTime);
      throw error;
    }
  }

  Object.defineProperty(timerified, "name", {
    value: `timerified ${fn.name}`,
    configurable: true,
  });
  Object.defineProperty(timerified, "length", {
    value: fn.length,
    configurable: true,
  });

  return timerified;
};

performance.timerify = timerify;

function monitorEventLoopDelay(options = {}) {
  const { resolution = 10 } = options;

  return new IntervalHistogram(resolution);
}

export default {
  performance,
  PerformanceObserver,
  PerformanceObserverEntryList,
  PerformanceEntry,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceResourceTiming,
  createHistogram,
  monitorEventLoopDelay,
  eventLoopUtilization,
  timerify,
  constants,
};

export {
  constants,
  createHistogram,
  eventLoopUtilization,
  monitorEventLoopDelay,
  performance,
  PerformanceEntry,
  PerformanceObserver,
  PerformanceObserverEntryList,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceResourceTiming,
  timerify,
};
