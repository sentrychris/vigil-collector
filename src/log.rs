use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use time::format_description::FormatItem;
use time::macros::format_description;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const TS_FMT: &[FormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

struct ClassicTimer;

impl FormatTime for ClassicTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let s = now.format(&TS_FMT).map_err(|_| std::fmt::Error)?;
        write!(w, "[{s}]")
    }
}

fn log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vigil-collector-logs")
}

fn log_file_path() -> PathBuf {
    log_dir().join("app.log")
}

/// Initialize tracing once. Truncates the existing log file (matches the
/// Python ``logger.clear_log()`` on startup) and writes to both file and
/// stdout. ``enabled = false`` mutes both layers entirely.
pub fn init(level: &str, enabled: bool) -> io::Result<()> {
    let dir = log_dir();
    std::fs::create_dir_all(&dir)?;
    let path = log_file_path();

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;

    let directive = if !enabled {
        "off".to_string()
    } else {
        match level.to_uppercase().as_str() {
            "DEBUG" => "vigil_collector=debug".to_string(),
            "WARNING" | "WARN" => "vigil_collector=warn".to_string(),
            "ERROR" => "vigil_collector=error".to_string(),
            _ => "vigil_collector=info".to_string(),
        }
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(directive));

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(io::stdout)
        .with_target(false)
        .with_level(true)
        .with_timer(ClassicTimer);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_target(false)
        .with_level(true)
        .with_timer(ClassicTimer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(())
}
