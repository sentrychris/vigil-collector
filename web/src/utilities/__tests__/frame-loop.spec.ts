import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DRAW_EVERY_MS, easeToward, frameLoop, onFrame, tween } from "../frame-loop";

/** Stands in for IntersectionObserver; the test moves the chart itself. */
function viewport() {
  let report: IntersectionObserverCallback = () => {};
  const disconnect = vi.fn();
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: IntersectionObserverCallback) {
        report = callback;
      }
      observe() {}
      disconnect = disconnect;
    },
  );
  return {
    scroll(onScreen: boolean) {
      report(
        [{ isIntersecting: onScreen } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    },
    disconnect,
  };
}

function hide(hidden: boolean) {
  Object.defineProperty(document, "hidden", { configurable: true, value: hidden });
  document.dispatchEvent(new Event("visibilitychange"));
}

describe("the charts' shared clock", () => {
  const target = document.createElement("div");
  let frames: ReturnType<typeof vi.fn>;
  let stopCounting: () => void;

  beforeEach(() => {
    // A 60 Hz display: fake timers show a frame every 16 ms.
    vi.useFakeTimers({
      toFake: [
        "setTimeout",
        "clearTimeout",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "performance",
      ],
    });
    // Every frame the page asks the display for, drawn or not.
    frames = vi.fn();
    const real = window.requestAnimationFrame;
    const counted = vi.fn((callback: FrameRequestCallback) =>
      real((at) => {
        frames();
        callback(at);
      }),
    );
    vi.stubGlobal("requestAnimationFrame", counted);
    stopCounting = () => vi.unstubAllGlobals();
  });

  afterEach(() => {
    hide(false);
    stopCounting();
    vi.useRealTimers();
  });

  it("draws thirty times a second, and asks for no other frames", () => {
    const draw = vi.fn();
    const stop = onFrame(draw);

    vi.advanceTimersByTime(1000);

    expect(DRAW_EVERY_MS).toBeCloseTo(33.3, 1);
    expect(draw.mock.calls.length).toBeGreaterThanOrEqual(29);
    expect(draw.mock.calls.length).toBeLessThanOrEqual(32);
    expect(frames.mock.calls.length).toBe(draw.mock.calls.length);
    stop();
  });

  it("draws every loop in the same frame", () => {
    const order: string[] = [];
    const stopA = onFrame(() => order.push("a"));
    vi.advanceTimersByTime(20);
    // Joins part-way between two frames.
    const stopB = onFrame(() => order.push("b"));

    vi.advanceTimersByTime(500);

    const firstB = order.indexOf("b");
    const paired = order.slice(firstB - 1).join("");
    expect(paired).toMatch(/^(ab)+$/);
    expect(frames.mock.calls.length).toBe(order.filter((who) => who === "a").length);
    stopA();
    stopB();
  });

  it("asks for nothing once the last loop has stopped", () => {
    const draw = vi.fn();
    const stop = onFrame(draw);
    vi.advanceTimersByTime(100);
    const drawn = draw.mock.calls.length;
    const asked = frames.mock.calls.length;

    stop();
    vi.advanceTimersByTime(1000);

    expect(draw).toHaveBeenCalledTimes(drawn);
    expect(frames).toHaveBeenCalledTimes(asked);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("rests while the page is hidden, and draws again when it's back", () => {
    const draw = vi.fn();
    const stop = onFrame(draw);
    vi.advanceTimersByTime(100);
    const drawn = draw.mock.calls.length;

    hide(true);
    vi.advanceTimersByTime(1000);
    expect(draw).toHaveBeenCalledTimes(drawn);
    expect(vi.getTimerCount()).toBe(0);

    hide(false);
    vi.advanceTimersByTime(20);
    expect(draw).toHaveBeenCalledTimes(drawn + 1);
    stop();
  });

  it("leaves a chart alone while it is scrolled out of view", () => {
    const view = viewport();
    const draw = vi.fn();
    const loop = frameLoop(target, draw);
    vi.advanceTimersByTime(100);
    const drawn = draw.mock.calls.length;
    expect(drawn).toBeGreaterThan(0);

    view.scroll(false);
    vi.advanceTimersByTime(1000);
    expect(draw).toHaveBeenCalledTimes(drawn);
    expect(vi.getTimerCount()).toBe(0);

    view.scroll(true);
    vi.advanceTimersByTime(20);
    expect(draw).toHaveBeenCalledTimes(drawn + 1);

    loop.stop();
    view.scroll(true);
    vi.advanceTimersByTime(1000);
    expect(draw).toHaveBeenCalledTimes(drawn + 1);
    expect(view.disconnect).toHaveBeenCalled();
  });

  it("runs a tween from start to exactly its end, then stops", () => {
    const seen: number[] = [];
    tween(700, (progress) => seen.push(progress));

    vi.advanceTimersByTime(2000);

    expect(seen.length).toBeGreaterThanOrEqual(20);
    expect(seen.length).toBeLessThanOrEqual(24);
    expect(seen[0]).toBeLessThan(0.1);
    expect([...seen].sort((a, b) => a - b)).toEqual(seen);
    expect(seen[seen.length - 1]).toBe(1);
    expect(seen.filter((progress) => progress === 1)).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cuts a tween short when asked", () => {
    const step = vi.fn();
    const cut = tween(700, step);
    vi.advanceTimersByTime(100);
    const steps = step.mock.calls.length;

    cut();
    vi.advanceTimersByTime(1000);

    expect(step).toHaveBeenCalledTimes(steps);
  });
});

describe("easeToward", () => {
  it("moves 15% of the way in one 60 Hz frame", () => {
    expect(easeToward(100, 200, 1000 / 60)).toBeCloseTo(115);
  });

  it("gets as far in one long frame as in the short ones it replaces", () => {
    const twoShort = easeToward(easeToward(100, 200, 1000 / 60), 200, 1000 / 60);
    expect(easeToward(100, 200, 2000 / 60)).toBeCloseTo(twoShort);
  });

  it("arrives when a long time has passed", () => {
    expect(easeToward(100, 200, 60_000)).toBeCloseTo(200);
  });
});
