// Paged loading for virtualised grids/lists over Query results with `total`.
import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../store/app";

export function usePagedGrid<T>(fetchPage: (offset: number, limit: number) => Promise<{ items: T[]; total: number }>, deps: unknown[], pageSize = 100) {
  const [rows, setRows] = useState<(T | undefined)[]>([]);
  const [total, setTotal] = useState(0);
  const loading = useRef(new Set<number>());
  const gen = useRef(0);
  const libraryVersion = useApp((s) => s.libraryVersion);
  const load = useCallback((page: number) => {
    if (loading.current.has(page)) return;
    loading.current.add(page);
    const g = gen.current;
    void fetchPage(page * pageSize, pageSize)
      .then((res) => {
        if (g !== gen.current) return;
        setTotal(res.total);
        setRows((prev) => {
          const next: (T | undefined)[] = prev.length === res.total ? [...prev] : Array.from({ length: res.total }, (_, i) => prev[i]);
          res.items.forEach((it, i) => { next[page * pageSize + i] = it; });
          return next;
        });
      })
      .catch((err: unknown) => console.warn("[paged] load failed", err))
      .finally(() => loading.current.delete(page));
  }, [fetchPage, pageSize]);
  useEffect(() => {
    gen.current += 1;
    loading.current.clear();
    setRows([]);
    setTotal(0);
    load(0);
  }, [...deps, libraryVersion]);
  const onNeedRange = useCallback((start: number, end: number) => {
    for (let p = Math.floor(Math.max(0, start) / pageSize); p <= Math.floor(Math.max(0, end) / pageSize); p++) {
      if (rows[p * pageSize] === undefined && p * pageSize < Math.max(total, 1)) load(p);
    }
  }, [rows, load, pageSize, total]);
  return { rows, total, onNeedRange };
}
