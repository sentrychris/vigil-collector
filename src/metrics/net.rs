use std::collections::BTreeMap;
use std::time::Instant;

use sysinfo::Networks;

use crate::snapshot::{NetIfaceStats, Throughput};

#[derive(Clone, Debug)]
pub struct NetIoPrev {
    pub at: Instant,
    pub recv: u64,
    pub sent: u64,
}

/// Aggregate rx/tx bytes/sec, derived from the delta of total bytes
/// received/sent across all interfaces (loopback excluded). The first
/// call (no previous reading) returns zeros.
pub fn collect_throughput(networks: &Networks, prev: &mut Option<NetIoPrev>) -> Throughput {
    let mut recv = 0u64;
    let mut sent = 0u64;
    for (name, data) in networks.iter() {
        if name == "lo" {
            continue;
        }
        recv = recv.saturating_add(data.total_received());
        sent = sent.saturating_add(data.total_transmitted());
    }
    let now = Instant::now();
    let last = prev.replace(NetIoPrev { at: now, recv, sent });
    let Some(last) = last else {
        return Throughput::default();
    };
    let elapsed = now.duration_since(last.at).as_secs_f64();
    if elapsed <= 0.0 {
        return Throughput::default();
    }
    Throughput {
        rx_bytes_per_sec: ((recv.saturating_sub(last.recv)) as f64 / elapsed).round() as u64,
        tx_bytes_per_sec: ((sent.saturating_sub(last.sent)) as f64 / elapsed).round() as u64,
    }
}

pub fn interface_names(networks: &Networks) -> Vec<String> {
    networks.iter().map(|(name, _)| name.clone()).collect()
}

pub fn statistics(networks: &Networks) -> BTreeMap<String, NetIfaceStats> {
    let mut out = BTreeMap::new();
    for (name, d) in networks.iter() {
        let stats = NetIfaceStats {
            mb_sent: d.total_transmitted() as f64 / (1024.0 * 1024.0),
            mb_received: d.total_received() as f64 / (1024.0 * 1024.0),
            pk_sent: d.total_packets_transmitted(),
            pk_received: d.total_packets_received(),
            error_in: d.total_errors_on_received(),
            error_out: d.total_errors_on_transmitted(),
            // sysinfo doesn't expose dropped-packet counters; psutil reads
            // them from /proc/net/dev. Backfill from there on Linux,
            // otherwise report 0.
            dropin: 0,
            dropout: 0,
        };
        out.insert(name.clone(), stats);
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(drops) = read_proc_net_drops() {
            for (name, (din, dout)) in drops {
                if let Some(s) = out.get_mut(&name) {
                    s.dropin = din;
                    s.dropout = dout;
                }
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn read_proc_net_drops() -> Option<BTreeMap<String, (u64, u64)>> {
    // /proc/net/dev format:
    //   Inter-|   Receive ...                            |  Transmit ...
    //    face | bytes packets errs drop fifo frame compressed multicast | bytes ...
    let raw = std::fs::read_to_string("/proc/net/dev").ok()?;
    let mut out = BTreeMap::new();
    for line in raw.lines().skip(2) {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface.trim().to_string();
        let cols: Vec<&str> = rest.split_whitespace().collect();
        // Receive columns 0..7, Transmit columns 8..15. We want
        // receive[3] (rx drop) and transmit[3] (tx drop, i.e. cols[11]).
        if cols.len() >= 12 {
            let din: u64 = cols[3].parse().unwrap_or(0);
            let dout: u64 = cols[11].parse().unwrap_or(0);
            out.insert(iface, (din, dout));
        }
    }
    Some(out)
}
