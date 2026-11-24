use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cpu: Cpu,
    /// The graphics card, where there is one that can be read (see
    /// ``metrics/gpu.rs``).
    pub gpu: Option<Gpu>,
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
    /// The processor's model, tidied of its trademark signs and clock
    /// speed: "Intel Core i7-11700". Empty where it doesn't say.
    pub name: String,
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

/// The graphics card: the one with the most memory of its own, on a
/// machine with several.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    /// How busy it is, in percent: its busiest engine (3D, video decode,
    /// compute…), which is the figure Task Manager shows.
    pub usage: f64,
    /// In °C, and the integer ``0`` where the driver doesn't say, like
    /// ``Cpu::temp``.
    #[serde(serialize_with = "serialize_temp")]
    pub temp: f64,
    /// Its video memory, in GiB like the machine's own.
    pub mem: GpuMemory,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct GpuMemory {
    pub total: f64,
    pub used: f64,
    pub free: f64,
    pub percent: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Mem {
    pub total: f64,
    pub used: f64,
    pub free: f64,
    pub percent: f64,
    /// The kind of memory fitted, such as ``DDR4``, and who made it, both
    /// as the machine's firmware describes its modules. Empty where the
    /// firmware doesn't say or can't be read (see ``metrics/smbios.rs``).
    #[serde(rename = "type")]
    pub kind: String,
    pub manufacturer: String,
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
    /// CPU% of the group over the last scan, on sysinfo's scale: 100 is
    /// one whole core.
    pub cpu: f64,
    /// How busy the group keeps the graphics card, in percent of the
    /// card. Left out of the payload where that can't be told apart by
    /// process (anywhere but Windows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProcessCpu {
    pub pid: u32,
    pub name: String,
    pub username: String,
    pub cpu: f64,
    /// Memory of the group in MiB, by the same accounting as
    /// ``Process::mem`` (see ``processes_metric``).
    pub mem: f64,
    /// As ``Process::gpu``.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f64>,
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
    /// ``null`` on a machine with no graphics card Vigil can read.
    pub gpu: Option<&'a Gpu>,
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
    /// ``null`` on a machine with no graphics card Vigil can read.
    pub gpu: Option<&'a Gpu>,
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
            gpu: self.gpu.as_ref(),
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
            gpu: self.gpu.as_ref(),
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

#[cfg(test)]
mod tests {
    use super::{Cpu, Gpu, GpuMemory, Process, Snapshot};

    #[test]
    fn missing_temperature_serializes_as_integer_zero() {
        let cpu = Cpu::default();
        let json = serde_json::to_value(&cpu).unwrap();
        assert_eq!(json["temp"], serde_json::json!(0));
        assert!(json["temp"].is_u64());
        assert!(json["load"].is_null());
    }

    #[test]
    fn real_temperature_serializes_as_float() {
        let cpu = Cpu {
            temp: 41.5,
            ..Cpu::default()
        };
        assert_eq!(
            serde_json::to_value(&cpu).unwrap()["temp"],
            serde_json::json!(41.5)
        );
    }

    #[test]
    fn a_machine_without_a_graphics_card_says_null() {
        let snapshot = Snapshot::default();
        for json in [
            serde_json::to_value(snapshot.http_view()).unwrap(),
            serde_json::to_value(snapshot.ws_view()).unwrap(),
        ] {
            assert!(json["gpu"].is_null());
            assert!(json.as_object().unwrap().contains_key("gpu"));
        }
    }

    #[test]
    fn a_graphics_card_is_in_both_payloads() {
        let snapshot = Snapshot {
            gpu: Some(Gpu {
                name: "Radeon RX 6800".into(),
                usage: 37.5,
                temp: 0.0,
                mem: GpuMemory {
                    total: 16.0,
                    used: 4.0,
                    free: 12.0,
                    percent: 25.0,
                },
            }),
            ..Snapshot::default()
        };
        let expected = serde_json::json!({
            "name": "Radeon RX 6800",
            "usage": 37.5,
            "temp": 0,
            "mem": { "total": 16.0, "used": 4.0, "free": 12.0, "percent": 25.0 },
        });
        assert_eq!(
            serde_json::to_value(snapshot.http_view()).unwrap()["gpu"],
            expected
        );
        assert_eq!(
            serde_json::to_value(snapshot.ws_view()).unwrap()["gpu"],
            expected
        );
    }

    #[test]
    fn a_process_says_its_gpu_use_only_where_it_is_known() {
        let mut process = Process {
            pid: 1,
            name: "game".into(),
            username: "chris".into(),
            mem: 1.0,
            cpu: 2.0,
            gpu: None,
        };
        let json = serde_json::to_value(&process).unwrap();
        assert!(!json.as_object().unwrap().contains_key("gpu"));
        process.gpu = Some(63.5);
        assert_eq!(serde_json::to_value(&process).unwrap()["gpu"], 63.5);
    }
}
