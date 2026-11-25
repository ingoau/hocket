// The size big covers (the player, a detail page's hero, the spotlight) ask for. Battery saver's
// smallArtwork turns it down to the grid's, which the lists and the player's colours already
// fetched for the same cover, so the current track's cover isn't fetched at 1280 too (#319).
import type { BatterySaver } from "@core/api";
import { ARTWORK_SIZES } from "@shared/constants";
import { useApp } from "../store/app";
import { saverHas } from "../store/reducer";

export function largeArtworkSize(saver: BatterySaver): number {
  return saverHas(saver, "smallArtwork") ? ARTWORK_SIZES.grid : ARTWORK_SIZES.full;
}

export function useLargeArtworkSize(): number {
  return useApp((s) => largeArtworkSize(s.batterySaver));
}
