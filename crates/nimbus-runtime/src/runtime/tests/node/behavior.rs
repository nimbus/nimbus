const COMMON_INDEX_FIXTURE: &str = include_str!("../node_compat_fixtures/test/common/index.js");
const COMMON_FIXTURES_FIXTURE: &str =
    include_str!("../node_compat_fixtures/test/common/fixtures.js");
const COMMON_TMPDIR_FIXTURE: &str = include_str!("../node_compat_fixtures/test/common/tmpdir.js");
const PROCESS_LIFECYCLE_DRAIN_POSTLUDE: &str = r#"
  if (typeof globalThis.process?.emit === "function") {
    globalThis.process.emit("beforeExit", globalThis.process.exitCode ?? 0);
    await Promise.resolve();
    await new Promise((resolve) => queueMicrotask(resolve));
    if (typeof globalThis.process?.nextTick === "function") {
      await new Promise((resolve) => globalThis.process.nextTick(resolve));
    }
    globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
  }
"#;
const PROCESS_BEFORE_EXIT_REENTRY_POSTLUDE: &str = r#"
  if (typeof globalThis.process?.emit === "function") {
    const __nimbusInitialExitCode = globalThis.process.exitCode ?? 0;
    const __nimbusDeadline = Date.now() + 1000;
    for (let __nimbusPass = 0; ; __nimbusPass += 1) {
      globalThis.process.emit(
        "beforeExit",
        globalThis.process.exitCode ?? __nimbusInitialExitCode,
      );
      if (typeof globalThis.__nimbusProcessTicksAndRejections === "function") {
        globalThis.__nimbusProcessTicksAndRejections();
      }
      await Promise.resolve();
      await new Promise((resolve) => queueMicrotask(resolve));
      if (typeof globalThis.process?.nextTick === "function") {
        await new Promise((resolve) => globalThis.process.nextTick(resolve));
      }
      const __nimbusHasMoreWork =
        typeof globalThis.__nimbusEventLoopHasMoreWork === "function" &&
        globalThis.__nimbusEventLoopHasMoreWork() === true;
      if (!__nimbusHasMoreWork) {
        break;
      }
      if (Date.now() >= __nimbusDeadline) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    globalThis.process.emit(
      "exit",
      globalThis.process.exitCode ?? __nimbusInitialExitCode,
    );
  }
"#;
const FORK_CHILD_SETTLE_POSTLUDE: &str = r#"
  if (typeof common.__nimbusFlushForkWorkers === "function") {
    await common.__nimbusFlushForkWorkers();
  }
"#;
const NODE_COMPAT_PROCESS_EXIT_SENTINEL_MARKER: &str = "__NIMBUS_NODE_COMPAT_PROCESS_EXIT__";
const PROCESS_EXIT_SENTINEL_PRELUDE: &str = r#"
import { createRequire as __nimbusCreateRequireForProcessExit } from "node:module";

const __nimbusRequireForProcessExit =
  __nimbusCreateRequireForProcessExit(import.meta.url);
const __nimbusProcessExitSentinel = "__NIMBUS_NODE_COMPAT_PROCESS_EXIT__";

if (globalThis.process && typeof globalThis.process.reallyExit === "function") {
  globalThis.process.reallyExit = (code) => {
    const resolvedCode = code ?? globalThis.process.exitCode ?? 0;
    globalThis.process.exitCode = resolvedCode;
    __nimbusRequireForProcessExit("./test/common/index.js").__nimbusAssert?.();
    const exitError = new Error(`${__nimbusProcessExitSentinel}:${resolvedCode}`);
    exitError.code = "NIMBUS_NODE_COMPAT_PROCESS_EXIT";
    throw exitError;
  };
}
"#;

const INTERACTIVE_TERMINAL_PRELUDE: &str = r#"
globalThis.__nimbusNodeCompatTerm = "xterm-256color";
if (globalThis.process?.env) {
  const originalEnv = globalThis.process.env;
  Object.defineProperty(globalThis.process, "env", {
    value: new Proxy(originalEnv, {
      get(target, property, receiver) {
        if (property === "TERM") {
          return "xterm-256color";
        }
        return Reflect.get(target, property, receiver);
      },
      has(target, property) {
        if (property === "TERM") {
          return true;
        }
        return Reflect.has(target, property);
      },
    }),
    configurable: true,
    enumerable: true,
    writable: false,
  });
}
"#;

const DNS_RESULT_ORDER_IPV4FIRST_PRELUDE: &str = r#"
import dns from "node:dns";
dns.setDefaultResultOrder("ipv4first");
"#;

const DNS_RESULT_ORDER_IPV6FIRST_PRELUDE: &str = r#"
import dns from "node:dns";
dns.setDefaultResultOrder("ipv6first");
"#;

const DNS_RESULT_ORDER_VERBATIM_PRELUDE: &str = r#"
import dns from "node:dns";
dns.setDefaultResultOrder("verbatim");
"#;

const EXPOSE_GC_PRELUDE: &str = "void 0;";
const CHECKOUT_ROOT_CWD_PRELUDE: &str = r#"
{
  const __nimbusPreludeRequire = createRequire(import.meta.url);
  const __nimbusPreludeFs = __nimbusPreludeRequire("node:fs");
  const __nimbusPreludePath = __nimbusPreludeRequire("node:path");
  const __nimbusPreludeUrl = __nimbusPreludeRequire("node:url");
  const __nimbusCompatBundleRoot = __nimbusPreludePath.dirname(
    __nimbusPreludeUrl.fileURLToPath(import.meta.url),
  );
  __nimbusPreludeFs.mkdirSync(
    __nimbusPreludePath.join(__nimbusCompatBundleRoot, "lib"),
    { recursive: true },
  );
  if (typeof globalThis.process?.chdir === "function") {
    globalThis.process.chdir(__nimbusCompatBundleRoot);
  }
}
"#;
const PENDING_DEPRECATION_PRELUDE: &str = r#"
{
  const __nimbusNodeProcess = createRequire(import.meta.url)("node:process");
  const existingNodeOptions =
    typeof __nimbusNodeProcess.env.NODE_OPTIONS === "string"
      ? __nimbusNodeProcess.env.NODE_OPTIONS
      : "";
  if (!existingNodeOptions.split(/\s+/u).includes("--pending-deprecation")) {
    __nimbusNodeProcess.env.NODE_OPTIONS = existingNodeOptions.trim().length === 0
      ? "--pending-deprecation"
      : `${existingNodeOptions} --pending-deprecation`;
  }
  globalThis.process ??= __nimbusNodeProcess;
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeCompatNamedPreludeBehavior {
    ProcessExitSentinel,
    InteractiveTerminal,
    DnsResultOrderIpv4First,
    DnsResultOrderIpv6First,
    DnsResultOrderVerbatim,
    ExposeGc,
    CheckoutRootCwd,
    PendingDeprecation,
}

impl NodeCompatNamedPreludeBehavior {
    pub(super) const ALL: [Self; 8] = [
        Self::ProcessExitSentinel,
        Self::InteractiveTerminal,
        Self::DnsResultOrderIpv4First,
        Self::DnsResultOrderIpv6First,
        Self::DnsResultOrderVerbatim,
        Self::ExposeGc,
        Self::CheckoutRootCwd,
        Self::PendingDeprecation,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::ProcessExitSentinel => "process_exit_sentinel",
            Self::InteractiveTerminal => "interactive_terminal",
            Self::DnsResultOrderIpv4First => "dns_result_order_ipv4first",
            Self::DnsResultOrderIpv6First => "dns_result_order_ipv6first",
            Self::DnsResultOrderVerbatim => "dns_result_order_verbatim",
            Self::ExposeGc => "expose_gc",
            Self::CheckoutRootCwd => "checkout_root_cwd",
            Self::PendingDeprecation => "pending_deprecation",
        }
    }

    pub(super) fn phase(self) -> &'static str {
        "prelude"
    }

    pub(super) fn selection_mode(self) -> &'static str {
        match self {
            Self::PendingDeprecation => "implicit_fixture_flag",
            _ => "default_fixture_mapping",
        }
    }

    fn script(self) -> &'static str {
        match self {
            Self::ProcessExitSentinel => PROCESS_EXIT_SENTINEL_PRELUDE,
            Self::InteractiveTerminal => INTERACTIVE_TERMINAL_PRELUDE,
            Self::DnsResultOrderIpv4First => DNS_RESULT_ORDER_IPV4FIRST_PRELUDE,
            Self::DnsResultOrderIpv6First => DNS_RESULT_ORDER_IPV6FIRST_PRELUDE,
            Self::DnsResultOrderVerbatim => DNS_RESULT_ORDER_VERBATIM_PRELUDE,
            Self::ExposeGc => EXPOSE_GC_PRELUDE,
            Self::CheckoutRootCwd => CHECKOUT_ROOT_CWD_PRELUDE,
            Self::PendingDeprecation => PENDING_DEPRECATION_PRELUDE,
        }
    }

    fn from_script(script: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|behavior| behavior.script() == script)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeCompatNamedPostludeBehavior {
    ProcessLifecycleDrain,
    ProcessBeforeExitReentry,
    ForkChildSettle,
}

impl NodeCompatNamedPostludeBehavior {
    pub(super) const ALL: [Self; 3] = [
        Self::ProcessLifecycleDrain,
        Self::ProcessBeforeExitReentry,
        Self::ForkChildSettle,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::ProcessLifecycleDrain => "process_lifecycle_drain",
            Self::ProcessBeforeExitReentry => "process_before_exit_reentry",
            Self::ForkChildSettle => "fork_child_settle",
        }
    }

    pub(super) fn phase(self) -> &'static str {
        "postlude"
    }

    pub(super) fn selection_mode(self) -> &'static str {
        "default_fixture_mapping"
    }

    fn script(self) -> &'static str {
        match self {
            Self::ProcessLifecycleDrain => PROCESS_LIFECYCLE_DRAIN_POSTLUDE,
            Self::ProcessBeforeExitReentry => PROCESS_BEFORE_EXIT_REENTRY_POSTLUDE,
            Self::ForkChildSettle => FORK_CHILD_SETTLE_POSTLUDE,
        }
    }

    fn from_script(script: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|behavior| behavior.script() == script)
    }
}

