// The renderer's own menus for things the action registry has no target for (filters, genres), and
// the "When clicked" choice and moves at the top of a pin's menu.
import type { Filter } from "@core/api";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { useApp, type MenuItem } from "../store/app";
import { filterName } from "./filters";
import { showToast } from "./ui-toast";
import { LIBRARY_PINS_KEY, choosesTap, isPinned, movePin, parsePins, pinTap, removePin, setTap, togglePin, type LibraryPin, type PinKind, type PinTap } from "./library-pins";

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
  const pinned = isPinned(currentPins(), kind, id);
  return { id: pinned ? "unpinFromLibrary" : "pinToLibrary", label: t(pinned ? "library.unpin" : "library.pin"), icon: pinned ? "pinOff" : "pin", enabled: true, run: () => savePins(togglePin(currentPins(), kind, id)) };
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
 * The top of a pin's menu: "When clicked" (albums, artists and playlists), then moving it. Genres and
 * filters (no registry target) also get Unpin here; the others get the registry's Unpin from Library.
 */
export function pinMenuItems(pin: LibraryPin, title: string, withUnpin: boolean): MenuItem[] {
  const pins = currentPins();
  const i = pins.findIndex((p) => p.kind === pin.kind && p.id === pin.id);
  const items: MenuItem[] = [];
  if (choosesTap(pin.kind)) {
    items.push({ id: "whenClicked", label: t("library.whenClicked"), icon: "", enabled: false, heading: true, run: () => {} });
    for (const tap of ["open", "play", "shuffle"] as PinTap[]) {
      items.push({ id: `tap-${tap}`, label: t(`library.tap.${tap}` as never), icon: tap === "open" ? "openInFull" : tap, enabled: true, checked: pinTap(pin) === tap, run: () => savePins(setTap(currentPins(), pin, tap)) });
    }
  }
  items.push({ id: "pinUp", label: t("library.moveUp"), icon: "arrowUp", enabled: i > 0, run: () => savePins(movePin(currentPins(), pin, -1)) });
  items.push({ id: "pinDown", label: t("library.moveDown"), icon: "arrowDown", enabled: i >= 0 && i < pins.length - 1, run: () => savePins(movePin(currentPins(), pin, 1)) });
  if (withUnpin) {
    items.push({ id: "unpin", label: t("library.unpin"), icon: "pinOff", enabled: true, run: () => {
      const before = currentPins();
      savePins(removePin(before, pin).pins);
      showToast(t("library.unpinned", { title }), { type: "setSetting", data: { key: LIBRARY_PINS_KEY, value: JSON.stringify(before) } });
    } });
  }
  return items;
}
