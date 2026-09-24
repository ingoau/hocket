// Resolves cover art through Query.Artwork (fixed cache sizes) and renders it
// via the hocket-art:// protocol. Falls back to a neutral block.
import { useEffect, useState } from "react";
import { artworkUrl, ARTWORK_SIZES } from "@shared/constants";
import { bridge } from "../core/bridge";
import { Icon } from "./Icon";

const cache = new Map<string, Promise<string | undefined>>();

export function resolveArtwork(id: string | undefined, size: number): Promise<string | undefined> {
  if (!id) return Promise.resolve(undefined);
  const key = `${id}@${size}`;
  let p = cache.get(key);
  if (!p) {
    p = bridge()
      .query({ type: "artwork", data: { id, size } })
      .then((r) => (r.type === "path" ? artworkUrl(r.data) : undefined))
      .catch(() => undefined);
    cache.set(key, p);
    if (cache.size > 2000) cache.delete(cache.keys().next().value as string);
  }
  return p;
}

export function useArtwork(id: string | undefined, size: number): string | undefined {
  const [url, setUrl] = useState<string | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    setUrl(undefined);
    void resolveArtwork(id, size).then((u) => alive && setUrl(u));
    return () => {
      alive = false;
    };
  }, [id, size]);
  return url;
}

export function Artwork({ id, size = ARTWORK_SIZES.grid, className = "art", alt = "", round = false }: { id: string | undefined; size?: number; className?: string; alt?: string; round?: boolean }) {
  const url = useArtwork(id, size);
  const cls = `${className}${round ? " round" : ""}`;
  if (!url) return <div className={`${cls} placeholder`} aria-hidden="true"><Icon name="music" size={size > 100 ? 28 : 14} /></div>;
  return <img className={cls} src={url} alt={alt} loading="lazy" draggable={false} />;
}
