// Query hooks: fetch through the bridge, refetch on the LibraryChanged events that touch them.
import { useCallback, useEffect, useRef, useState } from "react";
import type { ActionDescriptor, ActionTarget, Query, QueryResult } from "@core/api";
import { expectResult, type ResultData } from "@shared/core-handle";
import { bridge } from "../core/bridge";
import { useApp } from "./app";
import { changesSince, queryLibraryDeps, shownIds, touchesAny, type LibraryDeps } from "./library-changes";

export interface QueryState<T> {
  data: T | undefined;
  loading: boolean;
  error: string | undefined;
  refetch: () => void;
}

type Keys = QueryResult["type"];

/**
 * A number that moves when a LibraryChanged touches `deps` (see `library-changes.ts`). While the
 * page is hidden (the window in the tray, minimised) changes wait: one refetch when it is shown.
 * `null` deps never move. `shownIds` lists the track rows on screen, for `rows` deps.
 */
export function useLibraryVersion(deps: LibraryDeps | null, shownIds?: () => Iterable<string> | undefined): number {
  // -1 while there is nothing to refetch: no re-render per change, and no catching up afterwards
  // (whatever made the deps non-null fetches anyway).
  const seq = useApp((s) => (deps === null ? -1 : s.libraryChanges.seq));
  const visible = useApp((s) => s.pageVisible);
  const [version, setVersion] = useState(0);
  const seen = useRef(seq);
  const latest = useRef({ deps, shownIds });
  latest.current = { deps, shownIds };
  useEffect(() => {
    const { deps: d, shownIds: shown } = latest.current;
    if (seen.current === seq) return;
    if (d === null || seq === -1 || seen.current === -1) {
      seen.current = seq;
      return;
    }
    if (!visible) return;
    const changes = changesSince(useApp.getState().libraryChanges, seen.current);
    seen.current = seq;
    if (touchesAny(d, changes, shown)) setVersion((v) => v + 1);
  }, [seq, visible]);
  return version;
}

/**
 * `useQuery(() => query, "resultVariant", [deps])`. Passing `null` skips.
 * Refetches when a LibraryChanged touches the tables the query reads
 * (`queryLibraryDeps`) unless `opts.static` is set.
 */
export function useQuery<K extends Keys>(make: () => Query | null, result: K, deps: unknown[], opts: { static?: boolean; keep?: boolean } = {}): QueryState<ResultData<K>> {
  const [data, setData] = useState<ResultData<K> | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);
  const q = make();
  const dataRef = useRef(data);
  dataRef.current = data;
  // Nothing loaded (yet, or it failed) has no rows a track's change could touch.
  const libraryVersion = useLibraryVersion(opts.static || !q ? null : queryLibraryDeps(q), () => (dataRef.current === undefined ? [] : shownIds(dataRef.current)));
  const seq = useRef(0);
  const [nonce, setNonce] = useState(0);
  const refetch = useCallback(() => setNonce((n) => n + 1), []);
  useEffect(() => {
    if (!q) {
      setData(undefined);
      return;
    }
    const mine = ++seq.current;
    setLoading(true);
    bridge()
      .query(q)
      .then((r) => {
        if (mine !== seq.current) return;
        setData(expectResult(r, result) as never);
        setError(undefined);
      })
      .catch((err: unknown) => {
        if (mine !== seq.current) return;
        setError(String(err));
        if (!opts.keep) setData(undefined);
      })
      .finally(() => {
        if (mine === seq.current) setLoading(false);
      });
  }, [...deps, libraryVersion, nonce]);
  return { data, loading, error, refetch };
}

/** Actions applicable to a target for a surface, ordered per the user's customisation. */
export function useActions(surface: string, target: ActionTarget | null, extraDeps: unknown[] = []): ActionDescriptor[] {
  const actionsVersion = useApp((s) => s.actionsVersion);
  const key = JSON.stringify(target);
  const { data } = useQuery(() => (target ? { type: "actions", data: { surface, target } } : null), "actions", [surface, key, actionsVersion, ...extraDeps], { static: true });
  return data ?? [];
}

export async function fetchActions(surface: string, target: ActionTarget): Promise<ActionDescriptor[]> {
  return expectResult(await bridge().query({ type: "actions", data: { surface, target } }), "actions");
}
