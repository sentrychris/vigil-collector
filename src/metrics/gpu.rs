//! The graphics card: how busy it is, its memory and its temperature.
//!
//! - Windows reads the counters behind Task Manager's GPU figures, which
//!   cover every make of card (``gpu_windows.rs``).
//! - Linux reads what the kernel's own graphics drivers publish under
//!   ``/sys/class/drm`` (``gpu_linux.rs``), which today means AMD cards.
//! - Anywhere else, and on a machine with no card these can read, there
//!   is no reading and the payload's ``gpu`` is ``null``.
//!
//! Nothing here needs admin rights, a vendor's library or a driver of
//! our own.

#[cfg(windows)]
use std::collections::HashMap;

use crate::snapshot::Gpu;
#[cfg(any(windows, target_os = "linux", test))]
use crate::snapshot::GpuMemory;

pub struct Reader {
    #[cfg(windows)]
    windows: super::gpu_windows::Reader,
    #[cfg(target_os = "linux")]
    linux: super::gpu_linux::Reader,
}

impl Reader {
    pub fn start() -> Self {
        Self {
            #[cfg(windows)]
            windows: super::gpu_windows::Reader::start(),
            #[cfg(target_os = "linux")]
            linux: super::gpu_linux::Reader::new(),
        }
    }

    /// The newest reading, or ``None`` where there is no card to read.
    pub fn read(&mut self) -> Option<Gpu> {
        #[cfg(windows)]
        return self.windows.read();
        #[cfg(target_os = "linux")]
        return self.linux.read();
        #[cfg(not(any(windows, target_os = "linux")))]
        None
    }

    /// How busy each process keeps the graphics cards, in percent, by
    /// pid. ``None`` where that can't be told apart by process.
    #[cfg(windows)]
    pub fn by_process(&self) -> Option<HashMap<u32, f64>> {
        self.windows.by_process()
    }
}

/// Video memory, from bytes, in the units the machine's own memory is
/// reported in.
#[cfg(any(windows, target_os = "linux", test))]
pub(super) fn memory(total: u64, used: u64) -> GpuMemory {
    let used = if total > 0 { used.min(total) } else { used };
    GpuMemory {
        total: super::bytes_to_gib(total),
        used: super::bytes_to_gib(used),
        free: super::bytes_to_gib(total.saturating_sub(used)),
        percent: if total > 0 {
            super::round1(used as f64 / total as f64 * 100.0)
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::memory;

    #[test]
    fn memory_is_in_gib_and_adds_up() {
        let gib = 1024 * 1024 * 1024;
        let mem = memory(8 * gib, 2 * gib);
        assert_eq!(
            (mem.total, mem.used, mem.free, mem.percent),
            (8.0, 2.0, 6.0, 25.0)
        );
        // A counter a little over the card's size doesn't read as 104%.
        assert_eq!(memory(8 * gib, 9 * gib).percent, 100.0);
        // A card that doesn't say its size has no share in use.
        assert_eq!(memory(0, gib).percent, 0.0);
    }
}
