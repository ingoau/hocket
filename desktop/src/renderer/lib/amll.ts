// The AMLL lyric player, loaded on demand. Its bundle brings pixi.js (the background renderer
// it also exports) and was a third of the renderer's startup JavaScript, for a view that only
// some sessions open: it now loads once lyrics arrive (preloaded when idle, App.tsx) or when a
// lyrics view first mounts, whichever comes first.
import type * as Amll from "@applemusic-like-lyrics/core";

let loading: Promise<typeof Amll> | undefined;

export function loadAmll(): Promise<typeof Amll> {
  loading ??= import("./amll-chunk").catch((err: unknown) => {
    // A failed load (it is a local file, so this is rare) is retried on the next request.
    loading = undefined;
    throw err;
  });
  return loading;
}
