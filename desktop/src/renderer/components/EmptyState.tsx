import type { ReactNode } from "react";

/** Every empty state names the one action that fills it. No illustrations. */
export function EmptyState({ message, action, testId }: { message: string; action?: ReactNode; testId?: string }) {
  return (
    <div className="empty" data-testid={testId ?? "empty-state"}>
      <div>{message}</div>
      {action}
    </div>
  );
}
