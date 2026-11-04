import { Component, type ErrorInfo, type ReactNode } from 'react';
import { useRouteError } from 'react-router';

/** Tansen's own persisted frontend keys; the backend library and settings stay. */
const UI_KEY_PREFIXES = ['tansen:', 'bitchord:'];

function resetUiState() {
  for (const key of Object.keys(localStorage)) {
    if (UI_KEY_PREFIXES.some((prefix) => key.startsWith(prefix))) localStorage.removeItem(key);
  }
  location.reload();
}

/**
 * The crash screen: React Router renders it as `errorElement`, and any render
 * crash inside it is caught here instead of dead-ending on a blank window.
 */
export class AppError extends Component<{ children?: ReactNode; error?: Error; inline?: boolean }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('render failed', error, info.componentStack);
  }

  render() {
    const error = this.state.error ?? this.props.error ?? null;
    if (!error) return this.props.children ?? null;
    const details = `${error.message}\n${error.stack ?? ''}`;
    if (this.props.inline) {
      return (
        <section className="rounded-[10px] bg-quinary px-4 py-3">
          <p className="type-body-emphasized">This section couldn't load</p>
          <p className="mt-1 type-callout text-secondary">{error.message}</p>
          <button
            className="mt-2 rounded-[8px] bg-quinary px-3 py-1.5 type-callout-emphasized text-primary"
            onClick={() => this.setState({ error: null })}
          >
            Retry
          </button>
        </section>
      );
    }
    return (
      <div className="mx-(--body-gutter) max-w-xl py-11">
        <h1 className="type-header-emphasized">Something went wrong</h1>
        <p className="mt-3 type-body text-secondary">{error.message}</p>
        <p className="mt-2 type-callout text-secondary">
          Your library and settings are untouched. Reload to try again; resetting the UI state clears only this
          app's saved playback queue and view settings.
        </p>
        <div className="mt-6 flex flex-wrap items-center gap-2">
          <button className="rounded-pill bg-white px-4 py-2 type-body-emphasized text-black" onClick={() => location.reload()}>
            Reload
          </button>
          <a className="rounded-pill bg-quinary px-4 py-2 type-body-emphasized" href="/home">
            Go to Home
          </a>
          <button className="rounded-pill bg-quinary px-4 py-2 type-body" onClick={resetUiState}>
            Reset UI state
          </button>
          <button
            className="rounded-pill bg-quinary px-4 py-2 type-body"
            onClick={() => void navigator.clipboard.writeText(details)}
          >
            Copy details
          </button>
        </div>
      </div>
    );
  }
}

/** Route-level wrapper: React Router hands the thrown value to this element. */
export function RouteError() {
  const error = useRouteError();
  return <AppError error={error instanceof Error ? error : new Error(String(error))} />;
}
