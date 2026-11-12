use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_ADDRESS: &str = "localhost";
pub const DEFAULT_PORT: u16 = 4500;
pub const DEFAULT_MAX_WS_CONNECTIONS: usize = 20;
pub const DEFAULT_WS_PUSH_INTERVAL: f64 = 1.0;
pub const DEFAULT_PROBE_TIMEOUT: f64 = 5.0;
pub const DEFAULT_LOG_LEVEL: &str = "INFO";
pub const DEFAULT_LOG_ENABLED: bool = true;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub name: String,
    pub url: String,
}

fn default_probes() -> Vec<Probe> {
    vec![
        Probe {
            name: "edcs.app".into(),
            url: "https://edcs.app".into(),
        },
        Probe {
            name: "api.edcs.app".into(),
            url: "https://api.edcs.app".into(),
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_log_enabled")]
    pub logging_enabled: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_port")]
    pub port_number: u16,
    #[serde(default = "default_max_ws")]
    pub max_ws_connections: usize,
    #[serde(default = "default_push_interval")]
    pub ws_push_interval: f64,
    #[serde(default = "default_probes")]
    pub probes: Vec<Probe>,
    #[serde(default = "default_probe_timeout")]
    pub probe_timeout: f64,
}

fn default_log_enabled() -> bool {
    DEFAULT_LOG_ENABLED
}
fn default_log_level() -> String {
    DEFAULT_LOG_LEVEL.into()
}
fn default_address() -> String {
    DEFAULT_ADDRESS.into()
}
fn default_port() -> u16 {
    DEFAULT_PORT
}
fn default_max_ws() -> usize {
    DEFAULT_MAX_WS_CONNECTIONS
}
fn default_push_interval() -> f64 {
    DEFAULT_WS_PUSH_INTERVAL
}
fn default_probe_timeout() -> f64 {
    DEFAULT_PROBE_TIMEOUT
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            logging_enabled: DEFAULT_LOG_ENABLED,
            log_level: DEFAULT_LOG_LEVEL.into(),
            address: DEFAULT_ADDRESS.into(),
            port_number: DEFAULT_PORT,
            max_ws_connections: DEFAULT_MAX_WS_CONNECTIONS,
            ws_push_interval: DEFAULT_WS_PUSH_INTERVAL,
            probes: default_probes(),
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
        }
    }
}

/// The most WebSocket clients the cap can be set to.
pub const MAX_WS_CONNECTIONS_LIMIT: usize = 10_000;
/// The most service checks the probe list can hold.
pub const MAX_PROBES: usize = 50;
const LOG_LEVELS: [&str; 5] = ["DEBUG", "INFO", "WARNING", "WARN", "ERROR"];

impl Settings {
    /// Checks values that came from the Settings page before they are
    /// saved, and says what is wrong in words a person can act on. A file
    /// edited by hand is never checked this strictly (see
    /// ``load_settings``).
    pub fn validate(&self) -> Result<(), String> {
        if self.address.trim().is_empty() {
            return Err("The address can't be empty.".into());
        }
        if self.port_number == 0 {
            return Err("The port must be between 1 and 65535.".into());
        }
        if self.max_ws_connections > MAX_WS_CONNECTIONS_LIMIT {
            return Err(format!(
                "The connection limit can't be more than {MAX_WS_CONNECTIONS_LIMIT}."
            ));
        }
        if !(0.1..=3600.0).contains(&self.ws_push_interval) {
            return Err("The update interval must be between 0.1 and 3600 seconds.".into());
        }
        if !(0.1..=120.0).contains(&self.probe_timeout) {
            return Err("The service check timeout must be between 0.1 and 120 seconds.".into());
        }
        if !LOG_LEVELS.contains(&self.log_level.to_uppercase().as_str()) {
            return Err("The log level must be DEBUG, INFO, WARNING or ERROR.".into());
        }
        if self.probes.len() > MAX_PROBES {
            return Err(format!(
                "There can't be more than {MAX_PROBES} service checks."
            ));
        }
        for probe in &self.probes {
            if probe.name.trim().is_empty() {
                return Err("Every service check needs a name.".into());
            }
            if !(probe.url.starts_with("http://") || probe.url.starts_with("https://")) {
                return Err(format!(
                    "The address for \"{}\" must start with http:// or https://.",
                    probe.name
                ));
            }
        }
        Ok(())
    }

