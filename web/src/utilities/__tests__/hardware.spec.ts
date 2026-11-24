import { describe, expect, it } from "vitest";
import { gpuModel, memoryName } from "../hardware";

describe("hardware names", () => {
  it("leaves a graphics card's maker off its model", () => {
    expect(gpuModel("NVIDIA GeForce RTX 3070")).toBe("GeForce RTX 3070");
    expect(gpuModel("AMD Radeon RX 6800 XT")).toBe("Radeon RX 6800 XT");
    expect(gpuModel("Intel UHD Graphics 750")).toBe("UHD Graphics 750");
    expect(gpuModel("Graphics card")).toBe("Graphics card");
    expect(gpuModel("NVIDIA")).toBe("NVIDIA");
    expect(gpuModel(undefined)).toBe("");
  });

  it("says what memory is fitted with as much as is known", () => {
    expect(memoryName({ type: "DDR4", manufacturer: "Corsair" })).toBe("DDR4 · Corsair");
    expect(memoryName({ type: "DDR5", manufacturer: "" })).toBe("DDR5");
    expect(memoryName({ type: "", manufacturer: "" })).toBe("");
    // A collector from before it reported either.
    expect(memoryName({})).toBe("");
    expect(memoryName(undefined)).toBe("");
  });
});
