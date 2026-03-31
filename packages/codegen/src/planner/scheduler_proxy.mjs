import { recordOperation } from "./request_bindings.mjs";
import {
  normalizeCallableFunctionReference,
  normalizeScheduledCancel,
  normalizeScheduledRunAfter,
  normalizeScheduledRunAt,
} from "./function_references.mjs";

function createSchedulerProxy(filePath, operationLog, returnResultMarker = false) {
  return {
    runAfter(delayMs, functionRef, args = {}) {
      return recordOperation(
        operationLog,
        normalizeScheduledRunAfter(delayMs, functionRef, args, filePath),
        returnResultMarker,
      );
    },
    runAt(timestampMs, functionRef, args = {}) {
      return recordOperation(
        operationLog,
        normalizeScheduledRunAt(timestampMs, functionRef, args, filePath),
        returnResultMarker,
      );
    },
    cancel(jobId) {
      return recordOperation(
        operationLog,
        normalizeScheduledCancel(jobId, filePath),
        returnResultMarker,
      );
    },
  };
}

export { createSchedulerProxy, normalizeCallableFunctionReference };
