import { useRouterState } from "@tanstack/react-router";
import { Component, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { CopyChip } from "../components/copy-chip";
import { EmptyState } from "../components/empty-state";

type Props = { children: ReactNode; pathname: string };
type State = { error: Error | null; pathname: string };

/**
 * Last-resort boundary for a crash in the shell chrome itself. The router's
 * `defaultErrorComponent` (components/route-error.tsx) already catches inside
 * the `<Outlet/>`, so a failing view never reaches here; what does reach here
 * is a crash in the nav, panels, status bar, or the providers around them —
 * which is why this stays outside `<main>` and wraps the whole shell.
 *
 * Clearing on navigation is derived from the pathname rather than a `key`.
 * A `key` would remount the entire shell on every navigation, discarding
 * panel state, scroll position and live connections on the ~100% of
 * navigations where nothing crashed, to buy recovery on the rare one that
 * did. `getDerivedStateFromProps` pays nothing on the happy path: it drops
 * only the error flag, and React remounts the previously-thrown subtree by
 * itself because it had already unmounted it when it caught.
 */
class ShellErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { error: null, pathname: props.pathname };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  static getDerivedStateFromProps(props: Props, state: State): State | null {
    if (props.pathname === state.pathname) return null;
    return { error: null, pathname: props.pathname };
  }

  componentDidCatch(error: Error) {
    console.error("[nimbus-ui]", error);
  }

  reset = () => this.setState({ error: null });

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    const details = `shell: ${this.state.pathname}\nerror: ${error.message}${
      error.stack ? `\n${error.stack}` : ""
    }`;
    return (
      <div
        role="alert"
        className="flex h-full items-center justify-center bg-bg-canvas text-text-1"
        data-testid="error-boundary"
      >
        {/* A bare `w-[480px]` hung off BOTH edges of a phone viewport —
            measured -80px left / 400px right in a 320px window — on the one
            screen that exists because the shell already failed.

            It could not shrink out of it. The chip below overrides the shared
            `max-w-[28ch]` with `max-w-full`, so its `truncate` (and the
            `white-space:nowrap` inside it) is all that bounds a long message,
            and that pushes the card's min-content past its own 480px —
            486.8px for a representative TypeError against the built
            stylesheet, and it keeps growing with the message. A flex item's
            automatic minimum is min(specified-size-suggestion,
            content-size-suggestion), so once the content suggestion clears
            480px the minimum sticks at 480px and flex-shrink has nothing left
            to give. The control that proves the chip is the cause: in a
            reduced repro the identical card WITHOUT the chip has a 67.6px
            min-content and shrinks to 320px happily, while with it the
            min-content is 728.9px and the card will not move.

            Capping the specified width therefore drops the floor with it:
            the specified suggestion becomes 288px and the minimum follows.
            Measured 288px at a 320px viewport and 351px at 390px,
            fully on screen, still 480px at 1440px. `max-w-full` would work
            too, by a different route: css-flexbox-1 4.5 clamps the resulting
            minimum by the maximum main size, rather than feeding max-width
            into either suggestion. 90vw is chosen over it so the card keeps a
            gutter and still reads as a card rather than a full-bleed band. */}
        <div
          className="flex w-[min(480px,90vw)] flex-col items-center gap-3 rounded-lg border border-border-2 bg-bg-panel p-6"
          data-testid="error-boundary-card"
        >
          <EmptyState
            className="h-auto p-0"
            mascot="error"
            title="The console shell failed to render"
            body="Moving to another view clears this screen."
            testid="error-boundary-state"
          />
          <CopyChip
            label="error details"
            value={details}
            testid="error-boundary-copy"
            className="max-w-full border border-border-2 px-2 py-1 text-error"
          >
            {error.message}
          </CopyChip>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={this.reset}
              data-testid="error-boundary-retry"
            >
              Retry
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => window.location.reload()}
              data-testid="error-boundary-reload"
            >
              Reload console
            </Button>
          </div>
        </div>
      </div>
    );
  }
}

/**
 * Reads the location outside the boundary — a subscription inside the class
 * would die with the subtree it is supposed to revive — and hands it down so
 * the boundary can tell "the operator moved" from "the operator re-rendered".
 */
export function AppErrorBoundary({ children }: { children: ReactNode }) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  return (
    <ShellErrorBoundary pathname={pathname}>{children}</ShellErrorBoundary>
  );
}
