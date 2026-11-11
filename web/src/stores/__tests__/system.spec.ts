import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useSystemStore } from "../system";
import { staticMockData } from "./mock/system-response";

describe("System Store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("patches", () => {
    const system = useSystemStore();

    expect(system.live).toBe(false);
    system.staticUpdate(staticMockData);
    expect(system.data).toEqual(staticMockData);
  });
});
