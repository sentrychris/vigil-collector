use sysinfo::System;

use crate::snapshot::Mem;

use super::{bytes_to_gib, round1};

pub fn collect(sys: &System) -> Mem {
    // sysinfo's `available_memory` is the analogue of psutil's `available`:
    // memory that can be handed to a process without swapping. Using it as
    // the basis for `free` and `used` keeps `used + free == total` — the
    // raw `used`/`free` fields exclude reclaimable cache and would break
    // that identity.
    let total = sys.total_memory();
    let available = sys.available_memory();
    let used = total.saturating_sub(available);
    let percent = if total > 0 {
        round1(used as f64 / total as f64 * 100.0)
    } else {
        0.0
    };

    Mem {
        total: bytes_to_gib(total),
        used: bytes_to_gib(used),
        free: bytes_to_gib(available),
        percent,
    }
}
