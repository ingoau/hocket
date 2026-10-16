/**
 * The bare Crescendo wave (docs/brand.md), from the same geometry as build/icon.svg, cropped to the
 * wave. Its colours come from CSS (`--brand-voice-1` / `--brand-voice-2`): the two voices on dark
 * grounds, the word mark's ink on light ones, where the pale voice would vanish. Decorative.
 */
export function BrandMark({ className }: { className?: string }) {
  return (
    <svg className={`brand-mark ${className ?? ""}`} viewBox="10 28.4 80 43.2" aria-hidden="true" focusable="false">
      <g fill="none" strokeWidth="12" strokeLinecap="round">
        <path className="v1" d="M16 48.5 C23 37.81 26 37.81 33 48.5" />
        <path className="v2" d="M33 48.5 C40 63.2 43 63.2 50 48.5" />
        <path className="v1" d="M50 48.5 C57 29.8 60 29.8 67 48.5" />
        <path className="v2" d="M67 48.5 C74 71.21 77 71.21 84 48.5" />
      </g>
    </svg>
  );
}
