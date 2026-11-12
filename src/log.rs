use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, reload};

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

pub fn log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vigil-collector-logs")
}

pub fn log_file_path() -> PathBuf {
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

/// Whether this process's console can show colour. The Windows tray
/// launcher asks before starting the tray process, whose output is a pipe.
#[cfg(windows)]
pub fn console_supports_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() && enable_ansi()
}

/// The tray process writes to a pipe that the launcher relays to its
/// console, so it takes the launcher's word on colour.
#[cfg(windows)]
fn forced_color() -> bool {
    std::env::var_os(crate::tray::COLOR_ENV).is_some()
}

#[cfg(not(windows))]
fn forced_color() -> bool {
    false
}

/// How many recent log lines are kept in memory for the Settings page.
const BUFFER_LINES: usize = 2000;
/// The log file starts again from empty once it reaches this size. It
/// used to be cleared by every restart, but a restart no longer starts a
/// new process.
const FILE_LIMIT: u64 = 4 * 1024 * 1024;

/// One line of the in-memory log, numbered so a reader can ask for
/// everything after the last line it has seen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Line {
    pub seq: u64,
    pub text: String,
}

/// The most recent log lines, oldest first.
struct Buffer {
    next: u64,
    lines: VecDeque<Line>,
}

impl Buffer {
    const fn new() -> Self {
        Self {
            next: 1,
            lines: VecDeque::new(),
        }
    }

    fn push(&mut self, text: &str, capacity: usize) {
        for line in text.lines() {
            if self.lines.len() >= capacity {
                self.lines.pop_front();
            }
            self.lines.push_back(Line {
                seq: self.next,
                text: line.to_string(),
            });
            self.next += 1;
        }
    }

    /// Lines numbered above ``after``, and the number to pass next time.
    fn after(&self, after: u64) -> (Vec<Line>, u64) {
        let lines = self
            .lines
            .iter()
            .filter(|l| l.seq > after)
            .cloned()
            .collect();
        (lines, self.next - 1)
    }
}

static BUFFER: Mutex<Buffer> = Mutex::new(Buffer::new());

/// A log line can't leave the buffer or the file half-written in a way
/// that matters, so a poisoned lock is simply taken over.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Log lines numbered above ``after`` (0 for all that are kept), and the
/// number of the newest line.
pub fn lines_after(after: u64) -> (Vec<Line>, u64) {
    lock(&BUFFER).after(after)
}

/// Writes formatted log events into ``BUFFER``.
struct BufferWriter;

impl Write for BufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        lock(&BUFFER).push(&String::from_utf8_lossy(buf), BUFFER_LINES);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The log file, once it has been opened, and how much has been written
/// to it since it was last empty.
struct LogFile {
    file: Option<File>,
    written: u64,
}

static FILE: Mutex<LogFile> = Mutex::new(LogFile {
    file: None,
    written: 0,
});

/// Opens (and clears) the log file unless it is already open. A file that
/// can't be opened (read-only home, locked by another program) is
/// reported on stderr and skipped instead of stopping the collector.
fn ensure_log_file() {
    let mut state = lock(&FILE);
    if state.file.is_some() {
        return;
    }
    match open_log_file() {
        Ok(file) => {
            state.file = Some(file);
            state.written = 0;
        }
        Err(e) => eprintln!(
            "vigil-collector: file logging disabled, cannot open {}: {e}",
            log_file_path().display()
        ),
    }
}

