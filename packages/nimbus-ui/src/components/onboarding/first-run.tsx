import { Check } from "lucide-react";

import { cn } from "@/lib/utils";
import { CopyButton } from "../copy-button";
import { Mascot } from "../mascot";

// FirstRun is the Overview panel a server shows before it has anything to
// report: no functions, no tables, no runs. It replaces the stats and the
// runs table, not the connect panel, because the three steps below are how
// those numbers start moving. Completion is live: the same queries that
// fill the stats decide which steps are done, so the panel retires itself
// the moment the first function runs.
export type FirstRunProgress = {
  // functions is the count of deployed functions on the server.
  functions: number;
  // runs is the count of function runs the server has recorded.
  runs: number;
};

type Step = {
  id: "install" | "dev" | "run";
  title: string;
  body: string;
  // snippet is the one command that completes the step, when one exists.
  snippet?: string;
  done: (progress: FirstRunProgress) => boolean;
};

const STEPS: ReadonlyArray<Step> = [
  {
    id: "install",
    title: "Install the CLI",
    body: "Homebrew on macOS and Linux. Other platforms are on the releases page.",
    snippet: "brew install nimbus/tap/nimbus",
    // The console cannot see a local install. A deployed function proves it.
    done: (p) => p.functions > 0,
  },
  {
    id: "dev",
    title: "Run nimbus dev in an app",
    body: "It generates types, deploys the functions in the app, and creates the tables its schema names.",
    snippet: "nimbus init convex my-app && cd my-app && nimbus dev",
    done: (p) => p.functions > 0,
  },
  {
    id: "run",
    title: "Call a function",
    body: "The first run from your app, the SDK, or the Compute page shows up in the stats and in Observability.",
    done: (p) => p.runs > 0,
  },
];

export function firstRunComplete(progress: FirstRunProgress): boolean {
  return STEPS.every((step) => step.done(progress));
}

export function FirstRun({
  progress,
  testid = "first-run",
  className,
}: {
  progress: FirstRunProgress;
  testid?: string;
  className?: string;
}) {
  const doneCount = STEPS.filter((step) => step.done(progress)).length;
  return (
    <section
      aria-labelledby={`${testid}-title`}
      className={cn(
        "grid grid-cols-1 gap-6 rounded-lg border border-border-2 bg-bg-panel p-6 md:grid-cols-[auto_1fr]",
        className,
      )}
      data-testid={testid}
    >
      <div className="flex flex-col items-center gap-3 text-center md:w-56">
        <Mascot size={72} state="empty" variant="solid" decorative />
        <h2
          id={`${testid}-title`}
          className="text-base font-medium text-text-1"
          data-testid={`${testid}-title`}
        >
          Nothing here yet
        </h2>
        <p className="text-sm text-text-3">
          Deploy an app and its functions, tables, and runs appear here.
        </p>
      </div>
      <ol className="flex flex-col gap-4" data-testid={`${testid}-steps`}>
        {STEPS.map((step, index) => {
          const done = step.done(progress);
          return (
            <li
              key={step.id}
              className="flex gap-3"
              data-testid={`${testid}-step-${step.id}`}
              data-done={done ? "true" : "false"}
            >
              <span
                aria-hidden
                className={cn(
                  "mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full border text-xs font-medium tabular-nums",
                  done
                    ? "border-success bg-success text-accent-ink"
                    : "border-border-2 text-text-3",
                )}
              >
                {done ? <Check className="size-3.5" /> : index + 1}
              </span>
              <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <span
                  className={cn(
                    "text-sm font-medium",
                    done ? "text-text-3 line-through" : "text-text-1",
                  )}
                >
                  {step.title}
                  <span className="sr-only">{done ? " (done)" : ""}</span>
                </span>
                <span className="text-xs text-text-3">{step.body}</span>
                {step.snippet && !done ? (
                  <span className="flex max-w-full items-center gap-1 self-start rounded-sm border border-border-1 bg-bg-raised py-0.5 pr-0.5 pl-2.5">
                    <code className="truncate font-mono text-xs text-text-2">
                      {step.snippet}
                    </code>
                    <CopyButton text={step.snippet} label="command" />
                  </span>
                ) : null}
              </div>
            </li>
          );
        })}
      </ol>
      <span className="sr-only" data-testid={`${testid}-progress`}>
        {doneCount} of {STEPS.length} steps done
      </span>
    </section>
  );
}
