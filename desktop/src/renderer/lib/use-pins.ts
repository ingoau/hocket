// The Library's pins resolved for drawing: each pin's title, cover and what clicking it does. A pin
// whose album, artist, playlist, song or filter is gone is left out (not removed: it may come back
// after a sync).
import { useEffect, useMemo, useState } from "react";
import type { ActionTarget } from "@core/api";
import { bridge } from "../core/bridge";
import { useApp, useJsonSetting } from "../store/app";
import { filterName } from "./filters";
import { LIBRARY_PINS_KEY, isPinKind, parsePins, pinTap, type LibraryPin, type PinKind, type PinTap } from "./library-pins";
import { playFilter, playGenre } from "./pin-menus";

export interface ResolvedPin {
  pin: LibraryPin;
  kind: PinKind;
  title: string;
  coverArt?: string;
  /** The registry's target (albums, artists, playlists, songs); genres and filters have none. */
  target?: ActionTarget;
}

export function usePins(): LibraryPin[] {
  const v = useJsonSetting(LIBRARY_PINS_KEY);
  return useMemo(() => parsePins(v), [v]);
}

export function useResolvedPins(): ResolvedPin[] | undefined {
  const pins = usePins();
  const filters = useApp((s) => s.filters);
  const libraryVersion = useApp((s) => s.libraryVersion);
  const [resolved, setResolved] = useState<ResolvedPin[] | undefined>(undefined);
  const key = JSON.stringify(pins);
  useEffect(() => {
    let live = true;
    void Promise.all(pins.map(async (p): Promise<ResolvedPin | undefined> => {
      if (!isPinKind(p.kind)) return undefined;
      try {
        switch (p.kind) {
          case "album": { const r = await bridge().query({ type: "album", data: { id: p.id } }); return r.type === "albumDetail" && r.data ? { pin: p, kind: "album", title: r.data.name, coverArt: r.data.coverArt, target: { type: "albums", data: { ids: [p.id] } } } : undefined; }
          case "artist": { const r = await bridge().query({ type: "artist", data: { id: p.id } }); return r.type === "artistDetail" && r.data ? { pin: p, kind: "artist", title: r.data.name, coverArt: r.data.coverArt, target: { type: "artists", data: { ids: [p.id] } } } : undefined; }
          case "playlist": { const r = await bridge().query({ type: "playlist", data: { id: p.id } }); return r.type === "playlistDetail" && r.data ? { pin: p, kind: "playlist", title: r.data.name, coverArt: r.data.coverArt, target: { type: "playlists", data: { ids: [p.id] } } } : undefined; }
          case "track": { const r = await bridge().query({ type: "track", data: { id: p.id } }); return r.type === "trackDetail" && r.data ? { pin: p, kind: "track", title: r.data.title, coverArt: r.data.coverArt, target: { type: "tracks", data: { ids: [p.id] } } } : undefined; }
          case "genre": return { pin: p, kind: "genre", title: p.id };
          case "filter": { const f = filters.find((x) => x.id === p.id); return f ? { pin: p, kind: "filter", title: filterName(f) } : undefined; }
        }
      } catch {
        return undefined;
      }
    })).then((list) => { if (live) setResolved(list.filter((x): x is ResolvedPin => !!x)); });
    return () => { live = false; };
  }, [key, filters, libraryVersion]); // eslint-disable-line react-hooks/exhaustive-deps
  return resolved;
}

/** Does what clicking a pin does: open it, or play / shuffle it. */
export function performPin(rp: ResolvedPin, tap: PinTap = pinTap(rp.pin)): void {
  const app = useApp.getState();
  const serverId = app.servers[0]?.id ?? "";
  const id = rp.pin.id;
  const play = (kind: "album" | "artist" | "playlist", shuffle: boolean) =>
    bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: kind, data: { id } } as never, label: rp.title, sort: "default", tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
  switch (rp.kind) {
    case "track":
      bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: [id], start_index: 0, label: rp.title, shuffle: false } });
      return;
    case "album":
    case "artist":
    case "playlist":
      if (tap === "open") app.navigate({ view: rp.kind, id });
      else play(rp.kind, tap === "shuffle");
      return;
    case "genre":
      if (tap === "open") app.navigate({ view: "genre", id });
      else playGenre(serverId, id, tap === "shuffle");
      return;
    case "filter": {
      const f = app.filters.find((x) => x.id === id);
      if (tap === "open" || !f) app.navigate({ view: "filter", id });
      else playFilter(serverId, f, tap === "shuffle");
    }
  }
}
