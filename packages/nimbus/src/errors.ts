export type NimbusRetryability =
  | "retryable"
  | "retryable_after_backoff"
  | "restart_transaction"
  | "terminal";

export type NimbusCommitErrorKind =
  | "conflict"
  | "overloaded"
  | "committer_full"
  | "rejected_before_execution"
  | "rate_limited"
  | "out_of_retention"
  | "cap_exceeded";

/** Severity as the server's error envelope reports it. */
export type NimbusErrorSeverity = "fatal" | "error" | "warning";

/** The remediation a server error carries when it can name a fix. */
export type NimbusRemediation = {
  readonly action: string;
  readonly message: string;
};

export type NimbusErrorDetail = Readonly<Record<string, unknown>>;

export type NimbusErrorInit = {
  code: string;
  detail: NimbusErrorDetail;
  retryability: NimbusRetryability;
  retryAfterMs?: number;
  requestId?: string;
  severity?: NimbusErrorSeverity;
  timestamp?: string;
  remediation?: NimbusRemediation;
};

/**
 * Any error decoded from a Nimbus error envelope, whatever its code.
 *
 * The envelope carries a stable code, a request id, retry metadata, structured
 * detail, and — when the server can name a fix — a remediation block. This
 * class keeps all of it, so an unrecognized code stays as diagnosable as a
 * recognized one. Codes on the commit path decode further, into the
 * {@link NimbusCommitError} subclass that matches the code.
 */
export class NimbusError extends Error {
  readonly code: string;
  readonly retryability: NimbusRetryability;
  readonly retryable: boolean;
  readonly retryAfterMs?: number;
  readonly requestId?: string;
  readonly severity?: NimbusErrorSeverity;
  readonly timestamp?: string;
  readonly remediation?: NimbusRemediation;
  readonly detail: NimbusErrorDetail;

  constructor(message: string, init: NimbusErrorInit) {
    super(message);
    this.name = "NimbusError";
    this.code = init.code;
    this.retryability = init.retryability;
    this.retryable = init.retryability !== "terminal";
    this.retryAfterMs = init.retryAfterMs;
    this.requestId = init.requestId;
    this.severity = init.severity;
    this.timestamp = init.timestamp;
    this.remediation = init.remediation;
    this.detail = init.detail;
  }
}

export class NimbusCommitError extends NimbusError {
  readonly kind: NimbusCommitErrorKind;

  protected constructor(
    name: string,
    kind: NimbusCommitErrorKind,
    message: string,
    init: NimbusErrorInit,
  ) {
    super(message, init);
    this.name = name;
    this.kind = kind;
  }
}

export class NimbusConflictError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super("NimbusConflictError", "conflict", message, init);
  }
}

export class NimbusOverloadedError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super("NimbusOverloadedError", "overloaded", message, init);
  }
}

export class NimbusCommitterFullError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super("NimbusCommitterFullError", "committer_full", message, init);
  }
}

export class NimbusRejectedBeforeExecutionError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super(
      "NimbusRejectedBeforeExecutionError",
      "rejected_before_execution",
      message,
      init,
    );
  }
}

export class NimbusRateLimitedError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super("NimbusRateLimitedError", "rate_limited", message, init);
  }
}

export class NimbusOutOfRetentionError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super("NimbusOutOfRetentionError", "out_of_retention", message, init);
  }
}

export class NimbusCapExceededError extends NimbusCommitError {
  constructor(message: string, init: NimbusErrorInit) {
    super("NimbusCapExceededError", "cap_exceeded", message, init);
  }
}

export type NimbusCommitPathError =
  | NimbusConflictError
  | NimbusOverloadedError
  | NimbusCommitterFullError
  | NimbusRejectedBeforeExecutionError
  | NimbusRateLimitedError
  | NimbusOutOfRetentionError
  | NimbusCapExceededError;

type EnvelopeError = {
  code?: unknown;
  message?: unknown;
  requestId?: unknown;
  timestamp?: unknown;
  severity?: unknown;
  retryable?: unknown;
  detail?: unknown;
  remediation?: unknown;
};

