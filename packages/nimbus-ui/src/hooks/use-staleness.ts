import {
  createContext,
  createElement,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { toast } from "sonner";

import { fetchVersionInfo, type VersionInfo } from "../api/system";
import {
  type DesktopBridge,
  getDesktopBridge,
  isLocalHost,
} from "../lib/desktop-bridge";

export type StalenessState =
  | "hidden"
  | "available"
  | "confirming"
  | "upgrading"
  | "upgraded";

export type StalenessSnapshot = {
  state: StalenessState;
  info: VersionInfo | null;
  targetLatest: string | null;
  dismissed: boolean;
};

export type StalenessApi = {
  snapshot: StalenessSnapshot;
  openPopover: () => void;
  closePopover: () => void;
  startUpgrade: () => Promise<void>;
  copyCommand: () => Promise<void>;
  dismissToast: () => void;
  isLocal: boolean;
  hasDesktopBridge: boolean;
};

const DEFAULT_POLL_MS = 5 * 60 * 1000;
const ACTIVE_POLL_MS = 2_000;
const UPGRADE_TIMEOUT_MS = 10 * 60 * 1000;
const UPGRADED_HOLD_MS = 30_000;
const DISMISSED_KEY = "nimbus-ui:staleness-dismissed-version";

function readDismissedVersion(): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

function writeDismissedVersion(value: string | null) {
  if (typeof window === "undefined") return;
  try {
    if (value === null) window.localStorage.removeItem(DISMISSED_KEY);
    else window.localStorage.setItem(DISMISSED_KEY, value);
  } catch {
    // ignore
  }
}

function isLikelyAvailable(info: VersionInfo | null): boolean {
  if (!info) return false;
  if (!info.available) return false;
  if (info.checkStatus === "disabled" || info.checkStatus === "never") {
    return false;
  }
  return !!info.latest;
}

type UseStalenessDeps = {
  fetchInfo?: (signal?: AbortSignal) => Promise<VersionInfo>;
  defaultPollMs?: number;
  activePollMs?: number;
  upgradeTimeoutMs?: number;
  upgradedHoldMs?: number;
  bridge?: DesktopBridge | null;
  now?: () => number;
};

export function useStaleness(deps: UseStalenessDeps = {}): StalenessApi {
  const fetchInfo = deps.fetchInfo ?? fetchVersionInfo;
  const defaultPollMs = deps.defaultPollMs ?? DEFAULT_POLL_MS;
  const activePollMs = deps.activePollMs ?? ACTIVE_POLL_MS;
  const upgradeTimeoutMs = deps.upgradeTimeoutMs ?? UPGRADE_TIMEOUT_MS;
  const upgradedHoldMs = deps.upgradedHoldMs ?? UPGRADED_HOLD_MS;
  const bridge = useMemo(
    () => (deps.bridge === undefined ? getDesktopBridge() : deps.bridge),
    [deps.bridge],
  );

  const [state, setState] = useState<StalenessState>("hidden");
  const [info, setInfo] = useState<VersionInfo | null>(null);
  const [targetLatest, setTargetLatest] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<boolean>(false);
  const lastAnnouncedRef = useRef<string | null>(null);

  const stateRef = useRef(state);
  stateRef.current = state;
  const targetRef = useRef(targetLatest);
  targetRef.current = targetLatest;

  const isLocal = useMemo(() => isLocalHost(info?.host ?? null), [info?.host]);
  const hasDesktopBridge = bridge !== null;

  const poll = useCallback(
    async (signal: AbortSignal) => {
      try {
        const next = await fetchInfo(signal);
        if (signal.aborted) return;
        setInfo(next);

        // post-upgrade detection: if upgrading and the server now reports
        // current >= targetLatest, transition to upgraded.
        if (
          stateRef.current === "upgrading" &&
          targetRef.current &&
          next.current &&
          versionGte(next.current, targetRef.current)
        ) {
          setState("upgraded");
          toast.success(`Nimbus ${next.current} running`);
          return;
        }

        // available detection
        if (isLikelyAvailable(next)) {
          const latest = next.latest as string;
          if (
            stateRef.current === "hidden" ||
            stateRef.current === "upgraded"
          ) {
            setState("available");
          }
          const dismissedFor = readDismissedVersion();
          setDismissed(dismissedFor === latest);
          if (lastAnnouncedRef.current !== latest && dismissedFor !== latest) {
            lastAnnouncedRef.current = latest;
            toast(`Nimbus ${latest} available`, {
              description: `Update from ${next.current}.`,
              action: {
                label: "Update",
                onClick: () => {
                  setState("confirming");
                },
              },
              cancel: {
                label: "Dismiss",
                onClick: () => {
                  writeDismissedVersion(latest);
                  setDismissed(true);
                },
              },
              duration: Number.POSITIVE_INFINITY,
            });
          }
        } else if (
          stateRef.current === "available" ||
          stateRef.current === "confirming"
        ) {
          setState("hidden");
        }
      } catch (err) {
        if (signal.aborted) return;
        if ((err as Error).name === "AbortError") return;
        // keep prior state on transient fetch failures
      }
    },
    [fetchInfo],
  );

  // Background poll loop with cadence that depends on the current state.
  useEffect(() => {
    if (typeof window === "undefined") return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;

    const cadence = () =>
      stateRef.current === "upgrading" ? activePollMs : defaultPollMs;

    const tick = async () => {
      if (cancelled) return;
      await poll(controller.signal);
      if (cancelled) return;
      timer = setTimeout(tick, cadence());
    };

    void tick();

    return () => {
      cancelled = true;
      controller.abort();
      if (timer !== null) clearTimeout(timer);
    };
  }, [poll, defaultPollMs, activePollMs]);

  // Auto-clear the upgraded badge after the hold window.
  useEffect(() => {
    if (state !== "upgraded") return;
    if (typeof window === "undefined") return;
    const handle = setTimeout(() => {
      setState("hidden");
      setTargetLatest(null);
      lastAnnouncedRef.current = null;
    }, upgradedHoldMs);
    return () => clearTimeout(handle);
  }, [state, upgradedHoldMs]);

  // Safety net: if upgrading lasts longer than the timeout, revert.
  useEffect(() => {
    if (state !== "upgrading") return;
    if (typeof window === "undefined") return;
    const handle = setTimeout(() => {
      if (stateRef.current === "upgrading") {
        setState("available");
        toast.error("Upgrade not detected. Try again?");
      }
    }, upgradeTimeoutMs);
    return () => clearTimeout(handle);
  }, [state, upgradeTimeoutMs]);

  const openPopover = useCallback(() => {
    setState((s) =>
      s === "available" || s === "confirming" ? "confirming" : s,
    );
  }, []);

  const closePopover = useCallback(() => {
    setState((s) => (s === "confirming" ? "available" : s));
  }, []);

  const startUpgrade = useCallback(async () => {
    if (!info?.latest) return;
    if (!isLocal || !bridge) return;
    setTargetLatest(info.latest);
    setState("upgrading");
    try {
      for await (const event of bridge.runUpgrade(info.upgrade.method)) {
        if (event.kind === "error") {
          toast.error(`Upgrade failed: ${event.message}`);
          setState("available");
          return;
        }
        if (event.kind === "exit" && event.code !== 0) {
          toast.error(`Upgrade exited with code ${event.code}`);
          setState("available");
          return;
        }
      }
      // Bridge closed cleanly. The poll loop running at activePollMs cadence
      // will detect current >= targetLatest and flip to "upgraded".
    } catch (e) {
      toast.error(`Upgrade failed: ${(e as Error).message}`);
      setState("available");
    }
  }, [info, isLocal, bridge]);

  const copyCommand = useCallback(async () => {
    if (!info?.latest) return;
    if (!info.upgrade.command) return;
    setTargetLatest(info.latest);
    try {
      await navigator.clipboard.writeText(info.upgrade.command);
    } catch {
      toast.error("Copy failed. The clipboard is not available.");
      return;
    }
    setState("upgrading");
    const hostLabel = info.host || "the server host";
    toast(`Copied — run on ${hostLabel}`, { duration: 4000 });
  }, [info]);

  const dismissToast = useCallback(() => {
    if (!info?.latest) return;
    writeDismissedVersion(info.latest);
    setDismissed(true);
  }, [info]);

  const snapshot: StalenessSnapshot = {
    state,
    info,
    targetLatest,
    dismissed,
  };

  return {
    snapshot,
    openPopover,
    closePopover,
    startUpgrade,
    copyCommand,
    dismissToast,
    isLocal,
    hasDesktopBridge,
  };
}

const StalenessContext = createContext<StalenessApi | null>(null);

export function StalenessProvider({
  children,
  ...deps
}: UseStalenessDeps & { children: ReactNode }) {
  const api = useStaleness(deps);
  return createElement(StalenessContext.Provider, { value: api }, children);
}

export function useStalenessContext(): StalenessApi {
  const ctx = useContext(StalenessContext);
  if (!ctx) {
    throw new Error("useStalenessContext requires <StalenessProvider>");
  }
  return ctx;
}

export function versionGte(a: string, b: string): boolean {
  return compareSemver(a, b) >= 0;
}

function compareSemver(a: string, b: string): number {
  const pa = parseSemver(a);
  const pb = parseSemver(b);
  for (let i = 0; i < 3; i += 1) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

function parseSemver(v: string): [number, number, number] {
  const cleaned = v.replace(/^v/, "").split(/[+-]/)[0];
  const parts = cleaned.split(".").map((p) => Number.parseInt(p, 10));
  return [
    Number.isFinite(parts[0]) ? parts[0] : 0,
    Number.isFinite(parts[1]) ? parts[1] : 0,
    Number.isFinite(parts[2]) ? parts[2] : 0,
  ];
}
