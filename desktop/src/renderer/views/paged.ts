// Paged loading for virtualised grids/lists over Query results with `total`.
import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../store/app";
import { evictPages, materializePages } from "../lib/paged-window";

export function usePagedGrid<T>(fetchPage: (offset: number, limit: number) => Promise<{ items: T[]; total: number }>, deps: unknown[], pageSize = 100) {
  const [rows, setRows] = useState<(T | undefined)[]>([]);
  const [total, setTotal] = useState(0);
  const loading = useRef(new Set<number>());
  const gen = useRef(0);
  const pages = useRef(new Map<number, T[]>());
  const around = useRef(0);
  const libraryVersion = useApp((s) => s.libraryVersion);
  const load = useCallback((page: number) => {
    if (loading.current.has(page) || pages.current.has(page)) return;
    loading.current.add(page);
    const g = gen.current;
    void fetchPage(page * pageSize, pageSize)
      .then((res) => {
        if (g !== gen.current) return;
        pages.current.set(page, res.items);
        evictPages(pages.current, around.current);
        setTotal(res.total);
        setRows(materializePages(pages.current, res.total, pageSize));
      })
      .catch((err: unknown) => console.warn("[paged] load failed", err))
      .finally(() => loading.current.delete(page));
  }, [fetchPage, pageSize]);
  const depsKey = JSON.stringify(deps);
  const lastDeps = useRef<string | undefined>(undefined);
  useEffect(() => {
    gen.current += 1;
    loading.current.clear();
    const changed = lastDeps.current !== depsKey;
    lastDeps.current = depsKey;
    if (changed) {
      pages.current = new Map();
      around.current = 0;
      setRows([]);
      setTotal(0);
      load(0);
      return;
    }
    const keep = [...pages.current.keys()];
    pages.current = new Map();
    for (const p of keep.length ? keep : [0]) load(p);
  }, [depsKey, libraryVersion]);
  const onNeedRange = useCallback((start: number, end: number) => {
    around.current = Math.floor(((start + end) / 2) / pageSize);
    for (let p = Math.floor(Math.max(0, start) / pageSize); p <= Math.floor(Math.max(0, end) / pageSize); p++) {
      if (!pages.current.has(p) && p * pageSize < Math.max(total, 1)) load(p);
    }
  }, [load, pageSize, total]);
  return { rows, total, onNeedRange };
}
