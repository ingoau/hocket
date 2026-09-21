// Query hooks: fetch through the bridge, refetch on LibraryChanged.
import { useCallback, useEffect, useRef, useState } from "react";
import type { ActionDescriptor, ActionTarget, Query, QueryResult } from "@core/api";
import { expectResult, type ResultData } from "@shared/core-handle";
import { bridge } from "../core/bridge";
import { useApp } from "./app";

export interface QueryState<T> {
  data: T | undefined;
  loading: boolean;
  error: string | undefined;
  refetch: () => void;
}

type Keys = QueryResult["type"];

/**
 * `useQuery(() => query, "resultVariant", [deps])`. Passing `null` skips.
 * Refetches whenever the library changes (Event.LibraryChanged) unless
 * `opts.static` is set.
 */
export function useQuery<K extends Keys>(make: () => Query | null, result: K, deps: unknown[], opts: { static?: boolean; keep?: boolean } = {}): QueryState<ResultData<K>> {
  const [data, setData] = useState<ResultData<K> | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);
  const libraryVersion = useApp((s) => (opts.static ? 0 : s.libraryVersion));
  const seq = useRef(0);
  const [nonce, setNonce] = useState(0);
  const refetch = useCallback(() => setNonce((n) => n + 1), []);
  useEffect(() => {
    const q = make();
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
        setData(expectResult(r, result));
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
