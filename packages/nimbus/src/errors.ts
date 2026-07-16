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

export type NimbusCommitErrorDetail = Readonly<Record<string, unknown>>;

export type NimbusCommitErrorInit = {
  code: string;
  detail: NimbusCommitErrorDetail;
  retryability: NimbusRetryability;
  retryAfterMs?: number;
};

export class NimbusCommitError extends Error {
  readonly kind: NimbusCommitErrorKind;
  readonly code: string;
  readonly retryability: NimbusRetryability;
  readonly retryable: boolean;
  readonly retryAfterMs?: number;
  readonly detail: NimbusCommitErrorDetail;

  protected constructor(
    name: string,
    kind: NimbusCommitErrorKind,
    message: string,
    init: NimbusCommitErrorInit,
  ) {
    super(message);
    this.name = name;
    this.kind = kind;
    this.code = init.code;
    this.retryability = init.retryability;
    this.retryable = init.retryability !== "terminal";
    this.retryAfterMs = init.retryAfterMs;
    this.detail = init.detail;
  }
}

export class NimbusConflictError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
    super("NimbusConflictError", "conflict", message, init);
  }
}

export class NimbusOverloadedError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
    super("NimbusOverloadedError", "overloaded", message, init);
  }
}

export class NimbusCommitterFullError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
    super("NimbusCommitterFullError", "committer_full", message, init);
  }
}

export class NimbusRejectedBeforeExecutionError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
    super(
      "NimbusRejectedBeforeExecutionError",
      "rejected_before_execution",
      message,
      init,
    );
  }
}

export class NimbusRateLimitedError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
    super("NimbusRateLimitedError", "rate_limited", message, init);
  }
}

export class NimbusOutOfRetentionError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
    super("NimbusOutOfRetentionError", "out_of_retention", message, init);
  }
}

export class NimbusCapExceededError extends NimbusCommitError {
  constructor(message: string, init: NimbusCommitErrorInit) {
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
  retryable?: unknown;
  detail?: unknown;
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
  if (!(code in constructors)) {
    return new Error(message);
  }

  const detail = isRecord(encoded.detail) ? encoded.detail : {};
  const retryability = decodeRetryability(
    detail.retryability,
    code as CommitErrorCode,
    encoded.retryable === true,
  );
  const retryAfterMs = finiteNonNegativeNumber(detail.retryAfterMs);
  const Constructor = constructors[code as CommitErrorCode];
  return new Constructor(message, {
    code,
    detail,
    retryability,
    ...(retryAfterMs === undefined ? {} : { retryAfterMs }),
  });
}

function decodeRetryability(
  value: unknown,
  code: CommitErrorCode,
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