/// Writes formatted log events to the log file, if it is open.
struct FileWriter;

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = lock(&FILE);
        let LogFile { file, written } = &mut *state;
        let Some(file) = file else {
            return Ok(buf.len());
        };
        if *written + buf.len() as u64 > FILE_LIMIT {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            *written = 0;
        }
        file.write_all(buf)?;
        *written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut lock(&FILE).file {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// What ``apply`` needs to change the level after startup.
struct Control {
    filter: reload::Handle<EnvFilter, Registry>,
    /// ``RUST_LOG`` is set, and wins over the settings file.
    env_override: bool,
}

static CONTROL: OnceLock<Control> = OnceLock::new();

/// The filter directive for a settings-file level.
fn directive(level: &str, enabled: bool) -> &'static str {
    if !enabled {
        return "off";
    }
    match level.to_uppercase().as_str() {
        "DEBUG" => "vigil_collector=debug",
        "WARNING" | "WARN" => "vigil_collector=warn",
        "ERROR" => "vigil_collector=error",
        _ => "vigil_collector=info",
    }
}

/// Whether ``RUST_LOG`` is deciding the level, so the one in the settings
/// file has no effect.
pub fn env_override() -> bool {
    CONTROL.get().is_some_and(|c| c.env_override)
}

/// Changes the level while running, and opens the log file if logging has
/// just been turned on. Does nothing to the level when ``RUST_LOG`` is
/// set.
pub fn apply(level: &str, enabled: bool) {
    let Some(control) = CONTROL.get() else {
        return;
    };
    if enabled {
        ensure_log_file();
    }
    if !control.env_override {
        let filter = EnvFilter::new(directive(level, enabled));
        if let Err(e) = control.filter.modify(|f| *f = filter) {
            eprintln!("vigil-collector: couldn't change the log level: {e}");
        }
    }
}

/// Initialize tracing once. Clears the existing log file on startup and
/// writes to the file, stdout and an in-memory buffer of recent lines
/// (shown on the Settings page). ``enabled = false`` mutes all three; the
/// level and the switch can be changed later with ``apply``.
pub fn init(level: &str, enabled: bool) {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    let (env_filter, env_override) = match EnvFilter::try_from_default_env() {
        Ok(filter) => (filter, true),
        Err(_) => (EnvFilter::new(directive(level, enabled)), false),
    };
    let (filter, handle) = reload::Layer::new(env_filter);

    let ansi = std::env::var_os("NO_COLOR").is_none()
        && (forced_color() || (io::stdout().is_terminal() && enable_ansi()));

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(io::stdout)
        .with_ansi(ansi)
        .with_target(false)
        .with_level(true)
        .with_timer(ClassicTimer { offset });

    if enabled {
        ensure_log_file();
    }
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(|| FileWriter)
        .with_ansi(false)
        .with_target(false)
        .with_level(true)
        .with_timer(ClassicTimer { offset });

    let buffer_layer = tracing_subscriber::fmt::layer()
        .with_writer(|| BufferWriter)
        .with_ansi(false)
        .with_target(false)
        .with_level(true)
        .with_timer(ClassicTimer { offset });

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .with(buffer_layer)
        .init();

    let _ = CONTROL.set(Control {
        filter: handle,
        env_override,
    });
}

#[cfg(test)]
mod tests {
    use super::{Buffer, directive};

    #[test]
    fn buffer_keeps_the_newest_lines_and_numbers_them() {
        let mut buffer = Buffer::new();
        buffer.push("one\n", 3);
        buffer.push("two\nthree\n", 3);
        buffer.push("four\n", 3);
        let (lines, newest) = buffer.after(0);
        let text: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text, ["two", "three", "four"]);
        assert_eq!(newest, 4);
        let (lines, newest) = buffer.after(3);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].seq, 4);
        assert_eq!(newest, 4);
        assert!(buffer.after(4).0.is_empty());
    }

    #[test]
    fn empty_buffer_reports_no_lines() {
        let (lines, newest) = Buffer::new().after(0);
        assert!(lines.is_empty());
        assert_eq!(newest, 0);
    }

    #[test]
    fn directive_follows_the_settings_file() {
        assert_eq!(directive("debug", true), "vigil_collector=debug");
        assert_eq!(directive("WARNING", true), "vigil_collector=warn");
        assert_eq!(directive("nonsense", true), "vigil_collector=info");
        assert_eq!(directive("DEBUG", false), "off");
    }
}
