// The top of a library list: a Pinned group and a Recent group (each a single row of tiles, or a
// couple of compact rows for songs), hidden when empty, then the whole list under its own header
// with the sort control (lib/list-groups.ts has the rules, lib/list-sort.ts the sorts).
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Album, Artist, PlayHistoryEntry } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { useResolvedPins, type ResolvedPin } from "../lib/use-pins";
import type { PinKind } from "../lib/library-pins";
import { useRoving } from "../lib/roving";
import { Tile, TileList } from "./Tile";
import { Artwork } from "./Artwork";

/** One item of a group, however its list draws it. */
export interface GroupItem {
  key: string;
  title: string;
  subtitle?: string;
  coverArt?: string;
  round?: boolean;
  open: () => void;
  play?: () => void;
  menu?: (e: React.MouseEvent) => void;
}

/** A list part's header: its title, and for the whole list the sort control. */
export function ListHeader({ id, title, actions, className = "" }: { id: string; title: string; actions?: ReactNode; className?: string }) {
  return (
    <div className={`list-section-head ${className}`}>
      <h2 className="section-heading" id={id}>{title}</h2>
      {actions ? <div className="list-section-actions">{actions}</div> : null}
    </div>
  );
}

/** A titled part of a list page: Pinned, Recent, or the whole list when it isn't a grid of its own. */
export function ListSection({ id, title, actions, children, testId, className = "" }: { id: string; title: string; actions?: ReactNode; children: ReactNode; testId?: string; className?: string }) {
  return (
    <section className={`list-section ${className}`} aria-labelledby={id} data-testid={testId}>
      <ListHeader id={id} title={title} actions={actions} />
      {children}
    </section>
  );
}

/** A group as one row of artwork tiles (it scrolls sideways when the window is narrow). */
export function TileRow({ items, label, testId }: { items: GroupItem[]; label: string; testId: string }) {
  return (
    <TileList label={label} className="scroller group-row">
      {items.map((it) => <Tile key={it.key} title={it.title} subtitle={it.subtitle} coverArt={it.coverArt} round={it.round} onOpen={it.open} onPlay={it.play} onContextMenu={it.menu} testId={testId} />)}
    </TileList>
  );
}

/** A group of songs as compact rows (cover, title, artist): Enter or a click plays. */
export function SongRows({ items, label, testId }: { items: GroupItem[]; label: string; testId: string }) {
  const ref = useRef<HTMLUListElement>(null);
  const roving = useRoving(ref);
  return (
    <ul ref={ref} className="plain-list group-songs" aria-label={label} {...roving}>
      {items.map((it) => (
        <li key={it.key} className="group-song" onContextMenu={it.menu} data-testid={testId}>
          <button type="button" className="group-song-main" data-roving aria-label={it.subtitle ? `${it.title}, ${it.subtitle}` : it.title} onClick={it.open}>
            <Artwork id={it.coverArt} size={96} className="art" />
            <span className="text"><span className="t1">{it.title}</span>{it.subtitle ? <span className="t2">{it.subtitle}</span> : null}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * Pinned and Recent over a list, each only when it has something. They share a band: side by side
 * when both fit (a few pins beside the recent row), else Recent wraps under Pinned.
 */
export function ListGroups({ list, pinned, recent, render }: { list: string; pinned: GroupItem[]; recent: GroupItem[]; render: (items: GroupItem[], label: string, testId: string) => ReactNode }) {
  if (!pinned.length && !recent.length) return null;
  return (
    <div className="list-groups">
      {pinned.length ? <ListSection id={`${list}-pinned`} title={t("lists.pinned")} testId={`${list}-pinned`} className="pinned">{render(pinned, t("lists.pinned"), "group-pinned-item")}</ListSection> : null}
      {recent.length ? <ListSection id={`${list}-recent`} title={t("lists.recent")} testId={`${list}-recent`} className="recent">{render(recent, t("lists.recent"), "group-recent-item")}</ListSection> : null}
    </div>
  );
}

const NO_PINS: ResolvedPin[] = [];

/** The Library pins of one kind, in pin order (none until they're resolved). */
export function usePinsOf(kind: PinKind): ResolvedPin[] {
  const all = useResolvedPins();
  return useMemo(() => all?.filter((p) => p.kind === kind) ?? NO_PINS, [all, kind]);
}

/** The recently played songs, newest first (refreshed as the current song changes, as Home's). */
export function useRecentHistory(): PlayHistoryEntry[] | undefined {
  const nowId = useApp((s) => s.session?.current?.trackId);
  return useQuery(() => ({ type: "recentlyPlayed", data: { limit: 100 } }), "history", [nowId]).data;
}

/** Albums or artists by id, in that order (the core has no batch query for them). */
export function useDetails<K extends "album" | "artist">(kind: K, ids: string[]): (K extends "album" ? Album : Artist)[] {
  const key = ids.join(",");
  const libraryVersion = useApp((s) => s.libraryVersion);
  const [state, setState] = useState<{ key: string; items: (Album | Artist)[] }>({ key: "", items: [] });
  useEffect(() => {
    if (!ids.length) return;
    let live = true;
    void Promise.all(ids.map((id) => bridge().query(kind === "album" ? { type: "album", data: { id } } : { type: "artist", data: { id } })
      .then((r) => (r.type === "albumDetail" || r.type === "artistDetail" ? r.data : undefined)).catch(() => undefined)))
      .then((list) => { if (live) setState({ key, items: list.filter((x): x is Album | Artist => !!x) }); });
    return () => { live = false; };
  }, [key, kind, libraryVersion]); // eslint-disable-line react-hooks/exhaustive-deps
  return (ids.length && state.key === key ? state.items : []) as (K extends "album" ? Album : Artist)[];
}
