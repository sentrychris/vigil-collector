import type {
  CpuInformation,
  UsageInformation,
  DiskUsageInformation,
  DiskIoInformation,
  NetworkRateInformation,
  PlatformInformation,
  ProcessInformation,
} from "./SystemInformation";

export interface SystemResponse {
  cpu: CpuInformation;
  mem: UsageInformation;
  disk: UsageInformation;
  disks: DiskUsageInformation[];
  disk_io: DiskIoInformation;
  network: NetworkRateInformation;
  platform: PlatformInformation;
  processes: ProcessInformation[];
  user: string;
}

export interface RealtimeSystemResponse {
  cpu: CpuInformation;
  mem: UsageInformation;
  disk: UsageInformation;
  disk_io: DiskIoInformation;
  network: NetworkRateInformation;
  uptime?: string | null;
  processes: ProcessInformation[];
}
