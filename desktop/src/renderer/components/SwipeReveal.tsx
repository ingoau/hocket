// What a song row's swipe uncovers: the action's colour, icon and label at the edge the row moved
// away from; the icon grows once letting go would run it. Decorative: every swipe action is also
// in the row's menu.
import type { CSSProperties } from "react";
import type { RowSwipeAction } from "../lib/swipe-actions";
import type { Side } from "../lib/swipe";
import { Icon } from "./Icon";

export function SwipeReveal({ action, side, armed, style }: { action: RowSwipeAction; side: Side; armed: boolean; style?: CSSProperties }) {
  return (
    <div className={`swipe-reveal ${action.tone} ${side > 0 ? "from-start" : "from-end"} ${armed ? "armed" : ""}`} style={style} aria-hidden="true" data-testid="swipe-reveal" data-action={action.runId}>
      <Icon name={action.icon} size={20} />
      <span className="truncate">{action.label}</span>
    </div>
  );
}
