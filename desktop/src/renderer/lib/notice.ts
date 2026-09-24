// Player notices: the core sends a stable `code` (and the variable `detail`)
// alongside its English `message`; the renderer shows its own strings by code
// and falls back to the message for a code it doesn't know yet.
import type { PlayerNoticeCode } from "@core/api";
import { OFFLINE_NOTICE_CODES } from "@shared/constants";
import { hasString, t } from "@shared/strings";

export interface PlayerNotice {
  message?: string;
  code?: PlayerNoticeCode;
  detail?: string;
}

/** The text to show for a notice (undefined: nothing to show). */
export function noticeText(n: PlayerNotice | undefined): string | undefined {
  if (!n) return undefined;
  const id = n.code ? `notice.${n.code}` : undefined;
  if (id && hasString(id) && (n.detail !== undefined || !t(id).includes("{detail}"))) return t(id, { detail: n.detail ?? "" });
  return n.message ?? undefined;
}

/** Offline notices get the app-wide banner instead of the player bar. */
export function isOfflineNotice(n: PlayerNotice | undefined): boolean {
  return !!n?.code && OFFLINE_NOTICE_CODES.includes(n.code);
}
