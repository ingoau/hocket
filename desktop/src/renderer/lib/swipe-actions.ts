// What a song row's swipe does: the core's synced `swipe.list.*` / `swipe.queue.*` settings (the
// same as the phone's, `SWIPE_LIST_ACTIONS` / `SWIPE_QUEUE_ACTIONS` in the core's settings
// registry) turned into the revealed action for one row, and running it through the action
// registry like the context menu does. Android's counterpart is ui/components/SwipeActions.kt.
import type { ActionTarget, TrackRef } from "@core/api";
import { t, type StringId } from "@shared/strings";
import { executeAction, type ActionContext } from "../store/actions";
import { openContextMenu } from "../components/ContextMenu";
import { bridge } from "../core/bridge";
import { useApp } from "../store/app";
import type { Side } from "./swipe";

/** The setting keys, with the registry's defaults (crates/hocket-core/src/settings/registry.rs). */
export const SWIPE_KEYS = {
  list: { startToEnd: "swipe.list.startToEnd", endToStart: "swipe.list.endToStart" },
  queue: { startToEnd: "swipe.queue.startToEnd", endToStart: "swipe.queue.endToStart" },
} as const;
export const SWIPE_DEFAULTS = {
  list: { startToEnd: "playNext", endToStart: "playLater" },
  queue: { startToEnd: "removeFromQueue", endToStart: "removeFromQueue" },
} as const;

/** The choices each list offers, in the core's order. */
export const SWIPE_LIST_ACTIONS = ["none", "playNext", "addToQueue", "playLater", "love", "addToPlaylist", "download", "stash", "snooze", "unsnooze", "preview"] as const;
export const SWIPE_QUEUE_ACTIONS = ["none", "removeFromQueue", "playNext", "love", "addToPlaylist", "download", "snooze", "unsnooze", "preview"] as const;

export type SwipeTone = "primary" | "secondary" | "tertiary" | "danger";

export interface RowSwipeAction {
  /** The registry id run (love or unlove for "love"). */
  runId: string;
  label: string;
  icon: string;
  tone: SwipeTone;
  /** The row slides out (it leaves the list). */
  dismisses: boolean;
}

/** What the row knows about its song. */
export interface RowFacts {
  loved: boolean;
  downloaded: boolean;
  snoozed: boolean;
  /** The queue section setting: without it "add to queue" is "play later". */
  queueSection: boolean;
}

/** A setting's label in the settings pickers. */
export function swipeChoiceLabel(id: string): string {
  return t(`swipe.choice.${id}` as StringId);
}

const act = (runId: string, label: StringId, icon: string, tone: SwipeTone, dismisses = false): RowSwipeAction => ({ runId, label: t(label), icon, tone, dismisses });

/**
 * The action a swipe setting's `id` means for one row, or null: none, or nothing to do ("download"
 * on a song already downloaded, "unsnooze" on one that isn't snoozed). A side with null doesn't move.
 */
export function rowSwipeAction(id: string, f: RowFacts): RowSwipeAction | null {
  switch (id) {
    case "removeFromQueue":
      return act("removeFromQueue", "swipe.choice.removeFromQueue", "trash", "danger", true);
    case "playNext":
      return act("playNext", "swipe.choice.playNext", "playNext", "primary");
    case "addToQueue":
      return f.queueSection ? act("addToQueue", "swipe.choice.addToQueue", "queue", "primary") : rowSwipeAction("playLater", f);
    case "playLater":
      return act("playLater", "swipe.choice.playLater", "playLater", "secondary");
    case "love":
      return f.loved ? act("unlove", "swipe.unlove", "heartOff", "tertiary") : act("love", "swipe.choice.love", "heart", "tertiary");
    case "addToPlaylist":
      return act("addToPlaylist", "swipe.choice.addToPlaylist", "playlistAdd", "secondary");
    case "download":
      return f.downloaded ? null : act("download", "swipe.choice.download", "download", "primary");
    case "stash":
      return act("stash", "swipe.choice.stash", "stash", "tertiary");
    case "snooze":
      return f.snoozed ? null : act("snooze", "swipe.choice.snooze", "sleep", "tertiary");
    case "unsnooze":
      return f.snoozed ? act("unsnooze", "swipe.choice.unsnooze", "sleep", "tertiary") : null;
    case "preview":
      return act("preview", "swipe.choice.preview", "headphones", "primary");
    default:
      return null;
  }
}

/** Which setting a physical side runs: right is towards the end of the line in left-to-right text. */
export function sideKey(side: Side, rtl: boolean): "startToEnd" | "endToStart" {
  return (side > 0) !== rtl ? "startToEnd" : "endToStart";
}

export function isRtl(): boolean {
  return typeof document !== "undefined" && document.dir === "rtl";
}

/**
 * Run a row's swipe on `target`, `track` being its song. `anchor` is where the row sits at rest
 * (not where the swipe pushed it). Add to playlist opens the picker there; preview opens the song's
 * menu there and starts the preview, which plays while the menu is open (as Android's does).
 * Everything else is the registry's, as from the context menu.
 */
export function runRowSwipe(a: RowSwipeAction, target: ActionTarget, track: TrackRef, anchor: HTMLElement | null, ctx: ActionContext = {}): void {
  if (a.runId === "preview") {
    const r = anchor?.getBoundingClientRect();
    void openContextMenu({ clientX: Math.round(r ? r.left + 24 : 0), clientY: Math.round(r ? r.bottom : 0), preventDefault: () => {} }, target, ctx).then(() => {
      // Don't start one there is no menu to show it in.
      if (useApp.getState().contextMenu) bridge().dispatch({ type: "previewStart", data: { track } });
    });
    return;
  }
  if (a.runId === "addToPlaylist") {
    // A picker already open (for another song) would take this as the + that closes it.
    const app = useApp.getState();
    if (app.playlistPicker) app.closePlaylistPicker();
    void executeAction("addToPlaylist", target, { ...ctx, anchor: anchor ?? undefined });
    return;
  }
  void executeAction(a.runId, target, ctx);
}
