import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useLoadingStore } from "../loading";

describe("Loading Store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("loads", () => {
    const loading = useLoadingStore();

    expect(loading.loaded).toBe(false);
    loading.toggle(true);
    expect(loading.loaded).toBe(true);
  });
});
