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

// How much of the graphics card each uses, on a machine that can tell.
const gpuOf: Record<string, number> = {
  "chrome.exe": 21.37,
  "discord.exe": 64.2,
  "explorer.exe": 0.4,
};

/**
 * ``withGpu``: a collector that can tell says how much of the graphics
 * card each process uses, and the others leave it out. ``listByGpu``:
 * it also sends the ten using the card most, which one from before that
 * list was added doesn't.
 */
function mounted(withGpu = false, listByGpu = withGpu) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const system = useSystemStore();
  const withShare = (list: typeof byCpu) => list.map((p) => ({ ...p, gpu: gpuOf[p.name] ?? 0 }));
  const told = (list: typeof byCpu) => (withGpu ? withShare(list) : list);
  const byGpu = withShare(byCpu).sort((a, b) => b.gpu - a.gpu || b.cpu - a.cpu);
  system.staticUpdate({
    cpu: { usage: 8.21, freq: 3200, temp: 52, cores: 16 },
    processes: told(byMem),
    processes_by_cpu: told(byCpu),
    ...(listByGpu ? { processes_by_gpu: byGpu } : {}),
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

  it("has a GPU column when the collector says who is using the card", () => {
    const { wrapper, rows } = mounted(true);

    expect(wrapper.findAll("thead th").map((th) => th.text())).toEqual([
      "Name",
      "",
      "CPU",
      "GPU",
      "Memory",
    ]);
    // claude.exe, then chrome.exe: processor, graphics card, memory.
    expect(rows()[0].slice(2)).toEqual(["12.7%", "0.0%", "3.07GiB"]);
    expect(rows()[1].slice(2)).toEqual(["2.4%", "21.4%", "2.08GiB"]);
  });

  it("sorts by GPU when its heading is pressed", async () => {
    const { wrapper, rows } = mounted(true);

    await wrapper.find('button[title="Sort by GPU"]').trigger("click");

    // Those using the card, most first, and then the busiest on the
    // processor.
    expect(rows().map((row) => [row[0], row[3]])).toEqual([
      ["discord.exe", "64.2%"],
      ["chrome.exe", "21.4%"],
      ["explorer.exe", "0.4%"],
      ["claude.exe", "0.0%"],
      ["msedgewebview2.exe", "0.0%"],
    ]);
    const headings = wrapper.findAll("thead th");
    expect(headings[3].attributes("aria-sort")).toBe("descending");
    expect(headings[2].attributes("aria-sort")).toBe("none");
    // The bars are each one's share of the busiest on the card.
    const bars = wrapper.findAll(".share-fill").map((bar) => bar.attributes("style"));
    expect(bars[0]).toContain("width: 100%");
    expect(bars[3]).toContain("width: 2%");

    await wrapper.find('button[title="Sort by CPU"]').trigger("click");
    expect(rows()[0][0]).toBe("claude.exe");
    expect(headings[3].attributes("aria-sort")).toBe("none");
  });

  it("leaves the GPU heading as words for a collector that sends no list by GPU", () => {
    const { wrapper } = mounted(true, false);

    const heading = wrapper.findAll("thead th")[3];
    expect(heading.text()).toBe("GPU");
    expect(heading.find("button").exists()).toBe(false);
    expect(heading.attributes("aria-sort")).toBeUndefined();
  });

  it("goes back to sorting by CPU when the list by GPU goes away", async () => {
    const { wrapper, rows } = mounted(true);
    await wrapper.find('button[title="Sort by GPU"]').trigger("click");
    expect(rows()[0][0]).toBe("discord.exe");

    // Another machine is looked at, and it can't tell.
    useSystemStore().staticUpdate({
      cpu: { usage: 8.21, freq: 3200, temp: 52, cores: 16 },
      processes: byMem,
      processes_by_cpu: byCpu,
      processes_by_gpu: [],
    } as unknown as SystemResponse);
    await wrapper.vm.$nextTick();

    expect(rows()[0][0]).toBe("claude.exe");
    expect(wrapper.find("thead th:nth-child(3)").attributes("aria-sort")).toBe("descending");
  });

  it("has no GPU column when it doesn't", () => {
    const { wrapper, rows } = mounted();

    expect(wrapper.findAll("thead th")).toHaveLength(4);
    expect(rows()[0]).toHaveLength(4);
  });
});
