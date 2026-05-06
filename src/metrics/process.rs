use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use sysinfo::{System, Users};

use crate::snapshot::Process;

use super::round2;

#[derive(Default)]
struct Aggregated {
    mem_mb: f64,
    pids: Vec<u32>,
    usernames: BTreeSet<String>,
}

#[cfg(target_os = "linux")]
fn read_pss_kb(pid: u32) -> Option<u64> {
    // /proc/<pid>/smaps_rollup is the cheap PSS source — the kernel
    // pre-aggregates the per-mapping smaps into one rollup entry. Reading
    // requires root or same-uid; AccessDenied surfaces here as ``None`` so
    // the caller falls back to RSS for that process individually.
    let raw = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("Pss:") {
            let val = rest.trim().split_whitespace().next()?;
            return val.parse::<u64>().ok();
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn read_pss_kb(_pid: u32) -> Option<u64> {
    None
}

static PSS_SUPPORTED: OnceLock<bool> = OnceLock::new();

#[cfg(target_os = "linux")]
fn detect_pss(sys: &System) -> bool {
    if cfg!(not(target_os = "linux")) {
        return false;
    }
    let own_pid = std::process::id();
    // Sort foreign PIDs ascending so the probe sees the same first
    // candidate the Python collector does (psutil iterates /proc in
    // listdir order, which on Linux is pid-ascending). Without sorting,
    // sysinfo's HashMap order is hash-based and might land on a same-uid
    // process first, making us report "pss" where Python reports "rss".
    let mut pids: Vec<u32> = sys
        .processes()
        .keys()
        .map(|p| p.as_u32())
        .filter(|p| *p != own_pid)
        .collect();
    pids.sort_unstable();

    for p in pids {
        // First foreign process whose smaps_rollup is readable proves PSS
        // is available; first one whose isn't proves the opposite.
        // ``None`` here covers both EACCES (which is the load-bearing
        // signal) and ENOENT (process exited mid-probe — try the next).
        match read_pss_kb(p) {
            Some(_) => return true,
            None => {
                // Distinguish "doesn't exist" from "denied" via a fresh
                // existence check; if the directory is still there but
                // we couldn't read smaps_rollup, that's denied.
                if std::path::Path::new(&format!("/proc/{p}")).exists() {
                    return false;
                }
                // Process raced away — keep looking.
                continue;
            }
        }
    }
    false
}

#[cfg(not(target_os = "linux"))]
fn detect_pss(_sys: &System) -> bool {
    false
}

pub fn pss_supported(sys: &System) -> bool {
    *PSS_SUPPORTED.get_or_init(|| detect_pss(sys))
}

pub fn process_metric_label(sys: &System) -> &'static str {
    if pss_supported(sys) {
        "pss"
    } else {
        "rss"
    }
}

pub fn collect_top(sys: &System, users: &Users) -> Vec<Process> {
    let use_pss = pss_supported(sys);
    let mut by_name: HashMap<String, Aggregated> = HashMap::new();

    for (pid, proc_) in sys.processes() {
        // sysinfo lists every kernel task by default — including threads.
        // psutil counts only main processes, so threads here would (a)
        // double-count RSS for any multi-threaded program and (b) pollute
        // the top-N with thread names like "tokio-runtime-w" or
        // "DelayedTaskSche". Skip them.
        if proc_.thread_kind().is_some() {
            continue;
        }

        let pid_u: u32 = pid.as_u32();
        let name = display_name(proc_);

        let mem_bytes = if use_pss {
            match read_pss_kb(pid_u) {
                Some(kb) => kb.saturating_mul(1024),
                None => proc_.memory(),
            }
        } else {
            proc_.memory()
        };
        let mem_mb = mem_bytes as f64 / (1024.0 * 1024.0);

        let username = proc_
            .user_id()
            .and_then(|uid| users.get_user_by_id(uid))
            .map(|u| u.name().to_string())
            .unwrap_or_default();

        let entry = by_name.entry(name).or_default();
        entry.mem_mb += mem_mb;
        entry.pids.push(pid_u);
        if !username.is_empty() {
            entry.usernames.insert(username);
        }
    }

    let mut combined: Vec<Process> = by_name
        .into_iter()
        .map(|(name, agg)| Process {
            pid: agg.pids.first().copied().unwrap_or(0),
            name,
            username: agg
                .usernames
                .into_iter()
                .collect::<Vec<_>>()
                .join(", "),
            mem: round2(agg.mem_mb),
        })
        .collect();

    combined.sort_by(|a, b| b.mem.partial_cmp(&a.mem).unwrap_or(std::cmp::Ordering::Equal));
    combined.truncate(10);
    combined
}

/// Resolve a process display name. Matches psutil: returns the kernel
/// ``comm`` field as-is (sysinfo's ``Process::name()``), so multi-threaded
/// runtimes that overwrite it via ``PR_SET_NAME`` keep showing the same
/// label across both implementations. We rely on the thread-filter above
/// to drop the actual thread tasks; what's left is the process leader,
/// whose comm is normally the binary name.
fn display_name(proc_: &sysinfo::Process) -> String {
    let name = proc_.name().to_string_lossy();
    if name.is_empty() {
        "unknown".to_string()
    } else {
        name.into_owned()
    }
}

pub fn refresh_kind() -> sysinfo::ProcessRefreshKind {
    // Only the bits we surface: memory and user. Skipping CPU%, exe,
    // cmdline, etc. saves the per-process overhead sysinfo would
    // otherwise pay.
    sysinfo::ProcessRefreshKind::new()
        .with_memory()
        .with_user(sysinfo::UpdateKind::OnlyIfNotSet)
}
