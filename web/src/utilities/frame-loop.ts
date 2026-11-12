/**
 * The one clock every moving chart on the dashboard draws by.
 *
 * `requestAnimationFrame` on its own redraws on every frame the display
 * shows, sixty or more a second, whether or not anything can be seen to
 * have moved, and a page with several loops of its own redraws whenever
 * any one of them does. Each redraw is real work for the renderer and the
 * GPU, and on a system monitor that work turns up in its own readings.
 *
 * So the charts share this clock instead. It draws them all in the same
 * frame, `DRAW_EVERY_MS` apart, in step with the display; it asks for no
 * frames in between, none while the page is hidden, and none at all when
 * nothing is moving.
 */

/** How far apart drawn frames are: thirty a second. The charts scroll a
 *  pixel or less in that time, so the eye sees the same glide as at sixty. */
export const DRAW_EVERY_MS = 1000 / 30;

/** One frame of a 60 Hz display, in milliseconds. */
const FRAME_MS = 1000 / 60;

/** How much sooner than `DRAW_EVERY_MS` a frame may be drawn. Frame times
 *  wander by a fraction of a frame; without this, every so often a frame
 *  that arrived a hair early would be passed over for the next one. */
const EARLY_MS = 4;

type Draw = (now: number) => void;

const drawers = new Set<Draw>();
let timerId = 0;
let frameId = 0;
let drawnAt = -Infinity;

function frame(at: number) {
  frameId = 0;
  if (at - drawnAt >= DRAW_EVERY_MS - EARLY_MS) {
    drawnAt = at;
    // A copy, as drawing may end a loop or start one.
    for (const draw of [...drawers]) {
      if (drawers.has(draw)) draw(at);
    }
  }
  schedule();
}

/** Asks for the display frame nearest to when the next drawing is due,
 *  sleeping through the ones before it. */
function schedule() {
  if (drawers.size === 0 || timerId !== 0 || frameId !== 0 || document.hidden) return;
  const wait = drawnAt + DRAW_EVERY_MS - performance.now() - FRAME_MS / 2;
  if (wait > 1) {
    timerId = window.setTimeout(() => {
      timerId = 0;
      frameId = requestAnimationFrame(frame);
    }, wait);
  } else {
    frameId = requestAnimationFrame(frame);
  }
}

function rest() {
  if (timerId !== 0) clearTimeout(timerId);
  if (frameId !== 0) cancelAnimationFrame(frameId);
  timerId = 0;
  frameId = 0;
  drawnAt = -Infinity;
}

if (typeof document !== "undefined") {
  // Frames don't arrive while the page is hidden, and a hidden WebView2
  // control counts as hidden. Whatever was drawn last is stale on return.
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) rest();
    else schedule();
  });
}

/**
 * Calls ``draw`` on every frame the shared clock draws, with the frame's
 * time, until the function this returns is called.
 */
export function onFrame(draw: Draw): () => void {
  // Wrapped, so the same function given twice is two loops.
  const drawer: Draw = (now) => draw(now);
  drawers.add(drawer);
  schedule();
  return () => {
    drawers.delete(drawer);
    if (drawers.size === 0) rest();
  };
}

export interface FrameLoop {
  /** Ends the loop for good. */
  stop(): void;
}

/**
 * Calls ``draw`` on every frame of the shared clock for as long as
 * ``target`` is on screen, and stays off the clock while it is scrolled
 * out of view.
 */
export function frameLoop(target: Element, draw: () => void): FrameLoop {
  let leave: (() => void) | null = null;
  let stopped = false;

  const show = (onScreen: boolean) => {
    if (stopped || onScreen === (leave !== null)) return;
    if (onScreen) {
      leave = onFrame(draw);
    } else {
      leave?.();
      leave = null;
    }
  };

  // Engines without IntersectionObserver take the chart to be on screen,
  // as it always was.
  const observer =
    typeof IntersectionObserver === "undefined"
      ? null
      : new IntersectionObserver((entries) => {
          show(entries[entries.length - 1].isIntersecting);
        });
  observer?.observe(target);
  show(true);

  return {
    stop() {
      show(false);
      stopped = true;
      observer?.disconnect();
    },
  };
}

/**
 * Runs ``step`` with a progress from 0 to 1 over ``durationMs``, on the
 * shared clock, ending on exactly 1. Returns a function that cuts it short.
 */
export function tween(durationMs: number, step: (progress: number) => void): () => void {
  const startedAt = performance.now();
  const leave = onFrame((now) => {
    const progress = Math.min(1, Math.max(0, (now - startedAt) / durationMs));
    if (progress >= 1) leave();
    step(progress);
  });
  return leave;
}

/**
 * Moves ``from`` toward ``to`` by as much as ``elapsedMs`` allows. The
 * charts' axes have always eased by 15% a frame at 60 Hz; this is the same
 * ease by the clock, so it takes as long whatever pace the chart draws at.
 */
export function easeToward(from: number, to: number, elapsedMs: number): number {
  const kept = Math.pow(0.85, Math.max(0, elapsedMs) / FRAME_MS);
  return to + (from - to) * kept;
}
