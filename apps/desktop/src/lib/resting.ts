// The pointer coming to rest over something: moving slower than SPEED
// (pixels per millisecond) across its last SAMPLES moves, or not moving at
// all for IDLE ms, once while it stays; a pointer that crosses on its way
// elsewhere never does. What to do then is the caller's: read ahead what a
// click would open, or (dragging) open what it's held over.

/** Slower than this (px/ms: 100 px a second) across the last moves is resting. */
export const SPEED = 0.1;
/** The moves that speed is taken over. */
export const SAMPLES = 3;
/** No move for this long (ms) is resting too. */
export const IDLE = 100;

/** The timers it waits with: the page's, or a test's own clock. */
export interface Timers {
  set(run: () => void, ms: number): unknown;
  clear(timer: unknown): void;
}
const pageTimers: Timers = {
  set: (run, ms) => setTimeout(run, ms),
  clear: (timer) => clearTimeout(timer as ReturnType<typeof setTimeout>),
};

export interface Rest {
  /** The pointer moved to `x`, `y` at time `t` (ms, as an event's `timeStamp`). */
  move(x: number, y: number, t: number): void;
  /** The pointer left (or something else began, a drag): no rest until it moves over again. */
  leave(): void;
}

/** Calls `onrest` once each time the pointer comes to rest, as told by `move` and `leave`. */
export function resting(
  onrest: () => void,
  {
    speed = SPEED,
    samples = SAMPLES,
    idle = IDLE,
    timers = pageTimers,
  }: Partial<{ speed: number; samples: number; idle: number; timers: Timers }> = {},
): Rest {
  let points: { x: number; y: number; t: number }[] = [];
  let rested = false;
  let timer: unknown;
  const rest = () => {
    timers.clear(timer);
    timer = undefined;
    if (rested) return;
    rested = true;
    onrest();
  };
  return {
    move(x, y, t) {
      if (rested) return;
      points.push({ x, y, t });
      if (points.length > samples + 1) points = points.slice(-(samples + 1));
      timers.clear(timer);
      timer = timers.set(rest, idle);
      if (points.length <= samples) return;
      let path = 0;
      for (let i = 1; i < points.length; i++)
        path += Math.hypot(points[i].x - points[i - 1].x, points[i].y - points[i - 1].y);
      const time = points[points.length - 1].t - points[0].t;
      if (time > 0 && path / time < speed) rest();
    },
    leave() {
      timers.clear(timer);
      timer = undefined;
      points = [];
      rested = false;
    },
  };
}

/** Calls `onrest` when the pointer comes to rest over `node` (with no button
 *  held: a press or drag is no rest); for an element's `use:`. */
export function restOver(node: HTMLElement, onrest: () => void) {
  let callback = onrest;
  const rest = resting(() => callback());
  const move = (e: PointerEvent) => (e.buttons ? rest.leave() : rest.move(e.clientX, e.clientY, e.timeStamp));
  const leave = () => rest.leave();
  node.addEventListener("pointermove", move);
  node.addEventListener("pointerleave", leave);
  node.addEventListener("pointerdown", leave);
  return {
    update: (next: () => void) => void (callback = next),
    destroy() {
      rest.leave();
      node.removeEventListener("pointermove", move);
      node.removeEventListener("pointerleave", leave);
      node.removeEventListener("pointerdown", leave);
    },
  };
}
