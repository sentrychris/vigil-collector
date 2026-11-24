//! Windows' performance counters, read through PDH: one counter over
//! every instance of whatever it counts.

use windows_sys::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};

/// A PDH query over every instance of one counter.
pub struct Pdh {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl Pdh {
    /// ``path`` names the counter in English, with ``(*)`` for its
    /// instances. ``None`` when this PC has no such counter.
    pub fn open(path: &str) -> Option<Self> {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut query: PDH_HQUERY = std::ptr::null_mut();
        let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
        // SAFETY: out-pointers are valid locals; the query is closed on
        // failure here or in Drop.
        unsafe {
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                return None;
            }
            if PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &mut counter) != 0 {
                PdhCloseQuery(query);
                return None;
            }
        }
        Some(Self { query, counter })
    }

    /// One reading per instance, by the instance's name. ``None`` when
    /// there is nothing to read yet: a counter of a rate has no value
    /// until it has been sampled twice.
    pub fn sample(&mut self) -> Option<Vec<(String, f64)>> {
        // SAFETY: the handles are live for ``self``'s lifetime. The item
        // buffer is sized from PDH's own answer and aligned for the items;
        // the instance names it points into live inside that buffer.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let mut bytes = 0u32;
            let mut count = 0u32;
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                std::ptr::null_mut(),
            );
            if status != PDH_MORE_DATA || bytes == 0 {
                return None;
            }
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                Vec::with_capacity((bytes as usize).div_ceil(item));
            if PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                buf.as_mut_ptr(),
            ) != 0
            {
                return None;
            }
            let items = std::slice::from_raw_parts(buf.as_ptr(), count as usize);
            Some(
                items
                    .iter()
                    .filter(|i| {
                        matches!(
                            i.FmtValue.CStatus,
                            PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
                        )
                    })
                    .map(|i| (wide_to_string(i.szName), i.FmtValue.Anonymous.doubleValue))
                    .collect(),
            )
        }
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        // SAFETY: the query handle came from PdhOpenQueryW and is closed
        // exactly once; this also frees its counter.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// # Safety
/// ``p`` is null or points to a NUL-terminated UTF-16 string.
unsafe fn wide_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: per the contract, every unit up to the NUL is readable.
    unsafe {
        let len = (0..).take_while(|&i| *p.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}
