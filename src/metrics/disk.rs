use std::path::Path;
use std::time::Instant;

use sysinfo::Disks;

use crate::snapshot::{Disk, DiskInfo, DiskIo};

use super::bytes_to_gib;

#[cfg(unix)]
const ROOT_MOUNT: &str = "/";
#[cfg(windows)]
const ROOT_MOUNT: &str = "C:\\";

/// Tuple of (total, used, available) bytes for a mountpoint. Mirrors
/// psutil's exact arithmetic on Unix:
///
/// - ``total = f_blocks * f_frsize``
/// - ``used  = (f_blocks - f_bfree) * f_frsize`` (counts reserved-for-root
///    blocks as USED — the inverse of what sysinfo computes via
///    ``total - available``)
/// - ``free  = f_bavail * f_frsize``
/// - ``percent = used / (used + free) * 100`` (df's denominator, not total)
///
/// The reserved-for-root delta is meaningful (~5% on ext4 by default),
/// so matching psutil here is what keeps the dashboard's "10.8% used"
/// number from drifting from ``df -h``.
#[cfg(unix)]
fn statvfs_usage(path: &Path) -> Option<(u64, u64, u64, f64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut st) };
    if rc != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    let total = (st.f_blocks as u64).saturating_mul(frsize);
    let bfree_blocks = st.f_bfree as u64;
    let bavail_blocks = st.f_bavail as u64;
    let used = (st.f_blocks as u64)
        .saturating_sub(bfree_blocks)
        .saturating_mul(frsize);
    let free = bavail_blocks.saturating_mul(frsize);
    let denom = used + free;
    let percent = if denom > 0 {
        super::round1(used as f64 / denom as f64 * 100.0)
    } else {
        0.0
    };
    Some((total, used, free, percent))
}

#[cfg(not(unix))]
fn statvfs_usage(_path: &Path) -> Option<(u64, u64, u64, f64)> {
    None
}

fn usage_for_mount(path: &Path, fallback_total: u64, fallback_avail: u64) -> (f64, f64, f64, f64) {
    if let Some((total, used, free, percent)) = statvfs_usage(path) {
        return (
            bytes_to_gib(total),
            bytes_to_gib(used),
            bytes_to_gib(free),
            percent,
        );
    }
    // Non-Unix path or statvfs failure: fall back to sysinfo's view. Less
    // accurate (no reserved-block accounting) but always available.
    let used = fallback_total.saturating_sub(fallback_avail);
    let percent = if fallback_total > 0 {
        super::round1(used as f64 / fallback_total as f64 * 100.0)
    } else {
        0.0
    };
    (
        bytes_to_gib(fallback_total),
        bytes_to_gib(used),
        bytes_to_gib(fallback_avail),
        percent,
    )
}

pub fn collect_root(disks: &Disks) -> Disk {
    let target = disks
        .iter()
        .find(|d| d.mount_point().to_string_lossy() == ROOT_MOUNT)
        .or_else(|| disks.iter().find(|d| d.mount_point().to_string_lossy() == "/"))
        .or_else(|| disks.iter().next());

    if let Some(d) = target {
        let (total, used, free, percent) =
            usage_for_mount(d.mount_point(), d.total_space(), d.available_space());
        return Disk {
            total,
            used,
            free,
            percent,
        };
    }
    Disk::default()
}

