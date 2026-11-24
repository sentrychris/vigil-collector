import type { SystemMetric, SystemMetricTrack } from "./types/SystemTypes";

export interface CpuInformation {
  /** The processor's model, e.g. "Intel Core i7-11700". Empty where it
   *  doesn't say; older collectors leave it out. */
  name?: string;
  freq: number;
  temp: number;
  usage: number;
  cores: number;
}

export interface UsageInformation {
  total: number;
  used: number;
  free: number;
  percent: number; // (total - free) / total * 100
}

export interface MemoryInformation extends UsageInformation {
  /** The kind of memory fitted, e.g. "DDR4", and who made it. Either is
   *  empty where the machine's firmware doesn't say or can't be read. */
  type?: string;
  manufacturer?: string;
}

/** The graphics card: the one with the most memory of its own. */
export interface GpuInformation {
  name: string;
  /** Percent: its busiest engine. */
  usage: number;
  /** °C, or 0 where the driver doesn't say. */
  temp: number;
  /** Its video memory, GiB. */
  mem: UsageInformation;
}

export interface DiskUsageInformation extends UsageInformation {
  device: string;
  mountpoint: string;
  fstype: string;
}

export interface NetworkRateInformation {
  rx_bytes_per_sec: number;
  tx_bytes_per_sec: number;
}

export interface DiskIoInformation {
  read_bytes_per_sec: number;
  write_bytes_per_sec: number;
  read_iops: number;
  write_iops: number;
}

export interface PlatformInformation {
  distro: string;
  kernel: string;
  uptime: string;
}

export interface ProcessInformation {
  pid: number;
  username: string;
  name: string;
  /** MiB. */
  mem: number;
  /** Percent of one core, so it can pass 100 on a machine with several.
   *  Collectors before 0.3 leave it out of ``processes``. */
  cpu?: number;
  /** Percent of the graphics card. Only from a collector that can tell
   *  it apart by process (Windows). */
  gpu?: number;
}

export interface SystemMetrics {
  metric: SystemMetric;
  track: SystemMetricTrack;
}
