import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { HISTORY_LENGTH, useSystemStore } from "../system";
import { staticMockData } from "./mock/system-response";

describe("System Store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("patches", () => {
    const system = useSystemStore();

    expect(system.live).toBe(false);
    system.staticUpdate(staticMockData);
    expect(system.data).toEqual(staticMockData);
  });

  const reading = (cpu: number) => ({
    cpu: { usage: cpu, freq: 3200, temp: 0, cores: 16 },
    mem: { total: 32, used: 12, free: 20, percent: 37.5 },
    disk: { total: 475, used: 318, free: 157, percent: 66.9 },
    disk_io: { read_bytes_per_sec: 1000, write_bytes_per_sec: 24, read_iops: 1, write_iops: 1 },
    network: { rx_bytes_per_sec: 2048, tx_bytes_per_sec: 512 },
    uptime: "1 day, 6 hrs",
    processes: [],
  });

  it("keeps each live reading for the sparklines", () => {
    const system = useSystemStore();

    system.liveUpdate(reading(8));
    system.liveUpdate(reading(9));

    expect(system.history.cpu).toEqual([8, 9]);
    expect(system.history.mem).toEqual([37.5, 37.5]);
    // Reads and writes together.
    expect(system.history.diskIo).toEqual([1024, 1024]);
    expect(system.history.rx).toEqual([2048, 2048]);
    expect(system.history.tx).toEqual([512, 512]);
  });

  it("keeps no more than the last minute of them", () => {
    const system = useSystemStore();

    for (let i = 0; i < HISTORY_LENGTH + 5; i++) system.liveUpdate(reading(i));

    expect(system.history.cpu).toHaveLength(HISTORY_LENGTH);
    expect(system.history.cpu[0]).toBe(5);
    expect(system.history.cpu[HISTORY_LENGTH - 1]).toBe(HISTORY_LENGTH + 4);
  });

  it("takes a reading from an older collector, without the list by CPU", () => {
    const system = useSystemStore();

    system.liveUpdate(reading(8));

    expect(system.realtime.processes_by_cpu).toEqual([]);
  });
});
