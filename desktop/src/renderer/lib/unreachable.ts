// "Can't reach the server", said only once it has been true for a while. The
// core reports AccountInfo.reachable false at launch until its first check
// finishes, and again for a moment around every reconnect; shown at once,
// the hint would flash every time. So a server counts as unreachable once it
// has been out of reach, with the network up, for UNREACHABLE_GRACE_MS on end.
import { useSyncExternalStore } from "react";
import type { AccountInfo } from "@core/api";
import { unreachableServers } from "./offline";
import { selectOfflineReason, useApp } from "../store/app";

export const UNREACHABLE_GRACE_MS = 10_000;

/**
 * Since when each server has been out of reach with the network up. Any break (reachable, or the
 * network down, or the server removed) starts the count again.
 */
export class UnreachableWatch {
  private since = new Map<string, number>();
  private shown: readonly string[] = [];

  constructor(private readonly graceMs = UNREACHABLE_GRACE_MS) {}

  /** Takes in the accounts as they are now; true when the servers to show changed. */
  update(accounts: readonly AccountInfo[], offline: boolean, now: number): boolean {
    const next = new Map<string, number>();
    for (const a of unreachableServers(accounts, offline)) next.set(a.id, this.since.get(a.id) ?? now);
    this.since = next;
    return this.refresh(now);
  }

  /** The time has moved on (a timer): true when the servers to show changed. */
  refresh(now: number): boolean {
    const shown = [...this.since].filter(([, at]) => now - at >= this.graceMs).map(([id]) => id);
    if (shown.length === this.shown.length && shown.every((id, i) => id === this.shown[i])) return false;
    this.shown = shown;
    return true;
  }

  /** The ids of the servers to show as unreachable. */
  ids(): readonly string[] {
    return this.shown;
  }

  /** How long until the next server is due to show; undefined when none is waiting. */
  nextDueIn(now: number): number | undefined {
    let due: number | undefined;
    for (const at of this.since.values()) {
      const left = at + this.graceMs - now;
      if (left > 0 && (due === undefined || left < due)) due = left;
    }
    return due;
  }
}

// One watch for the app, fed by the store from the first component that asks, so every place that
// shows the hint agrees and opening a popover doesn't start the count again.
const watch = new UnreachableWatch();
const listeners = new Set<() => void>();
let started = false;
let timer: ReturnType<typeof setTimeout> | undefined;

function notify(): void {
  for (const l of listeners) l();
}

function schedule(): void {
  if (timer) clearTimeout(timer);
  timer = undefined;
  const due = watch.nextDueIn(Date.now());
  if (due === undefined) return;
  timer = setTimeout(() => {
    timer = undefined;
    if (watch.refresh(Date.now())) notify();
    schedule();
  }, due);
}

function feed(): void {
  const s = useApp.getState();
  if (watch.update(s.accounts, !!selectOfflineReason(s), Date.now())) notify();
  schedule();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (!started) {
    started = true;
    feed();
    useApp.subscribe((s, prev) => {
      if (s.accounts !== prev.accounts || s.network !== prev.network || s.settings !== prev.settings || s.leavingOfflineMode !== prev.leavingOfflineMode) feed();
    });
  }
  return () => listeners.delete(listener);
}

const snapshot = () => watch.ids();

/** The ids of the servers that have been out of reach, with the network up, for a while. */
export function useUnreachableIds(): readonly string[] {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}
