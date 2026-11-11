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
  });
});
