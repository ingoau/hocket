// Generated lyrics in all three tiers so the renderer's honesty rules can be
// exercised: syllable (with duets and background lines), line, unsynced, none.
import type { LyricLine, LyricSyllable, Lyrics, Track } from "@core/api";
import { Rng, hash32 } from "./random";
import { adaptEnhanced } from "./enhanced-lyrics";
import { SHOWCASE_TRACK_ID, showcaseLyrics } from "./showcase-lyrics";

const WORDS = ["hold", "the", "light", "we", "carry", "home", "through", "every", "silver", "street", "and", "wait", "for", "morning", "to", "find", "us", "here", "again", "under", "paper", "skies", "you", "said", "nothing", "lasts", "but", "this", "does", "slowly", "burning", "down", "the", "harbour", "line"];

function lineText(rng: Rng, n: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < n; i++) out.push(rng.pick(WORDS));
  const first = out[0];
  if (first) out[0] = first.charAt(0).toUpperCase() + first.slice(1);
  return out;
}

export function lyricsFor(track: Track, offsetMs: number): Lyrics | undefined {
  // One track carries a real enhanced (syllable + background agent) document.
  if (track.id === SHOWCASE_TRACK_ID) return adaptEnhanced(track.id, showcaseLyrics(), offsetMs);
  const h = hash32(track.id);
  const bucket = h % 10;
  if (bucket >= 8) return undefined; // 20% have no lyrics
  const tier = bucket < 4 ? "syllable" : bucket < 7 ? "line" : "unsynced";
  const rng = new Rng(h);
  const duet = tier === "syllable" && rng.chance(0.4);
  const lines: LyricLine[] = [];
  const intro = 4000 + rng.int(0, 8000);
  const usable = Math.max(20_000, track.durationMs - intro - 4000);
  const lineCount = Math.max(6, Math.floor(usable / 3600));
  let cursor = intro;
  for (let i = 0; i < lineCount; i++) {
    const wordCount = rng.int(3, 8);
    const words = lineText(rng, wordCount);
    const lineDur = rng.int(1800, 3600);
    const gap = rng.chance(0.15) ? rng.int(1500, 6000) : rng.int(100, 500);
    const start = cursor;
    const end = start + lineDur;
    const text = words.join(" ");
    const agent = duet ? (rng.chance(0.5) ? "v1" : "v2") : undefined;
    let syllables: LyricSyllable[] = [];
    if (tier === "syllable") {
      let t = start;
      const per = lineDur / wordCount;
      syllables = words.map((w, wi) => {
        const s = { text: w, startMs: Math.round(t), endMs: Math.round(t + per * (0.85 + rng.next() * 0.15)), joined: false };
        t += per;
        // occasionally split a word into two syllables to prove `joined`
        if (w.length > 5 && rng.chance(0.3) && wi < wordCount - 1) {
          const cut = Math.ceil(w.length / 2);
          const mid = Math.round((s.startMs + s.endMs) / 2);
          return [
            { text: w.slice(0, cut), startMs: s.startMs, endMs: mid, joined: true },
            { text: w.slice(cut), startMs: mid, endMs: s.endMs, joined: false },
          ];
        }
        return [s];
      }).flat();
    }
    if (tier === "unsynced") {
      lines.push({ text, syllables: [], background: false });
    } else {
      lines.push({ startMs: start, endMs: end, text, syllables, agent, background: false, translation: undefined });
      if (tier === "syllable" && rng.chance(0.2)) {
        const bgWords = lineText(rng, 2);
        const bgStart = start + Math.round(lineDur * 0.5);
        const bgEnd = end + 600;
        lines.push({
          startMs: bgStart,
          endMs: bgEnd,
          text: `(${bgWords.join(" ")})`,
          syllables: bgWords.map((w, k) => ({ text: k === 0 ? `(${w}` : `${w})`, startMs: bgStart + k * 500, endMs: bgStart + (k + 1) * 500, joined: false })),
          agent,
          background: true,
          translation: undefined,
        });
      }
    }
    cursor = end + gap;
    if (cursor > track.durationMs - 3000) break;
  }
  return {
    trackId: track.id,
    tier,
    lang: "en",
    displayArtist: track.artist,
    displayTitle: track.title,
    agents: duet
      ? [
          { id: "v1", name: track.artist ?? "Voice 1", side: 0 },
          { id: "v2", name: "Guest", side: 1 },
        ]
      : [],
    lines,
    source: bucket === 3 ? "external" : "server",
    offsetMs,
  };
}
