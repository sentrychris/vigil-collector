import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

/** Loads the store afresh, inside the Vigil window or in a browser. */
async function store(framed: boolean) {
  vi.resetModules();
  const postMessage = vi.fn();
  if (framed) (window as unknown as { ipc: unknown }).ipc = { postMessage };
  const { useThemeStore } = await import("../theme");
  const theme = useThemeStore();
  theme.load();
  postMessage.mockClear();
  return { theme, postMessage };
}

describe("theme store", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => {
    delete (window as unknown as { ipc?: unknown }).ipc;
    delete document.body.dataset.theme;
  });

  it("starts dark and toggles", async () => {
    const { theme } = await store(false);
    expect(document.body.dataset.theme).toBe("dark");

    theme.toggle();
    expect(theme.theme).toBe("light");
    expect(theme.active).toBe("light");
    expect(document.body.dataset.theme).toBe("light");
  });

  it("tells the Vigil window when it is toggled", async () => {
    const { theme, postMessage } = await store(true);

    theme.toggle();
    expect(postMessage).toHaveBeenLastCalledWith("theme:light");
    theme.toggle();
    expect(postMessage).toHaveBeenLastCalledWith("theme:dark");
  });

  it("can be set from the window without answering back", async () => {
    const { theme, postMessage } = await store(true);

    theme.set("light");
    expect(document.body.dataset.theme).toBe("light");
    expect(theme.active).toBe("light");
    expect(postMessage).not.toHaveBeenCalled();
  });
});
