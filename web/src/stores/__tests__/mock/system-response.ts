import type { SystemResponse } from "@/interfaces/SystemResponse";

export const staticMockData: SystemResponse = {
  cpu: {
    usage: 11.2,
    temp: 56.97,
    freq: 1500,
    cores: 4,
  },
  mem: {
    total: 3.75,
    used: 0.5,
    free: 2.08,
    percent: 18.5,
  },
  disk: {
    total: 28.99,
    used: 12.35,
    free: 15.43,
    percent: 44.4,
  },
  disks: [
    {
      device: "/dev/root",
      mountpoint: "/",
      fstype: "ext4",
      total: 28.99,
      used: 12.35,
      free: 15.43,
      percent: 44.4,
    },
  ],
  network: {
    rx_bytes_per_sec: 0,
    tx_bytes_per_sec: 0,
  },
  disk_io: {
    read_bytes_per_sec: 0,
    write_bytes_per_sec: 0,
    read_iops: 0,
    write_iops: 0,
  },
  platform: {
    distro: "Raspbian GNU/Linux 11 (bullseye)",
    kernel: "5.15.61-v7l+",
    uptime: "4 days, 19 hours, 42 minutes, 46 seconds",
  },
  user: "www-data",
  processes: [
    {
      pid: 634,
      username: "root",
      name: "Xorg",
      mem: 69.68,
    },
    {
      pid: 16830,
      username: "pi",
      name: "lxterminal",
      mem: 28.3,
    },
  ],
};
