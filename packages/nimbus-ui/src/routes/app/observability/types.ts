export type ActiveObservabilityTab = "logs" | "runs";

export type ObservabilitySearch = {
  tab?: ActiveObservabilityTab;
  level?: string;
  category?: string;
  source?: string;
  correlationId?: string;
  status?: string;
  functionPath?: string;
  follow?: boolean;
  pauseOnError?: boolean;
};

export type EventDoc = {
  _id: string;
  _creationTime?: number;
  source?: string;
  level?: string;
  category?: string;
  message?: string;
  data?: Record<string, unknown> | null;
  correlationId?: string | null;
  createdAt?: number;
};

export type RunDoc = {
  _id: string;
  _creationTime?: number;
  bundleId?: string;
  functionPath?: string;
  kind?: string;
  durationMs?: number;
  status?: string;
  error?: unknown;
  startedAt?: number;
};

export function parseTab(value: unknown): ActiveObservabilityTab | undefined {
  return value === "logs" || value === "runs" ? value : undefined;
}

export function parseString(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed.length === 0 ? undefined : trimmed;
}

export function parseBool(value: unknown): boolean | undefined {
  if (value === true || value === "1" || value === "true") return true;
  if (value === false || value === "0" || value === "false") return false;
  return undefined;
}
