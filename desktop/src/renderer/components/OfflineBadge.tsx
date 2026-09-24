// Where a track's audio lives on this device: Downloaded (pinned, never
// evicted) or Cached (complete in the stream cache, plays offline until
// evicted). Partial cache entries and primed starts are OfflineState "none"
// and show nothing. An image with an accessible name (role="img" + <title>),
// like every labelled icon in the app.
import type { OfflineState } from "@core/api";
import { t } from "@shared/strings";
import { Icon } from "./Icon";

export function offlineLabel(state: OfflineState | undefined): string | undefined {
  if (state === "downloaded") return t("a11y.downloaded");
  if (state === "cached") return t("a11y.cached");
  return undefined;
}

export function OfflineBadge({ state, size = 12 }: { state: OfflineState | undefined; size?: number }) {
  const label = offlineLabel(state);
  if (!label) return null;
  return (
    <span className={`offline-badge ${state}`} data-testid="offline-badge" data-state={state}>
      <Icon name={state === "downloaded" ? "download" : "cached"} size={size} title={label} />
    </span>
  );
}
