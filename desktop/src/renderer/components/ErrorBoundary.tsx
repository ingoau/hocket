// A part of the window that throws while rendering takes only itself down. Without a
// boundary React unmounts the whole tree and the window stays blank until restarted; with
// one, a page shows what happened and a way to try again while the sidebar, the side panel
// and the player bar keep working.
import { Component, type ErrorInfo, type ReactNode } from "react";
import { t } from "@shared/strings";
import { EmptyState } from "./EmptyState";

interface Props {
  children: ReactNode;
  /** Names the part in the log. */
  name: string;
  /** What shows instead; the default is the page message with a Try again button. */
  fallback?: ReactNode;
  onError?: (error: unknown) => void;
}

export class ErrorBoundary extends Component<Props, { failed: boolean }> {
  override state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error(`[ui] ${this.props.name} failed to render`, error, info.componentStack);
    this.props.onError?.(error);
  }

  override render(): ReactNode {
    if (!this.state.failed) return this.props.children;
    if (this.props.fallback !== undefined) return this.props.fallback;
    return (
      <div role="alert">
        <EmptyState icon="warn" testId="view-error" message={t("app.viewFailed")} action={<button type="button" className="btn tonal" onClick={() => this.setState({ failed: false })} data-testid="view-error-retry">{t("app.viewRetry")}</button>} />
      </div>
    );
  }
}
