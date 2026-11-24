/**
 * The names of a machine's parts, as the cards have room to show them,
 * and which of its disks get a card.
 */
import type { DiskIoInformation, DiskUsageInformation } from "@/interfaces/SystemInformation";

/** A graphics card's model without its maker: "NVIDIA GeForce RTX 3070"
 *  as "GeForce RTX 3070". */
export function gpuModel(name: string | null | undefined): string {
  const full = (name ?? "").trim();
  const model = full.replace(/^(NVIDIA|AMD|Intel)\s+/, "");
  return model || full;
}

/** What memory is fitted, as much as is known: "DDR4 · Corsair". */
export function memoryName(
  mem: { type?: string; manufacturer?: string } | null | undefined,
): string {
  return [mem?.type, mem?.manufacturer].filter(Boolean).join(" · ");
}

/**
 * The disks that get a card each in the compact dashboard, in the order
 * of the cards: the collector's list (the primary disk first) without
 * what isn't a disk to its owner. That is anything of no size (a card
 * reader with no card in it), Linux's loop devices (snap packages), and a
 * device already listed under another mount point.
 */
export function shownDisks(
  disks: DiskUsageInformation[] | null | undefined,
): DiskUsageInformation[] {
  const shown: DiskUsageInformation[] = [];
  for (const disk of disks ?? []) {
    const sized = Number.isFinite(disk.total) && disk.total > 0;
    const listed = shown.some((other) => other.device === disk.device);
    if (sized && !listed && !disk.device.startsWith("/dev/loop")) shown.push(disk);
  }
  return shown;
}

/** Which disk a card is for: where it is mounted, and what its owner
 *  called it if anything. "C: · Games", or "/home". */
export function diskName(disk: { mountpoint: string; label?: string }): string {
  const mountpoint = disk.mountpoint.replace(/\\+$/, "") || disk.mountpoint;
  return disk.label ? `${mountpoint} · ${disk.label}` : mountpoint;
}

/**
 * What a disk is reading and writing, in bytes a second, or null where
 * that isn't known. A machine's only disk does all of the machine's
 * reading and writing, so there the total stands in for a figure of its
 * own.
 */
export function diskThroughput(
  disk: DiskUsageInformation,
  shown: DiskUsageInformation[],
  total: DiskIoInformation | null | undefined,
): number | null {
  const both = (io: DiskIoInformation | null | undefined) =>
    (io?.read_bytes_per_sec ?? 0) + (io?.write_bytes_per_sec ?? 0);
  if (disk.io) return both(disk.io);
  return shown.length === 1 ? both(total) : null;
}
