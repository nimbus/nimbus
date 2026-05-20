export type SystemStatusDoc = {
  _id?: string;
  name?: string;
  version?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: Record<string, unknown> | null;
} | null;

export type BundleDoc = {
  _id?: string;
  _creationTime?: number;
  sha256?: string;
  sizeBytes?: number;
  sourceRef?: string;
  status?: string;
};

export type FunctionDoc = {
  _id?: string;
  bundleId?: string;
  path?: string;
  kind?: string;
  argsSchema?: unknown;
  returnsSchema?: unknown;
};

export type AdapterCapabilityDoc = {
  _id?: string;
  adapter?: string;
  feature?: string;
  status?: string;
  caveat?: string;
  evidence?: string;
};

export type LicenseSnapshot = {
  source?: { kind?: string; path?: string };
  kind?: string;
  status?: string;
  issued_to?: string | null;
  issued_by?: string | null;
  issued_at_unix_ms?: number | null;
  expires_at_unix_ms?: number | null;
  trial_expires_at_unix_ms?: number | null;
  revenue_limit_usd?: number | null;
  monthly_active_user_limit?: number | null;
  entitlements?: Record<string, unknown>;
  usage?: {
    month?: string;
    monthly_active_users?: number;
    limit?: number | null;
    limit_exceeded?: boolean | null;
    last_recorded_at_unix_ms?: number | null;
  };
  warnings?: string[];
};

export type EncryptionStatus = {
  enabled?: boolean;
  encrypted_families?: string[];
  descriptor?: Record<string, unknown> | null;
};

export type RuntimeDiagnostics = {
  limits?: {
    runtime_backend?: string;
    compatibility_target?: string;
    execution_model?: string;
    runtime_mode?: string;
    runtime_language?: string;
    runtime_preset?: string;
    runtime_pool_kind?: string;
    max_heap_mb?: number;
    initial_heap_mb?: number;
    execution_timeout_ms?: number;
    max_concurrent_runtime_instances?: number;
    worker_threads?: number;
    max_active_top_level_invocations_per_tenant?: number;
    max_in_flight_top_level_invocations_per_tenant?: number;
    max_queued_top_level_invocations_per_tenant?: number;
    max_nested_runtime_invocations?: number;
  };
  metrics?: Record<string, unknown>;
};

export type AsyncSnapshot<T> = T | "loading" | "error";
