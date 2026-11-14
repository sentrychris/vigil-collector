//! What Settings makes of what is typed into it: whether a form differs
//! from what is saved, what is wrong with it, and the changes to save.
//!
//! The rules and the wording are those of the Settings page of the web
//! dashboard (``web/src/views/SettingsView.vue``), which sends the same
//! changes to the same place (``server::control::Panel``).

use serde_json::{Map, Value, json};

use crate::config::{Probe, Settings};
use crate::supervisor::Status;

/// The names for "this computer only"; the first is the default.
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];
const EVERYWHERE: &str = "0.0.0.0";

/// Who can connect, as the form's three choices number them.
pub const LOCAL: i32 = 0;
pub const NETWORK: i32 = 1;
pub const CUSTOM: i32 = 2;

/// Which of the three choices an address is.
pub fn scope_of(address: &str) -> i32 {
    if LOOPBACK.contains(&address) {
        LOCAL
    } else if address == EVERYWHERE {
        NETWORK
    } else {
        CUSTOM
    }
}

/// The address the form stands for. ``current`` is the one in use:
/// whichever loopback name that is stays as it is.
pub fn address(scope: i32, custom: &str, current: &str) -> String {
    match scope {
        LOCAL if LOOPBACK.contains(&current) => current.into(),
        LOCAL => LOOPBACK[0].into(),
        NETWORK => EVERYWHERE.into(),
        _ => custom.trim().into(),
    }
}

/// A whole number typed into a field, if that is what it is.
fn whole(text: &str) -> Option<u64> {
    let number: f64 = text.trim().parse().ok()?;
    (number >= 0.0 && number.fract() == 0.0 && number <= 1e15).then_some(number as u64)
}

/// A number typed into a field, if that is what it is.
fn number(text: &str) -> Option<f64> {
    text.trim()
        .parse()
        .ok()
        .filter(|number: &f64| number.is_finite())
}

/// A number as a field shows it: ``1`` rather than ``1.0``.
pub fn shown(number: f64) -> String {
    number.to_string()
}

fn port(text: &str) -> Option<u16> {
    whole(text)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port >= 1)
}

/// What the server form comes to.
#[derive(Debug, Default, PartialEq)]
pub struct Server {
    /// The settings to change, by their names in the settings file.
    pub changes: Map<String, Value>,
    /// The form differs from what is in use.
    pub dirty: bool,
    pub port_valid: bool,
    pub savable: bool,
}

pub fn server(scope: i32, custom: &str, port_text: &str, status: &Status) -> Server {
    let address = address(scope, custom, &status.address);
    let port = port(port_text);
    let mut changes = Map::new();
    if address != status.address {
        changes.insert("address".into(), json!(address));
    }
    if let Some(port) = port
        && port != status.port
    {
        changes.insert("port_number".into(), json!(port));
    }
    let dirty = !changes.is_empty() || port.is_none();
    Server {
        changes,
        dirty,
        port_valid: port.is_some(),
        savable: dirty && port.is_some() && !address.is_empty(),
    }
}

/// The flags Vigil was started with that decide the address or the port
/// instead of the settings file, as a sentence names them.
pub fn from_command_line(status: &Status) -> String {
    let mut flags = Vec::new();
    if status.address_from_cli {
        flags.push(format!("--address {}", status.address));
    }
    if status.port_from_cli {
        flags.push(format!("--port {}", status.port));
    }
    flags.join(" and ")
}

/// What one of the other forms comes to.
#[derive(Debug, Default, PartialEq)]
pub struct Form {
    pub changes: Map<String, Value>,
    pub dirty: bool,
    /// What is wrong with it, or nothing.
    pub problem: &'static str,
}

pub fn connections(limit_text: &str, interval_text: &str, saved: &Settings) -> Form {
    let limit = whole(limit_text);
    let interval = number(interval_text);
    let mut changes = Map::new();
    if let Some(limit) = limit
        && limit != saved.max_ws_connections as u64
    {
        changes.insert("max_ws_connections".into(), json!(limit));
    }
    if let Some(interval) = interval
        && interval != saved.ws_push_interval
    {
        changes.insert("ws_push_interval".into(), json!(interval));
    }
    let valid = limit.is_some_and(|limit| limit <= 10_000)
        && interval.is_some_and(|interval| (0.1..=3600.0).contains(&interval));
    Form {
        dirty: !changes.is_empty() || limit.is_none() || interval.is_none(),
        changes,
        problem: if valid {
            ""
        } else {
            "The limit is 0 to 10000 and the interval 0.1 to 3600 seconds."
        },
    }
}

