use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cpu: Cpu,
    pub mem: Mem,
    pub disk: Disk,
    pub disks: Vec<DiskInfo>,
    pub disk_io: DiskIo,
    pub network: Throughput,
    pub user: String,
    pub platform: PlatformInfo,
    pub uptime: String,
    pub processes: Vec<Process>,
    pub processes_metric: String,
    pub processes_by_cpu: Vec<ProcessCpu>,
    pub interfaces: Vec<String>,
    pub network_stats: BTreeMap<String, NetIfaceStats>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Cpu {
    pub usage: f64,
    /// CPU package temperature in °C. Serializes as the integer ``0`` when
    /// no sensor is available (matching psutil's behavior of returning
    /// ``0`` on platforms without thermal probes), and as a float
    /// otherwise. Mixed JSON types preserve byte-compatibility with the
    /// Python collector.
    #[serde(serialize_with = "serialize_temp")]
    pub temp: f64,
    pub freq: f64,
    pub cores: u32,
    pub load: Option<[f64; 3]>,
}

fn serialize_temp<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if *v == 0.0 {
        s.serialize_u64(0)
    } else {
        s.serialize_f64(*v)
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Mem {
    pub total: f64,
    pub used: f64,
    pub free: f64,
    pub percent: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Disk {
    pub total: f64,
    pub used: f64,
    pub free: f64,
    pub percent: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
    pub total: f64,
    pub used: f64,
    pub free: f64,
    pub percent: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct DiskIo {
    pub read_bytes_per_sec: u64,
    pub write_bytes_per_sec: u64,
    pub read_iops: u64,
    pub write_iops: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Throughput {
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct PlatformInfo {
    pub distro: String,
    pub kernel: String,
    pub uptime: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub username: String,
    pub mem: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProcessCpu {
    pub pid: u32,
    pub name: String,
    pub username: String,
    pub cpu: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct NetIfaceStats {
    pub mb_sent: f64,
    pub mb_received: f64,
    pub pk_sent: u64,
    pub pk_received: u64,
    pub error_in: u64,
    pub error_out: u64,
    pub dropin: u64,
    pub dropout: u64,
}

/// HTTP `/system` projection. Mirrors ``server/http/system.py``.
#[derive(Serialize)]
pub struct HttpSystemView<'a> {
    pub cpu: &'a Cpu,
    pub mem: &'a Mem,
    pub disk: &'a Disk,
    pub disks: &'a [DiskInfo],
    pub disk_io: &'a DiskIo,
    pub network: &'a Throughput,
    pub user: &'a str,
    pub platform: &'a PlatformInfo,
    pub processes: &'a [Process],
    pub processes_metric: &'a str,
    pub processes_by_cpu: &'a [ProcessCpu],
}

/// WebSocket frame projection. Mirrors ``server/websocket/system.py`` —
/// note: no `disks`, no `user`, no `platform`; `uptime` is top-level.
#[derive(Serialize)]
pub struct WsSystemView<'a> {
    pub cpu: &'a Cpu,
    pub mem: &'a Mem,
    pub disk: &'a Disk,
    pub disk_io: &'a DiskIo,
    pub network: &'a Throughput,
    pub uptime: &'a str,
    pub processes: &'a [Process],
    pub processes_metric: &'a str,
    pub processes_by_cpu: &'a [ProcessCpu],
}

/// HTTP `/network` projection. Mirrors ``server/http/network.py``.
#[derive(Serialize)]
pub struct NetworkView<'a> {
    pub interfaces: &'a [String],
    pub statistics: &'a BTreeMap<String, NetIfaceStats>,
}

impl Snapshot {
    pub fn http_view(&self) -> HttpSystemView<'_> {
        HttpSystemView {
            cpu: &self.cpu,
            mem: &self.mem,
            disk: &self.disk,
            disks: &self.disks,
            disk_io: &self.disk_io,
            network: &self.network,
            user: &self.user,
            platform: &self.platform,
            processes: &self.processes,
            processes_metric: &self.processes_metric,
            processes_by_cpu: &self.processes_by_cpu,
        }
    }

    pub fn ws_view(&self) -> WsSystemView<'_> {
        WsSystemView {
            cpu: &self.cpu,
            mem: &self.mem,
            disk: &self.disk,
            disk_io: &self.disk_io,
            network: &self.network,
            uptime: &self.uptime,
            processes: &self.processes,
            processes_metric: &self.processes_metric,
            processes_by_cpu: &self.processes_by_cpu,
        }
    }

    pub fn network_view(&self) -> NetworkView<'_> {
        NetworkView {
            interfaces: &self.interfaces,
            statistics: &self.network_stats,
        }
    }
}
