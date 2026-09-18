// Auto-update hook.
//
// Deliberately NOT implemented: design.md "Still open → Auto-update" is
// blocked on the distribution decision (Flathub, AppImage, GitHub releases,
// Microsoft Store and the Mac App Store each update differently, and some do
// it for you). electron-builder.yml sets `publish: null` for the same reason.
//
// When a channel is chosen, implement `installUpdater` here (for GitHub
// releases: `electron-updater`'s autoUpdater with checkForUpdatesAndNotify(),
// gated on a "check for updates" setting and never on metered networks), and
// call it from src/main/index.ts after the main window exists. Keep the
// surface below so the call site doesn't change.
export interface UpdaterHooks {
  /** Called with a human-readable status for the About page. */
  onStatus(status: string): void;
}

export interface Updater {
  checkNow(): Promise<void>;
  readonly channel: string;
}

export function installUpdater(hooks: UpdaterHooks): Updater {
  hooks.onStatus("Updates are delivered by your distribution channel.");
  return {
    channel: "none",
    async checkNow() {
      hooks.onStatus("Automatic updates aren't configured in this build.");
    },
  };
}
