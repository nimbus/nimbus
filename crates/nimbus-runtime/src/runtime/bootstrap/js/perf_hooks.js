// Copyright 2018-2026 the Deno authors. MIT license.
//
// This is served through Nimbus's node:perf_hooks builtin override path in the
// restricted module loader so we can keep compatibility-specific module deltas
// local until they are worth promoting into the shared fork.

// TODO(petamoriken): enable prefer-primordials for node polyfills
// deno-lint-ignore-file prefer-primordials

import { core } from "ext:core/mod.js";
import {
  performance,
  PerformanceEntry,
  PerformanceMark,
  PerformanceMeasure,
  PerformanceObserver as WebPerformanceObserver,
  PerformanceObserverEntryList,
  PerformanceResourceTiming,
} from "ext:deno_web/15_performance.js";
import { EldHistogram } from "ext:core/ops";
import {
  ERR_ILLEGAL_CONSTRUCTOR,
  ERR_INVALID_ARG_TYPE,
  ERR_INVALID_ARG_VALUE,
  ERR_INVALID_THIS,
  ERR_OUT_OF_RANGE,
} from "ext:deno_node/internal/errors.ts";
import {
  validateInteger,
  validateNumber,
  validateObject,
} from "ext:deno_node/internal/validators.mjs";
import {
  customInspectSymbol,
  kEmptyObject,
} from "ext:deno_node/internal/util.mjs";
import { inspect } from "ext:deno_node/internal/util/inspect.mjs";

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
};

const EMPTY_HISTOGRAM_MIN_BIGINT = 9_223_372_036_854_775_807n;
const EMPTY_HISTOGRAM_MIN_NUMBER = Number(EMPTY_HISTOGRAM_MIN_BIGINT);
const INTERVAL_HISTOGRAM_CLONE_TYPE = "NimbusIntervalHistogramSnapshot";
const RECORDABLE_HISTOGRAM_CLONE_TYPE = "NimbusRecordableHistogram";
const kHistogramStateId = Symbol("kHistogramStateId");
const kHistogramRecordable = Symbol("kHistogramRecordable");
const kHistogramSkipThrow = Symbol("kHistogramSkipThrow");
const histogramStateRegistry = new Map();
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

// Node-compatible PerformanceObserver that throws proper Node.js errors
class PerformanceObserver extends WebPerformanceObserver {
  constructor(callback) {
    if (typeof callback !== "function") {
      throw new ERR_INVALID_ARG_TYPE("callback", "Function", callback);
    }
    super(callback);
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
    return super.observe(options);
  }

  static get supportedEntryTypes() {
    return WebPerformanceObserver.supportedEntryTypes;
  }
}

const eventLoopUtilization = () => {
  // TODO(@marvinhagemeister): Return actual non-stubbed values
  return { idle: 0, active: 0, utilization: 0 };
};

performance.eventLoopUtilization = eventLoopUtilization;

const nodeTiming = {
  nodeStart: 0,
  bootstrapComplete: performance.now(),
};

const seedNodeTimingMarks = () => {
  performance.mark("nodeStart", { startTime: nodeTiming.nodeStart });
  performance.mark("bootstrapComplete", {
    startTime: nodeTiming.bootstrapComplete,
  });
};

performance.nodeTiming = nodeTiming;
seedNodeTimingMarks();

const nodeTimingMarkNames = new Set(["nodeStart", "bootstrapComplete"]);
const isVisiblePerformanceEntry = (entry) =>
  entry.entryType !== "mark" || !nodeTimingMarkNames.has(entry.name);
const filterVisiblePerformanceEntries = (entries) =>
  entries.filter(isVisiblePerformanceEntry);

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
performance.getEntries = () => filterVisiblePerformanceEntries(originalGetEntries());

const originalGetEntriesByType = performance.getEntriesByType.bind(performance);
performance.getEntriesByType = (type) =>
  filterVisiblePerformanceEntries(originalGetEntriesByType(type));

const originalGetEntriesByName = performance.getEntriesByName.bind(performance);
performance.getEntriesByName = (name, type = undefined) =>
  filterVisiblePerformanceEntries(originalGetEntriesByName(name, type));

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
        return new fn(...args);
      } finally {
        recordHistogramDuration(options?.histogram, startTime);
      }
    }

    try {
      const result = fn.apply(this, args);
      if (result && typeof result.then === "function") {
        return Promise.resolve(result).finally(() => {
          recordHistogramDuration(options?.histogram, startTime);
        });
      }
      recordHistogramDuration(options?.histogram, startTime);
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
