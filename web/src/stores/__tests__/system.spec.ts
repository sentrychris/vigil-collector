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

  it("keeps the graphics card's readings too, and nothing where there is no card", () => {
    const system = useSystemStore();
    const gpu = (usage: number) => ({
      name: "NVIDIA GeForce RTX 3070",
      usage,
      temp: 54,
      mem: { total: 8, used: 2, free: 6, percent: 25 },
    });

    system.liveUpdate({ ...reading(8), gpu: gpu(40) });
    system.liveUpdate({ ...reading(9), gpu: gpu(55) });
    expect(system.history.gpu).toEqual([40, 55]);
    expect(system.realtime.gpu?.name).toBe("NVIDIA GeForce RTX 3070");

    // The card goes away (or the collector has none to read).
    system.liveUpdate({ ...reading(9), gpu: null });
    expect(system.realtime.gpu).toBeNull();
    expect(system.history.gpu).toEqual([40, 55, 0]);
  });

  it("keeps each disk's reads and writes while it is listed", () => {
    const system = useSystemStore();
    const disk = (mountpoint: string, read: number) => ({
      device: mountpoint,
      mountpoint,
      fstype: "NTFS",
      total: 475,
      used: 318,
      free: 157,
      percent: 66.9,
      io: { read_bytes_per_sec: read, write_bytes_per_sec: 24, read_iops: 1, write_iops: 1 },
    });

    system.liveUpdate({ ...reading(8), disks: [disk("C:\\", 1000), disk("D:\\", 0)] });
    system.liveUpdate({ ...reading(9), disks: [disk("C:\\", 2000), disk("D:\\", 0)] });
    expect(system.history.disks).toEqual({ "C:\\": [1024, 2024], "D:\\": [24, 24] });
    expect(system.realtime.disks).toHaveLength(2);

    // D: is unplugged, then an older collector's reading lists none.
    system.liveUpdate({ ...reading(9), disks: [disk("C:\\", 0)] });
    expect(system.history.disks).toEqual({ "C:\\": [1024, 2024, 24] });
    system.liveUpdate(reading(9));
    expect(system.history.disks).toEqual({});
    expect(system.realtime.disks).toEqual([]);
  });

  it("takes a reading from an older collector, without the list by CPU", () => {
    const system = useSystemStore();

    system.liveUpdate(reading(8));

    expect(system.realtime.processes_by_cpu).toEqual([]);
  });

  it("drops the list by GPU when a reading comes without one", () => {
    const system = useSystemStore();
    const game = { pid: 7, username: "chris", name: "game.exe", cpu: 40, mem: 900, gpu: 82.5 };

    system.liveUpdate({ ...reading(8), processes_by_gpu: [game] });
    expect(system.realtime.processes_by_gpu).toEqual([game]);

    system.liveUpdate(reading(9));
    expect(system.realtime.processes_by_gpu).toEqual([]);
  });
});
