// Maps api.Lyrics (the core's renderable model) onto AMLL's LyricLine/LyricWord
// model. Honesty rules from design.md "Lyrics → Tiers": syllable timing is
// used only when the core says the tier is Syllable; line tier becomes one
// word per line; unsynced becomes a plain list (AMLL gets lines with a
// monotonically increasing pseudo-time and is told not to animate them).
import type { Lyrics, LyricLine as ApiLine } from "@core/api";

export interface AmllWord {
  word: string;
  startTime: number;
  endTime: number;
}

export interface AmllLine {
  words: AmllWord[];
  translatedLyric: string;
  romanLyric: string;
  startTime: number;
  endTime: number;
  isBG: boolean;
  isDuet: boolean;
}

export interface MappedLyrics {
  lines: AmllLine[];
  tier: Lyrics["tier"];
  /** True when the lines carry no real timing (unsynced): render as a static list. */
  synced: boolean;
  offsetMs: number;
}

function sideOf(agent: string | undefined, lyrics: Lyrics): number {
  if (!agent) return 0;
  const a = lyrics.agents.find((x) => x.id === agent);
  if (a) return a.side;
  // Unknown agent id: treat the first seen as primary, others as secondary.
  const first = lyrics.lines.find((l) => l.agent)?.agent;
  return agent === first ? 0 : 1;
}

function lineWords(line: ApiLine, tier: Lyrics["tier"], offset: number): AmllWord[] {
  const start = (line.startMs ?? 0) + offset;
  const end = Math.max(start, (line.endMs ?? line.startMs ?? 0) + offset);
  if (tier === "syllable" && line.syllables.length) {
    const out: AmllWord[] = [];
    line.syllables.forEach((s, i) => {
      const text = s.joined || i === line.syllables.length - 1 ? s.text : `${s.text} `;
      out.push({ word: text, startTime: Math.max(0, s.startMs + offset), endTime: Math.max(0, s.endMs + offset) });
    });
    return out;
  }
  // Line tier: one word with the line's timing. Never fabricate syllables.
  return [{ word: line.text, startTime: Math.max(0, start), endTime: Math.max(0, end) }];
}

export function mapLyrics(lyrics: Lyrics): MappedLyrics {
  const offset = lyrics.offsetMs;
  if (lyrics.tier === "unsynced") {
    // Pseudo-timing so AMLL can lay the lines out; the view renders it as static text.
    let t = 0;
    const lines = lyrics.lines.map((l) => {
      const start = t;
      t += 4000;
      return { words: [{ word: l.text, startTime: start, endTime: t }], translatedLyric: l.translation ?? "", romanLyric: "", startTime: start, endTime: t, isBG: l.background, isDuet: sideOf(l.agent, lyrics) === 1 };
    });
    return { lines, tier: "unsynced", synced: false, offsetMs: offset };
  }
  const lines: AmllLine[] = [];
  for (const l of lyrics.lines) {
    if (l.startMs == null) continue; // synced tier but this line has no timing (null from the core): skip rather than guess
    const words = lineWords(l, lyrics.tier, offset);
    const startTime = Math.max(0, (l.startMs ?? 0) + offset);
    const endTime = Math.max(startTime, (l.endMs ?? words[words.length - 1]?.endTime ?? startTime) + (l.endMs === undefined ? 0 : offset));
    lines.push({ words, translatedLyric: l.translation ?? "", romanLyric: "", startTime, endTime: Math.max(endTime, words[words.length - 1]?.endTime ?? endTime), isBG: l.background, isDuet: sideOf(l.agent, lyrics) === 1 });
  }
  return { lines, tier: lyrics.tier, synced: true, offsetMs: offset };
}

/** Index of the line active at `timeMs` (last line whose start <= time), or -1. */
export function activeLineIndex(lines: AmllLine[], timeMs: number): number {
  let idx = -1;
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i] as AmllLine;
    if (l.isBG) continue;
    if (l.startTime <= timeMs) idx = i;
    else break;
  }
  return idx;
}
