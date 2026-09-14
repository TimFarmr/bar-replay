import { Component, type ErrorInfo, type ReactNode } from 'react'

/**
 * A render error in a desktop app must never leave a blank window with no
 * explanation — the user has no console to check and no way to report what
 * happened. This shows what broke and offers a way back.
 */
export class ErrorBoundary extends Component<
  { children: ReactNode },
  { message: string | null }
> {
  state: { message: string | null } = { message: null }

  static getDerivedStateFromError(error: unknown) {
    return { message: error instanceof Error ? error.message : String(error) }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Goes to the webview console only; nothing is sent anywhere.
    console.error('Bar Replay hit an unexpected error', error, info.componentStack)
  }

  render() {
    if (this.state.message === null) return this.props.children
    return (
      <div className="setup">
        <h1>Something went wrong</h1>
        <p className="error">{this.state.message}</p>
        <p className="hint">
          Your session is saved. Nothing was lost — the event log on disk is the
          record, and reopening the session replays it.
        </p>
        <button className="primary" onClick={() => this.setState({ message: null })}>
          Try again
        </button>
      </div>
    )
  }
}
