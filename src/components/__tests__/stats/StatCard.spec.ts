import { describe, it, vi, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { createTestingPinia } from "@pinia/testing";
import StatCard from "../../stats/StatCard.vue";

describe("StatCard", () => {
  const testProps = {
    title: "Lorem Ipsum",
    icon: "fa-solid fa-server",
    tone: "amber" as const,
  };

  const getWrapper = () =>
    mount(StatCard, {
      global: {
        plugins: [
          createTestingPinia({
            createSpy: vi.fn,
          }),
        ],
        stubs: ["FontAwesomeIcon"],
      },
      props: testProps,
    });

  it("renders properly", () => {
    const testSubject = getWrapper();

    expect(testSubject.exists()).toBeTruthy();
    expect(testSubject.props()).toEqual(testProps);
    expect(testSubject.text()).toContain("Lorem Ipsum");
    expect(testSubject.find(".stat-icon").classes()).toContain("tone-amber");
  });
});
