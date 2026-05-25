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

export type RuntimeTenantBudget = {
  max_active_runtime_slots?: number;
  max_in_flight_top_level_invocations?: number;
  max_queued_top_level_invocations?: number;
  max_worker_thread_slots?: number;
  max_heap_mb_per_runtime?: number;
  memory_enforcement?: string;
  max_active_heap_mb?: number;
  execution_timeout_ms?: number;
  max_nested_runtime_invocations_per_top_level?: number;
};

export type RuntimeLimits = {
  runtime_backend?: string;
  runtime_backend_trust_tier?: string;
  runtime_backend_lockdown_profile?: string;
  runtime_backend_lifecycle_policy?: string;
  bundle_content_kind?: string;
  javascript_evaluation_format?: string;
  compatibility_target?: string;
  execution_model?: string;
  runtime_mode?: string;
  runtime_language?: string;
  runtime_preset?: string;
  runtime_grants?: Record<string, unknown>;
  runtime_pool_kind?: string;
  memory_enforcement?: string;
  module_state_semantics?: string;
  routing_affinity?: string;
  routing_affinity_max_entries?: number;
  max_warm_pool_entries_per_worker?: number;
  max_warm_reuses?: number;
  max_heap_mb?: number;
  initial_heap_mb?: number;
  execution_timeout_ms?: number;
  max_concurrent_runtime_instances?: number;
  worker_threads?: number;
  max_active_top_level_invocations_per_tenant?: number;
  max_in_flight_top_level_invocations_per_tenant?: number;
  max_queued_top_level_invocations_per_tenant?: number;
  max_nested_runtime_invocations?: number;
  tenant_budget?: RuntimeTenantBudget;
};

export type RuntimeResetCapabilities = {
  op_state_per_invocation?: boolean;
  bootstrap_state_per_invocation?: boolean;
  user_module_state_per_invocation?: boolean;
};

export type RuntimeExecutionAdapterExpectedArtifact = {
  kind?: string;
  schema_version?: number;
  source_repository?: string;
  source_ref?: string;
  source_revision?: string;
  target_triple?: string;
  platform?: string;
  manifest_file?: string;
  library_file?: string;
  readme_file?: string;
  abi_name?: string;
  abi_version?: number;
  memory_enforcement?: string;
  lifecycle?: string;
  proof_target?: string;
  simdutf_namespace?: string;
  required_export_count?: number;
};

export type RuntimeExecutionAdapterManifestArtifact = {
  adapter_version?: string;
  nimbus_version?: string;
  source_repository?: string;
  source_ref?: string;
  source_revision?: string;
  target_triple?: string;
  platform?: string;
  library_file?: string;
  library_sha256?: string;
  abi_name?: string;
  abi_version?: number;
  checksum_file?: string;
  sbom?: string;
  slsa?: string;
};

export type RuntimeExecutionAdapterArtifactDiagnostics = {
  status?: string;
  source?: string;
  reason_code?: string;
  install_hint?: string | null;
  expected?: RuntimeExecutionAdapterExpectedArtifact | null;
  manifest?: RuntimeExecutionAdapterManifestArtifact | null;
};

export type RuntimeLaneDiagnostics = {
  lane_name: string;
  default_lane?: boolean;
  executor_started?: boolean;
  execution_adapter_state?: string;
  execution_adapter_artifact?: RuntimeExecutionAdapterArtifactDiagnostics;
  limits?: RuntimeLimits;
  reset_capabilities?: RuntimeResetCapabilities;
  metrics?: Record<string, unknown>;
};

export type RuntimeDiagnostics = {
  limits?: RuntimeLimits | null;
  reset_capabilities?: RuntimeResetCapabilities | null;
  metrics?: Record<string, unknown> | null;
  lanes?: RuntimeLaneDiagnostics[];
};

export type AsyncSnapshot<T> = T | "loading" | "error";
