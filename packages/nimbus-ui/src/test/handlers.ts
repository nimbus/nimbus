import { HttpResponse, http } from "msw";

import type {
  RuntimeDiagnostics,
  RuntimeLimits,
} from "../routes/operator/settings/-types";

export type TenantsResponse = { tenants: string[] };

export const defaultTenants: TenantsResponse = {
  tenants: ["demo", "staging"],
};

function runtimeLimits(overrides: Partial<RuntimeLimits> = {}): RuntimeLimits {
  const memoryEnforcement =
    overrides.memory_enforcement ?? "v8_isolate_heap_limit";
  return {
    runtime_backend: "v8",
    runtime_backend_trust_tier: "in_process_untrusted",
    runtime_backend_lockdown_profile: "v8_deno_core",
    runtime_backend_lifecycle_policy: "v8_deno_core_pool",
    bundle_content_kind: "javascript",
    javascript_evaluation_format: "es_module",
    compatibility_target: "node22",
    execution_model: "cooperative_locker",
    runtime_mode: "strict",
    runtime_language: "java_script",
    runtime_preset: "application",
    runtime_pool_kind: "warm_pool",
    memory_enforcement: memoryEnforcement,
    module_state_semantics: "warm_per_bundle",
    routing_affinity: "tenant",
    routing_affinity_max_entries: 512,
    max_warm_pool_entries_per_worker: 16,
    max_warm_reuses: 256,
    max_heap_mb: 128,
    initial_heap_mb: 8,
    execution_timeout_ms: 30_000,
    max_concurrent_runtime_instances: 16,
    worker_threads: 4,
    max_active_top_level_invocations_per_tenant: 8,
    max_in_flight_top_level_invocations_per_tenant: 16,
    max_queued_top_level_invocations_per_tenant: 64,
    max_nested_runtime_invocations: 64,
    ...overrides,
    tenant_budget: {
      max_active_runtime_slots: 8,
      max_in_flight_top_level_invocations: 16,
      max_queued_top_level_invocations: 64,
      max_worker_thread_slots: 4,
      max_heap_mb_per_runtime: overrides.max_heap_mb ?? 128,
      memory_enforcement: memoryEnforcement,
      max_active_heap_mb: (overrides.max_heap_mb ?? 128) * 8,
      execution_timeout_ms: overrides.execution_timeout_ms ?? 30_000,
      max_nested_runtime_invocations_per_top_level: 64,
      ...overrides.tenant_budget,
    },
  };
}

const v8AdapterArtifact = {
  status: "linked",
  source: "built_in",
  reason_code: "v8_builtin",
  install_hint: null,
  expected: null,
  manifest: null,
};

export const defaultRuntimeDiagnostics: RuntimeDiagnostics = {
  limits: runtimeLimits(),
  reset_capabilities: {
    op_state_per_invocation: true,
    bootstrap_state_per_invocation: true,
    user_module_state_per_invocation: false,
  },
  metrics: {
    worker_dispatched_invocations: 0,
    fallback_cross_runtime_dispatches: 0,
    retained_runtime_pool_entries: 0,
    tenants: {},
  },
  lanes: [
    {
      lane_name: "default",
      default_lane: true,
      executor_started: false,
      execution_adapter_state: "linked",
      execution_adapter_artifact: v8AdapterArtifact,
      limits: runtimeLimits({
        compatibility_target: "web_standard_isolate",
      }),
    },
    {
      lane_name: "node20",
      default_lane: false,
      executor_started: false,
      execution_adapter_state: "linked",
      execution_adapter_artifact: v8AdapterArtifact,
      limits: runtimeLimits({ compatibility_target: "node20" }),
    },
    {
      lane_name: "node22",
      default_lane: false,
      executor_started: false,
      execution_adapter_state: "linked",
      execution_adapter_artifact: v8AdapterArtifact,
      limits: runtimeLimits({ compatibility_target: "node22" }),
    },
    {
      lane_name: "node24",
      default_lane: false,
      executor_started: false,
      execution_adapter_state: "linked",
      execution_adapter_artifact: v8AdapterArtifact,
      limits: runtimeLimits({ compatibility_target: "node24" }),
    },
    {
      lane_name: "bun_jsc",
      default_lane: false,
      executor_started: false,
      execution_adapter_state: "not_linked",
      execution_adapter_artifact: {
        status: "not_linked",
        source: "build_feature_disabled",
        reason_code: "linked_adapter_feature_disabled",
        install_hint:
          "install the optional nimbus-bun-jsc-adapter package, set NIMBUS_BUN_JSC_ADAPTER_MANIFEST to a verified nimbus-bun-jsc-adapter.json, or set NIMBUS_BUN_EMBED_SHARED_LIBRARY for a development proof",
        expected: {
          kind: "nimbus.bun_jsc.adapter",
          schema_version: 1,
          source_repository: "https://github.com/nimbus/bun",
          source_ref: "bun-v1.4.2-nimbus.1",
          source_revision: "d6d4c5e39938b6c5ac243490a9230c26d52d737f",
          target_triple: "x86_64-unknown-linux-gnu",
          platform: "linux",
          manifest_file: "nimbus-bun-jsc-adapter.json",
          library_file: "libnimbus_bun_jsc_embedder.so",
          readme_file: "README.md",
          abi_name: "nimbus-bun-jsc-embedder",
          abi_version: 3,
          memory_enforcement: "outer_quota_required",
          lifecycle: "fresh_discard",
          proof_target: "check-bun-embed-shared",
          simdutf_namespace: "nimbus_bun_simdutf",
          required_export_count: 12,
        },
        manifest: null,
      },
      limits: runtimeLimits({
        runtime_backend: "bun_jsc",
        runtime_backend_lockdown_profile: "bun_jsc_in_process_untrusted",
        runtime_backend_lifecycle_policy:
          "bun_jsc_fresh_discard_pool_outer_quota_required",
        javascript_evaluation_format: "program_wrapper",
        compatibility_target: "bun_jsc",
        execution_model: "backend_owned_event_loop",
        runtime_pool_kind: "bun_jsc_fresh_discard",
        memory_enforcement: "outer_quota_required",
        module_state_semantics: "fresh_per_invocation",
        tenant_budget: {
          memory_enforcement: "outer_quota_required",
        },
      }),
    },
  ],
};

export const handlers = [
  http.get("*/api/tenants", () => HttpResponse.json(defaultTenants)),

  http.post("*/api/tenants", async ({ request }) => {
    const body = (await request.json()) as { id?: string };
    if (!body.id) {
      return HttpResponse.json(
        {
          error: {
            code: "validation.invalid",
            message: "id is required",
            requestId: "test-1",
            timestamp: new Date().toISOString(),
            severity: "error",
            retryable: false,
          },
        },
        { status: 400 },
      );
    }
    return HttpResponse.json({ id: body.id }, { status: 201 });
  }),

  http.delete("*/api/tenants/:id", () => HttpResponse.json({ ok: true })),

  http.get("*/debug/license/status", () =>
    HttpResponse.json({
      tier: "community",
      mauCap: 500,
      mauCurrent: 0,
    }),
  ),

  http.get("*/debug/encryption/status", () =>
    HttpResponse.json({ status: "ok", keyFingerprint: "fp_abc123" }),
  ),

  http.get("*/debug/runtime/metrics", () =>
    HttpResponse.json(defaultRuntimeDiagnostics),
  ),
];
