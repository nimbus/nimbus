import {
  type ConnectionState,
  NimbusProvider,
  type NimbusReactClient,
  useNimbusConnectionState,
  useQueries,
  useQuery,
} from "nimbus/react";
import { createElement } from "react";

import { api } from "../convex/_generated/api";

export function SystemQueryProof(props: { client: NimbusReactClient }) {
  return createElement(
    NimbusProvider,
    { client: props.client },
    createElement(SystemQueryProbe),
  );
}

function SystemQueryProbe() {
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 50,
  });
  const scheduledJobs = useQuery(api.scheduled_jobs.list, {
    tenantId: null,
    status: null,
    limit: 50,
  });
  const listeners = useQuery(api.listeners.list, {
    adapter: null,
    state: null,
    limit: 50,
  });
  const adapterCapabilities = useQuery(api.adapter_capabilities.list, {
    adapter: null,
    status: null,
    limit: 50,
  });
  const systemStatus = useQuery(api.system.status, {});
  const overview = useQueries({
    machines: {
      query: api.machines.list,
      args: { state: null, provider: null, limit: 10 },
    },
    listeners: {
      query: api.listeners.list,
      args: { adapter: null, state: null, limit: 10 },
    },
    status: {
      query: api.system.status,
      args: {},
    },
  });
  const connectionState = useNimbusConnectionState();

  expectOptionalList(machines);
  expectOptionalList(scheduledJobs);
  expectOptionalList(listeners);
  expectOptionalList(adapterCapabilities);
  expectOptionalSystemStatus(systemStatus);
  expectOptionalList(overview.machines);
  expectOptionalList(overview.listeners);
  expectOptionalSystemStatus(overview.status);
  expectConnectionState(connectionState);
  return null;
}

function expectOptionalList(value: readonly unknown[] | Error | undefined) {
  if (value instanceof Error) {
    throw value;
  }
  return value?.length ?? 0;
}

function expectOptionalSystemStatus(value: unknown | null | undefined) {
  return value ?? null;
}

function expectConnectionState(value: ConnectionState) {
  return value.isWebSocketConnected || value.hasInflightRequests;
}
