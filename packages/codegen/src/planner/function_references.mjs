import { unsupportedError } from "../errors.mjs";

import {
  normalizeDelayMs,
  normalizeJobId,
  normalizeRecord,
  normalizeTimestampMs,
} from "./shared.mjs";

function normalizeScheduledFunctionReference(functionRef, args, filePath) {
  if (!functionRef || typeof functionRef !== "object") {
    throw unsupportedError(filePath, "ctx.scheduler requires a generated mutation reference");
  }
  if (functionRef.kind !== "mutation") {
    throw unsupportedError(filePath, "ctx.scheduler only supports mutation references in 4B");
  }
  if (typeof functionRef.name !== "string" || functionRef.name.length === 0) {
    throw unsupportedError(filePath, "ctx.scheduler requires a named generated mutation reference");
  }

  return {
    name: functionRef.name,
    visibility: functionRef.visibility ?? "public",
    args: normalizeRecord(args, filePath, "ctx.scheduler args"),
  };
}

function normalizeCallableFunctionReference(
  functionRef,
  args,
  filePath,
  label,
  expectedKind,
) {
  if (!functionRef || typeof functionRef !== "object") {
    throw unsupportedError(filePath, `${label} requires a generated function reference`);
  }
  if (
    typeof functionRef.kind === "string" &&
    functionRef.kind !== expectedKind
  ) {
    throw unsupportedError(
      filePath,
      `${label} requires a ${expectedKind} reference, received ${functionRef.kind}`,
    );
  }
  if (typeof functionRef.name !== "string" || functionRef.name.length === 0) {
    throw unsupportedError(filePath, `${label} requires a named generated function reference`);
  }

  return {
    name: functionRef.name,
    visibility: functionRef.visibility ?? "public",
    args: normalizeRecord(args, filePath, `${label} args`),
  };
}

function normalizeScheduledRunAfter(delayMs, functionRef, args, filePath) {
  return {
    type: "schedule_run_after",
    delay_ms: normalizeDelayMs(delayMs, filePath),
    ...normalizeScheduledFunctionReference(functionRef, args, filePath),
  };
}

function normalizeScheduledRunAt(timestampMs, functionRef, args, filePath) {
  return {
    type: "schedule_run_at",
    timestamp_ms: normalizeTimestampMs(timestampMs, filePath),
    ...normalizeScheduledFunctionReference(functionRef, args, filePath),
  };
}

function normalizeScheduledCancel(jobId, filePath) {
  return {
    type: "schedule_cancel",
    job_id: normalizeJobId(jobId, filePath),
  };
}

export {
  normalizeCallableFunctionReference,
  normalizeScheduledCancel,
  normalizeScheduledRunAfter,
  normalizeScheduledRunAt,
};
