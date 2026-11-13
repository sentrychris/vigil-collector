import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import CompactProcesses from "../../compact/CompactProcesses.vue";
import { useSystemStore } from "@/stores/system";
import type { SystemResponse } from "@/interfaces/SystemResponse";

const process = (name: string, cpu: number, mem: number) => ({
  pid: name.length,
  username: "chris",
  name,
  cpu,
  mem,
});

// Seven by each measure, and not the same seven in the same order.
const byCpu = [
  process("claude.exe", 203.2, 3143.68),
  process("chrome.exe", 38.4, 2129.92),
  process("msedgewebview2.exe", 17.6, 1116.16),
  process("discord.exe", 9.6, 1085.44),
  process("explorer.exe", 6.4, 460),
  process("code.exe", 3.2, 900),
  process("steam.exe", 1.6, 300),
];
const byMem = [...byCpu].sort((a, b) => b.mem - a.mem);

function mounted() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const system = useSystemStore();
  system.staticUpdate({
    cpu: { usage: 8.21, freq: 3200, temp: 52, cores: 16 },
    processes: byMem,
    processes_by_cpu: byCpu,
  } as unknown as SystemResponse);
  const wrapper = mount(CompactProcesses, {
    global: { plugins: [pinia], stubs: ["FontAwesomeIcon"] },
  });
  const rows = () =>
    wrapper.findAll("tbody tr").map((row) => row.findAll("td").map((cell) => cell.text()));
  return { wrapper, rows };
}

describe("CompactProcesses", () => {
  it("lists the five busiest by CPU, as a share of the whole machine", () => {
    const { rows } = mounted();

    expect(rows().map((row) => row[0])).toEqual([
      "claude.exe",
      "chrome.exe",
      "msedgewebview2.exe",
      "discord.exe",
      "explorer.exe",
    ]);
    // 203.2% of one core on a 16-core machine.
    expect(rows()[0][2]).toBe("12.7%");
    expect(rows()[0][3]).toBe("3.07GiB");
    expect(rows()[4][3]).toBe("460MiB");
  });

  it("sorts by memory when its heading is pressed", async () => {
    const { wrapper, rows } = mounted();

    await wrapper.find('button[title="Sort by memory"]').trigger("click");

    expect(rows().map((row) => row[0])).toEqual([
      "claude.exe",
      "chrome.exe",
      "msedgewebview2.exe",
      "discord.exe",
      "code.exe",
    ]);
    expect(wrapper.find("th.col-mem").attributes("aria-sort")).toBe("descending");
  });

  it("shows the rest when asked, and fewer again", async () => {
    const { wrapper, rows } = mounted();
    const more = wrapper.find(".panel-link");
    expect(more.text()).toContain("View all");

    await more.trigger("click");
    expect(rows()).toHaveLength(7);
    expect(more.text()).toContain("Show fewer");

    await more.trigger("click");
    expect(rows()).toHaveLength(5);
  });

  it("falls back to the list by memory from a collector without CPU figures", () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    useSystemStore().staticUpdate({
      cpu: { usage: 1, freq: 2100, temp: 0, cores: 4 },
      processes: [{ pid: 1, username: "root", name: "node", mem: 126 }],
    } as unknown as SystemResponse);
    const wrapper = mount(CompactProcesses, {
      global: { plugins: [pinia], stubs: ["FontAwesomeIcon"] },
    });

    const cells = wrapper.findAll("tbody tr td").map((cell) => cell.text());
    expect(cells[0]).toBe("node");
    expect(cells[2]).toBe("–%");
    expect(cells[3]).toBe("126MiB");
  });
});
