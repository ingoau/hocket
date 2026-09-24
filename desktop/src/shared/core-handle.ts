// The one interface the main process programs against. Both the native napi
// addon (crates/hocket-node) and the TypeScript FakeCore implement it.
import type { Command, Event, Query, QueryResult, Snapshot } from "@core/api";

export type CoreKind = "native" | "fake";

export interface CoreHandle {
  readonly kind: CoreKind;
  dispatch(command: Command): void;
  query(query: Query): Promise<QueryResult>;
  /** Returns an unsubscribe function. Listeners are called on the main thread. */
  onEvent(listener: (event: Event) => void): () => void;
  /** Flush and stop; the process may exit afterwards. */
  shutdown(): Promise<void>;
}

/** Narrow a QueryResult to one variant or throw: the core answered with the wrong shape. */
export type ResultData<K extends QueryResult["type"]> = Extract<QueryResult, { type: K }>["data"];

export function expectResult<K extends QueryResult["type"]>(result: QueryResult, type: K): ResultData<K> {
  if (result.type !== type) {
    throw new Error(`expected query result '${type}', got '${result.type}'`);
  }
  return (result as unknown as { data: ResultData<K> }).data;
}

/**
 * The snapshot carried by an attach-style event: `started` (emitted once when
 * `Start` completes) and `snapshot` (the answer to `RequestSnapshot`; older
 * cores answer that with a second `started` instead). Anything else: undefined.
 */
export function snapshotOf(event: Event): Snapshot | undefined {
  const e = event as { type: string; data?: { snapshot?: Snapshot } };
  if (e.type !== "started" && e.type !== "snapshot") return undefined;
  const snapshot = e.data?.snapshot;
  return snapshot && typeof snapshot === "object" ? snapshot : undefined;
}
