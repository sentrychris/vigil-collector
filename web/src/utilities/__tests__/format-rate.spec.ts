import { describe, expect, it } from "vitest";
import { formatRate, rateText } from "../format-rate";

describe("formatRate", () => {
  it("keeps small rates in bytes", () => {
    expect(formatRate(0)).toEqual({ value: "0", unit: "B/s" });
    expect(formatRate(1023.4)).toEqual({ value: "1023", unit: "B/s" });
  });

  it("steps up a unit at each 1024", () => {
    expect(formatRate(1024)).toEqual({ value: "1.0", unit: "KB/s" });
    expect(formatRate(250_000)).toEqual({ value: "244.1", unit: "KB/s" });
    expect(formatRate(1024 * 1024)).toEqual({ value: "1.00", unit: "MB/s" });
    expect(formatRate(3 * 1024 ** 3)).toEqual({ value: "3.00", unit: "GB/s" });
  });

  it("reads anything that isn't a rate as none", () => {
    expect(rateText(Number.NaN)).toBe("0 B/s");
    expect(rateText(-5)).toBe("0 B/s");
  });
});
