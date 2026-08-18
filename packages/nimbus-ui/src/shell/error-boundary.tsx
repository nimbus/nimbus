import { useRouterState } from "@tanstack/react-router";
import { Component, type ReactNode } from "react";

import { CopyChip } from "../components/copy-chip";

const ACTION_CLASS =
  "rounded border border-app px-3 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default";

type Props = { children: ReactNode; pathname: string };
type State = { error: Error | null; pathname: string };

/**
 * Last-resort boundary for a crash in the shell chrome itself. The router's
 * `defaultErrorComponent` (components/route-error.tsx) already catches inside
 * the `<Outlet/>`, so a failing view never reaches here; what does reach here
 * is a crash in the nav, drawers, status bar, or the providers around them —
 * which is why this stays outside `<main>` and wraps the whole shell.
 *
 * Clearing on navigation is derived from the pathname rather than a `key`.
 * A `key` would remount the entire shell on every navigation, discarding
 * drawer state, scroll position and live connections on the ~100% of
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
        className="flex h-full items-center justify-center bg-canvas text-default"
        data-testid="error-boundary"
      >
        <div className="w-[480px] rounded-md border bg-surface p-4 border-app">
          <div className="text-sm font-mono uppercase tracking-wider text-danger">
            Error
          </div>
          <p className="mt-1 text-base">
            The console shell failed to render. Moving to another view clears
            this screen.
          </p>
          <CopyChip
            label="error details"
            value={details}
            testid="error-boundary-copy"
            className="mt-3 max-w-full border border-app px-2 py-1 text-danger"
          >
            {error.message}
          </CopyChip>
          <div className="mt-3 flex gap-2">
            <button
              type="button"
              onClick={this.reset}
              className={ACTION_CLASS}
              data-testid="error-boundary-retry"
            >
              Retry
            </button>
            <button
              type="button"
              onClick={() => window.location.reload()}
              className={ACTION_CLASS}
              data-testid="error-boundary-reload"
            >
              Reload console
            </button>
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