fn web_address(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// ``rows`` are the checks as typed: a name and an address each.
pub fn checks(rows: &[(String, String)], timeout_text: &str, saved: &Settings) -> Form {
    let probes: Vec<Probe> = rows
        .iter()
        .map(|(name, url)| Probe {
            name: name.trim().into(),
            url: url.trim().into(),
        })
        .collect();
    let timeout = number(timeout_text);
    let mut changes = Map::new();
    if probes != saved.probes {
        changes.insert("probes".into(), json!(probes));
    }
    if let Some(timeout) = timeout
        && timeout != saved.probe_timeout
    {
        changes.insert("probe_timeout".into(), json!(timeout));
    }
    let problem = if probes.iter().any(|probe| probe.name.is_empty()) {
        "Every check needs a name."
    } else if probes.iter().any(|probe| !web_address(&probe.url)) {
        "Every address must start with http:// or https://."
    } else if !timeout.is_some_and(|timeout| (0.1..=120.0).contains(&timeout)) {
        "The timeout must be between 0.1 and 120 seconds."
    } else {
        ""
    };
    Form {
        dirty: !changes.is_empty() || timeout.is_none(),
        changes,
        problem,
    }
}

/// The log level as the choice of four shows it: the settings file may
/// say ``WARN`` or use small letters.
pub fn level_shown(level: &str) -> String {
    match level.to_uppercase().as_str() {
        "WARN" => "WARNING".into(),
        other => other.into(),
    }
}

/// How long Vigil has been running, to the minute.
pub fn uptime(seconds: u64) -> String {
    let (days, hours, minutes) = (
        seconds / 86_400,
        seconds % 86_400 / 3600,
        seconds % 3600 / 60,
    );
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "Less than a minute".into()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CUSTOM, LOCAL, NETWORK, address, checks, connections, from_command_line, level_shown,
        scope_of, server, shown, uptime,
    };
    use crate::config::Settings;
    use crate::supervisor::Status;

    fn status(address: &str, port: u16) -> Status {
        Status {
            serving: true,
            listen: Some(format!("{address}:{port}")),
            url: Some(format!("http://{address}:{port}")),
            address: address.into(),
            port,
            address_from_cli: false,
            port_from_cli: false,
            error: None,
        }
    }

    #[test]
    fn an_address_is_one_of_three_choices() {
        assert_eq!(scope_of("localhost"), LOCAL);
        assert_eq!(scope_of("127.0.0.1"), LOCAL);
        assert_eq!(scope_of("::1"), LOCAL);
        assert_eq!(scope_of("0.0.0.0"), NETWORK);
        assert_eq!(scope_of("192.168.1.20"), CUSTOM);
    }

    #[test]
    fn this_computer_only_keeps_the_loopback_name_in_use() {
        assert_eq!(address(LOCAL, "", "127.0.0.1"), "127.0.0.1");
        assert_eq!(address(LOCAL, "", "0.0.0.0"), "localhost");
        assert_eq!(address(NETWORK, "", "localhost"), "0.0.0.0");
        assert_eq!(address(CUSTOM, " 10.0.0.4 ", "localhost"), "10.0.0.4");
    }

    #[test]
    fn an_untouched_server_form_has_nothing_to_save() {
        let form = server(LOCAL, "", "4500", &status("localhost", 4500));
        assert!(!form.dirty && !form.savable && form.port_valid);
        assert!(form.changes.is_empty());
    }

    #[test]
    fn only_what_differs_is_sent() {
        let now = status("localhost", 4500);
        let form = server(NETWORK, "", "4500", &now);
        assert_eq!(
            serde_json::Value::Object(form.changes),
            json!({"address": "0.0.0.0"})
        );
        let form = server(LOCAL, "", " 8080 ", &now);
        assert!(form.savable);
        assert_eq!(
            serde_json::Value::Object(form.changes),
            json!({"port_number": 8080})
        );
    }

    #[test]
    fn a_bad_port_or_no_address_can_not_be_saved() {
        let now = status("localhost", 4500);
        for port in ["", "0", "65536", "45.5", "port", "-1"] {
            let form = server(LOCAL, "", port, &now);
            assert!(form.dirty && !form.port_valid && !form.savable, "{port:?}");
        }
        assert!(server(LOCAL, "", "65535", &now).port_valid);
        let form = server(CUSTOM, "  ", "4500", &now);
        assert!(form.dirty && form.port_valid && !form.savable);
    }

    #[test]
    fn start_up_flags_are_named() {
        let mut now = status("0.0.0.0", 9000);
        assert_eq!(from_command_line(&now), "");
        now.port_from_cli = true;
        assert_eq!(from_command_line(&now), "--port 9000");
        now.address_from_cli = true;
        assert_eq!(from_command_line(&now), "--address 0.0.0.0 and --port 9000");
    }

    #[test]
    fn the_connections_form_is_checked_as_the_page_checks_it() {
        let saved = Settings::default();
        let untouched = connections("20", "1", &saved);
        assert!(!untouched.dirty && untouched.problem.is_empty());

        let changed = connections("50", "0.5", &saved);
        assert!(changed.dirty && changed.problem.is_empty());
        assert_eq!(
            serde_json::Value::Object(changed.changes),
            json!({"max_ws_connections": 50, "ws_push_interval": 0.5})
        );

        for (limit, interval) in [("10001", "1"), ("20", "0.05"), ("20", "3601"), ("", "1")] {
            let form = connections(limit, interval, &saved);
            assert!(form.dirty, "{limit:?} {interval:?}");
            assert_eq!(
                form.problem,
                "The limit is 0 to 10000 and the interval 0.1 to 3600 seconds."
            );
        }
        assert!(connections("0", "3600", &saved).problem.is_empty());
    }

    fn rows(saved: &Settings) -> Vec<(String, String)> {
        saved
            .probes
            .iter()
            .map(|probe| (probe.name.clone(), probe.url.clone()))
            .collect()
    }

    #[test]
    fn the_checks_are_trimmed_and_compared_with_what_is_saved() {
        let saved = Settings::default();
        let mut typed = rows(&saved);
        assert!(!checks(&typed, "5", &saved).dirty);
        // Spaces around a name are not a change.
        typed[0].0 = format!("  {} ", typed[0].0);
        assert!(!checks(&typed, "5", &saved).dirty);

        typed.push((" Router ".into(), " http://192.168.1.1/ ".into()));
        let form = checks(&typed, "2.5", &saved);
        assert!(form.dirty && form.problem.is_empty());
        let probes = form.changes["probes"].as_array().expect("a list");
        assert_eq!(
            probes.last(),
            Some(&json!({"name": "Router", "url": "http://192.168.1.1/"}))
        );
        assert_eq!(form.changes["probe_timeout"], json!(2.5));
    }

    #[test]
    fn what_is_wrong_with_the_checks_is_said_in_the_pages_words() {
        let saved = Settings::default();
        let mut typed = rows(&saved);
        typed.push((String::new(), "https://".into()));
        assert_eq!(
            checks(&typed, "5", &saved).problem,
            "Every check needs a name."
        );
        typed.last_mut().expect("a row").0 = "Router".into();
        typed.last_mut().expect("a row").1 = "192.168.1.1".into();
        assert_eq!(
            checks(&typed, "5", &saved).problem,
            "Every address must start with http:// or https://."
        );
        typed.last_mut().expect("a row").1 = "HTTP://192.168.1.1".into();
        assert_eq!(checks(&typed, "5", &saved).problem, "");
        for timeout in ["0", "121", "", "soon"] {
            assert_eq!(
                checks(&typed, timeout, &saved).problem,
                "The timeout must be between 0.1 and 120 seconds.",
                "{timeout:?}"
            );
        }
    }

    #[test]
    fn numbers_and_levels_are_shown_plainly() {
        assert_eq!(shown(1.0), "1");
        assert_eq!(shown(0.5), "0.5");
        assert_eq!(shown(3600.0), "3600");
        assert_eq!(level_shown("warn"), "WARNING");
        assert_eq!(level_shown("info"), "INFO");
    }

    #[test]
    fn uptime_is_said_to_the_minute() {
        assert_eq!(uptime(12), "Less than a minute");
        assert_eq!(uptime(60 * 7 + 3), "7m");
        assert_eq!(uptime(3600 * 2 + 60 * 5), "2h 5m");
        assert_eq!(uptime(86_400 * 3 + 3600 * 4 + 60), "3d 4h 1m");
    }
}
