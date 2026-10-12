// The now playing subtitle: the track's artist string as given ("Marlowe Reed
// feat. Juno"), never the album. Clicking it opens a small menu anchored to it:
// every artist on the track (each opens that artist's page), a divider, then the
// album (cover, name, album artist; opens the album's page). Used by the player
// bar, the side panel's overview and the fullscreen player's card.
// Keyboard: Enter or Space opens it with focus on the first entry; the arrows,
// Home and End move; Escape (or Tab) closes it and focus goes back to the artist.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { TrackSummary } from "@core/api";
import { t } from "@shared/strings";
import type { Route } from "../store/app";
import { usePresence } from "../lib/presence";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";

/** One artist on a track: a page to open when it has an id. */
export interface ArtistRef {
  id?: string;
  name: string;
}

/**
 * Every artist on `track`: the core's list where it has one (OpenSubsonic `artists`), else the
 * one artist it names.
 */
export function trackArtists(track: ChooserTrack): ArtistRef[] {
  if (track.artists?.length) return track.artists.map((a) => ({ id: a.id, name: a.name }));
  if (!track.artist && !track.artistId) return [];
  return [{ id: track.artistId, name: track.artist ?? t("misc.unknownArtist") }];
}

/** What the chooser reads of a track (`artists` and `albumArtist` where the core gives them). */
export type ChooserTrack = Pick<TrackSummary, "artist" | "artistId"> & Partial<Pick<TrackSummary, "id" | "album" | "albumId" | "coverArt">> & { albumArtist?: string; artists?: { id?: string; name: string }[] };

const WIDTH = 280;

export function ArtistChooser({ track, className, onNavigate, testId }: { track: ChooserTrack; className: string; onNavigate: (route: Route) => void; testId: string }) {
  const [open, setOpen] = useState(false);
  const [at, setAt] = useState<{ left: number; top?: number; bottom?: number } | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const presence = usePresence(open ? at : undefined, menu);
  const shown = presence.value;
  const artists = trackArtists(track);

  const close = (refocus = true) => {
    setOpen(false);
    setAt(undefined);
    if (refocus) button.current?.focus({ preventScroll: true });
  };
  const go = (route: Route) => {
    close(false);
    onNavigate(route);
  };
  // Lined up with the artist's left edge: below it (a dropdown), or above it where there is no
  // room below (the player bar).
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const r = button.current?.getBoundingClientRect();
      if (!r) return;
      const left = Math.max(8, Math.min(r.left - 8, window.innerWidth - WIDTH - 8));
      const height = 16 + 40 * artists.length + 9 + 56;
      setAt(window.innerHeight - r.bottom - 6 >= height + 8 ? { left, top: r.bottom + 6 } : { left, bottom: window.innerHeight - r.top + 6 });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, artists.length]);
  const placed = !!at;
  useEffect(() => {
    if (open && placed) menu.current?.querySelector<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])')?.focus({ preventScroll: true });
  }, [open, placed]);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      const target = e.target as Node;
      if (menu.current?.contains(target) || button.current?.contains(target)) return;
      setOpen(false);
      setAt(undefined);
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [open]);
  // Another song: the menu was about the last one.
  useEffect(() => {
    setOpen(false);
    setAt(undefined);
  }, [track.id]);

  const onMenuKey = (e: React.KeyboardEvent) => {
    const items = [...(menu.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    const to = e.key === "ArrowDown" ? (i + 1) % items.length : e.key === "ArrowUp" ? (i - 1 + items.length) % items.length : e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : -1;
    if (to >= 0 && items.length) {
      e.preventDefault();
      items[to]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Tab") {
      e.preventDefault();
      close();
    }
  };

  const host = button.current?.closest<HTMLElement>('[role="dialog"]') ?? document.body;
  return (
    <>
      <button ref={button} type="button" className={className} aria-haspopup="menu" aria-expanded={open} title={track.artist}
        onClick={() => (open ? close(false) : setOpen(true))}
        onKeyDown={(e) => {
          if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); close(); }
          else if (e.key === "ArrowDown" && !open) { e.preventDefault(); setOpen(true); }
        }} data-testid={testId}>
        {track.artist ?? t("misc.unknownArtist")}
      </button>
      {shown ? createPortal(
        <div {...presence.exitProps} className={`menu artist-menu ${shown.bottom !== undefined ? "above" : "below"} ${presence.motion}`} role="menu" aria-label={t("nowPlaying.artistMenu")} style={{ left: shown.left, top: shown.top, bottom: shown.bottom, width: WIDTH }} onKeyDown={onMenuKey} data-testid="artist-menu">
          {artists.map((a, i) => (
            <button key={`${a.id ?? ""}-${i}`} type="button" role="menuitem" className="mi am-artist" aria-disabled={a.id ? undefined : true} tabIndex={-1} onClick={() => a.id && go({ view: "artist", id: a.id })} data-testid="artist-menu-artist">
              <Icon name="artist" size={18} />
              <span className="grow truncate">{a.name}</span>
            </button>
          ))}
          {artists.length ? <div className="sep" role="separator" /> : null}
          <button type="button" role="menuitem" className="mi am-album" aria-disabled={track.albumId ? undefined : true} tabIndex={-1} onClick={() => track.albumId && go({ view: "album", id: track.albumId })} data-testid="artist-menu-album">
            <Artwork id={track.coverArt} size={96} className="am-art" />
            <span className="am-album-text">
              <span className="am-album-name truncate">{track.album ?? t("misc.unknownAlbum")}</span>
              {track.albumArtist ?? track.artist ? <span className="am-album-artist truncate">{track.albumArtist ?? track.artist}</span> : null}
            </span>
          </button>
        </div>,
        host,
      ) : null}
    </>
  );
}
