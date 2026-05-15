import { Component, type ReactNode } from "react";

type State = { error: Error | null };

export class AppErrorBoundary extends Component<
  { children: ReactNode },
  State
> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error) {
    console.error("[nimbus-ui]", error);
  }

  reset = () => this.setState({ error: null });

  render() {
    if (this.state.error) {
      return (
        <div
          role="alert"
          className="flex h-full items-center justify-center bg-app text-default"
          data-testid="error-boundary"
        >
          <div className="w-[480px] rounded-md border bg-surface p-4 border-app">
            <div className="text-sm font-mono uppercase tracking-wider text-danger">
              Error
            </div>
            <div className="mt-1 text-base">{this.state.error.message}</div>
            <div className="mt-3">
              <button
                type="button"
                onClick={this.reset}
                className="rounded border px-2 py-1 text-sm border-app hover:bg-surface-2"
              >
                Retry
              </button>
            </div>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
