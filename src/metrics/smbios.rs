//! What kind of memory is fitted and who made it, from the machine's
//! firmware: the SMBIOS table has a "Memory Device" record for every
//! slot.
//!
//! - Windows hands any program the table (``GetSystemFirmwareTable``).
//! - Linux keeps it in ``/sys/firmware/dmi/tables/DMI``, which only root
//!   may read, so there it is known when Vigil runs as root and blank
//!   otherwise.
//! - macOS has no such table to read.
//!
//! It is read once, at startup: modules aren't swapped while a machine
//! runs. The parsing is a pure function over the table's bytes, so it is
//! unit-tested on every platform.

/// What the fitted modules have in common. Either is empty where the
/// firmware doesn't say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Modules {
    /// "DDR4". Machines mixing kinds (there are hardly any) list each.
    pub kind: String,
    /// "Corsair", or each maker in turn where the modules differ.
    pub manufacturer: String,
}

/// The fitted modules, or blanks where the firmware can't be read.
pub fn modules() -> Modules {
    table().map(|table| parse(&table)).unwrap_or_default()
}

#[cfg(windows)]
fn table() -> Option<Vec<u8>> {
    use windows_sys::Win32::System::SystemInformation::GetSystemFirmwareTable;

    /// "RSMB": the raw SMBIOS table.
    const RAW_SMBIOS: u32 = u32::from_be_bytes(*b"RSMB");
    /// What Windows puts before the table itself: the version it follows
    /// and its length.
    const HEADER: usize = 8;

    // SAFETY: asked with no buffer it only says the size; the second call
    // is given a buffer of that size.
    unsafe {
        let size = GetSystemFirmwareTable(RAW_SMBIOS, 0, std::ptr::null_mut(), 0);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let got = GetSystemFirmwareTable(RAW_SMBIOS, 0, buf.as_mut_ptr(), size);
        if got == 0 || got > size {
            return None;
        }
        buf.truncate(got as usize);
        (buf.len() > HEADER).then(|| buf.split_off(HEADER))
    }
}

#[cfg(target_os = "linux")]
fn table() -> Option<Vec<u8>> {
    std::fs::read("/sys/firmware/dmi/tables/DMI").ok()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn table() -> Option<Vec<u8>> {
    None
}

/// The record type of a memory module's slot, and of the table's end.
const MEMORY_DEVICE: u8 = 17;
const END_OF_TABLE: u8 = 127;

/// Offsets into a Memory Device record.
const SIZE: usize = 0x0c;
const MEMORY_TYPE: usize = 0x12;
const MANUFACTURER: usize = 0x17;

/// Walks the table's records: each is a header (its type, then its
/// length), its fixed part, then its strings, each ending in a NUL, with
/// one more NUL after the last.
fn parse(table: &[u8]) -> Modules {
    let mut kinds: Vec<&'static str> = Vec::new();
    let mut makers: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(&[kind, len]) = table.get(at..at + 2) {
        let len = usize::from(len);
        let Some(fixed) = table.get(at..at + len).filter(|_| len >= 4) else {
            break;
        };
        let strings_from = at + len;
        let Some(strings_len) = table[strings_from..]
            .windows(2)
            .position(|pair| pair == [0, 0])
        else {
            break;
        };
        let strings = &table[strings_from..strings_from + strings_len];
        at = strings_from + strings_len + 2;

        if kind == END_OF_TABLE {
            break;
        }
        if kind != MEMORY_DEVICE {
            continue;
        }
        // An empty slot has no size, and nothing else worth reading.
        let size = fixed
            .get(SIZE..SIZE + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]));
        if matches!(size, None | Some(0)) {
            continue;
        }
        if let Some(kind) = fixed.get(MEMORY_TYPE).copied().and_then(kind_of)
            && !kinds.contains(&kind)
        {
            kinds.push(kind);
        }
        let maker = fixed
            .get(MANUFACTURER)
            .and_then(|index| string(strings, *index))
            .and_then(maker_of);
        if let Some(maker) = maker
            && !makers.contains(&maker)
        {
            makers.push(maker);
        }
    }
    Modules {
        kind: kinds.join(" / "),
        manufacturer: makers.join(", "),
    }
}

/// A record's string by its number, which counts from 1; 0 is "none".
fn string(strings: &[u8], index: u8) -> Option<String> {
    let index = usize::from(index).checked_sub(1)?;
    let bytes = strings.split(|b| *b == 0).nth(index)?;
    Some(String::from_utf8_lossy(bytes).trim().to_string())
}

/// SMBIOS's "Memory Device: Type", for the kinds a machine that can run
/// Vigil might have.
fn kind_of(code: u8) -> Option<&'static str> {
    Some(match code {
        0x12 => "DDR",
        0x13 | 0x14 => "DDR2",
        0x18 => "DDR3",
        0x1a => "DDR4",
        0x1b => "LPDDR",
        0x1c => "LPDDR2",
        0x1d => "LPDDR3",
        0x1e => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => return None,
    })
}

