use std::sync::OnceLock;

use sysinfo::System;

use crate::snapshot::PlatformInfo;

static USER: OnceLock<String> = OnceLock::new();
static DISTRO: OnceLock<String> = OnceLock::new();
static KERNEL: OnceLock<String> = OnceLock::new();

pub fn user() -> String {
    USER.get_or_init(|| whoami::username().unwrap_or_default())
        .clone()
}

pub fn distro() -> String {
    DISTRO
        .get_or_init(|| {
            // Linux: /etc/os-release PRETTY_NAME -> NAME (e.g. "Ubuntu
            // 24.04.1 LTS").
            // macOS / Windows: sysinfo's ``long_os_version()`` gives the
            // product name with its version ("Windows 11 Pro", "macOS
            // 15.1 Sequoia"); plain ``name()`` is just "Windows".
            #[cfg(target_os = "linux")]
            if let Ok(raw) = std::fs::read_to_string("/etc/os-release") {
                let mut pretty = None;
                let mut name = None;
                for line in raw.lines() {
                    if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                        pretty = Some(v.trim_matches('"').to_string());
                    } else if let Some(v) = line.strip_prefix("NAME=") {
                        name = Some(v.trim_matches('"').to_string());
                    }
                }
                if let Some(s) = pretty.or(name) {
                    return s;
                }
            }
            System::long_os_version()
                .or_else(|_| System::name())
                .unwrap_or_else(|_| "unknown".to_string())
        })
        .clone()
}

pub fn kernel() -> String {
    // Fixed for the life of the process; cached so the per-tick sampler
    // doesn't re-read /proc or the Windows registry every second. On
    // Windows the "kernel version" is the OS build number (e.g. "26100").
    KERNEL
        .get_or_init(|| System::kernel_version().unwrap_or_default())
        .clone()
}

pub fn uptime_string() -> String {
    format_uptime(System::uptime().unwrap_or(0))
}

fn format_uptime(total: u64) -> String {
    if total == 0 {
        return String::new();
    }
    let days = total / 86_400;
    let mut rem = total % 86_400;
    let hours = rem / 3_600;
    rem %= 3_600;
    let minutes = rem / 60;
    let seconds = rem % 60;

    let mut parts: Vec<String> = Vec::with_capacity(4);
    if days > 0 {
        parts.push(format!("{} day{}", days, if days != 1 { "s" } else { "" }));
    }
    if hours > 0 {
        parts.push(format!("{} hr{}", hours, if hours != 1 { "s" } else { "" }));
    }
    if minutes > 0 {
        parts.push(format!(
            "{} min{}",
            minutes,
            if minutes != 1 { "s" } else { "" }
        ));
    }
    if seconds > 0 {
        parts.push(format!(
            "{} sec{}",
            seconds,
            if seconds != 1 { "s" } else { "" }
        ));
    }
    parts.join(", ")
}

pub fn collect() -> PlatformInfo {
    let uptime = uptime_string();
    PlatformInfo {
        distro: distro(),
        kernel: kernel(),
        uptime,
    }
}

#[cfg(test)]
mod tests {
    use super::format_uptime;

    #[test]
    fn uptime_skips_zero_parts_and_pluralises() {
        assert_eq!(format_uptime(0), "");
        assert_eq!(format_uptime(1), "1 sec");
        assert_eq!(format_uptime(61), "1 min, 1 sec");
        assert_eq!(format_uptime(3_600), "1 hr");
        assert_eq!(
            format_uptime(2 * 86_400 + 2 * 3_600 + 120),
            "2 days, 2 hrs, 2 mins"
        );
    }
}