    /// These settings with the keys of ``changes`` (a JSON object) laid
    /// over them, so a caller can send only what it wants to change.
    pub fn merged(&self, changes: &serde_json::Value) -> Result<Settings, String> {
        let Some(changes) = changes.as_object() else {
            return Err("Expected a JSON object of settings.".into());
        };
        let mut value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        if let Some(current) = value.as_object_mut() {
            for (key, new) in changes {
                if !current.contains_key(key) {
                    return Err(format!("\"{key}\" isn't a setting."));
                }
                current.insert(key.clone(), new.clone());
            }
        }
        serde_json::from_value(value).map_err(|e| format!("That setting isn't valid: {e}"))
    }
}

pub fn settings_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vigil-collector")
}

pub fn settings_file() -> PathBuf {
    settings_dir().join("settings.json")
}

fn legacy_settings_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".psmonitor")
}

/// A message about the settings file, produced before the tracing
/// subscriber exists. ``main`` emits these once logging is up; the probes
/// endpoint (which re-reads settings per request) emits them immediately.
pub enum Note {
    Info(String),
    Warn(String),
    Error(String),
}

impl Note {
    pub fn emit(self) {
        match self {
            Note::Info(m) => tracing::info!("{m}"),
            Note::Warn(m) => tracing::warn!("{m}"),
            Note::Error(m) => tracing::error!("{m}"),
        }
    }
}

fn migrate_legacy(notes: &mut Vec<Note>) {
    let new = settings_dir();
    let old = legacy_settings_dir();
    if new.exists() || !old.is_dir() {
        return;
    }
    match fs::rename(&old, &new) {
        Ok(()) => notes.push(Note::Info(format!(
            "Migrated settings: {} -> {}",
            old.display(),
            new.display()
        ))),
        Err(e) => notes.push(Note::Warn(format!(
            "Could not migrate legacy settings dir {} -> {}: {}",
            old.display(),
            new.display(),
            e
        ))),
    }
}

/// Read settings, creating the file with defaults on first run. Always
/// returns *something* — file errors fall through to defaults so the
/// server never refuses to start because of a malformed config.
pub fn load_settings() -> (Settings, Vec<Note>) {
    let mut notes = Vec::new();
    migrate_legacy(&mut notes);
    let path = settings_file();
    if !path.exists() {
        let dir = settings_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            notes.push(Note::Error(format!(
                "Failed to create settings dir {}: {}",
                dir.display(),
                e
            )));
            return (Settings::default(), notes);
        }
        let defaults = Settings::default();
        match serde_json::to_string_pretty(&defaults) {
            Ok(s) => match fs::write(&path, s) {
                Ok(()) => notes.push(Note::Info(format!(
                    "Created settings file at {}",
                    path.display()
                ))),
                Err(e) => notes.push(Note::Error(format!(
                    "Failed to write defaults to {}: {}",
                    path.display(),
                    e
                ))),
            },
            Err(e) => notes.push(Note::Error(format!("Failed to serialize defaults: {e}"))),
        }
        return (defaults, notes);
    }

    let settings = match fs::read_to_string(&path) {
        Ok(s) => match parse_settings(&s) {
            Ok(cfg) => cfg,
            Err(e) => {
                notes.push(Note::Error(format!(
                    "Failed to parse {}: {} — falling back to defaults",
                    path.display(),
                    e
                )));
                Settings::default()
            }
        },
        Err(e) => {
            notes.push(Note::Error(format!(
                "Failed to read {}: {} — falling back to defaults",
                path.display(),
                e
            )));
            Settings::default()
        }
    };
    (settings, notes)
}

