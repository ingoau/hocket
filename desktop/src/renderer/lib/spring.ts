// Spring motion for CSS and the Web Animations API: a damped spring sampled
// into a `linear()` easing, so transitions and FLIP animations overshoot and
// settle like Material 3 Expressive's spatial springs without a JS loop.

export interface Spring {
  /** CSS easing: `linear(0, …, 1)`. */
  easing: string;
  /** Milliseconds until the spring has settled (within 0.1 % of rest). */
  duration: number;
}

/**
 * Position (0 → 1) of a spring released from rest at 0 towards 1, at time `t`
 * seconds. `stiffness` in N/m for a unit mass; `ratio` is the damping ratio
 * (1 = critically damped, below 1 overshoots).
 */
export function springAt(t: number, stiffness: number, ratio: number): number {
  const w = Math.sqrt(stiffness);
  if (ratio < 1) {
    const wd = w * Math.sqrt(1 - ratio * ratio);
    return 1 - Math.exp(-ratio * w * t) * (Math.cos(wd * t) + ((ratio * w) / wd) * Math.sin(wd * t));
  }
  return 1 - Math.exp(-w * t) * (1 + w * t);
}

/** Sample a spring into a `linear()` easing and the duration it needs. */
export function spring(stiffness: number, ratio: number, samples = 48): Spring {
  const w = Math.sqrt(stiffness);
  // The envelope e^(-ζωt) (or (1+ωt)e^(-ωt) when critically damped) below 0.001.
  let settle = ratio < 1 ? Math.log(1000) / (ratio * w) : 0;
  if (ratio >= 1) {
    settle = 0.05;
    while (Math.exp(-w * settle) * (1 + w * settle) > 0.001) settle += 0.01;
  }
  const points: string[] = [];
  for (let i = 0; i <= samples; i++) {
    const v = i === samples ? 1 : springAt((settle * i) / samples, stiffness, ratio);
    points.push(String(Math.round(v * 1000) / 1000));
  }
  return { easing: `linear(${points.join(", ")})`, duration: Math.round(settle * 1000) };
}

/** Large movements (the artwork flying between its slots): a gentle overshoot. */
export const SPRING_SPATIAL = spring(260, 0.72);
/** Small, quick shape changes (the play button morphing, a pressed button). */
export const SPRING_FAST = spring(700, 0.6);
/** Colour and opacity: no overshoot. */
export const SPRING_EFFECTS = spring(400, 1);
