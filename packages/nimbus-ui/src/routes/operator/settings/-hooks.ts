import { useApiRead } from "../../../hooks/use-api-read";
import type { LoadingValue } from "../../../shell/loading-value";
import type {
  EncryptionStatus,
  LicenseSnapshot,
  RuntimeDiagnostics,
} from "./-types";

// The settings page's three one-shot debug reads. Each is a `useApiRead` over
// its `/debug/*` endpoint, reporting the console's shared `LoadingValue<T>`.
export function useLicenseSnapshot(): LoadingValue<LicenseSnapshot> {
  return useApiRead<LicenseSnapshot>("/debug/license/status", []);
}

export function useEncryptionStatus(): LoadingValue<EncryptionStatus> {
  return useApiRead<EncryptionStatus>("/debug/encryption/status", []);
}

export function useRuntimeDiagnostics(): LoadingValue<RuntimeDiagnostics> {
  return useApiRead<RuntimeDiagnostics>("/debug/runtime/metrics", []);
}
