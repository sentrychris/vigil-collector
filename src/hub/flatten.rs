use std::collections::BTreeMap;

use crate::snapshot::Snapshot;

// 2 GiB ceiling on bytes-typed metrics. The hub validator rejects anything
// past this; mirror the Python ``_BYTES_CEILING`` for consistent gating.
const BYTES_CEILING: u64 = 1u64 << 41;
pub const PROCESSES_MAX: usize = 25;

fn gib_to_bytes(value: f64) -> u64 {
    let raw = (value * (1024.0_f64.powi(3))) as u64;
    raw.min(BYTES_CEILING)
}

/// Flatten the sampler snapshot into the Hub's wire-format metric map.
/// Dimensioned metrics carry their dim after a pipe (`disk.used_bytes|/`).
pub fn flatten(snapshot: &Snapshot) -> BTreeMap<String, f64> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();

    let cpu = &snapshot.cpu;
    out.insert("cpu.usage".into(), cpu.usage);
    if cpu.freq != 0.0 {
        out.insert("cpu.freq_mhz".into(), cpu.freq);
    }
    if let Some(load) = cpu.load {
        out.insert("cpu.load_1m".into(), load[0]);
        out.insert("cpu.load_5m".into(), load[1]);
        out.insert("cpu.load_15m".into(), load[2]);
    }
    if cpu.temp.is_finite() && cpu.temp != 0.0 {
        out.insert("cpu.temp_c".into(), cpu.temp);
    }

    let mem = &snapshot.mem;
    out.insert("mem.total_bytes".into(), gib_to_bytes(mem.total) as f64);
    out.insert("mem.used_bytes".into(), gib_to_bytes(mem.used) as f64);
    out.insert("mem.free_bytes".into(), gib_to_bytes(mem.free) as f64);
    out.insert("mem.percent".into(), mem.percent);

    let dio = &snapshot.disk_io;
    out.insert(
        "disk.io.read_bytes_per_s".into(),
        dio.read_bytes_per_sec as f64,
    );
    out.insert(
        "disk.io.write_bytes_per_s".into(),
        dio.write_bytes_per_sec as f64,
    );
    out.insert("disk.io.read_iops".into(), dio.read_iops as f64);
    out.insert("disk.io.write_iops".into(), dio.write_iops as f64);

    let net = &snapshot.network;
    out.insert("net.rx_bytes_per_s".into(), net.rx_bytes_per_sec as f64);
    out.insert("net.tx_bytes_per_s".into(), net.tx_bytes_per_sec as f64);

    for d in &snapshot.disks {
        let mount = &d.mountpoint;
        if mount.is_empty() {
            continue;
        }
        // Containerised mounts and snap loops bloat the metric namespace
        // without telling the operator anything actionable.
        if mount.starts_with("/snap/")
            || mount.starts_with("/var/lib/docker/")
            || mount.starts_with("/run/docker/")
        {
            continue;
        }
        out.insert(
            format!("disk.total_bytes|{mount}"),
            gib_to_bytes(d.total) as f64,
        );
        out.insert(
            format!("disk.used_bytes|{mount}"),
            gib_to_bytes(d.used) as f64,
        );
        out.insert(format!("disk.percent|{mount}"), d.percent);
    }

    out
}
