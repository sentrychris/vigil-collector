use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::sync::Mutex;

use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const TS_FMT: &[BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// Formats timestamps in the local offset captured at startup.
///
/// ``OffsetDateTime::now_local()`` refuses to answer on Unix once the
/// process has more than one thread (reading the TZ database is not
/// thread-safe there), so calling it per log line silently fell back to
/// UTC as soon as the blocking pool spun up. The offset is resolved once,
/// while ``main`` is still single-threaded.
struct ClassicTimer {
    offset: UtcOffset,
}

impl FormatTime for ClassicTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = OffsetDateTime::now_utc().to_offset(self.offset);
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

fn open_log_file() -> io::Result<File> {
    std::fs::create_dir_all(log_dir())?;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_file_path())
}

/// Enable ANSI escape processing on the Windows console. Windows Terminal
/// already has it on, but the classic console host (cmd.exe, PowerShell
/// 5) prints the raw escape codes unless the process opts in. Returns
/// whether colour output is safe to use.
#[cfg(windows)]
fn enable_ansi() -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
        SetConsoleMode,
    };

    // SAFETY: plain Win32 calls on the process's own stdout handle with a
    // valid out-pointer; failures are reported through the return value.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
            || SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_ansi() -> bool {
    true
}

/// Initialize tracing once. Truncates the existing log file on startup and
/// writes to both file and stdout. ``enabled = false`` mutes both layers
/// entirely. A log file that can't be opened (read-only home, locked by
/// another instance on Windows) is reported on stderr and skipped instead
/// of stopping the collector from starting.
pub fn init(level: &str, enabled: bool) {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    let directive = if !enabled {
        "off"
    } else {
        match level.to_uppercase().as_str() {
            "DEBUG" => "vigil_collector=debug",
            "WARNING" | "WARN" => "vigil_collector=warn",
            "ERROR" => "vigil_collector=error",
            _ => "vigil_collector=info",
        }
    };
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(directive));

    let ansi =
        io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() && enable_ansi();

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(io::stdout)
        .with_ansi(ansi)
        .with_target(false)
        .with_level(true)
        .with_timer(ClassicTimer { offset });

    let file_layer = if enabled {
        match open_log_file() {
            Ok(file) => Some(
                tracing_subscriber::fmt::layer()
                    .with_writer(Mutex::new(file))
                    .with_ansi(false)
                    .with_target(false)
                    .with_level(true)
                    .with_timer(ClassicTimer { offset }),
            ),
            Err(e) => {
                eprintln!(
                    "vigil-collector: file logging disabled, cannot open {}: {e}",
                    log_file_path().display()
                );
                None
            }
        }
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();
}
