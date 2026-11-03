// Resolves cover art through Query.Artwork (fixed cache sizes) and renders it
// via the hocket-art:// protocol. Falls back to a neutral block.
import { useEffect, useState } from "react";
import { artworkUrl, ARTWORK_SIZES } from "@shared/constants";
import { bridge } from "../core/bridge";
import { Icon } from "./Icon";

const ARTWORK_CACHE_CAP = 400;
const cache = new Map<string, Promise<string | undefined>>();

export function resolveArtwork(id: string | undefined, size: number): Promise<string | undefined> {
  if (!id) return Promise.resolve(undefined);
  const key = `${id}@${size}`;
  let p = cache.get(key);
  if (p) {
    cache.delete(key);
    cache.set(key, p);
    return p;
  }
  p = bridge()
    .query({ type: "artwork", data: { id, size } })
    .then((r) => (r.type === "path" ? artworkUrl(r.data) : undefined))
    .catch(() => undefined);
  cache.set(key, p);
  while (cache.size > ARTWORK_CACHE_CAP) {
    const oldest = cache.keys().next().value as string | undefined;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
  return p;
}

export function useArtwork(id: string | undefined, size: number): string | undefined {
  const key = `${id}@${size}`;
  // The url with the id and size it is for: on a change the last one is never returned, not even
  // for the render before the effect runs (that is another track's cover, #147).
  const [resolved, setResolved] = useState<{ key: string; url: string | undefined }>({ key: "", url: undefined });
  useEffect(() => {
    let alive = true;
    void resolveArtwork(id, size).then((u) => alive && setResolved({ key, url: u }));
    return () => {
      alive = false;
    };
  }, [id, size, key]);
  return resolved.key === key ? resolved.url : undefined;
}

export function Artwork({ id, size = ARTWORK_SIZES.grid, className = "art", alt = "", round = false }: { id: string | undefined; size?: number; className?: string; alt?: string; round?: boolean }) {
  const url = useArtwork(id, size);
  const cls = `${className}${round ? " round" : ""}`;
  if (!url) return <div className={`${cls} placeholder`} aria-hidden="true"><Icon name="music" size={size > 100 ? 28 : 14} /></div>;
  // A new element per url: one handed a new src keeps showing the old image until it decodes.
  return <img key={url} className={cls} src={url} alt={alt} loading="lazy" draggable={false} />;
}
