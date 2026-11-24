import type {
  CpuInformation,
  GpuInformation,
  MemoryInformation,
  UsageInformation,
  DiskUsageInformation,
  DiskIoInformation,
  NetworkRateInformation,
  PlatformInformation,
  ProcessInformation,
} from "./SystemInformation";

export interface SystemResponse {
  cpu: CpuInformation;
  /** ``null`` on a machine with no graphics card the collector can read;
   *  left out by older collectors. */
  gpu?: GpuInformation | null;
  mem: MemoryInformation;
  disk: UsageInformation;
  disks: DiskUsageInformation[];
  disk_io: DiskIoInformation;
  network: NetworkRateInformation;
  platform: PlatformInformation;
  processes: ProcessInformation[];
  processes_by_cpu?: ProcessInformation[];
  /** The ten using the graphics card most. Only from a collector that
   *  can tell that apart by process (Windows). */
  processes_by_gpu?: ProcessInformation[];
  user: string;
}

export interface RealtimeSystemResponse {
  cpu: CpuInformation;
  gpu?: GpuInformation | null;
  mem: MemoryInformation;
  disk: UsageInformation;
  disk_io: DiskIoInformation;
  network: NetworkRateInformation;
  uptime?: string | null;
  processes: ProcessInformation[];
  processes_by_cpu?: ProcessInformation[];
  /** The ten using the graphics card most. Only from a collector that
   *  can tell that apart by process (Windows). */
  processes_by_gpu?: ProcessInformation[];
}
