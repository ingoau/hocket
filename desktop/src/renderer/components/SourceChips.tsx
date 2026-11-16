// The source filter chip (docs/providers.md, Phase 3): which account the library, search and
// playlists list. Shown only with more than one account; "All" is the unified library.
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { MIXED_SOURCE, sourceOptions } from "../lib/sources";

export function SourceChips({ mixed = false }: { mixed?: boolean }) {
  const accounts = useApp((s) => s.accounts);
  const source = useApp((s) => s.librarySource);
  const set = useApp((s) => s.setLibrarySource);
  const options = sourceOptions(accounts, mixed);
  if (!options.length) return null;
  // A choice another view offered (Mixed) reads as All here.
  const current = options.some((o) => o.id === source) ? source : "";
  return (
    <div className="search-kinds source-chips" role="group" aria-label={t("source.filter")} data-testid="source-chips">
      {options.map((o) => (
        <button key={o.id || "all"} type="button" className={`search-kind ${current === o.id ? "on" : ""}`} aria-pressed={current === o.id} onClick={() => set(o.id)} data-testid={`source-chip-${o.id === MIXED_SOURCE ? "mixed" : o.id || "all"}`}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** The account a list query names: the chip's, or "" for the unified library. */
export function useLibrarySource(mixed = false): string {
  const accounts = useApp((s) => s.accounts);
  const source = useApp((s) => s.librarySource);
  return sourceOptions(accounts, mixed).some((o) => o.id === source) ? source : "";
}
