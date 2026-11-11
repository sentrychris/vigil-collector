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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    fn bom_is_stripped_before_parsing() {
        let s = super::parse_settings("\u{feff}{\"port_number\": 4701}").unwrap();
        assert_eq!(s.port_number, 4701);
        assert!(serde_json::from_str::<Settings>("\u{feff}{}").is_err());
    }
}
