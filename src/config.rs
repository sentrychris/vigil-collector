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

fn migrate_legacy() {
    let new = settings_dir();
    let old = legacy_settings_dir();
    if new.exists() {
        return;
    }
    if !old.is_dir() {
        return;
    }
    if let Err(e) = fs::rename(&old, &new) {
        tracing::warn!(
            "Could not migrate legacy settings dir {} -> {}: {}",
            old.display(),
            new.display(),
            e
        );
    } else {
        tracing::info!("Migrated settings: {} -> {}", old.display(), new.display());
    }
}

/// Read settings, creating the file with defaults on first run. Always
/// returns *something* — file errors fall through to defaults so the
/// server never refuses to start because of a malformed config.
pub fn read_settings() -> Settings {
    migrate_legacy();
    let path = settings_file();
    if !path.exists() {
        let dir = settings_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            tracing::error!("Failed to create settings dir {}: {}", dir.display(), e);
            return Settings::default();
        }
        let defaults = Settings::default();
        match serde_json::to_string_pretty(&defaults) {
            Ok(s) => {
                if let Err(e) = fs::write(&path, s) {
                    tracing::error!("Failed to write defaults to {}: {}", path.display(), e);
                } else {
                    tracing::info!("Created settings file at {}", path.display());
                }
            }
            Err(e) => tracing::error!("Failed to serialize defaults: {}", e),
        }
        return defaults;
    }

    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Settings>(&s) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!("Failed to parse {}: {} — falling back to defaults", path.display(), e);
                Settings::default()
            }
        },
        Err(e) => {
            tracing::error!("Failed to read {}: {} — falling back to defaults", path.display(), e);
            Settings::default()
        }
    }
}