/// Parse the settings file contents. Notepad and PowerShell 5's
/// ``Out-File`` save UTF-8 with a byte order mark, which serde_json
/// rejects; strip it so a settings file edited on Windows still parses.
fn parse_settings(raw: &str) -> serde_json::Result<Settings> {
    serde_json::from_str(raw.strip_prefix('\u{feff}').unwrap_or(raw))
}

/// Writes the settings file. The new contents go to a file beside it
/// first and are then moved into place, so a crash part-way through
/// can't leave a half-written settings file behind.
pub fn save_settings(settings: &Settings) -> std::io::Result<()> {
    let dir = settings_dir();
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    let tmp = dir.join("settings.json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, settings_file())
}

/// Where the desktop window's own details are remembered between runs.
fn window_file() -> PathBuf {
    settings_dir().join("window.json")
}

#[derive(Default, Serialize, Deserialize)]
struct WindowState {
    #[serde(default)]
    port: u16,
}

/// The local port the desktop window used last time, if any. Using the
/// same one again keeps what the dashboard stores in the browser (theme,
/// live or static mode), which is kept per address and port.
pub fn window_port() -> Option<u16> {
    let raw = fs::read_to_string(window_file()).ok()?;
    let state: WindowState = serde_json::from_str(&raw).ok()?;
    (state.port != 0).then_some(state.port)
}

/// Remembers the window's local port for next time. Best effort.
pub fn remember_window_port(port: u16) {
    if let Ok(json) = serde_json::to_string(&WindowState { port }) {
        let _ = fs::create_dir_all(settings_dir());
        let _ = fs::write(window_file(), json);
    }
}

/// ``load_settings`` for callers that run after logging is initialised.
pub fn read_settings() -> Settings {
    let (settings, notes) = load_settings();
    for note in notes {
        note.emit();
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn partial_settings_fill_in_defaults() {
        let s: Settings = serde_json::from_str(r#"{"port_number": 4700}"#).unwrap();
        assert_eq!(s.port_number, 4700);
        assert_eq!(s.address, super::DEFAULT_ADDRESS);
        assert_eq!(s.max_ws_connections, super::DEFAULT_MAX_WS_CONNECTIONS);
    }

    #[test]
    fn validate_explains_what_is_wrong() {
        let ok = Settings::default();
        assert!(ok.validate().is_ok());
        let bad = |change: fn(&mut Settings)| {
            let mut s = Settings::default();
            change(&mut s);
            s.validate().unwrap_err()
        };
        assert!(bad(|s| s.port_number = 0).contains("port"));
        assert!(bad(|s| s.address = "  ".into()).contains("address"));
        assert!(bad(|s| s.max_ws_connections = 10_001).contains("limit"));
        assert!(bad(|s| s.log_level = "LOUD".into()).contains("log level"));
        assert!(bad(|s| s.probes[0].url = "ftp://x".into()).contains("http"));
    }

    #[test]
    fn merged_changes_only_the_keys_given() {
        let base = Settings::default();
        let changed = base
            .merged(&serde_json::json!({"port_number": 4800, "address": "0.0.0.0"}))
            .unwrap();
        assert_eq!(changed.port_number, 4800);
        assert_eq!(changed.address, "0.0.0.0");
        assert_eq!(changed.probes, base.probes);
        assert!(base.merged(&serde_json::json!({"nope": 1})).is_err());
        assert!(
            base.merged(&serde_json::json!({"port_number": "x"}))
                .is_err()
        );
        assert!(base.merged(&serde_json::json!([1])).is_err());
    }

    #[test]
    fn bom_is_stripped_before_parsing() {
        let s = super::parse_settings("\u{feff}{\"port_number\": 4701}").unwrap();
        assert_eq!(s.port_number, 4701);
        assert!(serde_json::from_str::<Settings>("\u{feff}{}").is_err());
    }
}
