// The renderer's own menus for things the action registry has no target for (filters, genres), and
// the "When clicked" choice and moves at the top of a pin's menu.
import type { Filter } from "@core/api";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { useApp, type MenuItem } from "../store/app";
import { filterName } from "./filters";
import { showToast } from "./ui-toast";
import { LIBRARY_PINS_KEY, canPin, choosesTap, isPinned, movePin, parsePins, pinTap, removePin, restorePin, setTap, shownNeighbour, togglePin, type LibraryPin, type PinKind, type PinTap } from "./library-pins";

export function currentPins(): LibraryPin[] {
  const v = useApp.getState().setting<unknown>(LIBRARY_PINS_KEY, []);
  return parsePins(v);
}

export function savePins(pins: LibraryPin[]): void {
  useApp.getState().setSetting(LIBRARY_PINS_KEY, pins);
}

export function playFilter(serverId: string, f: Filter, shuffle = false): void {
  bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "filter", data: { filter: f } }, label: filterName(f), sort: f.sort, tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
}

export function playGenre(serverId: string, name: string, shuffle = false): void {
  bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "genre", data: { name } }, label: name, sort: "default", tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
}

function pinItem(kind: PinKind, id: string): MenuItem {
  const pins = currentPins();
  const pinned = isPinned(pins, kind, id);
  // A full Library offers Pin greyed out, as the registry's Pin to Library is.
  return { id: pinned ? "unpinFromLibrary" : "pinToLibrary", label: t(pinned ? "library.unpin" : "library.pin"), icon: pinned ? "pinOff" : "pin", enabled: pinned || canPin(pins), run: () => savePins(togglePin(currentPins(), kind, id)) };
}

/** A saved filter's menu: play, shuffle, edit, pin. */
export function filterMenuItems(f: Filter, serverId: string): MenuItem[] {
  return [
    { id: "play", label: t("library.tap.play"), icon: "play", enabled: true, run: () => playFilter(serverId, f) },
    { id: "shuffle", label: t("library.tap.shuffle"), icon: "shuffle", enabled: true, run: () => playFilter(serverId, f, true) },
    { id: "edit", label: t("library.edit"), icon: "edit", enabled: true, run: () => useApp.getState().navigate({ view: "filter", id: f.id }) },
    pinItem("filter", f.id),
  ];
}

/** A genre's menu: play, shuffle, pin. */
export function genreMenuItems(name: string, serverId: string): MenuItem[] {
  return [
    { id: "play", label: t("library.tap.play"), icon: "play", enabled: true, run: () => playGenre(serverId, name) },
    { id: "shuffle", label: t("library.tap.shuffle"), icon: "shuffle", enabled: true, run: () => playGenre(serverId, name, true) },
    pinItem("genre", name),
  ];
}

/**
 * The top of a pin's menu: "When clicked" (albums, artists and playlists), then moving it among the
 * pins [shown]. Genres and filters (no registry target) also get Open, Play, Shuffle and Unpin here;
 * the others get the registry's actions, its Unpin from Library among them.
 */
export function pinMenuItems(pin: LibraryPin, title: string, withUnpin: boolean, shown: readonly LibraryPin[]): MenuItem[] {
  const items: MenuItem[] = [];
  if (choosesTap(pin.kind)) {
    items.push({ id: "whenClicked", label: t("library.whenClicked"), icon: "", enabled: false, heading: true, run: () => {} });
    for (const tap of ["open", "play", "shuffle"] as PinTap[]) {
      items.push({ id: `tap-${tap}`, label: t(`library.tap.${tap}` as never), icon: tap === "open" ? "openInFull" : tap, enabled: true, checked: pinTap(pin) === tap, run: () => savePins(setTap(currentPins(), pin, tap)) });
    }
  }
  if (pin.kind === "genre" || pin.kind === "filter") {
    const app = useApp.getState();
    const serverId = app.servers[0]?.id ?? "";
    const filter = pin.kind === "filter" ? app.filters.find((f) => f.id === pin.id) : undefined;
    const play = (shuffle: boolean) => (filter ? playFilter(serverId, filter, shuffle) : playGenre(serverId, pin.id, shuffle));
    items.push({ id: "open", label: t("library.tap.open"), icon: "openInFull", enabled: true, run: () => useApp.getState().navigate(pin.kind === "genre" ? { view: "genre", id: pin.id } : { view: "filter", id: pin.id }) });
    items.push({ id: "play", label: t("library.tap.play"), icon: "play", enabled: pin.kind === "genre" || !!filter, run: () => play(false) });
    items.push({ id: "shuffle", label: t("library.tap.shuffle"), icon: "shuffle", enabled: pin.kind === "genre" || !!filter, run: () => play(true) });
  }
  items.push({ id: "pinUp", label: t("library.moveUp"), icon: "arrowUp", enabled: !!shownNeighbour(shown, pin, -1), run: () => savePins(movePin(currentPins(), pin, -1, shown)) });
  items.push({ id: "pinDown", label: t("library.moveDown"), icon: "arrowDown", enabled: !!shownNeighbour(shown, pin, 1), run: () => savePins(movePin(currentPins(), pin, 1, shown)) });
  if (withUnpin) {
    items.push({ id: "unpin", label: t("library.unpin"), icon: "pinOff", enabled: true, run: () => {
      const { pins, at } = removePin(currentPins(), pin);
      savePins(pins);
      showToast(t("library.unpinned", { title }), () => savePins(restorePin(currentPins(), pin, at)));
    } });
  }
  return items;
}
