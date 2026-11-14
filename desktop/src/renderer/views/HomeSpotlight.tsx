import { useEffect, useMemo, useRef, useState } from "react";
import { t } from "@shared/strings";
import type { ActionTarget, ListeningStats } from "@core/api";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { openContextMenu } from "../components/ContextMenu";
import { resolveArtwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { extractAccent } from "../lib/accent";
import { accentTokens, type ThemeName } from "../lib/contrast";
import { loadSpotlight, slideCover, slideKey, slideTitle, spotlightSeed, type SpotlightSlide } from "../lib/spotlight";

/** The theme in force (`data-theme` on the root, set by useTheme), following its changes. */
export function useResolvedTheme(): ThemeName {
  const read = (): ThemeName => (document.documentElement.getAttribute("data-theme") === "dark" ? "dark" : "light");
  const [theme, setTheme] = useState<ThemeName>(read);
  useEffect(() => {
    const mo = new MutationObserver(() => setTheme(read()));
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => mo.disconnect();
  }, []);
  return theme;
}

/** Each slide's accent from its cover (undefined until worked out, or with no cover). */
function useSlideAccents(slides: SpotlightSlide[]): Record<string, string | undefined> {
  const [accents, setAccents] = useState<Record<string, string | undefined>>({});
  const covers = slides.map((s) => slideCover(s) ?? "").join("|");
  useEffect(() => {
    let live = true;
    for (const s of slides) {
      const cover = slideCover(s);
      if (!cover) continue;
      void resolveArtwork(cover, 64).then((url) => (url ? extractAccent(url) : undefined)).then((c) => { if (live) setAccents((m) => (m[cover] === c ? m : { ...m, [cover]: c })); });
    }
    return () => { live = false; };
  }, [covers]); // eslint-disable-line react-hooks/exhaustive-deps
  return accents;
}

/**
 * Home's Spotlight carousel: up to five big picks (resume an album, what's on repeat, something new
 * …), each its cover across the slide fading into the page behind its reason, title and Play /
 * Shuffle / more. A slide's colours come from its cover but always in the app's light or dark: a
 * pale cover never turns a slide light in the dark theme. [onAccent] reports the slide in view's
 * accent, so the rest of Home can wear it; [onEmpty] says when there's nothing to show. With
 * [dynamic] (colour from artwork) off, slides keep the app's accent.
 *
 * It loads on arriving at Home, when another album starts, and when the favourites or the albums
 * to resume change; not per track. What's playing as it loads is left out, except from the slide
 * in view: pressing Play on a slide keeps it there rather than pulling it out from under the pointer.
 */
export function HomeSpotlight({ stats, statsReady, dynamic, onAccent, onEmpty }: { stats?: ListeningStats; statsReady: boolean; dynamic: boolean; onAccent: (accent: string | undefined) => void; onEmpty: (empty: boolean) => void }) {
  const serverId = useApp((s) => s.accounts[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const resumes = useApp((s) => s.albumResumes);
  const playingAlbum = useApp((s) => s.nowPlaying?.track.albumId);
  const resumeKey = resumes.map((r) => r.albumId).sort().join(",");
  // What the picks are made from, not the stats object itself (a new one after every library sync).
  const statsKey = stats ? [...stats.topAlbums.slice(0, 12).map((a) => a.id), "|", ...stats.topArtists.slice(0, 4).map((a) => a.id)].join(",") : "";
  const statsRef = useRef(stats);
  statsRef.current = stats;
  const theme = useResolvedTheme();
  const [slides, setSlides] = useState<SpotlightSlide[] | undefined>(undefined);
  const [index, setIndex] = useState(0);
  const section = useRef<HTMLElement>(null);
  const slidesRef = useRef(slides);
  slidesRef.current = slides;
  const shownKey = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!serverId) { setSlides([]); return; }
    if (!statsReady) return;
    let live = true;
    const st = useApp.getState();
    const track = st.nowPlaying?.track;
    const shown = slidesRef.current?.find((x) => slideKey(x) === shownKey.current);
    const fromShown = !!track && !!shown && (shown.album?.id === track.albumId || (!shown.album && shown.artist?.id === track.artistId));
    const playing = fromShown ? undefined : track;
    void loadSpotlight({ serverId, resumes: st.albumResumes, playing: { albumId: playing?.albumId, artistId: playing?.artistId }, stats: statsRef.current, seed: spotlightSeed(), query: (q) => bridge().query(q) })
      .then((s) => {
        if (!live) return;
        setSlides(s);
        // Stay on the slide in view if it's still there.
        const keep = shownKey.current ? s.findIndex((x) => slideKey(x) === shownKey.current) : -1;
        setIndex(keep >= 0 ? keep : 0);
      });
    return () => { live = false; };
  }, [serverId, statsReady, statsKey, resumeKey, playingAlbum]);
  const list = slides ?? [];
  const at = Math.min(index, Math.max(0, list.length - 1));
  shownKey.current = list[at] ? slideKey(list[at]) : undefined;
  const allAccents = useSlideAccents(list);
  const accents = dynamic ? allAccents : {};
  const latestResume = new Map(resumes.map((r) => [r.albumId, r]));
  const current = list[at];
  const currentAccent = current ? accents[slideCover(current) ?? ""] : undefined;
  useEffect(() => onAccent(currentAccent), [currentAccent]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => onEmpty(slides !== undefined && slides.length === 0), [slides]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!slides || !list.length) return null;
  const go = (i: number) => setIndex((i + list.length) % list.length);
  const onKey = (e: React.KeyboardEvent) => {
    if (e.target instanceof HTMLElement && e.target.closest("input, textarea")) return;
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    e.preventDefault();
    // Focus on a slide's control follows to the same control on the next slide (the old one is hidden).
    const role = e.target instanceof HTMLElement && e.target.closest(".spot-slide") ? e.target.dataset.spot : undefined;
    go(at + (e.key === "ArrowRight" ? 1 : -1));
    if (role) requestAnimationFrame(() => section.current?.querySelector<HTMLElement>(`.spot-slide.active [data-spot="${role}"]`)?.focus());
  };
  const open = (s: SpotlightSlide) => {
    if (s.album) navigate({ view: "album", id: s.album.id });
    else if (s.artist) navigate({ view: "artist", id: s.artist.id });
    else if (s.playlist) navigate({ view: "playlist", id: s.playlist.id });
  };
  return (
    <section ref={section} className="home-spotlight" aria-roledescription="carousel" aria-label={t("spotlight.label")} data-testid="home-spotlight" onKeyDown={onKey}>
      <div className="spot-track" style={{ transform: `translateX(${-at * 100}%)` }}>
        {list.map((s, i) => (
          <Slide key={slideKey(s)} slide={s.resume ? { ...s, resume: latestResume.get(s.resume.albumId) ?? s.resume } : s} active={i === at} position={t("spotlight.goTo", { n: i + 1, total: list.length })} accent={accents[slideCover(s) ?? ""]} theme={theme} serverId={serverId} onOpen={() => open(s)} />
        ))}
      </div>
      {list.length > 1 ? (
        <div className="spot-nav">
          <button type="button" className="btn icon spot-arrow" aria-label={t("spotlight.prev")} onClick={() => go(at - 1)} data-testid="spotlight-prev"><Icon name="chevronLeft" size={20} /></button>
          <div className="spot-dots">
            {list.map((s, i) => (
              <button key={slideKey(s)} type="button" className={`spot-dot ${i === at ? "on" : ""}`} aria-label={t("spotlight.goTo", { n: i + 1, total: list.length })} aria-current={i === at ? "true" : undefined} onClick={() => go(i)} />
            ))}
          </div>
          <button type="button" className="btn icon spot-arrow" aria-label={t("spotlight.next")} onClick={() => go(at + 1)} data-testid="spotlight-next"><Icon name="chevronRight" size={20} /></button>
        </div>
      ) : null}
    </section>
  );
}

