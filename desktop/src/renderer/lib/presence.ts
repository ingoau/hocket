// Exit animations for transient surfaces (dialogs, menus, popovers, toasts).
//
// React unmounts a closed surface in the same commit that closes it, so a CSS
// exit never gets a frame to play. usePresence keeps the surface mounted for
// one more animation: `value` holds the last open value while `closing` is
// true, the element swaps its `fade-in` class for `closing` (the stylesheet's
// exit keyframes, `animation-fill-mode: forwards` so it holds the end state
// until React removes it) and is inert, so it can't take focus, keys or
// clicks on the way out. It unmounts on its own animationend, at once when
// there is no exit animation to wait for (reduced motion turns them all off),
// or after a fallback timeout should the event never arrive.
//
// Only the mounted/unmounted decision lives here; what the surface does when
// it opens or closes (return focus, dispatch a command) stays keyed on the
// live state, so none of that waits for the animation.
import { useLayoutEffect, useRef, useState, type AnimationEvent, type RefObject } from "react";

/** Longer than any exit animation: the unmount fallback when animationend never arrives. */
const FALLBACK_MS = 600;

/** Whether `el` has an animation of its own still to play (not one inside it). */
function animating(el: Element): boolean {
  return el.getAnimations().some((a) => a.playState !== "finished" && a.playState !== "idle");
}

export interface ExitProps<E extends HTMLElement> {
  ref: RefObject<E | null>;
  /** Set while closing: nothing inside can take focus or pointer events any more. */
  inert: boolean | undefined;
  onAnimationEnd: (e: AnimationEvent<E>) => void;
}

export interface Presence<T, E extends HTMLElement> {
  /** The live value while open, the last open value while the exit plays, else undefined. */
  value: T | undefined;
  /** True while the exit animation plays. */
  closing: boolean;
  /** The surface's motion class: `fade-in` while open, `closing` on the way out. */
  motion: "fade-in" | "closing";
  /** Spread onto the element that carries the exit animation. */
  exitProps: ExitProps<E>;
}

/**
 * Presence for one surface. `live` is its open state (`undefined`, `null` or
 * `false` when closed); pass a stable value, as a new object each render
 * would re-render forever. `ref` is the animated element's ref when the
 * component has its own; otherwise one is made and returned in `exitProps`.
 * With `instant`, a close unmounts at once (nothing worth animating, or the
 * surface is part of the layout rather than floating over it).
 */
export function usePresence<T, E extends HTMLElement = HTMLElement>(live: T | undefined | null | false, ref?: RefObject<E | null>, opts?: { instant?: boolean }): Presence<T, E> {
  const own = useRef<E>(null);
  const el = ref ?? own;
  const open = live !== undefined && live !== null && live !== false;
  const [held, setHeld] = useState<T | undefined>(open ? (live as T) : undefined);
  const [wasOpen, setWasOpen] = useState(open);
  const [closing, setClosing] = useState(false);
  // State adjusted during render (React's "storing information from previous
  // renders"): the last open value is kept through the exit, and a close
  // starts the exit while a reopen cancels it.
  if (open && held !== live) setHeld(live as T);
  if (wasOpen !== open) {
    setWasOpen(open);
    setClosing(!open && !opts?.instant);
  }

  useLayoutEffect(() => {
    if (!closing) return;
    const node = el.current;
    // No exit to wait for: reduced motion, or a surface without exit keyframes.
    if (!node || !animating(node)) {
      setClosing(false);
      return;
    }
    const t = setTimeout(() => setClosing(false), FALLBACK_MS);
    return () => clearTimeout(t);
  }, [closing, el]);

  const onAnimationEnd = (e: AnimationEvent<E>) => {
    if (e.target === e.currentTarget) setClosing(false);
  };
  return {
    value: open ? (live as T) : closing ? held : undefined,
    closing,
    motion: closing ? "closing" : "fade-in",
    exitProps: { ref: el, inert: closing || undefined, onAnimationEnd },
  };
}

export interface PresenceEntry<T> {
  item: T;
  key: string;
  closing: boolean;
}

/**
 * The next rendered list once `items` is the live list: entries keep their
 * place, ones no longer live start closing, new ones join at the end.
 */
export function mergePresence<T>(rendered: PresenceEntry<T>[], items: T[], keyOf: (item: T) => string): PresenceEntry<T>[] {
  const live = new Map(items.map((item) => [keyOf(item), item]));
  const out = rendered.map((e) => {
    const item = live.get(e.key);
    if (item !== undefined) return e.item === item && !e.closing ? e : { item, key: e.key, closing: false };
    return e.closing ? e : { ...e, closing: true };
  });
  const seen = new Set(rendered.map((e) => e.key));
  for (const item of items) {
    const key = keyOf(item);
    if (!seen.has(key)) out.push({ item, key, closing: false });
  }
  return out;
}

export interface PresenceListEntry<T, E extends HTMLElement> extends PresenceEntry<T> {
  motion: "fade-in" | "closing";
  exitProps: { ref: (el: E | null) => void; inert: boolean | undefined; onAnimationEnd: (e: AnimationEvent<E>) => void };
}

/** Presence for a keyed list (toasts): each item removed from `items` stays until its exit ends. */
export function usePresenceList<T, E extends HTMLElement = HTMLElement>(items: T[], keyOf: (item: T) => string): PresenceListEntry<T, E>[] {
  const [seen, setSeen] = useState(items);
  const [rendered, setRendered] = useState<PresenceEntry<T>[]>(() => mergePresence([], items, keyOf));
  if (seen !== items) {
    setSeen(items);
    setRendered((prev) => mergePresence(prev, items, keyOf));
  }
  const nodes = useRef(new Map<string, E>());
  const drop = (keys: string[]) => setRendered((prev) => prev.filter((e) => !(e.closing && keys.includes(e.key))));

  useLayoutEffect(() => {
    const closing = rendered.filter((e) => e.closing).map((e) => e.key);
    if (!closing.length) return;
    const still = closing.filter((key) => {
      const node = nodes.current.get(key);
      return !node || !animating(node);
    });
    if (still.length) drop(still);
    const t = setTimeout(() => drop(closing), FALLBACK_MS);
    return () => clearTimeout(t);
  }, [rendered]);

  return rendered.map((e) => ({
    ...e,
    motion: e.closing ? "closing" : "fade-in",
    exitProps: {
      ref: (el: E | null) => { if (el) nodes.current.set(e.key, el); else nodes.current.delete(e.key); },
      inert: e.closing || undefined,
      onAnimationEnd: (ev: AnimationEvent<E>) => { if (ev.target === ev.currentTarget) drop([e.key]); },
    },
  }));
}
