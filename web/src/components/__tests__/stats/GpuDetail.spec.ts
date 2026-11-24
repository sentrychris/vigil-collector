import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import GpuDetail from "../../stats/GpuDetail.vue";

describe("GpuDetail", () => {
  const detail = {
    name: "NVIDIA GeForce RTX 3070",
    usage: 23.8,
    temp: 54,
    mem: { total: 8, used: 2.6, free: 5.4, percent: 32.5 },
  };

  it("shows how busy the card is, its model, memory and temperature", () => {
    const testSubject = mount(GpuDetail, { props: { detail } });
    const text = testSubject.text();
    expect(text).toContain("23.8%");
    // The maker is left to the tooltip, which has the whole name.
    expect(text).toContain("MODELGeForce RTX 3070");
    expect(testSubject.find(".row-name").attributes("title")).toBe(detail.name);
    expect(text).toContain("VRAM2.6 / 8.0 GiB");
    expect(text).toContain("TEMP54.0°C");
  });

  it("shows n/a for a card whose driver gives no temperature", () => {
    const testSubject = mount(GpuDetail, { props: { detail: { ...detail, temp: 0 } } });
    expect(testSubject.text()).toContain("TEMPn/a");
  });

  it("shows a small card's memory in MiB", () => {
    const testSubject = mount(GpuDetail, {
      props: { detail: { ...detail, mem: { total: 0.5, used: 0.25, free: 0.25, percent: 50 } } },
    });
    expect(testSubject.text()).toContain("VRAM256.0 / 512.0 MiB");
  });
});
