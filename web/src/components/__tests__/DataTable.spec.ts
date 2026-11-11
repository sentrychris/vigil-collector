import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import DataTable from "../DataTable.vue";

describe("DataTable", () => {
  const testProps = {
    type: "horizontal",
    nested: false,
    data: [
      {
        pid: 634,
        username: "root",
        name: "Xorg",
        mem: 71.37,
      },
      {
        pid: 1493,
        username: "pi",
        name: "node",
        mem: 63.8,
      },
    ],
  };

  const getWrapper = () =>
    mount(DataTable, {
      props: testProps,
      global: {
        stubs: ["FontAwesomeIcon"],
      },
    });

  it("renders properly", () => {
    const testSubject = getWrapper();

    expect(testSubject.exists()).toBeTruthy();
    expect(testSubject.props()).toEqual(testProps);

    expect(testSubject.html()).toContain(`Pid</th>`);
    expect(testSubject.html()).toContain(`Username</th>`);
    expect(testSubject.html()).toContain(`Name</th>`);
    expect(testSubject.html()).toContain(`Mem</th>`);

    const cells = testSubject.findAll("td");
    expect(cells.map((c) => c.text())).toEqual([
      "634",
      "root",
      "Xorg",
      "71.37 MiB",
      "1493",
      "pi",
      "node",
      "63.80 MiB",
    ]);

    expect(cells[0].classes()).toContain("mono");
    expect(cells[3].classes()).toContain("mono");
    expect(cells[1].classes()).not.toContain("mono");
    expect(cells[2].classes()).not.toContain("mono");
  });
});
