// Theme (light/dark/system), accent colour setting and dynamic accent from artwork.
import { useEffect, useState } from "react";
import { useApp, useSetting } from "../store/app";
import { extractAccent } from "./accent";
import { resolveArtwork } from "../components/Artwork";
import { DEFAULT_ACCENT, SK } from "@shared/settings-keys";
import { accentTokens, type ThemeName } from "./contrast";

export function useTheme() {
  const theme = useSetting<"system" | "light" | "dark">(SK.displayTheme, "system");
  const accentSetting = useSetting<string | null>(SK.displayAccent, null) || DEFAULT_ACCENT;
  const dynamic = useSetting(SK.displayDynamicColour, true);
  const cover = useApp((s) => s.nowPlaying?.track.coverArt);
  const accent = useApp((s) => s.accent);
  const setAccent = useApp((s) => s.setAccent);
  const [resolved, setResolved] = useState<ThemeName>("light");
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const name: ThemeName = theme === "system" ? (mq.matches ? "dark" : "light") : theme;
      document.documentElement.setAttribute("data-theme", name);
      setResolved(name);
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    if (!dynamic || !cover) {
      setAccent(undefined);
      return;
    }
    let alive = true;
    void resolveArtwork(cover, 64).then((url) => url && extractAccent(url)).then((c) => alive && setAccent(c));
    return () => { alive = false; };
  }, [dynamic, cover, setAccent]);
  // Every accent-derived colour is contrast-checked for the theme (lib/contrast.ts).
  useEffect(() => {
    const tokens = accentTokens(accent ?? accentSetting, resolved);
    const style = document.documentElement.style;
    style.setProperty("--accent", tokens.accent);
    style.setProperty("--accent-fg", tokens.accentFg);
    style.setProperty("--accent-text", tokens.accentText);
    style.setProperty("--accent-inverse", tokens.accentInverse);
    style.setProperty("--accent-tint", tokens.accentTint);
    style.setProperty("--focus-ring", tokens.focusRing);
  }, [accent, accentSetting, resolved]);
}
