'use strict';

const assert = require('assert');
const fs = require('node:fs');
const path = require('node:path');
const { inspect } = require('util');

const bits = ['arm64', 'loong64', 'mips', 'mipsel', 'ppc64', 'riscv64', 's390x', 'x64']
  .includes(process.arch) ? 64 : 32;
const noop = () => {};
const mustCallChecks = [];
const isDebug = process.features?.debug === true;
const isAIX = process.platform === 'aix';
const isIBMi = process.platform === 'os400';
const isRiscv64 = process.arch === 'riscv64';
const isWindows = process.platform === 'win32';
const hasInspector = process.features?.inspector === true;
const hasSQLite = Boolean(process.versions?.sqlite);
let localhostIPv4 = null;
const localIPv6Hosts = ['localhost'];
const tmpdir = require('./tmpdir.js');
const PIPE = (() => {
  const pipeName = `n.${process.pid}.sock`;
  if (isWindows) {
    return path.join('\\\\.\\pipe\\', pipeName);
  }
  fs.mkdirSync(tmpdir.path, { recursive: true });
  const pipePath = path.join(tmpdir.path, pipeName);
  fs.rmSync(pipePath, { force: true });
  return pipePath;
})();

function runCallChecks() {
  const failed = mustCallChecks.filter((context) => {
    if ('minimum' in context) {
      context.messageSegment = `at least ${context.minimum}`;
      return context.actual < context.minimum;
    }

    context.messageSegment = `exactly ${context.exact}`;
    return context.actual !== context.exact;
  });

  if (failed.length === 0) {
    return;
  }

  const detail = failed.map((context) => (
    `Expected ${context.name} to be called ${context.messageSegment}, actual ${context.actual}.`
  )).join('\n');
  assert.fail(`Mismatched function calls:\n${detail}`);
}

function _mustCallInner(fn, criteria = 1, field) {
  if (typeof fn === 'number') {
    criteria = fn;
    fn = noop;
  } else if (fn === undefined) {
    fn = noop;
  }

  if (typeof criteria !== 'number') {
    throw new TypeError(`Invalid ${field} value: ${criteria}`);
  }

  const context = {
    [field]: criteria,
    actual: 0,
    name: fn.name || '<anonymous>',
  };
  mustCallChecks.push(context);

  const wrapped = function(...args) {
    context.actual += 1;
    return fn.apply(this, args);
  };

  Object.defineProperties(wrapped, {
    name: {
      value: fn.name,
      writable: false,
      enumerable: false,
      configurable: true,
    },
    length: {
      value: fn.length,
      writable: false,
      enumerable: false,
      configurable: true,
    },
  });

  return wrapped;
}

function mustCall(fn, exact) {
  return _mustCallInner(fn, exact, 'exact');
}

function mustSucceed(fn, exact) {
  return mustCall(function(err, ...args) {
    assert.ifError(err);
    if (typeof fn === 'function') {
      return fn.apply(this, args);
    }
  }, exact);
}

function mustCallAtLeast(fn, minimum) {
  return _mustCallInner(fn, minimum, 'minimum');
}

function mustNotCall(msg) {
  return function mustNotCall(...args) {
    const argsInfo = args.length > 0 ?
      `\ncalled with arguments: ${args.map((arg) => inspect(arg)).join(', ')}` : '';
    assert.fail(`${msg || 'function should not have been called'}${argsInfo}`);
  };
}

const mustNotMutateObjectDeepProxies = new WeakMap();

function mustNotMutateObjectDeep(original) {
  if (original === null || typeof original !== 'object') {
    return original;
  }

  const cachedProxy = mustNotMutateObjectDeepProxies.get(original);
  if (cachedProxy) {
    return cachedProxy;
  }

  const handler = {
    defineProperty(target, property) {
      assert.fail(`Expected no side effects, got ${inspect(property)} defined`);
    },
    deleteProperty(target, property) {
      assert.fail(`Expected no side effects, got ${inspect(property)} deleted`);
    },
    get(target, property, receiver) {
      return mustNotMutateObjectDeep(Reflect.get(target, property, receiver));
    },
    preventExtensions(target) {
      assert.fail(`Expected no side effects, got extensions prevented on ${inspect(target)}`);
    },
    set(target, property, value) {
      assert.fail(
        `Expected no side effects, got ${inspect(value)} assigned to ${inspect(property)}`
      );
    },
    setPrototypeOf(target, prototype) {
      assert.fail(`Expected no side effects, got set prototype to ${prototype}`);
    },
  };

  const proxy = new Proxy(original, handler);
  mustNotMutateObjectDeepProxies.set(original, proxy);
  return proxy;
}

function printSkipMessage(msg) {
  console.log(`1..0 # Skipped: ${msg}`);
}

function skip(msg) {
  printSkipMessage(msg);
  const error = new Error(`Neovex node_compat skip: ${msg}`);
  error.code = 'NEOVEX_NODE_COMPAT_SKIP';
  error.__neovexSkip = true;
  throw error;
}

function skipIf32Bits() {
  if (bits < 64) {
    skip('The tested feature is not available in 32bit builds');
  }
}

function skipIfDumbTerminal() {
  if (process.env.TERM === 'dumb') {
    skip('skipping - dumb terminal');
  }
}

