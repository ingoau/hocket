import type { SnoozeItem } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { openContextMenu } from "../components/ContextMenu";
import { Artwork } from "../components/Artwork";

function ends(item: SnoozeItem): string {
  if (item.until == null) return t("snooze.untilNever");
  return t("snooze.until", { when: new Date(item.until).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) });
}

/** Everything snoozed, and when each one ends. */
export function Snoozed() {
  const items = useApp((s) => s.snoozes);
  const serverId = useApp((s) => s.accounts[0]?.id ?? "");
  const play = (item: SnoozeItem) => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: [item.trackRef.id], start_index: 0, label: item.track.title, shuffle: false } });
  return (
    <div className="view" data-testid="view-snoozed">
      <div className="view-header"><h1 className="brand-type">{t("snooze.title")}</h1><span className="muted count-chip">{items.length}</span></div>
      <div className="view-body">
        {items.length === 0 ? <p className="empty" data-testid="snoozed-empty">{t("snooze.empty")}</p> : (
          <ul className="snoozed-list" data-testid="snoozed-list">
            {items.map((item) => (
              <li key={item.trackRef.id}>
                <button
                  type="button"
                  className="tr snoozed snoozed-row"
                  data-testid="snoozed-row"
                  onClick={() => play(item)}
                  onContextMenu={(e) => void openContextMenu(e, { type: "tracks", data: { ids: [item.trackRef.id] } })}
                >
                  <Artwork id={item.track.coverArt} size={64} className="art" />
                  <span className="grow truncate">
                    <span className="truncate">{item.track.title}</span>
                    <span className="sub truncate">{item.track.artist ?? ""}</span>
                  </span>
                  <span className="muted truncate">{ends(item)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
