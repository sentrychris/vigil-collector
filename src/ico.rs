//! Picks one image out of a ``.ico`` file, for the Windows tray icon.
//!
//! An ``.ico`` file is a small directory followed by the images it lists
//! (PNG or BMP data). Windows' ``CreateIconFromResourceEx`` takes a single
//! image, so the tray code needs to find the right one by size. Kept free
//! of Win32 calls so the tests run on every platform.

/// Byte range of the image to use for an icon drawn ``desired`` pixels
/// square: the smallest image at least that big, or the largest one if
/// none is. ``None`` if the data isn't a well-formed icon file.
pub fn pick(ico: &[u8], desired: u32) -> Option<std::ops::Range<usize>> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(ico.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(ico.get(i..i + 4)?.try_into().ok()?));

    // Header: reserved (0), type (1 = icon), image count.
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    let count = usize::from(u16_at(4)?);

    let mut best: Option<(u32, std::ops::Range<usize>)> = None;
    for n in 0..count {
        let entry = 6 + 16 * n;
        // A width byte of 0 means 256.
        let width = match *ico.get(entry)? {
            0 => 256,
            w => u32::from(w),
        };
        let len = u32_at(entry + 8)? as usize;
        let start = u32_at(entry + 12)? as usize;
        let end = start.checked_add(len)?;
        if len == 0 || end > ico.len() {
            return None;
        }
        let better = match &best {
            None => true,
            Some((current, _)) => {
                let fits = width >= desired;
                let current_fits = *current >= desired;
                match (fits, current_fits) {
                    (true, true) => width < *current,
                    (true, false) => true,
                    (false, true) => false,
                    (false, false) => width > *current,
                }
            }
        };
        if better {
            best = Some((width, start..end));
        }
    }
    best.map(|(_, range)| range)
}

#[cfg(test)]
mod tests {
    use super::pick;

    /// Builds an icon file with one fake image per width, each image's
    /// bytes filled with its own width so the tests can tell them apart.
    fn ico(widths: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 1, 0];
        out.extend_from_slice(&(widths.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * widths.len();
        let mut images = Vec::new();
        for &w in widths {
            let len = 4usize;
            out.extend_from_slice(&[w, w, 0, 0, 1, 0, 32, 0]);
            out.extend_from_slice(&(len as u32).to_le_bytes());
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            images.extend(std::iter::repeat_n(w, len));
            offset += len;
        }
        out.extend(images);
        out
    }

    fn picked(data: &[u8], desired: u32) -> u8 {
        data[pick(data, desired).unwrap()][0]
    }

    #[test]
    fn picks_the_smallest_image_that_is_big_enough() {
        let data = ico(&[16, 24, 32, 48, 0]);
        assert_eq!(picked(&data, 16), 16);
        assert_eq!(picked(&data, 20), 24);
        assert_eq!(picked(&data, 32), 32);
        assert_eq!(picked(&data, 40), 48);
        // 0 stands for 256.
        assert_eq!(picked(&data, 100), 0);
    }

    #[test]
    fn falls_back_to_the_largest_image() {
        let data = ico(&[32, 16]);
        assert_eq!(picked(&data, 64), 32);
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(pick(&[], 16).is_none());
        assert!(pick(b"not an icon file", 16).is_none());
        let mut data = ico(&[16]);
        data.truncate(data.len() - 1); // image runs past the end
        assert!(pick(&data, 16).is_none());
    }

    #[test]
    fn reads_the_dashboard_favicon() {
        let data = include_bytes!("../web/public/favicon.ico");
        let range = pick(data, 16).unwrap();
        assert_eq!(&data[range.start..range.start + 4], b"\x89PNG");
    }
}
