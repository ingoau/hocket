// A syllable-timed OpenSubsonic v2 enhanced payload in exactly the shape a
// Navidrome 0.64 `getLyricsBySongId?enhanced=true` answer has (see the real
// "Tally" capture used for the review): `line` carries one entry per main
// line, `cueLine` carries more (a background-vocal agent `__nd_bg__|v1` with
// role `bg` adds lines that share the `index` of the main line they follow),
// each cue has 0-based INCLUSIVE `byteStart`/`byteEnd` offsets into the
// line's UTF-8 value, words are split into real syllables ("ti" + "tle"), a
// line's `end` can sit well before the next line's `start` (a gap: the last
// syllable holds, nothing sweeps), one line has no cues at all (it renders at
// line tier) and one word is multi-byte UTF-8.
//
// The words are invented; the timing and structure are what matter. The
// FakeCore serves it for `SHOWCASE_TRACK_ID` so the e2e suite can assert
// word-level sync against a document the real adapter would also produce.
import type { EnhancedCue, EnhancedCueLine, EnhancedLyrics } from "./enhanced-lyrics";

export const SHOWCASE_TRACK_ID = "tr-1";
export const SHOWCASE_TITLE = "Word by Word";
export const SHOWCASE_DURATION_MS = 92_000;

const MAIN = "v1";
const BG = "__nd_bg__|v1";
const enc = new TextEncoder();

/** Cues for `value` from `[syllable, durationMs]` pairs, offsets computed from the text itself. */
function cues(value: string, start: number, parts: [string, number][]): EnhancedCue[] {
  const out: EnhancedCue[] = [];
  let t = start;
  let bytePos = 0;
  let charPos = 0;
  for (const [text, dur] of parts) {
    const at = value.indexOf(text, charPos);
    if (at < 0) throw new Error(`syllable "${text}" not in "${value}"`);
    bytePos += enc.encode(value.slice(charPos, at)).length;
    const len = enc.encode(text).length;
    out.push({ start: t, end: t + dur, byteStart: bytePos, byteEnd: bytePos + len - 1, value: text });
    bytePos += len;
    charPos = at + text.length;
    t += dur;
  }
  return out;
}

function cueLine(index: number, start: number, value: string, agentId: string, parts: [string, number][], end?: number): EnhancedCueLine {
  const c = cues(value, start, parts);
  return { index, start, end: end ?? c[c.length - 1]!.end, value, agentId, cue: c };
}

