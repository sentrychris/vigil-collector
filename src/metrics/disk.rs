use std::path::Path;
use std::time::Instant;

use sysinfo::Disks;

use crate::snapshot::{Disk, DiskInfo, DiskIo};

use super::bytes_to_gib;

/// Byte counts for one mounted filesystem, already reduced to the four
/// numbers the wire format carries.
struct Usage {
    total: u64,
    used: u64,
    free: u64,
    percent: f64,
}

impl Usage {
    /// ``percent = used / (used + free) * 100``: df's denominator, not
    /// ``total``. The two differ by the reserved-for-root blocks on ext4
    /// (~5% by default) and by per-user quotas on NTFS.
    fn new(total: u64, used: u64, free: u64) -> Self {
        let denom = used.saturating_add(free);
        let percent = if denom > 0 {
            super::round1(used as f64 / denom as f64 * 100.0)
        } else {
            0.0
        };
        Self {
            total,
            used,
            free,
            percent,
        }
    }
}

/// Mirrors psutil's exact arithmetic on Unix:
///
/// - ``total = f_blocks * f_frsize``
/// - ``used  = (f_blocks - f_bfree) * f_frsize`` (counts reserved-for-root
///   blocks as USED — the inverse of what sysinfo computes via
///   ``total - available``)
/// - ``free  = f_bavail * f_frsize``
///
/// Matching psutil here is what keeps the dashboard's "10.8% used" number
/// from drifting from ``df -h``.
#[cfg(unix)]
fn fs_usage(path: &Path) -> Option<Usage> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs is plain-old-data, so all-zeroes is a valid value,
    // and c_path is a NUL-terminated string that outlives the call.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut st) };
    if rc != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    let blocks = st.f_blocks as u64;
    let total = blocks.saturating_mul(frsize);
    let used = blocks
        .saturating_sub(st.f_bfree as u64)
        .saturating_mul(frsize);
    let free = (st.f_bavail as u64).saturating_mul(frsize);
    Some(Usage::new(total, used, free))
}

/// Windows equivalent of ``statvfs``, via ``GetDiskFreeSpaceExW``:
///
/// - ``total = TotalNumberOfBytes``
/// - ``used  = TotalNumberOfBytes - TotalNumberOfFreeBytes`` (psutil's
///   definition)
/// - ``free  = FreeBytesAvailableToCaller`` (honours disk quotas, the same
///   role ``f_bavail`` plays on Unix)
///
/// Called every tick: sysinfo only refreshes its own stored usage when the
/// disk list is re-scanned, which the sampler does once a minute.
#[cfg(windows)]
fn fs_usage(path: &Path) -> Option<Usage> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut avail_to_caller = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    // SAFETY: ``wide`` is NUL-terminated and outlives the call; the three
    // out-pointers are valid u64 slots.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut avail_to_caller,
            &mut total,
            &mut total_free,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(Usage::new(
        total,
        total.saturating_sub(total_free),
        avail_to_caller,
    ))
}

#[cfg(not(any(unix, windows)))]
fn fs_usage(_path: &Path) -> Option<Usage> {
    None
}

fn usage_for(d: &sysinfo::Disk) -> Usage {
    // Platform call failed (e.g. a drive was ejected between the list
    // refresh and now): fall back to sysinfo's last-known view. Less
    // accurate (no reserved-block accounting) but always available.
    fs_usage(d.mount_point()).unwrap_or_else(|| {
        let total = d.total_space();
        let free = d.available_space();
        Usage::new(total, total.saturating_sub(free), free)
    })
}

/// The partition reported as the top-level ``disk``: ``/`` on Unix, the
/// system drive (``%SystemDrive%``, almost always ``C:``) on Windows.
fn is_primary_mount(mount: &Path) -> bool {
    #[cfg(windows)]
    {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let mount = mount.to_string_lossy();
        let mount = mount.trim_end_matches('\\');
        mount.eq_ignore_ascii_case(drive.trim_end_matches('\\'))
    }
    #[cfg(not(windows))]
    {
        mount == Path::new("/")
    }
}

