// The Add to playlist picker on its own: the context menu's submenu (PlaylistSubmenu), opened beside
// the "+" in the player bar, the side panel and the full-screen player (or by the shortcut), so
// adding to a playlist looks and works the same wherever it starts. It closes on a click outside,
// Escape or Tab, and focus goes back to whatever opened it.
import { useEffect, useRef } from "react";
import { useApp } from "../store/app";
import { usePresence } from "../lib/presence";
import { PlaylistSubmenu } from "./PlaylistSubmenu";

export function PlaylistPicker() {
  const live = useApp((s) => s.playlistPicker);
  const close = useApp((s) => s.closePlaylistPicker);
  const ref = useRef<HTMLDivElement>(null);
  const { value, motion, closing, exitProps } = usePresence(live, ref);
  const returnTo = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!live) return;
    const a = document.activeElement;
    returnTo.current = a instanceof HTMLElement && a !== document.body ? a : null;
    // Its changes apply at once with an Undo toast, so a click on the toasts keeps it open.
    const onDown = (e: MouseEvent) => {
      const at = e.target as Element | null;
      // So does one on the button that opened it: its click closes it (store/actions).
      if (at?.closest?.("[data-context-layer]") || at?.closest?.('[data-testid="toasts"]') || (returnTo.current?.hasAttribute("aria-haspopup") && at && returnTo.current.contains(at))) return;
      close();
    };
    const onBlur = () => close();
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
      // Back to the button, unless something else (a dialog, a click elsewhere) took focus.
      const el = returnTo.current;
      returnTo.current = null;
      const current = document.activeElement;
      const lost = !current || current === document.body || !!current.closest?.("[data-context-layer]");
      if (el?.isConnected && lost && !useApp.getState().dialog) el.focus({ preventScroll: true });
    };
  }, [live, close]);

  if (!value) return null;
  const finish = (then?: () => void) => {
    close();
    then?.();
  };
  return <PlaylistSubmenu key={value.trackIds.join()} trackIds={value.trackIds} anchor={value.anchor} closing={closing} motion={motion} onBack={close} onClose={finish} exitProps={exitProps} />;
}
