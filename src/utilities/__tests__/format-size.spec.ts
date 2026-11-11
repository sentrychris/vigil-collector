import { describe, expect, it } from "vitest";
import { formatSize, sizeText } from "../format-size";

describe("formatSize", () => {
  it("keeps GiB below 1 TiB", () => {
    expect(formatSize(475.07)).toEqual({ value: "475.07", unit: "GiB" });
  });

  it("moves to TiB at 1024 GiB", () => {
    expect(formatSize(1863)).toEqual({ value: "1.82", unit: "TiB" });
    expect(formatSize(1024)).toEqual({ value: "1.00", unit: "TiB" });
  });

  it("drops to MiB below 1 GiB", () => {
    expect(formatSize(0.5)).toEqual({ value: "512.00", unit: "MiB" });
  });

  it("starts from MiB values", () => {
    expect(sizeText(32.86, "MiB")).toBe("32.86 MiB");
    expect(sizeText(2600, "MiB")).toBe("2.54 GiB");
    expect(sizeText(3.5 * 1024 * 1024, "MiB")).toBe("3.50 TiB");
  });

  it("never goes past TiB", () => {
    expect(sizeText(2048 * 1024)).toBe("2048.00 TiB");
  });

  it("handles missing values", () => {
    expect(sizeText(undefined)).toBe("-- GiB");
    expect(formatSize(Number.NaN, "MiB")).toEqual({ value: "--", unit: "MiB" });
  });

  it("honours decimals", () => {
    expect(sizeText(1863, "GiB", 1)).toBe("1.8 TiB");
  });
});
