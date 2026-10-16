import { useId } from "react";

/**
 * The app icon (docs/brand.md): the Crescendo wave in its two voices on the rounded tile, drawn
 * from the same geometry as build/icon.svg. Sized by CSS (1.15em in the lockup); decorative.
 */
export function BrandTile({ className }: { className?: string }) {
  const id = useId();
  return (
    <svg className={`brand-tile ${className ?? ""}`} viewBox="0 0 100 100" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2A2447" />
          <stop offset="1" stopColor="#1B1730" />
        </linearGradient>
      </defs>
      <rect width="100" height="100" rx="22.45" fill={`url(#${id})`} />
      <g fill="none" strokeWidth="12" strokeLinecap="round">
        <path stroke="#E4DFFF" d="M16 48.5 C23 37.81 26 37.81 33 48.5" />
        <path stroke="#8F80F5" d="M33 48.5 C40 63.2 43 63.2 50 48.5" />
        <path stroke="#E4DFFF" d="M50 48.5 C57 29.8 60 29.8 67 48.5" />
        <path stroke="#8F80F5" d="M67 48.5 C74 71.21 77 71.21 84 48.5" />
      </g>
    </svg>
  );
}