/// A maker's name as the firmware has it, unless it is one of the things
/// firmware says when it doesn't know. Some modules give their maker as
/// its JEDEC number, and the usual ones are turned back into names.
fn maker_of(text: String) -> Option<String> {
    const BLANKS: [&str; 7] = [
        "unknown",
        "undefined",
        "not specified",
        "not available",
        "none",
        "no dimm",
        "to be filled by o.e.m.",
    ];
    let lower = text.to_lowercase();
    let known = !text.is_empty()
        && !BLANKS.contains(&lower.as_str())
        && !lower.starts_with("manufacturer")
        && text.bytes().any(|b| b != b'0');
    if !known {
        return None;
    }
    let digits = lower.strip_prefix("0x").unwrap_or(&lower);
    let is_number = digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_hexdigit());
    let named = is_number
        .then(|| match &digits[..4] {
            "80ce" | "ce00" => Some("Samsung"),
            "80ad" | "ad00" => Some("SK Hynix"),
            "802c" | "2c00" => Some("Micron"),
            "0198" => Some("Kingston"),
            "029e" => Some("Corsair"),
            "04cd" => Some("G.Skill"),
            "04cb" => Some("ADATA"),
            "04ef" => Some("Team Group"),
            "059b" | "859b" => Some("Crucial"),
            _ => None,
        })
        .flatten();
    Some(named.map_or(text, str::to_string))
}

#[cfg(test)]
mod tests {
    use super::{Modules, maker_of, parse};

    /// A record of ``kind``: its fixed part (after the four-byte header,
    /// which this writes) and its strings.
    fn record(kind: u8, fixed: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut bytes = vec![kind, (fixed.len() + 4) as u8, 0, 0];
        bytes.extend_from_slice(fixed);
        for text in strings {
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(0);
        }
        if strings.is_empty() {
            bytes.push(0);
        }
        bytes.push(0);
        bytes
    }

    /// A memory slot holding ``size_mb``, of SMBIOS type ``kind``, whose
    /// maker is its second string.
    fn slot(size_mb: u16, kind: u8, maker: &str) -> Vec<u8> {
        // The fixed part runs from offset 4 to past the manufacturer.
        let mut fixed = vec![0u8; 0x18 - 4];
        fixed[0x0c - 4..0x0e - 4].copy_from_slice(&size_mb.to_le_bytes());
        fixed[0x12 - 4] = kind;
        fixed[0x17 - 4] = 2;
        record(17, &fixed, &["DIMM_A1", maker, "CMK32GX4M2E3200C16"])
    }

    fn table(records: &[Vec<u8>]) -> Vec<u8> {
        let mut table: Vec<u8> = records.concat();
        table.extend(record(127, &[], &[]));
        table
    }

    #[test]
    fn says_what_the_modules_are_and_who_made_them() {
        let table = table(&[
            // Something else first: the BIOS's own record.
            record(
                0,
                &[1, 2, 0, 0xe0, 3],
                &["American Megatrends", "1.2", "01/01/2024"],
            ),
            slot(16384, 0x1a, "Corsair"),
            // An empty slot says nothing, whatever its strings claim.
            slot(0, 0x02, "Unknown"),
            slot(16384, 0x1a, "Corsair"),
        ]);
        assert_eq!(
            parse(&table),
            Modules {
                kind: "DDR4".into(),
                manufacturer: "Corsair".into(),
            }
        );
    }

    #[test]
    fn mixed_modules_are_each_named_once() {
        let table = table(&[
            slot(8192, 0x22, "Kingston"),
            slot(8192, 0x22, "80AD000080AD"),
            slot(8192, 0x22, "Kingston"),
        ]);
        let modules = parse(&table);
        assert_eq!(modules.kind, "DDR5");
        assert_eq!(modules.manufacturer, "Kingston, SK Hynix");
    }

    #[test]
    fn firmware_that_does_not_say_leaves_blanks() {
        let table = table(&[slot(4096, 0x02, "To Be Filled By O.E.M.")]);
        assert_eq!(parse(&table), Modules::default());
        assert_eq!(parse(&[]), Modules::default());
    }

    #[test]
    fn a_cut_short_or_garbled_table_is_not_trusted_past_the_damage() {
        let whole = table(&[slot(16384, 0x1a, "Corsair"), slot(16384, 0x1a, "G.Skill")]);
        // Every prefix parses without a panic, and never invents a maker.
        for end in 0..whole.len() {
            let modules = parse(&whole[..end]);
            assert!(["", "Corsair", "Corsair, G.Skill"].contains(&modules.manufacturer.as_str()));
        }
        // A record that claims to be shorter than its own header.
        assert_eq!(parse(&[17, 2, 0, 0, 0, 0]), Modules::default());
        // An old, short record with no room for a maker still has a kind.
        let mut short = vec![0u8; 0x13 - 4];
        short[0x0c - 4] = 1;
        short[0x12 - 4] = 0x18;
        assert_eq!(parse(&table(&[record(17, &short, &[])])).kind, "DDR3");
    }

    #[test]
    fn makers_given_as_numbers_are_named() {
        assert_eq!(maker_of("04CD".into()).as_deref(), Some("G.Skill"));
        assert_eq!(maker_of("0x029E".into()).as_deref(), Some("Corsair"));
        assert_eq!(
            maker_of("CE00000000000000".into()).as_deref(),
            Some("Samsung")
        );
        // A number nobody here knows is shown as it came.
        assert_eq!(maker_of("7F7F".into()).as_deref(), Some("7F7F"));
        assert_eq!(
            maker_of("Micron Technology".into()).as_deref(),
            Some("Micron Technology")
        );
        assert_eq!(maker_of("0000".into()), None);
        assert_eq!(maker_of("Undefined".into()), None);
        assert_eq!(maker_of("Manufacturer00".into()), None);
        assert_eq!(maker_of(String::new()), None);
    }
}
