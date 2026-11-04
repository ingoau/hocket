// The matched phrase inside a cached lyric line. Offsets are Unicode scalar
// values, the same indexes the core counts.
export function LyricSnippet({ line, start, end }: { line: string; start: number; end: number }) {
  const chars = Array.from(line);
  const a = Math.max(0, Math.min(start, chars.length));
  const b = Math.max(a, Math.min(end, chars.length));
  return (
    <span className="lyric-snippet">
      {chars.slice(0, a).join("")}
      <mark>{chars.slice(a, b).join("")}</mark>
      {chars.slice(b).join("")}
    </span>
  );
}
