import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import CpuDetail from "../../stats/CpuDetail.vue";

describe("CpuDetail", () => {
  const testProps = {
    detail: {
      temp: 25,
      usage: 15,
      freq: 1500,
      cores: 4,
    },
  };

  const getWrapper = () =>
    mount(CpuDetail, {
      props: testProps,
    });

  it("renders properly", () => {
    const testSubject = getWrapper();

    expect(testSubject.exists()).toBeTruthy();
    expect(testSubject.props()).toEqual(testProps);
    const text = testSubject.text();
    expect(text).toContain(`${testProps.detail.usage}%`);
    expect(text).toContain(`${testProps.detail.freq} MHz`);
    expect(text).toContain(`${testProps.detail.cores} vCPU`);
    expect(text).toContain("TEMP25.0°C");
  });

  it("names the processor when the collector says what it is", () => {
    expect(getWrapper().text()).not.toContain("MODEL");
    const testSubject = mount(CpuDetail, {
      props: { detail: { ...testProps.detail, name: "Intel Core i7-11700" } },
    });
    expect(testSubject.text()).toContain("MODELIntel Core i7-11700");
  });

  it("shows n/a without a temperature sensor", () => {
    const testSubject = mount(CpuDetail, {
      props: { detail: { ...testProps.detail, temp: 0 } },
    });
    expect(testSubject.text()).toContain("TEMPn/a");
  });
});
