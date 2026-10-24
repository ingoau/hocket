// What Share offers, and doing it: a Navidrome public link (createShare), a Song.link search page,
// or the audio file itself. Open in Genius is a row of the menu, not of Share. The file never
// comes back to the page as a path.
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { showToast } from "./ui-toast";
import type { MenuFacts } from "./menu-model";
import { resolveTrackIds } from "../store/actions";

export interface ShareChoices {
  navidrome: boolean;
  songLink: boolean;
  file: boolean;
}

const NONE: ShareChoices = { navidrome: false, songLink: false, file: false };

/** Which of these a target can offer. One song has all three; an artist only Song.link. */
export function shareChoices(facts: { kind?: MenuFacts["kind"]; count: number } | undefined): ShareChoices {
  if (!facts || facts.count < 1) return NONE;
  const one = facts.count === 1;
  switch (facts.kind) {
    case "song":
      return { navidrome: true, songLink: one, file: one };
    case "album":
      return { navidrome: true, songLink: one, file: false };
    case "artist":
      return { navidrome: false, songLink: one, file: false };
    case "playlist":
      return { navidrome: true, songLink: false, file: false };
    default:
      return NONE;
  }
}

/** Song.link's search page. The renderer cannot call api.song.link (the page's connect-src is 'self'). */
export function songLinkSearch(query: string): string {
  return `https://song.link/s/${encodeURIComponent(query.trim())}`;
}

/** Genius search for `"title" "artist"`. Undefined when there is no title. */
export function geniusSearch(title: string, artist?: string): string | undefined {
  const song = title.trim();
  if (!song) return undefined;
  const who = artist?.trim();
  const q = who ? `"${song}" "${who}"` : `"${song}"`;
  return `https://genius.com/search?q=${encodeURIComponent(q)}`;
}

/** Ids `createShare` accepts: songs become track ids; an album or playlist is shared as itself. */
export async function navidromeShareIds(target: ActionTarget, facts: MenuFacts): Promise<string[]> {
  if (facts.kind === "album" && target.type === "albums") return target.data.ids.filter(Boolean);
  if (facts.kind === "playlist" && target.type === "playlists") return target.data.ids.filter(Boolean);
  if (facts.kind === "song") return facts.trackIds?.length ? facts.trackIds : resolveTrackIds(target);
  return [];
}

async function songLinkQuery(target: ActionTarget, facts: MenuFacts): Promise<string | undefined> {
  const b = bridge();
  if (facts.kind === "song") {
    const id = facts.trackIds?.[0];
    if (!id) return facts.header?.title;
    const r = await b.query({ type: "track", data: { id } });
    const track = r.type === "trackDetail" ? r.data : undefined;
    return [track?.artist, track?.title ?? facts.header?.title].filter(Boolean).join(" ") || undefined;
  }
  if (facts.kind === "album" && target.type === "albums" && target.data.ids[0]) {
    const r = await b.query({ type: "album", data: { id: target.data.ids[0] } });
    const album = r.type === "albumDetail" ? r.data : undefined;
    return [album?.artist, album?.name ?? facts.header?.title].filter(Boolean).join(" ") || undefined;
  }
  if (facts.kind === "artist") return facts.header?.title;
  return undefined;
}

function copied(url: string | undefined): void {
  if (!url) {
    showToast(t("toast.shareFailed"));
    return;
  }
  bridge().clipboard.writeText(url);
  showToast(t("toast.linkCopied"));
}

export async function copyNavidromeShare(target: ActionTarget, facts: MenuFacts): Promise<void> {
  const ids = await navidromeShareIds(target, facts);
  if (!ids.length) {
    showToast(t("toast.shareFailed"));
    return;
  }
  const r = await bridge().query({ type: "createShare", data: { ids } });
  copied(r.type === "share" ? r.data?.url : undefined);
}

export async function copySongLink(target: ActionTarget, facts: MenuFacts): Promise<void> {
  const query = (await songLinkQuery(target, facts))?.trim();
  copied(query ? songLinkSearch(query) : undefined);
}

/** Open Genius's search for this one song: `"title" "artist"`. */
export async function openGenius(facts: MenuFacts): Promise<void> {
  const id = facts.trackIds?.[0];
  let title = facts.header?.title?.trim() ?? "";
  let artist: string | undefined;
  if (id) {
    const r = await bridge().query({ type: "track", data: { id } });
    const track = r.type === "trackDetail" ? r.data : undefined;
    title = track?.title?.trim() || title;
    artist = track?.artist?.trim() || undefined;
  }
  const url = geniusSearch(title, artist);
  if (!url) {
    showToast(t("toast.shareFailed"));
    return;
  }
  bridge().shell.openExternal(url);
}

/** Hand the track's original file to the system share sheet (or a save dialog, off macOS). */
export async function shareAudioFile(trackId: string | undefined): Promise<void> {
  if (!trackId) {
    showToast(t("toast.shareFailed"));
    return;
  }
  const result = await bridge().shareAudio(trackId);
  if (result === "saved") showToast(t("toast.shareSaved"));
  else if (result === "failed") showToast(t("toast.shareFailed"));
}
