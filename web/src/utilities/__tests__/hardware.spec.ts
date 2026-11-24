import { describe, expect, it } from "vitest";
import { diskName, diskThroughput, gpuModel, memoryName, shownDisks } from "../hardware";

describe("hardware names", () => {
  it("leaves a graphics card's maker off its model", () => {
    expect(gpuModel("NVIDIA GeForce RTX 3070")).toBe("GeForce RTX 3070");
    expect(gpuModel("AMD Radeon RX 6800 XT")).toBe("Radeon RX 6800 XT");
    expect(gpuModel("Intel UHD Graphics 750")).toBe("UHD Graphics 750");
    expect(gpuModel("Graphics card")).toBe("Graphics card");
    expect(gpuModel("NVIDIA")).toBe("NVIDIA");
    expect(gpuModel(undefined)).toBe("");
  });

  it("says what memory is fitted with as much as is known", () => {
    expect(memoryName({ type: "DDR4", manufacturer: "Corsair" })).toBe("DDR4 · Corsair");
    expect(memoryName({ type: "DDR5", manufacturer: "" })).toBe("DDR5");
    expect(memoryName({ type: "", manufacturer: "" })).toBe("");
    // A collector from before it reported either.
    expect(memoryName({})).toBe("");
    expect(memoryName(undefined)).toBe("");
  });
});

describe("disks", () => {
  const disk = (device: string, mountpoint: string, total = 475.1) => ({
    device,
    mountpoint,
    fstype: "ext4",
    total,
    used: total / 2,
    free: total / 2,
    percent: 50,
  });
  const io = (read: number, write: number) => ({
    read_bytes_per_sec: read,
    write_bytes_per_sec: write,
    read_iops: 0,
    write_iops: 0,
  });

  it("gives a card to each real disk, once", () => {
    const shown = shownDisks([
      disk("/dev/nvme0n1p2", "/"),
      // The same partition again, a snap package and an empty slot.
      disk("/dev/nvme0n1p2", "/var/lib/docker"),
      disk("/dev/loop3", "/snap/core22/1122", 0.07),
      disk("/dev/sdb1", "/media/card", 0),
      disk("/dev/sda1", "/home", 1863),
    ]);
    expect(shown.map((d) => d.mountpoint)).toEqual(["/", "/home"]);
    expect(shownDisks(undefined)).toEqual([]);
  });

  it("names a disk by where it is mounted and what it is called", () => {
    expect(diskName({ mountpoint: "C:\\" })).toBe("C:");
    expect(diskName({ mountpoint: "D:\\", label: "Games" })).toBe("D: · Games");
    expect(diskName({ mountpoint: "/", label: "" })).toBe("/");
    expect(diskName({ mountpoint: "/home" })).toBe("/home");
  });

  it("reads a disk's own reads and writes, or none where it doesn't say", () => {
    const quiet = disk("D:\\", "D:\\");
    const busy = { ...disk("C:\\", "C:\\"), io: io(1000, 24) };
    expect(diskThroughput(busy, [busy, quiet], io(5000, 0))).toBe(1024);
    expect(diskThroughput(quiet, [busy, quiet], io(5000, 0))).toBeNull();
    // An only disk does all of the machine's reading and writing.
    expect(diskThroughput(quiet, [quiet], io(5000, 0))).toBe(5000);
  });
});
