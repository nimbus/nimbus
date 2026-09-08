import { Button } from "../../../components/ui/button";
import type { ServiceDoc } from "../../../lib/types/service";
import {
  ACTION_LABELS,
  actionsForState,
  OPTIMISTIC_STATES,
  type ServiceActions,
} from "./-service-lifecycle";

// The lifecycle buttons a detail header shows for one service: the actions
// its current state allows, disabled while a request is in flight, with
// the last refusal under them. Both detail pages share it so the operator
// and developer surfaces answer the same state the same way.
export function LifecycleButtons({
  service,
  actions,
  testid,
}: {
  service: ServiceDoc;
  actions: ServiceActions;
  testid: string;
}) {
  const inFlight = actions.pending[service._id];
  const shownState = inFlight ? OPTIMISTIC_STATES[inFlight] : service.state;
  const allowed = actionsForState(shownState);
  const error = actions.errors[service._id];
  return (
    <div className="flex flex-col items-end gap-1" data-testid={testid}>
      <div className="flex items-center gap-1.5">
        {allowed.map((action) => (
          <Button
            key={action}
            type="button"
            variant={action === "stop" ? "outline" : "default"}
            size="sm"
            disabled={inFlight !== undefined}
            onClick={() => void actions.runAction(service, action)}
            data-testid={`${testid}-${action}`}
          >
            {ACTION_LABELS[action]}
          </Button>
        ))}
        {allowed.length === 0 && inFlight ? (
          <span className="text-xs text-text-3">
            {ACTION_LABELS[inFlight]} in flight…
          </span>
        ) : null}
      </div>
      {error ? (
        <span
          className="max-w-md text-right text-xs text-error"
          data-testid={`${testid}-error`}
        >
          {error}
        </span>
      ) : null}
    </div>
  );
}

// The state a header pill shows: the optimistic state while a request is
// in flight, else the row's own.
export function shownStateOf(
  service: ServiceDoc,
  actions: ServiceActions,
): string | undefined {
  const inFlight = actions.pending[service._id];
  return inFlight ? OPTIMISTIC_STATES[inFlight] : service.state;
}
