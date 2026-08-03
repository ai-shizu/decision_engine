/**
 * Root Error Boundary — never fail-silent to a black WKWebView.
 * Coraxis terminal aesthetics: mono, zero radius, crimson FATAL banner.
 */

import { Component, type ErrorInfo, type ReactNode } from "react";

interface GlobalErrorBoundaryProps {
  children: ReactNode;
}

interface GlobalErrorBoundaryState {
  error: Error | null;
  info: ErrorInfo | null;
}

export class GlobalErrorBoundary extends Component<
  GlobalErrorBoundaryProps,
  GlobalErrorBoundaryState
> {
  state: GlobalErrorBoundaryState = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<GlobalErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    this.setState({ error, info });
    // stderr-equivalent for WebView console — never outbound.
    console.error("[CORAXIS FATAL]", error, info.componentStack);
  }

  render(): ReactNode {
    const { error, info } = this.state;
    if (!error) {
      return this.props.children;
    }

    const stack = [error.stack, info?.componentStack]
      .filter((s): s is string => Boolean(s && s.trim()))
      .join("\n\n--- componentStack ---\n");

    return (
      <div className="fatal-crash" role="alert" aria-live="assertive">
        <header className="fatal-crash-head">
          <span className="fatal-crash-glyph">[ FATAL SYSTEM CRASH ]</span>
          <span className="fatal-crash-meta">CORAXIS · RENDER TREE HALTED</span>
        </header>
        <p className="fatal-crash-msg">{error.message || String(error)}</p>
        <pre className="fatal-crash-stack">{stack || "(no stack)"}</pre>
        <button
          type="button"
          className="fatal-crash-reload"
          onClick={() => window.location.reload()}
        >
          RELOAD
        </button>
      </div>
    );
  }
}
