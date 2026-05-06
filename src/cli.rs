use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "vigil-collector",
    version,
    about = "Lightweight system and network monitoring server."
)]
pub struct Cli {
    /// Listen address for the HTTP/WS server.
    #[arg(long, default_value = "")]
    pub address: String,

    /// Listen port for the HTTP/WS server.
    #[arg(long, default_value_t = 0)]
    pub port: u16,

    /// WebSocket URL of a Vigil Pro hub to push to.
    #[arg(long, env = "VIGIL_COLLECTOR_HUB")]
    pub hub: Option<String>,

    /// Per-host API key issued by the hub.
    #[arg(long = "hub-key", env = "VIGIL_COLLECTOR_HUB_KEY")]
    pub hub_key: Option<String>,

    /// Host name to advertise to the hub (defaults to system hostname).
    #[arg(long = "hub-name", env = "VIGIL_COLLECTOR_HUB_NAME")]
    pub hub_name: Option<String>,

    /// Comma-separated tags to advertise to the hub.
    #[arg(long = "hub-tags", env = "VIGIL_COLLECTOR_HUB_TAGS")]
    pub hub_tags: Option<String>,
}

impl Cli {
    pub fn parse_tags(&self) -> Vec<String> {
        self.hub_tags
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }
}
