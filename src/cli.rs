use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "vigil-collector",
    version,
    about = "Vigil: a system monitoring utility with remote connection capabilities."
)]
pub struct Cli {
    /// Listen address for the HTTP/WS server (overrides settings.json).
    #[arg(long)]
    pub address: Option<String>,

    /// Listen port for the HTTP/WS server (overrides settings.json).
    #[arg(long)]
    pub port: Option<u16>,

    /// Windows: stay in this console instead of moving to the system tray.
    /// Use it for scripts and scheduled tasks. Ignored on other platforms.
    #[arg(long)]
    pub foreground: bool,
}
