// Adapts an OpenSubsonic songLyrics v2 *enhanced* entry (`cueLine` with
// per-syllable `cue`s and inclusive `byteStart`/`byteEnd` offsets, plus
// `agents` with a `bg` role) into the core's renderable `Lyrics`, the same
// way crates/hocket-core/src/lyrics/adapt.rs does, so the FakeCore can serve
// a real syllable-tier document and the e2e suite can assert word-level sync.
//
// Rules (design.md "Lyrics → Tiers"): syllables exist only when every cue on
// a line has a start; a cue line with no cues is a plain timed line; nothing
// is interpolated. A cue is `joined` to the next one when the next cue's
// byteStart is exactly this cue's byteEnd + 1 (no byte between them, so no
// space): "ti" + "tle" is one word, "I" + "lost" are two.
import type { LyricLine, LyricSyllable, Lyrics, LyricsAgent } from "@core/api";

export interface EnhancedCue {
  start?: number;
  end?: number;
  value: string;
  byteStart?: number;
  byteEnd?: number;
}

export interface EnhancedCueLine {
  index?: number;
  start?: number;
  end?: number;
  value: string;
  agentId?: string;
  cue?: EnhancedCue[];
}

export interface EnhancedLyrics {
  displayArtist?: string;
  displayTitle?: string;
  lang?: string;
  kind?: string;
  synced: boolean;
  offset?: number;
  line: { start?: number; value: string }[];
  agents?: { id: string; role: string; name?: string }[];
  cueLine?: EnhancedCueLine[];
}

const utf8 = new TextEncoder();

/** Byte length of `s` in UTF-8 (offsets in the payload are byte offsets). */
export function byteLength(s: string): number {
  return utf8.encode(s).length;
}

function syllablesFrom(cl: EnhancedCueLine, shift: number): LyricSyllable[] {
  const cues = cl.cue ?? [];
  if (!cues.length || cues.some((c) => c.start === undefined)) return [];
  const out: LyricSyllable[] = [];
  for (let i = 0; i < cues.length; i++) {
    const cue = cues[i]!;
    if (!cue.value.trim()) {
      const prev = out[out.length - 1];
      if (prev) prev.joined = false;
      continue;
    }
    const next = cues.slice(i + 1).find((n) => n.value.trim());
    const start = (cue.start ?? 0) + shift;
    const end = (cue.end ?? next?.start ?? cl.end ?? start - shift) + shift;
    // Inclusive byte offsets: adjacent when the next cue starts one byte after
    // this one ends. Fall back to whitespace when a server omits the offsets.
    let joined: boolean;
    if (next && cue.byteEnd !== undefined && next.byteStart !== undefined) joined = next.byteStart === cue.byteEnd + 1;
    else joined = !!next && !/\s$/.test(cue.value) && !/^\s/.test(next.value);
    out.push({ text: cue.value.trimEnd(), startMs: Math.max(0, start), endMs: Math.max(0, Math.max(end, start)), joined });
  }
  const last = out[out.length - 1];
  if (last) last.joined = false;
  return out;
}

/** Side assignment mirrors the core: main voices alternate, `bg` shares the previous side. */
function buildAgents(entry: EnhancedLyrics): LyricsAgent[] {
  const ordered = [...(entry.agents ?? [])].sort((a, b) => Number(b.role.toLowerCase() === "main") - Number(a.role.toLowerCase() === "main"));
  let voice = 0;
  let lastSide = 0;
  return ordered.map((a) => {
    let side: number;
    if (a.role.toLowerCase() === "bg") side = lastSide;
    else {
      side = voice % 2;
      voice += 1;
      lastSide = side;
    }
    return { id: a.id, name: a.name, side };
  });
}

export function adaptEnhanced(trackId: string, entry: EnhancedLyrics, offsetMs = 0): Lyrics {
  // Server `offset` follows the LRC convention (positive = lyrics early → move earlier).
  const shift = -(entry.offset ?? 0);
  const bgIds = new Set((entry.agents ?? []).filter((a) => a.role.toLowerCase() === "bg").map((a) => a.id));
  const synced = entry.synced && entry.line.some((l) => l.start !== undefined);
  const cueLines = entry.cueLine ?? [];
  const indexed = cueLines.every((c) => c.index !== undefined);
  const lines: LyricLine[] = [];
  const pushCueLine = (cl: EnhancedCueLine, raw: { start?: number; value: string } | undefined) => {
    const syllables = synced ? syllablesFrom(cl, shift) : [];
    const start = cl.start ?? raw?.start;
    const lastSyl = syllables[syllables.length - 1];
    lines.push({
      startMs: synced && start !== undefined ? Math.max(0, start + shift) : undefined,
      endMs: synced ? (cl.end !== undefined ? Math.max(0, cl.end + shift) : lastSyl?.endMs) : undefined,
      text: cl.value || raw?.value || "",
      syllables,
      agent: cl.agentId,
      background: cl.agentId !== undefined && bgIds.has(cl.agentId),
      translation: undefined,
    });
  };
  if (indexed && cueLines.length) {
    // Every cue line is kept, in order: a background line shares the `index`
    // of the main line it decorates and directly follows it.
    entry.line.forEach((raw, i) => {
      const own = cueLines.filter((c) => c.index === i);
      if (own.length) for (const cl of own) pushCueLine(cl, raw);
      else lines.push({ startMs: synced && raw.start !== undefined ? Math.max(0, raw.start + shift) : undefined, endMs: undefined, text: raw.value, syllables: [], agent: undefined, background: false, translation: undefined });
    });
  } else {
    entry.line.forEach((raw, i) => {
      const cl = cueLines[i];
      if (cl) pushCueLine(cl, raw);
      else lines.push({ startMs: synced && raw.start !== undefined ? Math.max(0, raw.start + shift) : undefined, endMs: undefined, text: raw.value, syllables: [], agent: undefined, background: false, translation: undefined });
    });
  }
  if (synced) {
    for (let i = 0; i < lines.length; i++) {
      const l = lines[i]!;
      if (l.endMs === undefined && l.startMs !== undefined) {
        const next = lines.slice(i + 1).find((n) => n.startMs !== undefined)?.startMs;
        if (next !== undefined && next >= l.startMs) l.endMs = next;
      }
    }
  }
  const tier: Lyrics["tier"] = !synced ? "unsynced" : lines.some((l) => l.syllables.length >= 2) ? "syllable" : "line";
  const lang = entry.lang && entry.lang !== "xxx" && entry.lang !== "und" ? entry.lang : undefined;
  return { trackId, tier, lang, displayArtist: entry.displayArtist, displayTitle: entry.displayTitle, agents: buildAgents(entry), lines, source: "server", offsetMs };
}