function Slide({ slide, active, position, accent, theme, serverId, onOpen }: { slide: SpotlightSlide; active: boolean; position: string; accent?: string; theme: ThemeName; serverId: string; onOpen: () => void }) {
  const cover = slideCover(slide);
  const [url, setUrl] = useState<string | undefined>(undefined);
  useEffect(() => {
    let live = true;
    void resolveArtwork(cover, 1000).then((u) => { if (live) setUrl(u); });
    return () => { live = false; };
  }, [cover]);
  // Contrast-checked for the app's theme, never the cover's lightness.
  const vars = useMemo(() => {
    if (!accent) return undefined;
    const tk = accentTokens(accent, theme);
    return { "--spot-accent": tk.accent, "--spot-accent-fg": tk.accentFg, "--spot-tint": tk.accentTint } as React.CSSProperties;
  }, [accent, theme]);
  const title = slideTitle(slide);
  const eyebrow = eyebrowOf(slide);
  const sub = subtitleOf(slide);
  const tab = active ? undefined : -1;
  const target: ActionTarget | undefined = slide.album ? { type: "albums", data: { ids: [slide.album.id] } } : slide.artist ? { type: "artists", data: { ids: [slide.artist.id] } } : slide.playlist ? { type: "playlists", data: { ids: [slide.playlist.id] } } : undefined;
  return (
    <div className={`spot-slide ${active ? "active" : ""}`} role="group" aria-roledescription="slide" aria-label={position} aria-hidden={!active} style={vars} data-testid={`spotlight-slide-${slide.kind}`} onContextMenu={(e) => { if (target) void openContextMenu(e, target); }}>
      {url ? <img className="spot-bg" src={url} alt="" aria-hidden="true" /> : <span className="spot-bg spot-bg-empty" aria-hidden="true" />}
      {url ? <img className="spot-art" src={url} alt="" aria-hidden="true" /> : null}
      <span className="spot-scrim" aria-hidden="true" />
      <div className="spot-content">
        <span className="spot-eyebrow">{eyebrow}</span>
        <h2 className="spot-title"><button type="button" className="spot-open" data-spot="open" tabIndex={tab} onClick={onOpen} title={title}>{title}</button></h2>
        {sub ? <p className="spot-sub">{sub}</p> : null}
        <div className="spot-actions">
          <button type="button" className="btn spot-play" data-spot="play" tabIndex={tab} onClick={() => playSlide(serverId, slide, false)} data-testid="spotlight-play"><Icon name="play" size={18} /> {slide.resume ? t("album.resume") : t("album.play")}</button>
          <button type="button" className="btn icon spot-shuffle" data-spot="shuffle" tabIndex={tab} aria-label={`${t("album.shuffle")}: ${title}`} onClick={() => playSlide(serverId, slide, true)}><Icon name="shuffle" size={18} /></button>
          {target ? <button type="button" className="btn icon spot-more" data-spot="more" tabIndex={tab} aria-label={`${t("misc.more")}: ${title}`} onClick={(e) => void openContextMenu(e, target)}><Icon name="moreVert" size={18} /></button> : null}
        </div>
      </div>
    </div>
  );
}

