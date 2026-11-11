use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "vigil-collector",
    version,
    about = "Lightweight system and network monitoring server."
)]
pub struct Cli {
    /// Listen address for the HTTP/WS server (overrides settings.json).
    #[arg(long)]
    pub address: Option<String>,

    /// Listen port for the HTTP/WS server (overrides settings.json).
    #[arg(long)]
    pub port: Option<u16>,
}
