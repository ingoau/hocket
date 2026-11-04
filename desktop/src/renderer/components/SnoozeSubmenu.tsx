// The song menu's Snooze flyout: 1 day, 3 days, 1 week, 1 month, indefinitely, or a custom date
// and time (a dialog).
import type { SnoozeFor } from "@core/api";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { useApp } from "../store/app";
import type { FlyoutAnchor } from "../lib/flyout";
import type { ExitProps } from "../lib/presence";
import { MenuFlyout, type FlyoutRow } from "./MenuFlyout";

const DURATIONS: { id: SnoozeFor; label: Parameters<typeof t>[0] }[] = [
  { id: "day", label: "snooze.day" },
  { id: "threeDays", label: "snooze.threeDays" },
  { id: "week", label: "snooze.week" },
  { id: "month", label: "snooze.month" },
  { id: "indefinite", label: "snooze.indefinite" },
];

interface Props {
  trackIds: string[];
  anchor: FlyoutAnchor;
  motion: string;
  exitProps: ExitProps<HTMLDivElement>;
  onBack: () => void;
  onClose: (then?: () => void) => void;
}

export function SnoozeSubmenu({ trackIds, ...flyout }: Props) {
  const openDialog = useApp((s) => s.openDialog);
  const rows: FlyoutRow[] = [
    ...DURATIONS.map((d) => ({
      id: d.id,
      label: t(d.label),
      icon: "sleep",
      run: () => bridge().dispatch({ type: "snooze", data: { track_ids: trackIds, duration: d.id } }),
    })),
    { id: "custom", label: t("snooze.custom"), icon: "calendar", run: () => openDialog({ kind: "snoozeUntil", trackIds }) },
  ];
  return <MenuFlyout rows={rows} label={t("action.snooze")} testId="snooze-submenu" {...flyout} />;
}
