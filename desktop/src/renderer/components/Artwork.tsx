// Resolves cover art through Query.Artwork (fixed cache sizes) and renders it
// via the hocket-art:// protocol. Falls back to a neutral block.
import { useCallback, useEffect, useRef, useState } from "react";
import { artworkUrl, ARTWORK_SIZES } from "@shared/constants";
import { bridge } from "../core/bridge";
import { Icon } from "./Icon";

const ARTWORK_CACHE_CAP = 400;
const cache = new Map<string, Promise<string | undefined>>();

/**
 * Forget every resolved cover. Offline (or with the server away) the core answers with whatever
 * size it kept, and the renderer only sees an opaque token, so it can't tell a stand-in from the
 * real size: everything is asked for again when the network or a server's reachability changes,
 * mounted covers included (one that found nothing offline gets its cover when the network is back).
 */
export function forgetArtwork(): void {
  cache.clear();
  for (const l of forgotten) l();
}

/** Mounted covers, told when everything is forgotten so they ask again (a placeholder from offline gets its cover). */
const forgotten = new Set<() => void>();

export function resolveArtwork(id: string | undefined, size: number): Promise<string | undefined> {
  if (!id) return Promise.resolve(undefined);
  const key = `${id}@${size}`;
  let p = cache.get(key);
  if (p) {
    cache.delete(key);
    cache.set(key, p);
    return p;
  }
  const asked: Promise<string | undefined> = bridge()
    .query({ type: "artwork", data: { id, size } })
    .then((r) => (r.type === "path" ? artworkUrl(r.data) : undefined))
    .catch(() => undefined)
    .then((u) => {
      // "No file" (offline, a failed fetch) is not remembered: the next view asks again.
      if (u === undefined && cache.get(key) === asked) cache.delete(key);
      return u;
    });
  p = asked;
  cache.set(key, p);
  while (cache.size > ARTWORK_CACHE_CAP) {
    const oldest = cache.keys().next().value as string | undefined;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
  return p;
}

/**
 * How long a cover in a virtualised list (`dwell`) the renderer hasn't asked for yet waits before
 * it is asked for. A fast scroll mounts (and unmounts) hundreds of rows, and a core query, once
 * sent, can't be called back. Everything else (the player, the mini player, the now-playing
 * sidebar) asks at once: a wait there is a placeholder flashing on every track change.
 */
export const ARTWORK_DWELL_MS = 150;

/** Whether a cover waits `ARTWORK_DWELL_MS` before it is asked for: only in a list that asked to, and only for one not asked for yet. */
export function artworkWaits(dwell: boolean, id: string | undefined, asked: boolean): boolean {
  return dwell && !!id && !asked;
}

function useResolvedArtwork(id: string | undefined, size: number, dwell: boolean): { url: string | undefined; onError: () => void } {
  const key = `${id}@${size}`;
  // The url with the id and size it is for: on a change the last one is never returned, not even
  // for the render before the effect runs (that is another track's cover, #147).
  const [resolved, setResolved] = useState<{ key: string; url: string | undefined }>({ key: "", url: undefined });
  const [attempt, setAttempt] = useState(0);
  const retried = useRef<string | undefined>(undefined);
  const [epoch, setEpoch] = useState(0);
  useEffect(() => {
    const l = () => setEpoch((n) => n + 1);
    forgotten.add(l);
    return () => {
      forgotten.delete(l);
    };
  }, []);
  useEffect(() => {
    let alive = true;
    const ask = () => void resolveArtwork(id, size).then((u) => alive && setResolved({ key, url: u }));
    if (!artworkWaits(dwell, id, cache.has(key))) {
      ask();
      return () => {
        alive = false;
      };
    }
    const timer = setTimeout(ask, ARTWORK_DWELL_MS);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id, size, key, attempt, epoch, dwell]);
  // The file behind a url can go (the core's image budget evicts covers the renderer still holds):
  // forget it and ask the core again, which fetches it anew. Once per cover; a second failure
  // leaves the placeholder.
  const onError = useCallback(() => {
    cache.delete(key);
    setResolved({ key, url: undefined });
    if (retried.current === key) return;
    retried.current = key;
    setAttempt((n) => n + 1);
  }, [key]);
  return { url: resolved.key === key ? resolved.url : undefined, onError };
}

export function useArtwork(id: string | undefined, size: number): string | undefined {
  return useResolvedArtwork(id, size, false).url;
}

/** `dwell`: for virtualised rows and grids, see `ARTWORK_DWELL_MS`. */
export function Artwork({ id, size = ARTWORK_SIZES.grid, className = "art", alt = "", round = false, dwell = false }: { id: string | undefined; size?: number; className?: string; alt?: string; round?: boolean; dwell?: boolean }) {
  const { url, onError } = useResolvedArtwork(id, size, dwell);
  const cls = `${className}${round ? " round" : ""}`;
  if (!url) return <div className={`${cls} placeholder`} aria-hidden="true"><Icon name="music" size={size > 100 ? 28 : 14} /></div>;
  // A new element per url: one handed a new src keeps showing the old image until it decodes.
  return <img key={url} className={cls} src={url} alt={alt} loading="lazy" draggable={false} onError={onError} />;
}
