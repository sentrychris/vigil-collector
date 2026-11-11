import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import PageHeader from "../PageHeader.vue";

const testProps = {
  decorTitle: "Lorem ipsum",
  title: "Dolor sit amet",
};

describe("PageHeader", () => {
  const getWrapper = () =>
    mount(PageHeader, {
      props: testProps,
    });

  it("renders properly", () => {
    const testSubject = getWrapper();

    expect(testSubject.exists()).toBeTruthy();
    expect(testSubject.props()).toEqual({ ...testProps, compact: false });
    expect(testSubject.find(".eyebrow").text()).toBe(testProps.decorTitle);
    expect(testSubject.find("h1.page-title").text()).toBe(testProps.title);
  });
});
