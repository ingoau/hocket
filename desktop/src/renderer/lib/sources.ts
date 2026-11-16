import type { AccountInfo } from "../../core/api";
import { t } from "@shared/strings";

/** Where Mixed playlists live (`Playlist.serverId`), and the New playlist dialog's Mixed choice. */
export const MIXED_SOURCE = "hocket";

export interface SourceOption {
  /** "" for All. */
  id: string;
  label: string;
}

/** The source chip's choices: none with one account; else All, each account, then Mixed when asked. */
export function sourceOptions(accounts: readonly Pick<AccountInfo, "id" | "name" | "url">[], mixed = false): SourceOption[] {
  if (accounts.length < 2) return [];
  return [
    { id: "", label: t("source.all") },
    ...accounts.map((a) => ({ id: a.id, label: a.name || a.url })),
    ...(mixed ? [{ id: MIXED_SOURCE, label: t("source.mixed") }] : []),
  ];
}