export function showcaseLyrics(): EnhancedLyrics {
  const cueLine_: EnhancedCueLine[] = [
    // "title" is two syllables; "I" is byteStart 0 byteEnd 0 like the real capture.
    cueLine(0, 1500, "I lost my rank and title", MAIN, [["I", 300], ["lost", 250], ["my", 200], ["rank", 250], ["and", 180], ["ti", 200], ["tle", 300]]),
    cueLine(1, 3400, "I turned my back on it", MAIN, [["I", 320], ["turned", 210], ["my", 200], ["back", 190], ["on", 180], ["it", 300]]),
    cueLine(2, 5000, "Sold it all at a discount (Yeah, yeah)", MAIN, [["Sold", 300], ["it", 150], ["all", 250], ["at", 150], ["a", 100], ["dis", 250], ["count", 400]]),
    // Background vocal on the same index, starting inside the main line and ending after it.
    cueLine(2, 6900, "(Yeah, yeah)", BG, [["(Yeah,", 500], ["yeah)", 600]]),
    // Ends 1.6 s before the next line starts: a gap the renderer must hold, not sweep.
    cueLine(3, 8800, "I wanted to progress things", MAIN, [["I", 250], ["wan", 200], ["ted", 200], ["to", 150], ["pro", 220], ["gress", 260], ["things", 420]], 10_500),
    cueLine(4, 12_100, "I wanted my soul set free", MAIN, [["I", 250], ["wan", 200], ["ted", 200], ["my", 180], ["soul", 300], ["set", 220], ["free", 500]]),
    cueLine(5, 14_200, "Lost it all at a discount (Yeah, yeah)", MAIN, [["Lost", 300], ["it", 150], ["all", 250], ["at", 150], ["a", 100], ["dis", 250], ["count", 400]]),
    cueLine(5, 16_100, "(Yeah, yeah)", BG, [["(Yeah,", 500], ["yeah)", 600]]),
    // Multi-byte UTF-8 ("café" is 5 bytes) so the inclusive offsets are byte offsets, not char indexes.
    cueLine(6, 18_000, "Met you at the café on the corner", MAIN, [["Met", 250], ["you", 200], ["at", 150], ["the", 150], ["ca", 200], ["fé", 300], ["on", 150], ["the", 150], ["cor", 200], ["ner", 350]]),
    // A cue line with no cues at all: honest fallback to a plain timed line.
    { index: 7, start: 21_000, end: 23_500, value: "We've seen it several times", agentId: MAIN, cue: [] },
    cueLine(8, 24_000, "You want a tally, I lost the count", MAIN, [["You", 250], ["want", 220], ["a", 120], ["tal", 200], ["ly,", 300], ["I", 200], ["lost", 250], ["the", 150], ["count", 500]]),
    cueLine(9, 27_000, "You want to love me, I'll let you down", MAIN, [["You", 250], ["want", 220], ["to", 120], ["love", 300], ["me,", 300], ["I'll", 250], ["let", 200], ["you", 200], ["down", 500]]),
    cueLine(10, 30_500, "Still now, you believe in me somehow", MAIN, [["Still", 350], ["now,", 400], ["you", 200], ["be", 150], ["lieve", 350], ["in", 150], ["me", 250], ["some", 250], ["how", 600]]),
    cueLine(10, 33_600, "(Somehow)", BG, [["(Some", 400], ["how)", 600]]),
    cueLine(11, 36_000, "When I replay it in my mind", MAIN, [["When", 250], ["I", 150], ["re", 200], ["play", 300], ["it", 150], ["in", 150], ["my", 200], ["mind", 600]]),
    cueLine(12, 39_000, "I see your heart break every time", MAIN, [["I", 200], ["see", 250], ["your", 200], ["heart", 300], ["break", 300], ["ev", 150], ["ery", 200], ["time", 600]]),
    cueLine(13, 42_000, "Still now, you believe in me somehow", MAIN, [["Still", 350], ["now,", 400], ["you", 200], ["be", 150], ["lieve", 350], ["in", 150], ["me", 250], ["some", 250], ["how", 600]]),
    cueLine(13, 45_100, "(You believe in me somehow)", BG, [["(You", 300], ["be", 150], ["lieve", 300], ["in", 150], ["me", 200], ["some", 250], ["how)", 500]]),
    cueLine(14, 50_000, "You believe in me somehow", MAIN, [["You", 300], ["be", 150], ["lieve", 350], ["in", 150], ["me", 250], ["some", 250], ["how", 700]]),
    cueLine(15, 56_000, "You believe in me somehow", MAIN, [["You", 300], ["be", 150], ["lieve", 350], ["in", 150], ["me", 250], ["some", 250], ["how", 700]]),
  ];
  const mainLines = new Map<number, EnhancedCueLine>();
  for (const c of cueLine_) if (c.agentId === MAIN && !mainLines.has(c.index!)) mainLines.set(c.index!, c);
  const line = [...mainLines.values()].sort((a, b) => a.index! - b.index!).map((c) => ({ start: c.start, value: c.value }));
  return {
    displayArtist: "The Ambers",
    displayTitle: SHOWCASE_TITLE,
    kind: "main",
    lang: "en",
    synced: true,
    line,
    agents: [{ id: MAIN, role: "main" }, { id: BG, role: "bg" }],
    cueLine: cueLine_,
  };
}
