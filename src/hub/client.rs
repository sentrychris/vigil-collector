use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arc_swap::ArcSwap;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use crate::snapshot::Snapshot;

use super::flatten::{flatten, PROCESSES_MAX};
use super::protocol::{
    Hello, HostInfo, HubFrame, ProcessItem, Processes, Samples, AGENT_VERSION, PROTOCOL_VERSION,
};

const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const BACKOFF_GROWTH: f64 = 1.7;
const BYTES_CEILING: u64 = 1u64 << 41;

pub struct HubClient {
    url: String,
    key: String,
    host_name: String,
    tags: Vec<String>,
    snapshot: Arc<ArcSwap<Snapshot>>,
    sampler_interval: Duration,
    stop: Arc<Notify>,
    handle: Option<JoinHandle<()>>,
}

impl HubClient {
    pub fn new(
        url: String,
        key: String,
        host_name: Option<String>,
        tags: Vec<String>,
        snapshot: Arc<ArcSwap<Snapshot>>,
        sampler_interval: Duration,
    ) -> Self {
        let host_name = host_name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                gethostname::gethostname()
                    .into_string()
                    .unwrap_or_else(|_| "unknown".into())
            });
        Self {
            url,
            key,
            host_name,
            tags,
            snapshot,
            sampler_interval,
            stop: Arc::new(Notify::new()),
            handle: None,
        }
    }

    pub fn start(&mut self) {
        if self.handle.is_some() {
            return;
        }
        let url = self.url.clone();
        let key = self.key.clone();
        let host_name = self.host_name.clone();
        let tags = self.tags.clone();
        let snapshot = self.snapshot.clone();
        let interval = self.sampler_interval;
        let stop = self.stop.clone();

        let handle = tokio::spawn(async move {
            supervise(url, key, host_name, tags, snapshot, interval, stop).await;
        });
        self.handle = Some(handle);
    }

    pub fn stop(&mut self) {
        self.stop.notify_waiters();
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

async fn supervise(
    url: String,
    key: String,
    host_name: String,
    tags: Vec<String>,
    snapshot: Arc<ArcSwap<Snapshot>>,
    sampler_interval: Duration,
    stop: Arc<Notify>,
) {
    let mut backoff = BACKOFF_INITIAL;
    loop {
        let connect_result = tokio::select! {
            _ = stop.notified() => return,
            r = connect_and_push(&url, &key, &host_name, &tags, &snapshot, sampler_interval, &stop) => r,
        };

        match connect_result {
            Ok(()) => {
                backoff = BACKOFF_INITIAL;
            }
            Err(e) => {
                tracing::warn!("hub.connection_lost: {}", e);
            }
        }

        tokio::select! {
            _ = stop.notified() => return,
            _ = tokio::time::sleep(backoff) => {}
        }

        let next = (backoff.as_secs_f64() * BACKOFF_GROWTH).min(BACKOFF_MAX.as_secs_f64());
        backoff = Duration::from_secs_f64(next);
    }
}

async fn connect_and_push(
    url: &str,
    key: &str,
    host_name: &str,
    tags: &[String],
    snapshot: &Arc<ArcSwap<Snapshot>>,
    sampler_interval: Duration,
    stop: &Notify,
) -> anyhow::Result<()> {
    let (ws, _resp) = tokio_tungstenite::connect_async(url).await?;
    let (mut tx, mut rx) = ws.split();

    let cpu_cores = num_cpus_logical();
    let hostname_owned = gethostname::gethostname()
        .into_string()
        .unwrap_or_else(|_| "unknown".into());
    let os = detect_os();
    let arch = arch_label();

    let hello = Hello {
        v: PROTOCOL_VERSION,
        t: "hello",
        key,
        host: HostInfo {
            name: host_name,
            hostname: &hostname_owned,
            os: &os,
            arch: &arch,
            agent_version: AGENT_VERSION,
            cpu_cores,
            tags,
        },
    };
    tx.send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await?;

    let interval = match rx.next().await {
        Some(Ok(Message::Text(s))) => match serde_json::from_str::<HubFrame>(&s)? {
            HubFrame::Welcome { host_id, interval } => {
                let interval = interval.unwrap_or(sampler_interval.as_secs_f64());
                tracing::info!(
                    "hub.connected: host={} host_id={:?} interval={:.2}s",
                    host_name,
                    host_id,
                    interval
                );
                Duration::from_secs_f64(interval.max(0.1))
            }
            HubFrame::Error { code, message } => {
                anyhow::bail!(
                    "hub error: {} {}",
                    code.unwrap_or_default(),
                    message.unwrap_or_default()
                );
            }
            _ => anyhow::bail!("hub: unexpected first frame"),
        },
        Some(Ok(_)) => anyhow::bail!("hub: non-text first frame"),
        Some(Err(e)) => return Err(e.into()),
        None => anyhow::bail!("hub closed before welcome"),
    };

    push_loop(&mut tx, &mut rx, snapshot, interval, stop).await
}

async fn push_loop<S, R>(
    tx: &mut S,
    rx: &mut R,
    snapshot: &Arc<ArcSwap<Snapshot>>,
    interval: Duration,
    stop: &Notify,
) -> anyhow::Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let snap = snapshot.load_full();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let metrics = flatten(&snap);
        if !metrics.is_empty() {
            let payload = Samples {
                v: PROTOCOL_VERSION,
                t: "samples",
                ts,
                metrics: &metrics,
            };
            tx.send(Message::Text(serde_json::to_string(&payload)?.into()))
                .await?;
        }

        if !snap.processes.is_empty() {
            let items: Vec<ProcessItem> = snap
                .processes
                .iter()
                .take(PROCESSES_MAX)
                .map(|p| ProcessItem {
                    pid: p.pid,
                    name: clip(&p.name, 256),
                    username: clip(&p.username, 256),
                    mem_bytes: ((p.mem * 1024.0 * 1024.0) as u64).min(BYTES_CEILING),
                })
                .collect();
            let frame = Processes {
                v: PROTOCOL_VERSION,
                t: "processes",
                ts,
                metric: &snap.processes_metric,
                items,
            };
            tx.send(Message::Text(serde_json::to_string(&frame)?.into()))
                .await?;
        }

        // Sleep, but bail early if stop fires or the hub closes the WS.
        let mut sleep = std::pin::pin!(tokio::time::sleep(interval));
        loop {
            tokio::select! {
                _ = stop.notified() => return Ok(()),
                _ = sleep.as_mut() => break,
                msg = rx.next() => match msg {
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.into()),
                }
            }
        }
    }
}

fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // String slicing on byte boundary — utf-8 safe truncation. For
        // names/usernames this is a defensive guard; real values are
        // small.
        let mut end = max;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &s[..end]
    }
}

fn num_cpus_logical() -> Option<u32> {
    std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u32)
}

fn arch_label() -> String {
    std::env::consts::ARCH.to_string()
}

fn detect_os() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(raw) = std::fs::read_to_string("/etc/os-release") {
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                    return v.trim_matches('"').to_string();
                }
            }
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix("NAME=") {
                    return v.trim_matches('"').to_string();
                }
            }
        }
        return "linux".into();
    }
    #[cfg(target_os = "macos")]
    {
        return "macOS".into();
    }
    #[cfg(target_os = "windows")]
    {
        return "Windows".into();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        return std::env::consts::OS.to_string();
    }
}
