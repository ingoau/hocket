// The context menu's Share flyout: a Navidrome public link, a Song.link page, and the audio file.
// Each row closes the menu and does the share. Open in Genius is a row of the menu itself.
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { copyNavidromeShare, copySongLink, shareAudioFile, shareChoices } from "../lib/share";
import type { MenuFacts } from "../lib/menu-model";
import type { FlyoutAnchor } from "../lib/flyout";
import type { ExitProps } from "../lib/presence";
import { MenuFlyout, type FlyoutRow } from "./MenuFlyout";

interface Props {
  facts: MenuFacts;
  target: ActionTarget;
  anchor: FlyoutAnchor;
  motion: string;
  exitProps: ExitProps<HTMLDivElement>;
  onBack: () => void;
  onClose: (then?: () => void) => void;
}

export function ShareSubmenu({ facts, target, ...flyout }: Props) {
  const choices = shareChoices(facts);
  const rows: FlyoutRow[] = [];
  if (choices.navidrome) rows.push({ id: "share-navidrome", label: t("menu.shareNavidrome"), icon: "share", run: () => void copyNavidromeShare(target, facts) });
  if (choices.songLink) rows.push({ id: "share-songLink", label: t("menu.shareSongLink"), icon: "song", run: () => void copySongLink(target, facts) });
  if (choices.file) rows.push({ id: "share-file", label: t("menu.shareFile"), icon: "download", run: () => void shareAudioFile(facts.trackIds?.[0]) });
  return <MenuFlyout rows={rows} label={t("menu.share")} testId="share-submenu" {...flyout} />;
}
