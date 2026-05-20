import { useEffect, useState } from "react";
import type {
  AsyncSnapshot,
  EncryptionStatus,
  LicenseSnapshot,
  RuntimeDiagnostics,
} from "./types";

export function useLicenseSnapshot(): AsyncSnapshot<LicenseSnapshot> {
  const [license, setLicense] =
    useState<AsyncSnapshot<LicenseSnapshot>>("loading");
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch("/debug/license/status", {
          credentials: "include",
        });
        if (!res.ok) throw new Error(`license ${res.status}`);
        const body = (await res.json()) as LicenseSnapshot;
        if (!cancelled) setLicense(body);
      } catch {
        if (!cancelled) setLicense("error");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);
  return license;
}

export function useEncryptionStatus(): AsyncSnapshot<EncryptionStatus> {
  const [encryption, setEncryption] =
    useState<AsyncSnapshot<EncryptionStatus>>("loading");
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch("/debug/encryption/status", {
          credentials: "include",
        });
        if (!res.ok) throw new Error(`encryption ${res.status}`);
        const body = (await res.json()) as EncryptionStatus;
        if (!cancelled) setEncryption(body);
      } catch {
        if (!cancelled) setEncryption("error");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);
  return encryption;
}

export function useRuntimeDiagnostics(): AsyncSnapshot<RuntimeDiagnostics> {
  const [diagnostics, setDiagnostics] =
    useState<AsyncSnapshot<RuntimeDiagnostics>>("loading");
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch("/debug/runtime/metrics", {
          credentials: "include",
        });
        if (!res.ok) throw new Error(`metrics ${res.status}`);
        const body = (await res.json()) as RuntimeDiagnostics;
        if (!cancelled) setDiagnostics(body);
      } catch {
        if (!cancelled) setDiagnostics("error");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);
  return diagnostics;
}
