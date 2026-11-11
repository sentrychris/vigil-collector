import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import PlatformDetail from "../../stats/PlatformDetail.vue";

describe("PlatformDetail", () => {
  const testProps = {
    detail: {
      distro: "distribution info",
      kernel: "here is the kernel",
      uptime: "test the uptime pls",
    },
  };

  const getWrapper = () =>
    mount(PlatformDetail, {
      props: testProps,
    });

  it("renders properly", () => {
    const testSubject = getWrapper();

    expect(testSubject.exists()).toBeTruthy();
    expect(testSubject.props()).toEqual(testProps);
    const text = testSubject.text();
    expect(text).toContain(testProps.detail.distro);
    expect(text).toContain(testProps.detail.kernel);
    expect(text).toContain(testProps.detail.uptime);
  });
});
