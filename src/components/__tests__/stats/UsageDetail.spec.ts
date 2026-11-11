import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import UsageDetail from "../../stats/UsageDetail.vue";

describe("UsageDetail", () => {
  const testProps = {
    detail: {
      used: 50,
      free: 50,
      total: 100,
      percent: 50,
    },
  };

  const getWrapper = () =>
    mount(UsageDetail, {
      props: testProps,
    });

  it("renders properly", () => {
    const testSubject = getWrapper();

    expect(testSubject.exists()).toBeTruthy();
    expect(testSubject.props()).toEqual(testProps);
    const text = testSubject.text();
    expect(text).toContain(`${testProps.detail.percent}%`);
    expect(text).toContain("FREE50.00 GiB");
    expect(text).toContain("TOTAL100.00 GiB");
  });

  it("switches to TiB for large totals", () => {
    const testSubject = mount(UsageDetail, {
      props: { detail: { used: 1200, free: 663, total: 1863, percent: 64.4 } },
    });
    const text = testSubject.text();
    expect(text).toContain("FREE663.00 GiB");
    expect(text).toContain("TOTAL1.82 TiB");
  });
});