pub fn collect_all(disks: &Disks) -> Vec<DiskInfo> {
    let nodev = nodev_filesystems();
    disks
        .iter()
        .filter_map(|d| {
            let device = d.name().to_string_lossy().into_owned();
            let fstype = d.file_system().to_string_lossy().into_owned();

            // psutil's ``disk_partitions(all=False)`` filters partitions
            // whose device is empty/synthetic OR whose fstype is marked
            // ``nodev`` in /proc/filesystems (i.e. pseudo filesystems
            // like tmpfs, proc, sysfs, overlay, 9p, fuse.snapfuse, ...).
            // Match that on Linux; on other platforms, sysinfo's list is
            // already physical-only.
            #[cfg(target_os = "linux")]
            {
                if !device.starts_with('/') {
                    return None;
                }
                if nodev.contains(&fstype) {
                    return None;
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = &nodev; // silence unused on non-Linux
            }

            let (total, used, free, percent) =
                usage_for_mount(d.mount_point(), d.total_space(), d.available_space());
            Some(DiskInfo {
                device,
                mountpoint: d.mount_point().to_string_lossy().into_owned(),
                fstype,
                total,
                used,
                free,
                percent,
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn nodev_filesystems() -> std::collections::HashSet<String> {
    // /proc/filesystems lines are either "nodev<TAB><fstype>" or
    // "<TAB><fstype>". We collect the nodev set; everything else is
    // implicitly a "physical" filesystem worth surfacing.
    let mut set = std::collections::HashSet::new();
    let Ok(raw) = std::fs::read_to_string("/proc/filesystems") else {
        return set;
    };
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("nodev") {
            set.insert(rest.trim().to_string());
        }
    }
    // Snap loops show up as ``fuse.snapfuse`` which /proc/filesystems
    // doesn't list directly — they register through the generic ``fuse``
    // entry. psutil filters them via the device path heuristic, but to
    // be safe we add common fuse-ish pseudo entries here.
    set.insert("fuse.snapfuse".into());
    set
}

#[cfg(not(target_os = "linux"))]
fn nodev_filesystems() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

#[derive(Clone, Debug)]
pub struct DiskIoPrev {
    pub at: Instant,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ops: u64,
    pub write_ops: u64,
}

#[cfg(target_os = "linux")]
fn whole_disks() -> std::collections::HashSet<String> {
    // /sys/block contains one entry per whole-disk block device; partitions
    // sit *inside* those entries (e.g. /sys/block/sda/sda1). Summing only
    // whole-disk rows from /proc/diskstats avoids double-counting bytes
    // attributed to both a disk and its constituent partitions.
    let mut set = std::collections::HashSet::new();
    let Ok(rd) = std::fs::read_dir("/sys/block") else {
        return set;
    };

    for entry in rd.flatten() { // skip failed entries, e.g. permission-denied vfs
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("loop") || name.starts_with("ram") {
            continue;
        }
        set.insert(name);
    }
    set
}

#[cfg(target_os = "linux")]
fn read_diskstats() -> Option<(u64, u64, u64, u64)> {
    let raw = std::fs::read_to_string("/proc/diskstats").ok()?;
    let allowed = whole_disks();
    let mut read_ops = 0u64;
    let mut read_sectors = 0u64;
    let mut write_ops = 0u64;
    let mut write_sectors = 0u64;

    for line in raw.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 14 {
            continue;
        }
        let dev = f[2];
        if !allowed.contains(dev) {
            continue;
        }
        // /proc/diskstats column order (1-indexed in kernel docs, 0-indexed
        // here after the leading major/minor/dev fields):
        //   3: reads completed   5: sectors read
        //   7: writes completed  9: sectors written
        let r_ops: u64 = f[3].parse().unwrap_or(0);
        let r_sec: u64 = f[5].parse().unwrap_or(0);
        let w_ops: u64 = f[7].parse().unwrap_or(0);
        let w_sec: u64 = f[9].parse().unwrap_or(0);
        read_ops = read_ops.saturating_add(r_ops);
        read_sectors = read_sectors.saturating_add(r_sec);
        write_ops = write_ops.saturating_add(w_ops);
        write_sectors = write_sectors.saturating_add(w_sec);
    }

    // /proc/diskstats reports sectors in 512-byte units regardless of the
    // device's actual sector size — see Documentation/iostats.rst.
    Some((read_sectors * 512, write_sectors * 512, read_ops, write_ops))
}

#[cfg(not(target_os = "linux"))]
fn read_diskstats() -> Option<(u64, u64, u64, u64)> {
    None
}

pub fn collect_io(prev: &mut Option<DiskIoPrev>) -> DiskIo {
    let Some((read_bytes, write_bytes, read_ops, write_ops)) = read_diskstats() else {
        return DiskIo::default();
    };
    let now = Instant::now();
    let last = prev.replace(DiskIoPrev {
        at: now,
        read_bytes,
        write_bytes,
        read_ops,
        write_ops,
    });
    let Some(last) = last else {
        return DiskIo::default();
    };
    let elapsed = now.duration_since(last.at).as_secs_f64();
    if elapsed <= 0.0 {
        return DiskIo::default();
    }
    DiskIo {
        read_bytes_per_sec: ((read_bytes.saturating_sub(last.read_bytes)) as f64 / elapsed)
            .round() as u64,
        write_bytes_per_sec: ((write_bytes.saturating_sub(last.write_bytes)) as f64 / elapsed)
            .round() as u64,
        read_iops: ((read_ops.saturating_sub(last.read_ops)) as f64 / elapsed).round() as u64,
        write_iops: ((write_ops.saturating_sub(last.write_ops)) as f64 / elapsed).round() as u64,
    }
}
