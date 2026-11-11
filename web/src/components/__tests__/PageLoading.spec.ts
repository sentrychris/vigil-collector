import { describe, it, vi, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { createTestingPinia } from "@pinia/testing";
import PageLoading from "../PageLoading.vue";

describe("PageLoading", () => {
  const getWrapper = () =>
    mount(PageLoading, {
      global: {
        plugins: [
          createTestingPinia({
            createSpy: vi.fn,
          }),
        ],
        stubs: ["FontAwesomeIcon"],
      },
    });

  it("renders properly", () => {
    const testSubject = getWrapper();
    expect(testSubject.exists()).toBeTruthy();
  });
});
