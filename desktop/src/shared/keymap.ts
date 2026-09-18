// Default keyboard map from docs/design.md "Interface → Keyboard". Every entry
// is rebindable; the core's Query.Shortcuts overrides these by action id when
// it knows the action, and SetShortcut persists the user's changes. Ids with a
// `ui.` prefix are executed by the renderer, everything else by the core via
// Command.RunAction.
export interface DefaultBinding {
  actionId: string;
  shortcut: string;
  labelId: string;
  category: string;
}

export const DEFAULT_KEYMAP: DefaultBinding[] = [
  { actionId: "ui.palette", shortcut: "Mod+K", labelId: "action.palette", category: "app" },
  { actionId: "ui.search", shortcut: "Mod+F", labelId: "action.search", category: "app" },
  { actionId: "transport.togglePlay", shortcut: "Space", labelId: "action.togglePlay", category: "playback" },
  { actionId: "transport.seekBack", shortcut: "ArrowLeft", labelId: "action.seekBack", category: "playback" },
  { actionId: "transport.seekForward", shortcut: "ArrowRight", labelId: "action.seekForward", category: "playback" },
  { actionId: "transport.previous", shortcut: "Shift+ArrowLeft", labelId: "action.previous", category: "playback" },
  { actionId: "transport.next", shortcut: "Shift+ArrowRight", labelId: "action.next", category: "playback" },
  { actionId: "ui.queue", shortcut: "Q", labelId: "action.toggleQueue", category: "view" },
  { actionId: "ui.lyrics", shortcut: "L", labelId: "action.toggleLyrics", category: "view" },
  { actionId: "ui.fullscreen", shortcut: "F", labelId: "action.fullscreen", category: "view" },
  { actionId: "ui.miniPlayer", shortcut: "M", labelId: "action.miniPlayer", category: "view" },
  { actionId: "rate.0", shortcut: "0", labelId: "action.rate0", category: "library" },
  { actionId: "rate.1", shortcut: "1", labelId: "action.rate1", category: "library" },
  { actionId: "rate.2", shortcut: "2", labelId: "action.rate2", category: "library" },
  { actionId: "rate.3", shortcut: "3", labelId: "action.rate3", category: "library" },
  { actionId: "rate.4", shortcut: "4", labelId: "action.rate4", category: "library" },
  { actionId: "rate.5", shortcut: "5", labelId: "action.rate5", category: "library" },
  { actionId: "undo", shortcut: "Mod+Z", labelId: "action.undo", category: "edit" },
  { actionId: "redo", shortcut: "Mod+Shift+Z", labelId: "action.redo", category: "edit" },
  { actionId: "redo.alt", shortcut: "Mod+Y", labelId: "action.redo", category: "edit" },
  { actionId: "ui.selectAll", shortcut: "Mod+A", labelId: "action.selectAll", category: "edit" },
  { actionId: "ui.delete", shortcut: "Delete", labelId: "action.delete", category: "edit" },
  { actionId: "ui.escape", shortcut: "Escape", labelId: "action.escape", category: "app" },
  { actionId: "ui.settings", shortcut: "Mod+,", labelId: "action.settings", category: "app" },
  { actionId: "transport.shuffle", shortcut: "S", labelId: "action.shuffle", category: "playback" },
  { actionId: "transport.repeat", shortcut: "R", labelId: "action.repeat", category: "playback" },
  { actionId: "transport.volumeUp", shortcut: "Mod+ArrowUp", labelId: "action.volumeUp", category: "playback" },
  { actionId: "transport.volumeDown", shortcut: "Mod+ArrowDown", labelId: "action.volumeDown", category: "playback" },
  { actionId: "track.love", shortcut: "Mod+L", labelId: "action.love", category: "library" },
  { actionId: "ui.back", shortcut: "Alt+ArrowLeft", labelId: "action.back", category: "view" },
  { actionId: "ui.forward", shortcut: "Alt+ArrowRight", labelId: "action.forward", category: "view" },
];
