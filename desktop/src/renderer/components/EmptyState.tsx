import type { ReactNode } from "react";
import { Icon } from "./Icon";

/** Every empty state names the one action that fills it. No illustrations; a library view may add one glyph in a tonal shape. */
export function EmptyState({ message, action, testId, icon }: { message: string; action?: ReactNode; testId?: string; icon?: string }) {
  return (
    <div className="empty" data-testid={testId ?? "empty-state"}>
      {icon ? <span className="empty-icon" aria-hidden="true"><Icon name={icon} size={32} /></span> : null}
      <div>{message}</div>
      {action}
    </div>
  );
}
