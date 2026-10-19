import { bench, describe } from "vitest";
import { evictPages, materializePages } from "./paged-window";
import { wavePath, WAVE } from "../components/WavySeek";
import { extrapolate } from "../store/position";

const BENCH = { warmupTime: 200, time: 1000 };

function trackLike(i: number) {
  return {
    id: `t${i}`,
    title: `Track ${i}`,
    artist: "Artist",
    album: "Album",
    durationMs: 180_000,
  };
}

describe("library pages", () => {
  bench(
    "materialize 12 pages of 200 into a 50k holey array (new)",
    () => {
      const pages = new Map<number, number[]>();
      for (let p = 10; p < 22; p++) pages.set(p, Array.from({ length: 200 }, (_, i) => p * 200 + i));
      materializePages(pages, 50_000, 200);
    },
    BENCH,
  );

  bench(
    "dense 50k copy then splice 200 (old page merge)",
    () => {
      const prev = new Array<number | undefined>(50_000);
      for (let i = 0; i < 2400; i++) prev[2000 + i] = i;
      const next = [...prev];
      for (let i = 0; i < 200; i++) next[4000 + i] = i;
      void next[4100];
    },
    BENCH,
  );

  bench(
    "evict 40 pages down to 12",
    () => {
      const pages = new Map<number, number[]>();
      for (let p = 0; p < 40; p++) pages.set(p, [p]);
      evictPages(pages, 20);
    },
    BENCH,
  );
});

describe("queue JSON (renderer analogue of QueueChanged)", () => {
  const full = Array.from({ length: 10_000 }, (_, i) => trackLike(i));
  const windowed = full.slice(0, 100);

  bench(
    "stringify 10_000 upcoming (old full view)",
    () => {
      JSON.stringify({ type: "queueChanged", upcoming: full, totalUpcoming: 10_000 });
    },
    BENCH,
  );

  bench(
    "stringify 100 upcoming (windowed view)",
    () => {
      JSON.stringify({ type: "queueChanged", upcoming: windowed, totalUpcoming: 10_000 });
    },
    BENCH,
  );
});

describe("seek paint", () => {
  bench(
    "wavePath 400px (player bar)",
    () => {
      wavePath(3, 400, 20, WAVE.amplitude, 8, WAVE.wavelength);
    },
    BENCH,
  );

  bench(
    "wavePath 1200px (fullscreen)",
    () => {
      wavePath(3, 1200, 20, WAVE.amplitude, 8, WAVE.wavelength);
    },
    BENCH,
  );

  bench(
    "extrapolate",
    () => {
      extrapolate({ positionMs: 12_000, takenAt: Date.now() - 40, isPlaying: true, rate: 1 }, Date.now(), 0, 240_000);
    },
    BENCH,
  );
});
