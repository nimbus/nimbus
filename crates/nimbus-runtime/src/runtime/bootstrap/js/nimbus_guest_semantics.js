// Convex default-runtime guest semantics controller.
//
// Installed (post-bootstrap) only when the runtime contract carries
// `guest_semantics: "convex_default"`. It reshapes the guest-visible
// time/randomness surface to the Convex default runtime contract:
//
// - `Math.random()` is a seeded ChaCha20-based PRNG: seeded from the bundle
//   deploy stamp during module evaluation (so module-scope values are stable
//   across runs and restarts) and re-seeded with fresh per-invocation entropy
//   at the start of every query/paginated_query/mutation.
// - System time (`Date.now()`, `new Date()`) is the deploy timestamp during
//   module evaluation and is frozen at invocation start for the whole of a
//   query/paginated_query/mutation handler. Actions see the host clock.
// - `performance.now()` is fixed during import and query execution,
//   increments inside mutations and actions.
// - `performance.timeOrigin` is pinned to the deploy timestamp for all
//   invocation kinds.
//
// This file only DEFINES the controller (it is executed during startup
// snapshot creation); activation happens via
// `__nimbusInstallGuestSemantics(contract)` from the post-bootstrap step, and
// phase transitions via `__nimbusEnterGuestImportPhase` (driver, before
// module evaluation) and `__nimbusBeginGuestInvocation` (invoke expression
// prelude). All three are safe no-ops on Host-semantics lanes.
{
  const __nimbusGuestState = {
    installed: false,
    // null => host clock; number => frozen wall-clock ms for Date.now/new Date().
    frozenNowMs: null,
    // null => host Math.random; function => seeded PRNG stream.
    prng: null,
    // Deploy timestamp pinned as performance.timeOrigin (null until known).
    deployTsMs: null,
    // true => performance.now() is fixed at 0 (import/query); false => host.
    perfFixed: false,
  };

  const __nimbusGuestHexToBytes = function __nimbusGuestHexToBytes(hex) {
    const normalized = typeof hex === "string" && hex.length >= 2 ? hex : "00";
    const bytes = new Uint8Array(32);
    for (let index = 0; index < 32; index++) {
      const pair = normalized.substr((index * 2) % normalized.length, 2);
      bytes[index] = Number.parseInt(pair, 16) || 0;
    }
    return bytes;
  };

  // Compact ChaCha20 block generator; "strong" seeded PRNG per the Convex
  // default runtime contract (guest-observable stream, not a secrecy
  // boundary — the seed lives in the same trust domain as the guest).
  const __nimbusGuestChaCha20 = function __nimbusGuestChaCha20(keyBytes) {
    const rotl = (value, count) =>
      ((value << count) | (value >>> (32 - count))) >>> 0;
    const key = new Uint32Array(8);
    for (let index = 0; index < 8; index++) {
      key[index] =
        (keyBytes[index * 4] |
          (keyBytes[index * 4 + 1] << 8) |
          (keyBytes[index * 4 + 2] << 16) |
          (keyBytes[index * 4 + 3] << 24)) >>>
        0;
    }
    const input = new Uint32Array(16);
    const output = new Uint32Array(16);
    let counterLo = 0;
    let counterHi = 0;
    let blockIndex = 16;
    const quarterRound = (s, a, b, c, d) => {
      s[a] = (s[a] + s[b]) >>> 0;
      s[d] = rotl(s[d] ^ s[a], 16);
      s[c] = (s[c] + s[d]) >>> 0;
      s[b] = rotl(s[b] ^ s[c], 12);
      s[a] = (s[a] + s[b]) >>> 0;
      s[d] = rotl(s[d] ^ s[a], 8);
      s[c] = (s[c] + s[d]) >>> 0;
      s[b] = rotl(s[b] ^ s[c], 7);
    };
    const refill = () => {
      input[0] = 0x61707865;
      input[1] = 0x3320646e;
      input[2] = 0x79622d32;
      input[3] = 0x6b206574;
      for (let index = 0; index < 8; index++) {
        input[4 + index] = key[index];
      }
      input[12] = counterLo;
      input[13] = counterHi;
      input[14] = 0;
      input[15] = 0;
      output.set(input);
      for (let round = 0; round < 10; round++) {
        quarterRound(output, 0, 4, 8, 12);
        quarterRound(output, 1, 5, 9, 13);
        quarterRound(output, 2, 6, 10, 14);
        quarterRound(output, 3, 7, 11, 15);
        quarterRound(output, 0, 5, 10, 15);
        quarterRound(output, 1, 6, 11, 12);
        quarterRound(output, 2, 7, 8, 13);
        quarterRound(output, 3, 4, 9, 14);
      }
      for (let index = 0; index < 16; index++) {
        output[index] = (output[index] + input[index]) >>> 0;
      }
      counterLo = (counterLo + 1) >>> 0;
      if (counterLo === 0) {
        counterHi = (counterHi + 1) >>> 0;
      }
      blockIndex = 0;
    };
    const nextU32 = () => {
      if (blockIndex >= 16) {
        refill();
      }
      return output[blockIndex++];
    };
    return function random() {
      // 53-bit uniform double in [0, 1).
      const hi = nextU32() >>> 5;
      const lo = nextU32() >>> 6;
      return (hi * 67108864 + lo) / 9007199254740992;
    };
  };

  const __nimbusGuestMarkTimerFunction = function __nimbusGuestMarkTimerFunction(fn) {
    // Carry the side-channel hardening marker so a later hardening pass never
    // re-wraps the dispatcher (a frozen or coarsened-base clock is already at
    // least as coarse as the hardening contract requires).
    Object.defineProperty(fn, "__nimbusCoarsenedTimer", {
      configurable: false,
      enumerable: false,
      value: true,
      writable: false,
    });
    return fn;
  };

  const __nimbusInstallGuestSemantics = function __nimbusInstallGuestSemantics(contract) {
    if (__nimbusGuestState.installed) {
      return;
    }
    if (!contract || contract.guest_semantics !== "convex_default") {
      return;
    }
    __nimbusGuestState.installed = true;

    // Bases are captured AFTER side-channel hardening, so host-mode falls
    // back to the coarsened clocks rather than the raw natives.
    const hostMathRandom = Math.random.bind(Math);
    const NativeDate = Date;
    const hostDateNow = Date.now.bind(Date);

    Object.defineProperty(Math, "random", {
      configurable: true,
      enumerable: false,
      writable: true,
      value: function random() {
        const prng = __nimbusGuestState.prng;
        return prng !== null ? prng() : hostMathRandom();
      },
    });

    const dispatchedDateNow = __nimbusGuestMarkTimerFunction(function now() {
      const frozen = __nimbusGuestState.frozenNowMs;
      return frozen !== null ? frozen : hostDateNow();
    });
    Object.defineProperty(NativeDate, "now", {
      configurable: true,
      enumerable: false,
      writable: true,
      value: dispatchedDateNow,
    });
    // `new Date()` (and `Date()`) must observe the same frozen clock. The
    // proxy keeps NativeDate.prototype as the instance prototype, so
    // platform-created Date objects still satisfy `instanceof Date`.
    const DateProxy = new Proxy(NativeDate, {
      construct(target, args, newTarget) {
        if (args.length === 0 && __nimbusGuestState.frozenNowMs !== null) {
          return Reflect.construct(
            target,
            [__nimbusGuestState.frozenNowMs],
            newTarget === DateProxy ? target : newTarget,
          );
        }
        return Reflect.construct(
          target,
          args,
          newTarget === DateProxy ? target : newTarget,
        );
      },
      apply(target, _thisArg, _args) {
        if (__nimbusGuestState.frozenNowMs !== null) {
          return new target(__nimbusGuestState.frozenNowMs).toString();
        }
        return target();
      },
    });
    Object.defineProperty(globalThis, "Date", {
      configurable: true,
      enumerable: false,
      writable: true,
      value: DateProxy,
    });

    const performanceValue = globalThis.performance;
    if (performanceValue !== null && typeof performanceValue === "object") {
      const hostPerformanceNow =
        typeof performanceValue.now === "function"
          ? performanceValue.now.bind(performanceValue)
          : () => 0;
      const hostTimeOrigin = performanceValue.timeOrigin;
      Object.defineProperty(performanceValue, "now", {
        configurable: true,
        enumerable: false,
        writable: true,
        value: __nimbusGuestMarkTimerFunction(function now() {
          return __nimbusGuestState.perfFixed ? 0 : hostPerformanceNow();
        }),
      });
      Object.defineProperty(performanceValue, "timeOrigin", {
        configurable: true,
        enumerable: false,
        get() {
          const deploy = __nimbusGuestState.deployTsMs;
          return deploy !== null ? deploy : hostTimeOrigin;
        },
      });
    }
  };
  // All three controller entry points are host-invoked by name (post
  // bootstrap, driver import-phase script, and the invoke-expression
  // prelude). They are non-writable and non-configurable so guest code can
  // neither replace them (which would let a query keep import-phase state or
  // re-enable time/randomness nondeterminism) nor define its own on lanes
  // where the host would then call it.
  Object.defineProperty(globalThis, "__nimbusInstallGuestSemantics", {
    configurable: false,
    enumerable: false,
    writable: false,
    value: Object.freeze(__nimbusInstallGuestSemantics),
  });

  // Driver-invoked, immediately before module evaluation on ConvexDefault
  // lanes: module-scope code observes the deploy-stamped clock and the
  // deploy-seeded PRNG, making import-time values stable across runs.
  Object.defineProperty(globalThis, "__nimbusEnterGuestImportPhase", {
    configurable: false,
    enumerable: false,
    writable: false,
    value: Object.freeze(function __nimbusEnterGuestImportPhase(stamp) {
      if (!__nimbusGuestState.installed || !stamp || typeof stamp !== "object") {
        return;
      }
      const deployTsMs = Number(stamp.deploy_ts_ms);
      __nimbusGuestState.deployTsMs = Number.isFinite(deployTsMs) ? deployTsMs : 0;
      __nimbusGuestState.frozenNowMs = __nimbusGuestState.deployTsMs;
      __nimbusGuestState.prng = __nimbusGuestChaCha20(
        __nimbusGuestHexToBytes(stamp.deploy_seed_hex),
      );
      __nimbusGuestState.perfFixed = true;
    }),
  });

  // Invoke-expression prelude: reconfigures the surface for the invocation
  // that is about to run (frozen clock + fresh seed for deterministic kinds,
  // host clock for actions).
  Object.defineProperty(globalThis, "__nimbusBeginGuestInvocation", {
    configurable: false,
    enumerable: false,
    writable: false,
    value: Object.freeze(function __nimbusBeginGuestInvocation() {
      if (!__nimbusGuestState.installed) {
        return;
      }
      const descriptor = __nimbusCoreOps.op_nimbus_runtime_invocation_determinism();
      if (!descriptor || descriptor.enabled !== true) {
        __nimbusGuestState.frozenNowMs = null;
        __nimbusGuestState.prng = null;
        __nimbusGuestState.perfFixed = false;
        return;
      }
      const kind = descriptor.kind;
      if (kind === "query" || kind === "paginated_query" || kind === "mutation") {
        __nimbusGuestState.frozenNowMs = descriptor.now_ms;
        __nimbusGuestState.prng = __nimbusGuestChaCha20(
          __nimbusGuestHexToBytes(descriptor.seed_hex),
        );
        __nimbusGuestState.perfFixed = kind !== "mutation";
      } else {
        __nimbusGuestState.frozenNowMs = null;
        __nimbusGuestState.prng = null;
        __nimbusGuestState.perfFixed = false;
      }
    }),
  });
}