pub fn collect_root(disks: &Disks) -> Disk {
    let target = disks
        .iter()
        .find(|d| is_primary_mount(d.mount_point()))
        .or_else(|| disks.iter().next());

    let Some(d) = target else {
        return Disk::default();
    };
    let u = usage_for(d);
    Disk {
        total: bytes_to_gib(u.total),
        used: bytes_to_gib(u.used),
        free: bytes_to_gib(u.free),
        percent: u.percent,
    }
}

// Only Linux filters entries out; elsewhere every listed disk is kept.
#[cfg_attr(not(target_os = "linux"), allow(clippy::unnecessary_filter_map))]
pub fn collect_all(disks: &Disks) -> Vec<DiskInfo> {
    #[cfg(target_os = "linux")]
    let nodev = nodev_filesystems();

    disks
        .iter()
        .filter_map(|d| {
            let fstype = d.file_system().to_string_lossy().into_owned();
            let mountpoint = d.mount_point().to_string_lossy().into_owned();

            // psutil's ``disk_partitions(all=False)`` filters partitions
            // whose device is empty/synthetic OR whose fstype is marked
            // ``nodev`` in /proc/filesystems (i.e. pseudo filesystems
            // like tmpfs, proc, sysfs, overlay, 9p, fuse.snapfuse, ...).
            // Match that on Linux; on other platforms, sysinfo's list is
            // already physical-only (fixed + removable drives on Windows).
            #[cfg(target_os = "linux")]
            let device = {
                let device = d.name().to_string_lossy().into_owned();
                if !device.starts_with('/') || nodev.contains(&fstype) {
                    return None;
                }
                device
            };

            // sysinfo's Windows ``name()`` is the volume *label* ("Windows",
            // "Data", often empty), not a device path. psutil reports the
            // drive root as the device there, so do the same.
            #[cfg(windows)]
            let device = mountpoint.clone();

            #[cfg(not(any(target_os = "linux", windows)))]
            let device = d.name().to_string_lossy().into_owned();

            let u = usage_for(d);
            Some(DiskInfo {
                device,
                mountpoint,
                fstype,
                total: bytes_to_gib(u.total),
                used: bytes_to_gib(u.used),
                free: bytes_to_gib(u.free),
                percent: u.percent,
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

/// Cumulative disk counters since boot, summed over whole disks.
#[derive(Clone, Copy, Debug, Default)]
struct IoCounters {
    read_bytes: u64,
    write_bytes: u64,
    read_ops: u64,
    write_ops: u64,
}

#[derive(Clone, Debug)]
pub struct DiskIoPrev {
    at: Instant,
    counters: IoCounters,
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

    // Skip failed entries, e.g. permission-denied vfs.
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("loop") || name.starts_with("ram") {
            continue;
        }
        set.insert(name);
    }
    set
}

#[cfg(target_os = "linux")]
fn read_io_counters() -> Option<IoCounters> {
    let raw = std::fs::read_to_string("/proc/diskstats").ok()?;
    let allowed = whole_disks();
    let mut c = IoCounters::default();
    let mut read_sectors = 0u64;
    let mut write_sectors = 0u64;

    for line in raw.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 14 {
            continue;
        }
        if !allowed.contains(f[2]) {
            continue;
        }
        // /proc/diskstats column order (0-indexed, after the leading
        // major/minor/dev fields):
        //   3: reads completed   5: sectors read
        //   7: writes completed  9: sectors written
        let num = |i: usize| f[i].parse::<u64>().unwrap_or(0);
        c.read_ops = c.read_ops.saturating_add(num(3));
        read_sectors = read_sectors.saturating_add(num(5));
        c.write_ops = c.write_ops.saturating_add(num(7));
        write_sectors = write_sectors.saturating_add(num(9));
    }

    // /proc/diskstats reports sectors in 512-byte units regardless of the
    // device's actual sector size — see Documentation/iostats.rst.
    c.read_bytes = read_sectors.saturating_mul(512);
    c.write_bytes = write_sectors.saturating_mul(512);
    Some(c)
}

/// Sum ``IOCTL_DISK_PERFORMANCE`` over every ``\\.\PhysicalDriveN``, the
/// same source psutil's ``disk_io_counters`` uses. Querying physical
/// drives rather than volumes avoids double-counting a disk with several
/// partitions, the Windows analogue of the whole-disk filter on Linux.
/// Opening a drive with zero access rights is enough for this IOCTL, so
/// it works without admin.
#[cfg(windows)]
fn read_io_counters() -> Option<IoCounters> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{DISK_PERFORMANCE, IOCTL_DISK_PERFORMANCE};

    // psutil's upper bound. Drive numbers can have gaps (an unplugged USB
    // disk keeps its slot until reboot), so probe the whole range; a
    // missing drive fails ``CreateFileW`` immediately.
    const MAX_PHYSICAL_DRIVES: u32 = 32;

    let mut c = IoCounters::default();
    let mut any = false;
    for n in 0..MAX_PHYSICAL_DRIVES {
        let path: Vec<u16> = format!(r"\\.\PhysicalDrive{n}")
            .encode_utf16()
            .chain([0])
            .collect();
        // SAFETY: ``path`` is NUL-terminated and outlives the call; the
        // handle is closed below on every path that opened it.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: DISK_PERFORMANCE is plain-old-data; the output buffer
        // and its size describe the same local.
        let mut perf: DISK_PERFORMANCE = unsafe { std::mem::zeroed() };
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_DISK_PERFORMANCE,
                std::ptr::null(),
                0,
                (&mut perf as *mut DISK_PERFORMANCE).cast(),
                std::mem::size_of::<DISK_PERFORMANCE>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: ``handle`` came from a successful CreateFileW above.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            continue;
        }
        any = true;
        c.read_bytes = c.read_bytes.saturating_add(perf.BytesRead.max(0) as u64);
        c.write_bytes = c
            .write_bytes
            .saturating_add(perf.BytesWritten.max(0) as u64);
        c.read_ops = c.read_ops.saturating_add(u64::from(perf.ReadCount));
        c.write_ops = c.write_ops.saturating_add(u64::from(perf.WriteCount));
    }
    any.then_some(c)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn read_io_counters() -> Option<IoCounters> {
    None
}

pub fn collect_io(prev: &mut Option<DiskIoPrev>) -> DiskIo {
    let Some(now_c) = read_io_counters() else {
        return DiskIo::default();
    };
    let now = Instant::now();
    let Some(last) = prev.replace(DiskIoPrev {
        at: now,
        counters: now_c,
    }) else {
        return DiskIo::default();
    };
    let elapsed = now.duration_since(last.at).as_secs_f64();
    if elapsed <= 0.0 {
        return DiskIo::default();
    }
    let rate = |cur: u64, old: u64| (cur.saturating_sub(old) as f64 / elapsed).round() as u64;
    let l = last.counters;
    DiskIo {
        read_bytes_per_sec: rate(now_c.read_bytes, l.read_bytes),
        write_bytes_per_sec: rate(now_c.write_bytes, l.write_bytes),
        read_iops: rate(now_c.read_ops, l.read_ops),
        write_iops: rate(now_c.write_ops, l.write_ops),
    }
}

#[cfg(test)]
mod tests {
    use super::Usage;

    #[test]
    fn percent_uses_df_denominator() {
        // 100 total, 10 reserved: used 45 + free 45 -> 50%, not 45%.
        let u = Usage::new(100, 45, 45);
        assert_eq!(u.percent, 50.0);
    }

    #[test]
    fn empty_filesystem_is_zero_percent() {
        assert_eq!(Usage::new(0, 0, 0).percent, 0.0);
    }

    #[test]
    fn primary_mount_matches_platform_root() {
        #[cfg(windows)]
        {
            let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            assert!(super::is_primary_mount(std::path::Path::new(&format!(
                "{drive}\\"
            ))));
            assert!(!super::is_primary_mount(std::path::Path::new("Z:\\")));
        }
        #[cfg(not(windows))]
        {
            assert!(super::is_primary_mount(std::path::Path::new("/")));
            assert!(!super::is_primary_mount(std::path::Path::new("/home")));
        }
    }
}