function skipIfInspectorDisabled() {
  if (!hasInspector) {
    skip('V8 inspector is disabled');
  }
}

function skipIfSQLiteMissing() {
  if (!hasSQLite) {
    skip('missing SQLite');
  }
}

function skipIfWorker() {
  if (!isMainThread) {
    skip('This test only works on a main thread');
  }
}

function platformTimeout(ms) {
  const multipliers = typeof ms === 'bigint' ?
    { two: 2n, four: 4n } : { two: 2, four: 4 };

  if (isDebug) {
    ms = multipliers.two * ms;
  }

  if (isAIX || isIBMi) {
    return multipliers.two * ms;
  }

  if (isRiscv64) {
    return multipliers.four * ms;
  }

  return ms;
}

function invalidArgTypeHelper(input) {
  if (input == null) {
    return ` Received ${input}`;
  }
  if (typeof input === 'function') {
    return ` Received function ${input.name}`;
  }
  if (typeof input === 'object') {
    if (input.constructor?.name) {
      return ` Received an instance of ${input.constructor.name}`;
    }
    return ` Received ${inspect(input, { depth: -1 })}`;
  }

  let inspected = inspect(input, { colors: false });
  if (inspected.length > 28) {
    inspected = `${inspected.slice(0, 25)}...`;
  }

  return ` Received type ${typeof input} (${inspected})`;
}

function _expectWarning(name, expected, code) {
  if (typeof expected === 'string') {
    expected = [[expected, code]];
  } else if (!Array.isArray(expected)) {
    expected = Object.entries(expected).map(([warningCode, message]) => [message, warningCode]);
  } else if (expected.length !== 0 && !Array.isArray(expected[0])) {
    expected = [[expected[0], expected[1]]];
  }

  if (name === 'DeprecationWarning') {
    expected.forEach(([_, warningCode]) => {
      assert(warningCode, `Missing deprecation code: ${expected}`);
    });
  }

  return mustCall((warning) => {
    const expectedProperties = expected.shift();
    if (!expectedProperties) {
      assert.fail(`Unexpected extra warning received: ${warning}`);
    }

    const [message, warningCode] = expectedProperties;
    assert.strictEqual(warning.name, name);
    if (typeof message === 'string') {
      assert.strictEqual(warning.message, message);
    } else {
      assert.match(warning.message, message);
    }
    assert.strictEqual(warning.code, warningCode);
  }, expected.length);
}

let catchWarning;

const hasCrypto = (() => {
  try {
    const crypto = require('node:crypto');
    return typeof crypto.createSecretKey === 'function' &&
      typeof crypto.KeyObject?.from === 'function' &&
      typeof globalThis.crypto?.subtle?.importKey === 'function' &&
      typeof globalThis.crypto?.subtle?.generateKey === 'function';
  } catch {
    return false;
  }
})();

function opensslVersionNumber(major = 0, minor = 0, patch = 0) {
  assert(major >= 0 && major <= 0xf);
  assert(minor >= 0 && minor <= 0xff);
  assert(patch >= 0 && patch <= 0xff);
  return (major << 28) | (minor << 20) | (patch << 4);
}

let cachedOpenSSLVersionNumber;
function hasOpenSSL(major = 0, minor = 0, patch = 0) {
  if (!hasCrypto || !process.versions?.openssl) {
    return false;
  }

  if (cachedOpenSSLVersionNumber === undefined) {
    const regexp = /(?<m>\d+)\.(?<n>\d+)\.(?<p>\d+)/;
    const match = String(process.versions.openssl).match(regexp);
    if (!match?.groups) {
      return false;
    }
    const { m, n, p } = match.groups;
    cachedOpenSSLVersionNumber = opensslVersionNumber(m, n, p);
  }

  return cachedOpenSSLVersionNumber >= opensslVersionNumber(major, minor, patch);
}

function expectWarning(nameOrMap, expected, code) {
  if (catchWarning === undefined) {
    catchWarning = {};
    process.on('warning', (warning) => {
      if (!catchWarning[warning.name]) {
        throw new TypeError(
          `"${warning.name}" was triggered without being expected.\n${inspect(warning)}`
        );
      }
      catchWarning[warning.name](warning);
    });
  }

  if (typeof nameOrMap === 'string') {
    catchWarning[nameOrMap] = _expectWarning(nameOrMap, expected, code);
  } else {
    Object.keys(nameOrMap).forEach((name) => {
      catchWarning[name] = _expectWarning(name, nameOrMap[name]);
    });
  }
}

function isAlive(pid) {
  try {
    process.kill(pid, 'SIGCONT');
    return true;
  } catch {
    return false;
  }
}

function expectsError(validator, exact) {
  return mustCall((...args) => {
    if (args.length !== 1) {
      assert.fail(`Expected one argument, got ${inspect(args)}`);
    }

    const error = args.pop();
    assert.strictEqual(
      Object.prototype.propertyIsEnumerable.call(error, 'message'),
      false,
    );
    assert.throws(() => {
      throw error;
    }, validator);
    return true;
  }, exact);
}

