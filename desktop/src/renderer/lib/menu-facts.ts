// What the context menu shows about its target beyond the registry's descriptors: the header (cover,
// title, subtitle), whether the items are loved, rated or downloaded (Love / Loved, the stars, the
// mixed states), the playlist the songs are listed in and the songs the add to playlist submenu adds.
// Fetched alongside the descriptors when the menu opens, so it draws complete, without a jump.
import type { ActionTarget, Album, Artist, Playlist, Track } from "@core/api";
import { expectResult } from "@shared/core-handle";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { resolveTrackIds, type ActionContext } from "../store/actions";
import { fmtDuration } from "./format";
import type { MenuFacts } from "./menu-model";

/** Beyond this many items the menu shows no per-item state (a whole library selected). */
const MAX_DETAIL = 2000;

const songs = (n: number) => (n === 1 ? t("menu.songs.one") : t("menu.songs.other", { n }));
const joined = (parts: (string | number | undefined)[]) => parts.filter((p) => p !== undefined && p !== "").join(" · ");
const isDownloaded = (o: string | undefined) => o === "downloaded" || o === "downloading";

async function one<T>(q: Parameters<ReturnType<typeof bridge>["query"]>[0], result: "albumDetail" | "artistDetail" | "playlistDetail"): Promise<T | undefined> {
  return (expectResult(await bridge().query(q), result) ?? undefined) as T | undefined;
}

export async function fetchMenuFacts(target: ActionTarget, context?: ActionContext): Promise<MenuFacts | undefined> {
  try {
    const facts = await factsFor(target);
    if (facts && context?.playlistId) {
      const p = await one<Playlist>({ type: "playlist", data: { id: context.playlistId } }, "playlistDetail");
      if (p) facts.playlistName = p.name;
    }
    return facts;
  } catch {
    // The menu still works from the descriptors alone.
    return undefined;
  }
}

async function factsFor(target: ActionTarget): Promise<MenuFacts | undefined> {
  const b = bridge();
  switch (target.type) {
    case "tracks":
    case "queueItems": {
      const ids = await resolveTrackIds(target);
      if (!ids.length) return undefined;
      if (ids.length > MAX_DETAIL) return { kind: "song", count: ids.length, trackIds: ids };
      const found = expectResult(await b.query({ type: "tracksByIds", data: { ids } }), "trackList");
      const byId = new Map(found.map((x) => [x.id, x]));
      const tracks = ids.map((id) => byId.get(id)).filter((x): x is Track => !!x);
      if (tracks.length !== ids.length) return { kind: "song", count: ids.length, trackIds: ids };
      const first = tracks[0] as Track;
      const header = tracks.length === 1
        ? { covers: [first.coverArt], title: first.title, subtitle: joined([first.artist, first.album]) }
        : {
            covers: [...new Set(tracks.map((x) => x.coverArt))].slice(0, 3),
            title: t("menu.songsTime", { n: tracks.length, time: fmtDuration(tracks.reduce((s, x) => s + x.durationMs, 0)) }),
            subtitle: tracks.slice(0, 12).map((x) => x.title).join(", "),
          };
      return {
        kind: "song",
        count: tracks.length,
        loved: tracks.filter((x) => x.loved).length,
        downloaded: tracks.filter((x) => isDownloaded(x.offline)).length,
        ratings: tracks.map((x) => x.rating),
        header,
        trackIds: ids,
      };
    }
    case "albums": {
      const albums = (await Promise.all(target.data.ids.slice(0, 50).map((id) => one<Album>({ type: "album", data: { id } }, "albumDetail")))).filter((a): a is Album => !!a);
      if (!albums.length) return undefined;
      const first = albums[0] as Album;
      const all = albums.length === target.data.ids.length;
      return {
        kind: "album",
        count: target.data.ids.length,
        loved: all ? albums.filter((a) => a.loved).length : undefined,
        downloaded: all ? albums.filter((a) => isDownloaded(a.offline)).length : undefined,
        ratings: all ? albums.map((a) => a.rating) : undefined,
        header: albums.length === 1
          ? { covers: [first.coverArt], title: first.name, subtitle: joined([first.artist, first.year]) }
          : { covers: albums.slice(0, 3).map((a) => a.coverArt), title: t("menu.albums", { n: target.data.ids.length }), subtitle: albums.slice(0, 12).map((a) => a.name).join(", ") },
        trackIds: await resolveTrackIds(target),
      };
    }
    case "artists": {
      const artists = (await Promise.all(target.data.ids.slice(0, 50).map((id) => one<Artist>({ type: "artist", data: { id } }, "artistDetail")))).filter((a): a is Artist => !!a);
      if (!artists.length) return undefined;
      const first = artists[0] as Artist;
      return {
        kind: "artist",
        count: target.data.ids.length,
        loved: artists.length === target.data.ids.length ? artists.filter((a) => a.loved).length : undefined,
        header: artists.length === 1
          ? { covers: [first.coverArt], title: first.name, subtitle: songs(first.songCount), round: true }
          : { covers: artists.slice(0, 3).map((a) => a.coverArt), title: t("menu.artists", { n: target.data.ids.length }), subtitle: artists.slice(0, 12).map((a) => a.name).join(", "), round: true },
      };
    }
    case "playlists": {
      const lists = (await Promise.all(target.data.ids.slice(0, 50).map((id) => one<Playlist>({ type: "playlist", data: { id } }, "playlistDetail")))).filter((p): p is Playlist => !!p);
      if (!lists.length) return undefined;
      const first = lists[0] as Playlist;
      return {
        kind: "playlist",
        count: target.data.ids.length,
        downloaded: lists.length === target.data.ids.length ? lists.filter((p) => isDownloaded(p.offline)).length : undefined,
        header: lists.length === 1
          ? { covers: [first.coverArt], title: first.name, subtitle: joined([songs(first.songCount), first.durationMs ? fmtDuration(first.durationMs) : undefined]) }
          : { covers: lists.slice(0, 3).map((p) => p.coverArt), title: t("menu.playlists", { n: target.data.ids.length }), subtitle: lists.slice(0, 12).map((p) => p.name).join(", ") },
      };
    }
    default:
      return undefined;
  }
}
