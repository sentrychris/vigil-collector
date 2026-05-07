use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use sysinfo::{Pid, System, Users};

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

/// PSS sum for a process group in MB, falling back per-pid to the
/// process's RSS (from sysinfo's already-cached value) when smaps_rollup
/// is denied or the process exited mid-tick.
fn group_pss_mb(sys: &System, agg: &Aggregated) -> f64 {
    let mut sum_bytes: u64 = 0;
    for &pid in &agg.pids {
        let bytes = match read_pss_kb(pid) {
            Some(kb) => kb.saturating_mul(1024),
            None => sys
                .process(Pid::from_u32(pid))
                .map(|p| p.memory())
                .unwrap_or(0),
        };
        sum_bytes = sum_bytes.saturating_add(bytes);
    }
    sum_bytes as f64 / (1024.0 * 1024.0)
}

pub fn collect_top(sys: &System, users: &Users) -> Vec<Process> {
    // First pass: aggregate by name using RSS only. PSS would require a
    // /proc/<pid>/smaps_rollup syscall per process — hundreds per slow tick
    // on a busy host. PSS ≤ RSS, which lets the second pass skip most of
    // them via the streaming bound below.
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
        let mem_mb = proc_.memory() as f64 / (1024.0 * 1024.0);

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

    let mut entries: Vec<(String, Aggregated)> = by_name.into_iter().collect();
    entries.sort_by(|a, b| {
        b.1.mem_mb
            .partial_cmp(&a.1.mem_mb)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    const TOP_N: usize = 10;

    let final_entries: Vec<(String, Aggregated)> = if pss_supported(sys) {
        // Streaming top-N by PSS. Walk RSS-descending, keeping the top-N
        // by PSS seen so far (sorted desc). Stop once the next candidate's
        // RSS no longer exceeds the smallest PSS in the running top-N —
        // RSS is monotone descending and PSS ≤ RSS, so no later entry can
        // break in either. Exact result; typically ~10-20 PSS reads on a
        // normal system, degrading to reading all entries only under
        // pathological shared-page compression.
        let mut top: Vec<(String, Aggregated)> = Vec::with_capacity(TOP_N + 1);
        for (name, mut agg) in entries.into_iter() {
            if top.len() == TOP_N && agg.mem_mb <= top[TOP_N - 1].1.mem_mb {
                break;
            }
            agg.mem_mb = group_pss_mb(sys, &agg);
            top.push((name, agg));
            top.sort_by(|a, b| {
                b.1.mem_mb
                    .partial_cmp(&a.1.mem_mb)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if top.len() > TOP_N {
                top.pop();
            }
        }
        top
    } else {
        entries.truncate(TOP_N);
        entries
    };

    final_entries
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
        .collect()
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