function getArrayBufferViews(buf) {
  const { buffer, byteOffset, byteLength } = buf;

  const out = [];
  const nodeMajorVersion = Number.parseInt(
    String(process?.versions?.node ?? '').split('.')[0],
    10,
  );
  const arrayBufferViews = [
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Float32Array,
    Float64Array,
    BigInt64Array,
    BigUint64Array,
    DataView,
  ];

  if (nodeMajorVersion >= 24 && typeof Float16Array === 'function') {
    arrayBufferViews.splice(7, 0, Float16Array);
  }

  for (const type of arrayBufferViews) {
    const { BYTES_PER_ELEMENT = 1 } = type;
    if (byteLength % BYTES_PER_ELEMENT === 0) {
      out.push(new type(buffer, byteOffset, byteLength / BYTES_PER_ELEMENT));
    }
  }

  return out;
}

function getBufferSources(buf) {
  return [...getArrayBufferViews(buf), new Uint8Array(buf).buffer];
}

function canCreateSymLink() {
  if (process.platform !== 'win32') {
    return true;
  }

  try {
    const { execSync } = require('node:child_process');
    const whoamiPath = `${process.env.SystemRoot}\\System32\\whoami.exe`;
    return execSync(`${whoamiPath} /priv`, { timeout: 1000 })
      .includes('SeCreateSymbolicLinkPrivilege');
  } catch {
    return false;
  }
}

function runWithInvalidFD(func) {
  let fd = 1 << 30;
  try {
    while (fs.fstatSync(fd--) && fd > 0);
  } catch {
    return func(fd);
  }

  printSkipMessage('Could not generate an invalid fd');
}

function allowGlobals(..._allowlist) {
  // The Neovex node_compat harness does not run the upstream leaked-global
  // audit, but some official fixtures still register globals through this
  // helper before exiting. Keep the public helper present so those fixtures
  // can execute their intended contract.
}

function installEnvShim() {
  if (!process || !process.env) {
    return;
  }

  const env = process.env;
  const termOverride = globalThis.__neovexNodeCompatTerm ?? 'dumb';
  const shimmedMissingValues = new Map([
    ['TERM', termOverride],
    ['TEST_PARALLEL', undefined],
    ['NODE_TEST_DIR', undefined],
    ['TEST_SERIAL_ID', undefined],
    ['TEST_THREAD_ID', undefined],
    ['NODE_V8_COVERAGE', undefined],
    ['__MINIMATCH_TESTING_PLATFORM__', undefined],
  ]);

  const shim = new Proxy(env, {
    get(target, property, receiver) {
      if (property === 'TERM' && globalThis.__neovexNodeCompatTerm !== undefined) {
        return globalThis.__neovexNodeCompatTerm;
      }
      if (typeof property === 'string' && shimmedMissingValues.has(property)) {
        try {
          return Reflect.get(target, property, receiver);
        } catch (error) {
          if (String(error?.message ?? '').includes('runtime env capability denied')) {
            return shimmedMissingValues.get(property);
          }
          throw error;
        }
      }
      return Reflect.get(target, property, receiver);
    },
    has(target, property) {
      if (property === 'TERM' && globalThis.__neovexNodeCompatTerm !== undefined) {
        return true;
      }
      if (typeof property === 'string' && shimmedMissingValues.has(property)) {
        try {
          return Reflect.has(target, property);
        } catch (error) {
          if (String(error?.message ?? '').includes('runtime env capability denied')) {
            return shimmedMissingValues.get(property) !== undefined;
          }
          throw error;
        }
      }
      return Reflect.has(target, property);
    },
  });

  Object.defineProperty(process, 'env', {
    value: shim,
    configurable: true,
    enumerable: true,
    writable: false,
  });
}

installEnvShim();

const neovexChildProcessShimInstalled = Symbol.for('neovex.nodeCompatChildProcessShimInstalled');
const neovexClusterShimInstalled = Symbol.for('neovex.nodeCompatClusterShimInstalled');
const neovexForkExitCleanupInstalled = Symbol.for('neovex.nodeCompatForkExitCleanupInstalled');
const neovexForkWorkers = new Set();
const neovexForkWorkerCompletions = new Set();

