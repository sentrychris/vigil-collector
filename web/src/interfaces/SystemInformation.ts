import type { SystemMetric, SystemMetricTrack } from "./types/SystemTypes";

export interface CpuInformation {
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
  mem: number;
}

export interface SystemMetrics {
  metric: SystemMetric;
  track: SystemMetricTrack;
}