const constructors = {
  "op.conflict": NimbusConflictError,
  OCC: NimbusConflictError,
  "rate.overloaded": NimbusOverloadedError,
  Overloaded: NimbusOverloadedError,
  "rate.committer_full": NimbusCommitterFullError,
  CommitterFullError: NimbusCommitterFullError,
  "rate.rejected_before_execution": NimbusRejectedBeforeExecutionError,
  RejectedBeforeExecution: NimbusRejectedBeforeExecutionError,
  "rate.limited": NimbusRateLimitedError,
  RateLimited: NimbusRateLimitedError,
  "op.out_of_retention": NimbusOutOfRetentionError,
  OutOfRetention: NimbusOutOfRetentionError,
  "op.cap_exceeded": NimbusCapExceededError,
  PaginationLimit: NimbusCapExceededError,
} as const;

type CommitErrorCode = keyof typeof constructors;

export function isNimbusError(error: unknown): error is NimbusError {
  return error instanceof NimbusError;
}

export function isNimbusCommitError(error: unknown): error is NimbusCommitPathError {
  return error instanceof NimbusCommitError;
}

/** Decode Nimbus's `{ error: { ... } }` HTTP envelope into a typed error. */
export function decodeNimbusErrorEnvelope(
  response: unknown,
  fallback = "Nimbus request failed",
): Error {
  if (typeof response === "string" && response.length > 0) {
    return new Error(response);
  }
  if (!isRecord(response) || !("error" in response)) {
    if (isRecord(response) && typeof response.message === "string") {
      return new Error(response.message || fallback);
    }
    return new Error(fallback);
  }
  if (typeof response.error === "string") {
    return new Error(response.error || fallback);
  }
  if (!isRecord(response.error)) {
    return new Error(fallback);
  }

  const encoded = response.error as EnvelopeError;
  const message = typeof encoded.message === "string" ? encoded.message : fallback;
  const code = typeof encoded.code === "string" ? encoded.code : "";
  const detail = isRecord(encoded.detail) ? encoded.detail : {};
  const retryability = decodeRetryability(
    detail.retryability,
    code,
    encoded.retryable === true,
  );
  const retryAfterMs = finiteNonNegativeNumber(detail.retryAfterMs);
  const severity = decodeSeverity(encoded.severity);
  const remediation = decodeRemediation(encoded.remediation);
  const init: NimbusErrorInit = {
    code,
    detail,
    retryability,
    ...(retryAfterMs === undefined ? {} : { retryAfterMs }),
    ...(typeof encoded.requestId === "string" ? { requestId: encoded.requestId } : {}),
    ...(severity === undefined ? {} : { severity }),
    ...(typeof encoded.timestamp === "string" ? { timestamp: encoded.timestamp } : {}),
    ...(remediation === undefined ? {} : { remediation }),
  };

  // An unrecognized code is still a Nimbus error: it keeps its code, request
  // id, retry metadata, detail, and remediation instead of collapsing to a
  // bare message.
  if (!(code in constructors)) {
    return new NimbusError(message, init);
  }
  const Constructor = constructors[code as CommitErrorCode];
  return new Constructor(message, init);
}

function decodeSeverity(value: unknown): NimbusErrorSeverity | undefined {
  return value === "fatal" || value === "error" || value === "warning"
    ? value
    : undefined;
}

function decodeRemediation(value: unknown): NimbusRemediation | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const { action, message } = value;
  if (typeof action !== "string" || typeof message !== "string") {
    return undefined;
  }
  return { action, message };
}

function decodeRetryability(
  value: unknown,
  code: string,
  retryable: boolean,
): NimbusRetryability {
  if (
    value === "retryable" ||
    value === "retryable_after_backoff" ||
    value === "restart_transaction" ||
    value === "terminal"
  ) {
    return value;
  }
  if (code === "op.out_of_retention" || code === "OutOfRetention") {
    return "restart_transaction";
  }
  if (code === "op.cap_exceeded" || code === "PaginationLimit") {
    return "terminal";
  }
  if (
    code === "rate.overloaded" ||
    code === "Overloaded" ||
    code === "rate.committer_full" ||
    code === "CommitterFullError" ||
    code === "rate.limited" ||
    code === "RateLimited"
  ) {
    return "retryable_after_backoff";
  }
  return retryable ? "retryable" : "terminal";
}

function finiteNonNegativeNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