function eyebrowOf(s: SpotlightSlide): string {
  if (s.kind === "topGenre") return s.genre ? t("spotlight.topGenre", { genre: s.genre }) : t("spotlight.topGenre.plain");
  return t(`spotlight.${s.kind}` as never);
}

/** "Halden Fields · 2025 · 23 plays"; "… · Track 4 of 10" to resume; an artist's albums; a playlist's songs. */
function subtitleOf(s: SpotlightSlide): string {
  if (s.artist) return t("spotlight.albums", { n: s.artist.albumCount });
  if (s.playlist) return [s.playlist.isMine ? undefined : s.playlist.owner, t("spotlight.songs", { n: s.playlist.songCount })].filter(Boolean).join(" · ");
  const a = s.album;
  if (!a) return "";
  const third = s.resume
    ? t("spotlight.track", { n: s.resume.trackIndex + 1, total: a.songCount })
    : a.playCount > 0 && s.kind !== "newlyAdded" && s.kind !== "luckyDip" ? (a.playCount === 1 ? t("spotlight.plays.one") : t("spotlight.plays", { n: a.playCount })) : undefined;
  return [a.artist ?? t("misc.unknownArtist"), a.year ? String(a.year) : undefined, third].filter(Boolean).join(" · ");
}

/** Play (or shuffle) a slide's album, artist or playlist; Resume carries on from the saved point. */
function playSlide(serverId: string, s: SpotlightSlide, shuffle: boolean): void {
  if (s.resume && !shuffle) {
    bridge().dispatch({ type: "resumeAlbum", data: { album_id: s.resume.albumId } });
    return;
  }
  const kind = s.album ? { type: "album", data: { id: s.album.id } } : s.artist ? { type: "artist", data: { id: s.artist.id } } : s.playlist ? { type: "playlist", data: { id: s.playlist.id } } : undefined;
  if (!kind) return;
  bridge().dispatch({ type: "playContext", data: { args: { context: { source: serverId, kind: kind as never, label: slideTitle(s), sort: "default", tracks: [] }, shuffle, saveOutgoing: true } } });
}
