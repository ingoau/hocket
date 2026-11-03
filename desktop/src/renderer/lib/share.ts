// What Share offers, and doing it: a Navidrome public link (createShare), a Squigly page the
// core finds on Deezer, or the audio file itself. Open in Genius is a row of the menu, not of Share. The file never
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

/** Which of these a target can offer. One song has all three. Squigly gets no artist pages from us. */
export function shareChoices(facts: { kind?: MenuFacts["kind"]; count: number } | undefined): ShareChoices {
  if (!facts || facts.count < 1) return NONE;
  const one = facts.count === 1;
  switch (facts.kind) {
    case "song":
      return { navidrome: true, songLink: one, file: one };
    case "album":
      return { navidrome: true, songLink: one, file: false };
    case "playlist":
      return { navidrome: true, songLink: false, file: false };
    default:
      return NONE;
  }
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

/** What the core's `songLink` query looks up: one song, or one album. */
export function songLinkIds(target: ActionTarget, facts: MenuFacts): { track_id?: string; album_id?: string } | undefined {
  if (facts.kind === "song") {
    const id = facts.trackIds?.[0] ?? (target.type === "tracks" ? target.data.ids[0] : undefined);
    return id ? { track_id: id } : undefined;
  }
  if (facts.kind === "album" && target.type === "albums" && target.data.ids[0]) return { album_id: target.data.ids[0] };
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

/** Squigly takes another service's link, not a search: the core finds the song on Deezer first. */
export async function copySongLink(target: ActionTarget, facts: MenuFacts): Promise<void> {
  const ids = songLinkIds(target, facts);
  if (!ids) {
    copied(undefined);
    return;
  }
  const r = await bridge().query({ type: "songLink", data: ids });
  copied(r.type === "songLink" ? r.data : undefined);
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