async function flushNeovexForkWorkers() {
  const deadline = Date.now() + 1000;

  for (;;) {
    if (neovexForkWorkerCompletions.size === 0) {
      await Promise.resolve();
      await new Promise((resolve) => queueMicrotask(resolve));
      if (typeof process.nextTick === 'function') {
        await new Promise((resolve) => process.nextTick(resolve));
      }
      if (neovexForkWorkerCompletions.size === 0) {
        return;
      }
    }

    await Promise.allSettled([...neovexForkWorkerCompletions]);
    if (Date.now() >= deadline) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function isNeovexNodeCompatCommand(command) {
  if (typeof command !== 'string' || command.length === 0) {
    return false;
  }
  if (command === process.execPath) {
    return true;
  }
  if (!path.isAbsolute(command) || !fs.existsSync(command)) {
    return false;
  }
  const execBase = path.basename(process.execPath || '');
  return execBase.length > 0 && path.basename(command) === execBase;
}

function canUseNeovexSpawnSync(command, args = [], options = {}) {
  return typeof globalThis.__neovexSyncHostValue === 'function' &&
    isNeovexNodeCompatCommand(String(command)) &&
    Array.isArray(args) &&
    (options == null || typeof options === 'object') &&
    (options.stdio === undefined || options.stdio === 'pipe') &&
    options.shell !== true &&
    options.timeout === undefined &&
    options.uid === undefined &&
    options.gid === undefined;
}

function canUseNeovexAsyncSpawn(command, args = [], options = {}) {
  return typeof globalThis.__neovexAsyncHostValue === 'function' &&
    isNeovexNodeCompatCommand(String(command)) &&
    Array.isArray(args) &&
    (options == null || typeof options === 'object') &&
    options.shell !== true &&
    options.signal === undefined &&
    options.timeout === undefined &&
    options.uid === undefined &&
    options.gid === undefined &&
    (options.stdio === undefined || options.stdio === 'pipe' || options.stdio === 'inherit');
}

function encodeNeovexSpawnOutput(buffer, encoding) {
  if (encoding && encoding !== 'buffer') {
    return buffer.toString(encoding);
  }
  return buffer;
}

function encodeNeovexSpawnInput(input) {
  if (input === undefined) {
    return null;
  }
  if (typeof input === 'string') {
    return Buffer.from(input, 'utf8').toString('base64');
  }
  if (Buffer.isBuffer(input)) {
    return input.toString('base64');
  }
  if (ArrayBuffer.isView(input)) {
    return Buffer.from(input.buffer, input.byteOffset, input.byteLength).toString('base64');
  }
  if (input instanceof ArrayBuffer) {
    return Buffer.from(input).toString('base64');
  }
  return Buffer.from(String(input), 'utf8').toString('base64');
}

function runNeovexSpawnSync(command, args = [], options = {}) {
  const encoding = options?.encoding;
  const env =
    options?.env && typeof options.env === 'object'
      ? Object.fromEntries(
        Object.entries(options.env)
          .filter(([key, value]) => typeof key === 'string' && value != null)
          .map(([key, value]) => [key, String(value)]),
      )
      : null;

  try {
    const result = globalThis.__neovexSyncHostValue('op_neovex_runtime_test_spawn_sync', {
      command: String(command),
      args: args.map((value) => String(value)),
      cwd: typeof options?.cwd === 'string' ? options.cwd : null,
      env,
      stdinBase64: encodeNeovexSpawnInput(options?.input),
    });
    const stdoutBuffer = Buffer.from(result?.stdout ?? '', 'utf8');
    const stderrBuffer = Buffer.from(result?.stderr ?? '', 'utf8');
    const stdout = encodeNeovexSpawnOutput(stdoutBuffer, encoding);
    const stderr = encodeNeovexSpawnOutput(stderrBuffer, encoding);
    return {
      pid: typeof result?.pid === 'number' ? result.pid : 0,
      output: [null, stdout, stderr],
      stdout,
      stderr,
      status: typeof result?.code === 'number' ? result.code : 1,
      signal: result?.signal ?? null,
    };
  } catch (error) {
    const rendered = typeof error?.stack === 'string' ? error.stack : String(error);
    const stdoutBuffer = Buffer.alloc(0);
    const stderrBuffer = Buffer.from(`${rendered}\n`, 'utf8');
    const stdout = encodeNeovexSpawnOutput(stdoutBuffer, encoding);
    const stderr = encodeNeovexSpawnOutput(stderrBuffer, encoding);
    return {
      pid: 0,
      output: [null, stdout, stderr],
      stdout,
      stderr,
      status: 1,
      signal: null,
      error,
    };
  }
}

function encodeNeovexAsyncSpawnEnv(options = {}) {
  return options?.env && typeof options.env === 'object'
    ? Object.fromEntries(
      Object.entries(options.env)
        .filter(([key, value]) => typeof key === 'string' && value != null)
        .map(([key, value]) => [key, String(value)]),
    )
    : null;
}

async function runNeovexSpawn(command, args = [], options = {}) {
  return globalThis.__neovexAsyncHostValue('op_neovex_runtime_test_spawn', {
    command: String(command),
    args: args.map((value) => String(value)),
    cwd: typeof options?.cwd === 'string' ? options.cwd : null,
    env: encodeNeovexAsyncSpawnEnv(options),
  });
}

function canUseNeovexFork(modulePath, args = [], options = {}) {
  return typeof globalThis.__neovexAsyncHostValue === 'function' &&
    (typeof modulePath === 'string' || modulePath instanceof URL) &&
    Array.isArray(args) &&
    (options == null || typeof options === 'object') &&
    options.shell !== true &&
    options.signal === undefined &&
    options.timeout === undefined &&
    options.uid === undefined &&
    options.gid === undefined &&
    (options.cwd === undefined || typeof options.cwd === 'string') &&
    (options.execPath === undefined || String(options.execPath) === process.execPath) &&
    (options.execArgv === undefined ||
      (Array.isArray(options.execArgv) &&
        options.execArgv.every((value) => typeof value === 'string'))) &&
    options.serialization === undefined &&
    (options.stdio === undefined || options.stdio === 'pipe');
}

function terminateNeovexForkWorkers() {
  for (const worker of neovexForkWorkers) {
    void worker.terminate();
  }
  neovexForkWorkers.clear();
}

function installNeovexForkExitCleanup() {
  if (process[neovexForkExitCleanupInstalled] === true) {
    return;
  }

  if (typeof process.reallyExit === 'function') {
    const originalReallyExit = process.reallyExit.bind(process);
    process.reallyExit = function neovexHarnessReallyExit(code) {
      terminateNeovexForkWorkers();
      return originalReallyExit(code);
    };
  } else {
    process.once('exit', () => {
      terminateNeovexForkWorkers();
    });
  }

  Object.defineProperty(process, neovexForkExitCleanupInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

function installClusterShim() {
  let cluster;
  try {
    cluster = require('node:cluster');
  } catch {
    return;
  }

  if (cluster[neovexClusterShimInstalled] === true || cluster.isPrimary !== true) {
    return;
  }

  const originalDisconnect = cluster.Worker?.prototype?.disconnect;
  const originalFork = typeof cluster.fork === 'function'
    ? cluster.fork.bind(cluster)
    : null;
  if (typeof originalDisconnect !== 'function') {
    return;
  }

  const patchedWorkers = new WeakSet();
  const patchWorkerLifecycle = (worker) => {
    if (!worker?.process || patchedWorkers.has(worker)) {
      return worker;
    }
    patchedWorkers.add(worker);
    if (typeof worker.process.prependListener === 'function') {
      worker.process.prependListener('listening', (address) => {
        worker.state = 'listening';
        worker.emit('listening', address);
        cluster.emit('listening', worker, address);
      });
    }
    if (typeof worker.process.prependListener === 'function') {
      worker.process.prependListener('disconnect', () => {
        worker.exitedAfterDisconnect = !!worker.exitedAfterDisconnect;
        worker.state = 'disconnected';
      });
      worker.process.prependListener('exit', () => {
        worker.exitedAfterDisconnect = !!worker.exitedAfterDisconnect;
        worker.state = 'dead';
      });
    }
    return worker;
  };

  cluster.Worker.prototype.disconnect = function neovexHarnessClusterDisconnect() {
    if (this.process?.connected && typeof this.process.__neovexClusterDisconnect === 'function') {
      this.exitedAfterDisconnect = true;
      this.process.__neovexClusterDisconnect();
      return this;
    }
    return originalDisconnect.apply(this, arguments);
  };

  if (originalFork) {
    cluster.fork = function neovexHarnessClusterFork() {
      return patchWorkerLifecycle(originalFork.apply(this, arguments));
    };
  }

  Object.defineProperty(cluster, neovexClusterShimInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

function createNeovexForkChildProcess(modulePath, args = [], options = {}) {
  const { EventEmitter } = require('node:events');
  const { Worker } = require('node:worker_threads');
  const child = new EventEmitter();
  child.pid = 0;
  child.killed = false;
  child.connected = true;
  child.exitCode = null;
  child.signalCode = null;
  child.stdin = null;
  child.stdout = null;
  child.stderr = null;
  let resolveCompletion;
  child.__neovexCompletion = new Promise((resolve) => {
    resolveCompletion = resolve;
  });
  const trackedCompletion = child.__neovexCompletion.finally(() => {
    neovexForkWorkerCompletions.delete(trackedCompletion);
  });
  neovexForkWorkerCompletions.add(trackedCompletion);

  const workerBootstrap = `
    const { EventEmitter } = require("node:events");
    const { parentPort, workerData } = require("node:worker_threads");
    const workerThreads = require("node:worker_threads");
    const ipc = new EventEmitter();
    const processObject = require("node:process");
    const requireFromChild = require("node:module").createRequire(workerData.modulePath);
    const neovexListeningNotified = Symbol.for("neovex.nodeCompatForkListeningNotified");

    const originalEmit = processObject.emit.bind(processObject);
    const originalOn = processObject.on.bind(processObject);
    const originalOnce = processObject.once.bind(processObject);
    const originalOff = typeof processObject.off === "function"
      ? processObject.off.bind(processObject)
      : null;
    const originalRemoveListener = processObject.removeListener.bind(processObject);
    const syncIpcRefState = () => {
      if (
        typeof parentPort.ref !== "function" ||
        typeof parentPort.unref !== "function"
      ) {
        return;
      }
      if (processObject.connected !== false && ipc.listenerCount("message") > 0) {
        parentPort.ref();
      } else {
        parentPort.unref();
      }
    };

    processObject.argv.length = 0;
    processObject.argv.push(workerData.execPath, workerData.modulePath, ...workerData.args);
    if (Array.isArray(processObject.execArgv)) {
      processObject.execArgv.length = 0;
      processObject.execArgv.push(...workerData.execArgv);
    } else {
      processObject.execArgv = [...workerData.execArgv];
    }
    processObject.execPath = workerData.execPath;
    processObject.connected = true;
    processObject.exitCode = null;
    const patchForkWorkerThreadView = (target) => {
      if (!target || typeof target !== "object") {
        return;
      }
      try {
        Object.defineProperties(target, {
          isMainThread: {
            value: true,
            configurable: true,
            enumerable: true,
            writable: true,
          },
          parentPort: {
            value: null,
            configurable: true,
            enumerable: true,
            writable: true,
          },
          threadId: {
            value: 0,
            configurable: true,
            enumerable: true,
            writable: true,
          },
        });
      } catch {
        try {
          target.isMainThread = true;
          target.parentPort = null;
          target.threadId = 0;
        } catch {
          // Best-effort only; the emulated fork child just needs to stop
          // presenting as a worker when fixtures probe worker_threads.
        }
      }
    };
    patchForkWorkerThreadView(workerThreads);
    patchForkWorkerThreadView(workerThreads.default);
    try {
      const net = require("node:net");
      const originalServerListen = net.Server.prototype.listen;
      net.Server.prototype.listen = function neovexForkServerListen() {
        if (!this[neovexListeningNotified]) {
          this[neovexListeningNotified] = true;
          this.once("listening", () => {
            let address = null;
            try {
              address = typeof this.address === "function" ? this.address() : null;
            } catch {
              address = null;
            }
            try {
              parentPort.postMessage({ type: "listening", value: address });
            } catch {
              // Best-effort only.
            }
          });
        }
        return originalServerListen.apply(this, arguments);
      };
    } catch {
      // Best-effort only.
    }
    processObject.exit = function exit(code) {
      if (code !== undefined) {
        processObject.exitCode = code;
      }
      const exitCode = processObject.exitCode == null
        ? 0
        : Number(processObject.exitCode);
      if (processObject.connected) {
        processObject.connected = false;
        try {
          parentPort.postMessage({ __neovexType: "disconnect" });
        } catch {
          // Best-effort only; the parent can still observe a terminated worker.
        }
      }
      if (!processObject._exiting) {
        processObject._exiting = true;
        originalEmit("exit", exitCode);
      }
      try {
        parentPort.postMessage({
          __neovexType: "exit",
          code: exitCode,
        });
      } catch {
        // Best-effort only; the parent can still observe a terminated worker.
      }
    };
    processObject.reallyExit = processObject.exit;
    processObject.send = function send(message) {
      parentPort.postMessage({ type: "message", value: message });
      return true;
    };
    processObject.disconnect = function disconnect() {
      if (!processObject.connected) {
        return;
      }
      processObject.connected = false;
      try {
        parentPort.postMessage({ __neovexType: "disconnect" });
      } catch {
        // Best-effort only.
      }
      originalEmit("disconnect");
      syncIpcRefState();
    };
    processObject.on = function on(name, listener) {
      if (name === "message") {
        ipc.on(name, listener);
        syncIpcRefState();
        return processObject;
      }
      return originalOn(name, listener);
    };
    processObject.once = function once(name, listener) {
      if (name === "message") {
        ipc.once(name, listener);
        syncIpcRefState();
        return processObject;
      }
      return originalOnce(name, listener);
    };
    processObject.off = function off(name, listener) {
      if (name === "message") {
        ipc.off(name, listener);
        syncIpcRefState();
        return processObject;
      }
      if (originalOff) {
        return originalOff(name, listener);
      }
      return processObject;
    };
    processObject.removeListener = function removeListener(name, listener) {
      if (name === "message") {
        ipc.removeListener(name, listener);
        syncIpcRefState();
        return processObject;
      }
      return originalRemoveListener(name, listener);
    };

    for (const key of Object.keys(processObject.env)) {
      delete processObject.env[key];
    }
    for (const [key, value] of Object.entries(workerData.env)) {
      processObject.env[key] = value;
    }
    if (typeof workerData.cwd === "string" && workerData.cwd.length > 0) {
      processObject.chdir(workerData.cwd);
    }

    ipc.on("removeListener", (name) => {
      if (name === "message") {
        syncIpcRefState();
      }
    });

    parentPort.on("message", (message) => {
      if (message && message.__neovexType === "clusterDisconnect") {
        try {
          const cluster = require("node:cluster");
          if (cluster?.isWorker && cluster.worker) {
            cluster.worker.exitedAfterDisconnect = true;
            cluster.worker.state = "disconnecting";
          }
        } catch {
          // Best-effort only.
        }
        processObject.disconnect();
        return;
      }
      if (message && message.__neovexType === "disconnect") {
        processObject.disconnect();
        return;
      }
      ipc.emit("message", message);
    });
    syncIpcRefState();
    requireFromChild(workerData.modulePath);
    parentPort.postMessage({ type: "online" });
  `;

  const env =
    options?.env && typeof options.env === 'object'
      ? Object.fromEntries(
        Object.entries(options.env)
          .filter(([key, value]) => typeof key === 'string' && value != null)
          .map(([key, value]) => [key, String(value)]),
      )
      : Object.fromEntries(
        Object.entries(process.env)
          .filter(([key, value]) => typeof key === 'string' && value != null)
          .map(([key, value]) => [key, String(value)]),
      );
  const execArgv = Array.isArray(options?.execArgv)
    ? options.execArgv.map((value) => String(value))
    : Array.isArray(process.execArgv)
      ? process.execArgv.map((value) => String(value))
      : [];
  const execPath = typeof options?.execPath === 'string' ? options.execPath : process.execPath;
  const cwd = typeof options?.cwd === 'string' ? options.cwd : null;

  const worker = new Worker(workerBootstrap, {
    eval: true,
    workerData: {
      modulePath: String(modulePath),
      args: args.map((value) => String(value)),
      cwd,
      env,
      execArgv,
      execPath,
    },
  });
  neovexForkWorkers.add(worker);
  let requestedExitCode = null;
  let requestedSignalCode = null;

  worker.once('online', () => {
    child.pid = process.pid;
  });
  worker.on('message', (message) => {
    if (message?.__neovexType === 'disconnect') {
      if (child.connected) {
        child.connected = false;
        child.emit('disconnect');
      }
    } else if (message?.__neovexType === 'exit') {
      requestedExitCode = Number.isInteger(message.code) ? message.code : 0;
      void worker.terminate();
    } else if (message?.type === 'online') {
      child.emit('online');
    } else if (message?.type === 'listening') {
      child.emit('listening', message.value ?? null);
    } else if (message?.type === 'message') {
      child.emit('message', message.value);
    }
  });
  worker.once('error', (error) => {
    neovexForkWorkers.delete(worker);
    child.connected = false;
    child.exitCode = 1;
    child.signalCode = null;
    resolveCompletion?.({
      code: 1,
      signal: null,
      error,
    });
    child.emit('error', error);
  });
  worker.once('exit', (code) => {
    neovexForkWorkers.delete(worker);
    child.connected = false;
    child.exitCode = requestedSignalCode == null ? (requestedExitCode ?? code) : null;
    child.signalCode = requestedSignalCode;
    resolveCompletion?.({
      code: child.exitCode,
      signal: child.signalCode,
    });
    child.emit('exit', child.exitCode, child.signalCode);
    child.emit('close', child.exitCode, child.signalCode);
  });

  child.send = function send(message) {
    worker.postMessage(message);
    return true;
  };
  child.kill = function kill(signal = 'SIGTERM') {
    this.killed = true;
    requestedSignalCode = typeof signal === 'string' && signal.length > 0 ? signal : 'SIGTERM';
    requestedExitCode = null;
    if (this.connected) {
      this.connected = false;
      child.emit('disconnect');
    }
    neovexForkWorkers.delete(worker);
    void worker.terminate();
    return true;
  };
  child.disconnect = function disconnect() {
    if (!this.connected) {
      return;
    }
    worker.postMessage({ __neovexType: 'disconnect' });
  };
  child.__neovexClusterDisconnect = function __neovexClusterDisconnect() {
    if (!this.connected) {
      return;
    }
    worker.postMessage({ __neovexType: 'clusterDisconnect' });
  };

  return child;
}

function createNeovexAsyncChildProcess(command, args = [], options = {}) {
  const { EventEmitter } = require('node:events');
  const { PassThrough } = require('node:stream');
  const child = new EventEmitter();
  child.pid = 0;
  child.killed = false;
  child.stdin = null;
  const pipedStdio = options?.stdio === undefined || options?.stdio === 'pipe';
  child.stdout = pipedStdio ? new PassThrough() : null;
  child.stderr = pipedStdio ? new PassThrough() : null;
  child.kill = function kill() {
    this.killed = true;
    return true;
  };

  child.__neovexCompletion = (async () => {
    try {
      const result = await runNeovexSpawn(command, args, options);
      child.pid = typeof result?.pid === 'number' ? result.pid : 0;
      if (options?.stdio === 'inherit') {
        if (typeof result?.stdout === 'string' && result.stdout.length > 0) {
          process.stdout.write(result.stdout);
        }
        if (typeof result?.stderr === 'string' && result.stderr.length > 0) {
          process.stderr.write(result.stderr);
        }
      } else {
        if (child.stdout && typeof result?.stdout === 'string' && result.stdout.length > 0) {
          child.stdout.write(result.stdout);
        }
        if (child.stderr && typeof result?.stderr === 'string' && result.stderr.length > 0) {
          child.stderr.write(result.stderr);
        }
      }
      child.stdout?.end();
      child.stderr?.end();
      const signal = result?.signal ?? null;
      const code = typeof result?.code === 'number' ? result.code : 1;
      child.emit('exit', code, signal);
      child.emit('close', code, signal);
      return result;
    } catch (error) {
      child.stdout?.end();
      child.stderr?.end();
      child.emit('error', error);
      throw error;
    }
  })();

  return child;
}

function createNeovexExecFileError(command, args, result) {
  const stderr = result?.stderr ?? '';
  const error = new Error(
    `Command failed: ${command}${args.length > 0 ? ` ${args.join(' ')}` : ''}\n${stderr}`
  );
  error.code = typeof result?.code === 'number' ? result.code : 1;
  error.killed = false;
  error.signal = result?.signal ?? null;
  error.cmd = `${command}${args.length > 0 ? ` ${args.join(' ')}` : ''}`;
  return error;
}

function installChildProcessShim() {
  let childProcess;
  try {
    childProcess = require('node:child_process');
  } catch {
    return;
  }

  if (childProcess[neovexChildProcessShimInstalled] === true) {
    return;
  }

  installNeovexForkExitCleanup();

  const originalSpawnSync = childProcess.spawnSync;
  const originalExecFileSync = childProcess.execFileSync;
  const originalSpawn = childProcess.spawn;
  const originalExecFile = childProcess.execFile;
  const originalFork = childProcess.fork;
  childProcess.spawnSync = function neovexHarnessSpawnSync(command, args, options) {
    if (canUseNeovexSpawnSync(command, args, options)) {
      return runNeovexSpawnSync(command, args, options);
    }
    return originalSpawnSync.apply(this, arguments);
  };
  childProcess.execFileSync = function neovexHarnessExecFileSync(command, args, options) {
    if (canUseNeovexSpawnSync(command, args, options)) {
      const result = runNeovexSpawnSync(command, args, options);
      if (result.status === 0) {
        return result.stdout;
      }
      const error = new Error(result.stderr.toString());
      error.status = result.status;
      error.signal = result.signal;
      error.stdout = result.stdout;
      error.stderr = result.stderr;
      throw error;
    }
    return originalExecFileSync.apply(this, arguments);
  };
  childProcess.spawn = function neovexHarnessSpawn(command, args, options) {
    if (canUseNeovexAsyncSpawn(command, args, options)) {
      return createNeovexAsyncChildProcess(command, args, options);
    }
    return originalSpawn.apply(this, arguments);
  };
  childProcess.execFile = function neovexHarnessExecFile(
    command,
    argsOrOptionsOrCallback,
    optionsOrCallback,
    maybeCallback,
  ) {
    let args = [];
    let options = {};
    let callback;

    if (Array.isArray(argsOrOptionsOrCallback)) {
      args = argsOrOptionsOrCallback;
    } else if (typeof argsOrOptionsOrCallback === 'function') {
      callback = argsOrOptionsOrCallback;
    } else if (argsOrOptionsOrCallback != null) {
      options = argsOrOptionsOrCallback;
    }

    if (callback === undefined) {
      if (typeof optionsOrCallback === 'function') {
        callback = optionsOrCallback;
      } else if (optionsOrCallback != null) {
        options = optionsOrCallback;
        callback = maybeCallback;
      }
    }

    if (canUseNeovexAsyncSpawn(command, args, options)) {
      const child = createNeovexAsyncChildProcess(command, args, options);
      if (typeof callback === 'function') {
        child.once('close', async () => {
          const result = await child.__neovexCompletion;
          const stdout = result?.stdout ?? '';
          const stderr = result?.stderr ?? '';
          if ((result?.code ?? 1) === 0 && result?.signal == null) {
            callback(null, stdout, stderr);
          } else {
            callback(createNeovexExecFileError(command, args, result), stdout, stderr);
          }
        });
        child.once('error', (error) => callback(error));
      }
      return child;
    }

    return originalExecFile.apply(this, arguments);
  };
  childProcess.fork = function neovexHarnessFork(modulePath, argsOrOptions, maybeOptions) {
    let args = [];
    let options = {};

    if (Array.isArray(argsOrOptions)) {
      args = argsOrOptions;
      if (maybeOptions != null) {
        options = maybeOptions;
      }
    } else if (argsOrOptions != null) {
      options = argsOrOptions;
    }

    if (canUseNeovexFork(modulePath, args, options)) {
      return createNeovexForkChildProcess(modulePath, args, options);
    }
    return originalFork.apply(this, arguments);
  };

  Object.defineProperty(childProcess, neovexChildProcessShimInstalled, {
    value: true,
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

installChildProcessShim();
installClusterShim();

const isMainThread = (() => {
  try {
    return require('node:worker_threads').isMainThread;
  } catch {
    return true;
  }
})();

function spawnPromisified(command, args = [], options = {}) {
  if (typeof globalThis.__neovexAsyncHostValue !== 'function') {
    return Promise.reject(
      new Error('Neovex node_compat harness is missing __neovexAsyncHostValue')
    );
  }

  return globalThis.__neovexAsyncHostValue('op_neovex_runtime_test_spawn', {
    command: String(command),
    args: Array.isArray(args) ? args.map((value) => String(value)) : [],
    cwd: typeof options?.cwd === 'string' ? options.cwd : null,
  });
}

module.exports = {
  hasCrypto,
  hasOpenSSL,
  hasSQLite,
  hasIntl: typeof Intl === 'object' && typeof Intl.DateTimeFormat === 'function',
  isDumbTerminal: process.env.TERM === 'dumb',
  isAIX,
  isIBMi,
  isMacOS: process.platform === 'darwin',
  isRiscv64,
  isWindows,
  isAlive,
  localIPv6Hosts,
  mustCall,
  mustSucceed,
  mustCallAtLeast,
  mustNotCall,
  mustNotMutateObjectDeep,
  platformTimeout,
  printSkipMessage,
  skip,
  skipIf32Bits,
  skipIfDumbTerminal,
  skipIfInspectorDisabled,
  skipIfSQLiteMissing,
  skipIfWorker,
  invalidArgTypeHelper,
  expectWarning,
  expectsError,
  getArrayBufferViews,
  getBufferSources,
  allowGlobals,
  canCreateSymLink,
  runWithInvalidFD,
  isMainThread,
  PIPE,
  spawnPromisified,
  __neovexFlushForkWorkers: flushNeovexForkWorkers,
  get localhostIPv4() {
    if (localhostIPv4 === null) {
      localhostIPv4 = '127.0.0.1';
    }
    return localhostIPv4;
  },
  get enoughTestMem() {
    try {
      return require('node:v8').getHeapStatistics().heap_size_limit > 0x70000000;
    } catch {
      return true;
    }
  },
  get hasFipsCrypto() {
    try {
      return hasCrypto && require('node:crypto').getFips() === 1;
    } catch {
      return false;
    }
  },
  get hasOpenSSL3() {
    return hasOpenSSL(3);
  },
  __neovexAssert: runCallChecks,
};
