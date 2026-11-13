import { afterEach, describe, expect, it, vi } from "vitest";

/** Loads the composables afresh, as a page that starts ``width`` wide,
 *  inside the Vigil window or not. */
async function page(width: number, framed: boolean) {
  vi.resetModules();
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  if (framed) (window as unknown as { ipc: unknown }).ipc = { postMessage: vi.fn() };
  const { useCompactLayout } = await import("../useCompactLayout");
  return useCompactLayout();
}

function resize(width: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  window.dispatchEvent(new Event("resize"));
}

describe("useCompactLayout", () => {
  afterEach(() => {
    delete (window as unknown as { ipc?: unknown }).ipc;
  });

  it("is compact in the Vigil window at its usual size", async () => {
    const { compact } = await page(420, true);
    expect(compact.value).toBe(true);
  });

  it("gives way to the full dashboard when the window is made wide, and comes back", async () => {
    const { compact } = await page(420, true);

    resize(1280);
    expect(compact.value).toBe(false);

    resize(899);
    expect(compact.value).toBe(true);
  });

  it("is never compact in a browser, however narrow", async () => {
    const { compact } = await page(390, false);
    expect(compact.value).toBe(false);

    resize(320);
    expect(compact.value).toBe(false);
  });
});
