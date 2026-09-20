// Ranking for the command palette: library results first, then actions.
// Scoring is simple and predictable: exact prefix > word prefix > substring
// > fuzzy subsequence, with a small boost for shorter labels.
export interface Rankable {
  id: string;
  label: string;
  /** Secondary text (artist, category) that matches with lower weight. */
  sub?: string;
  kind: "track" | "album" | "artist" | "playlist" | "action";
}

export interface Ranked<T extends Rankable> {
  item: T;
  score: number;
  /** Indices in `label` to highlight. */
  matches: number[];
}

function subsequence(query: string, text: string): number[] | undefined {
  const out: number[] = [];
  let ti = 0;
  for (const qc of query) {
    const idx = text.indexOf(qc, ti);
    if (idx < 0) return undefined;
    out.push(idx);
    ti = idx + 1;
  }
  return out;
}

export function scoreLabel(query: string, label: string): { score: number; matches: number[] } | undefined {
  const q = query.toLowerCase().trim();
  const l = label.toLowerCase();
  if (!q) return { score: 1, matches: [] };
  if (l === q) return { score: 1000, matches: range(0, q.length) };
  if (l.startsWith(q)) return { score: 800 - l.length * 0.5, matches: range(0, q.length) };
  const wordIdx = l.search(new RegExp(`(^|[\\s\\-_(/])${escape(q)}`));
  if (wordIdx >= 0) {
    const start = wordIdx === 0 && l.startsWith(q) ? 0 : wordIdx + 1;
    return { score: 600 - l.length * 0.5, matches: range(start, start + q.length) };
  }
  const sub = l.indexOf(q);
  if (sub >= 0) return { score: 400 - sub - l.length * 0.25, matches: range(sub, sub + q.length) };
  const seq = subsequence(q, l);
  if (seq && q.length >= 2) {
    // Penalise gaps.
    const span = (seq[seq.length - 1] ?? 0) - (seq[0] ?? 0) + 1;
    return { score: 200 - (span - q.length) * 4 - l.length * 0.25, matches: seq };
  }
  return undefined;
}

function range(a: number, b: number): number[] {
  const out: number[] = [];
  for (let i = a; i < b; i++) out.push(i);
  return out;
}

function escape(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const KIND_ORDER: Record<Rankable["kind"], number> = { track: 0, album: 1, artist: 2, playlist: 3, action: 4 };

export function rank<T extends Rankable>(query: string, items: T[], limit = 50): Ranked<T>[] {
  const out: Ranked<T>[] = [];
  for (const item of items) {
    const primary = scoreLabel(query, item.label);
    const secondary = item.sub ? scoreLabel(query, item.sub) : undefined;
    if (!primary && !secondary) continue;
    const score = Math.max(primary?.score ?? 0, (secondary?.score ?? 0) * 0.6);
    if (query.trim() && score <= 0) continue;
    out.push({ item, score, matches: primary?.matches ?? [] });
  }
  out.sort((a, b) => {
    // Results (library) before actions, then by score.
    const ka = a.item.kind === "action" ? 1 : 0;
    const kb = b.item.kind === "action" ? 1 : 0;
    if (ka !== kb) return ka - kb;
    if (b.score !== a.score) return b.score - a.score;
    if (KIND_ORDER[a.item.kind] !== KIND_ORDER[b.item.kind]) return KIND_ORDER[a.item.kind] - KIND_ORDER[b.item.kind];
    return a.item.label.localeCompare(b.item.label);
  });
  return out.slice(0, limit);
}
