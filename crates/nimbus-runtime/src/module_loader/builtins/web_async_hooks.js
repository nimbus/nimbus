// Convex default-runtime Node-API subset: `node:async_hooks` limited to
// AsyncLocalStorage + AsyncResource, implemented directly over deno_core's
// async context frame (core.AsyncVariable). Served ONLY on WebStandardIsolate
// lanes with convex_default guest semantics — this is not the Node lane's
// full async_hooks surface (no createHook/executionAsyncId machinery).
import { core } from "ext:core/mod.js";

const { AsyncVariable, getAsyncContext, setAsyncContext } = core;

let nextAsyncId = 1;

export class AsyncResource {
  #snapshot;
  #asyncId;

  constructor(type) {
    if (typeof type !== "string" || type.length === 0) {
      throw new TypeError('The "type" argument must be a non-empty string');
    }
    this.type = type;
    this.#snapshot = getAsyncContext();
    this.#asyncId = nextAsyncId++;
  }

  asyncId() {
    return this.#asyncId;
  }

  triggerAsyncId() {
    return 0;
  }

  runInAsyncScope(fn, thisArg, ...args) {
    const previous = getAsyncContext();
    try {
      setAsyncContext(this.#snapshot);
      return fn.apply(thisArg, args);
    } finally {
      setAsyncContext(previous);
    }
  }

  emitDestroy() {
    return this;
  }

  bind(fn, thisArg) {
    if (typeof fn !== "function") {
      throw new TypeError('The "fn" argument must be a function');
    }
    const resource = this;
    const bound =
      thisArg === undefined
        ? function (...args) {
            return resource.runInAsyncScope(fn, this, ...args);
          }
        : resource.runInAsyncScope.bind(resource, fn, thisArg);
    Object.defineProperty(bound, "length", {
      configurable: true,
      enumerable: false,
      value: fn.length,
      writable: false,
    });
    return bound;
  }

  static bind(fn, type, thisArg) {
    return new AsyncResource(type || fn.name || "bound-anonymous-fn").bind(
      fn,
      thisArg,
    );
  }
}

export class AsyncLocalStorage {
  #variable = new AsyncVariable();
  #defaultValue;
  #name = "";
  enabled = false;

  constructor(options = {}) {
    if (options === null || typeof options !== "object") {
      throw new TypeError('The "options" argument must be an object');
    }
    this.#defaultValue = options.defaultValue;
    if (options.name !== undefined) {
      this.#name = `${options.name}`;
    }
  }

  get name() {
    return this.#name;
  }

  run(store, callback, ...args) {
    this.enabled = true;
    const previous = this.#variable.enter(store);
    try {
      return callback(...args);
    } finally {
      setAsyncContext(previous);
    }
  }

  exit(callback, ...args) {
    if (!this.enabled) {
      return callback(...args);
    }
    this.enabled = false;
    try {
      return callback(...args);
    } finally {
      this.enabled = true;
    }
  }

  getStore() {
    if (!this.enabled) {
      return this.#defaultValue;
    }
    const value = this.#variable.get();
    return value === undefined ? this.#defaultValue : value;
  }

  enterWith(store) {
    this.enabled = true;
    this.#variable.enter(store);
  }

  disable() {
    this.enabled = false;
  }

  static bind(fn) {
    return AsyncResource.bind(fn);
  }

  static snapshot() {
    const snapshot = getAsyncContext();
    return (callback, ...args) => {
      const previous = getAsyncContext();
      try {
        setAsyncContext(snapshot);
        return callback(...args);
      } finally {
        setAsyncContext(previous);
      }
    };
  }
}

export default { AsyncLocalStorage, AsyncResource };
