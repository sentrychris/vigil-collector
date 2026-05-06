use std::sync::OnceLock;

use sysinfo::System;

use crate::snapshot::PlatformInfo;

static USER: OnceLock<String> = OnceLock::new();
static DISTRO: OnceLock<String> = OnceLock::new();

pub fn user() -> String {
    USER.get_or_init(|| whoami::username()).clone()
}

pub fn distro() -> String {
    DISTRO
        .get_or_init(|| {
            // Linux: /etc/os-release PRETTY_NAME -> NAME -> "Linux".
            // macOS / Windows: sysinfo's `name()` returns a usable label;
            // append the OS version when we have one for parity with the
            // Python output (e.g. "Ubuntu 24.04.1 LTS").
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
            System::name().unwrap_or_else(|| "unknown".to_string())
        })
        .clone()
}

pub fn kernel() -> String {
    System::kernel_version().unwrap_or_default()
}

pub fn uptime_string() -> String {
    let secs = System::uptime();
    format_uptime(secs)
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
