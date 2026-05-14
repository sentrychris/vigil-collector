use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const AGENT_VERSION: &str = "0.1.1";

#[derive(Serialize)]
pub struct HostInfo<'a> {
    pub name: &'a str,
    pub hostname: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    pub agent_version: &'a str,
    pub cpu_cores: Option<u32>,
    pub tags: &'a [String],
}

#[derive(Serialize)]
pub struct Hello<'a> {
    pub v: u32,
    pub t: &'static str,
    pub key: &'a str,
    pub host: HostInfo<'a>,
}

#[derive(Deserialize)]
#[serde(tag = "t")]
pub enum HubFrame {
    #[serde(rename = "welcome")]
    Welcome {
        host_id: Option<i64>,
        #[serde(default)]
        interval: Option<f64>,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        message: Option<String>,
    },
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "pong")]
    Pong {},
    #[serde(other)]
    Unknown,
}

#[derive(Serialize)]
pub struct Samples<'a> {
    pub v: u32,
    pub t: &'static str,
    pub ts: i64,
    pub metrics: &'a BTreeMap<String, f64>,
}

#[derive(Serialize)]
pub struct ProcessItem<'a> {
    pub pid: u32,
    pub name: &'a str,
    pub username: &'a str,
    pub mem_bytes: u64,
}

#[derive(Serialize)]
pub struct Processes<'a> {
    pub v: u32,
    pub t: &'static str,
    pub ts: i64,
    pub metric: &'a str,
    pub items: Vec<ProcessItem<'a>>,
}

#[derive(Serialize)]
pub struct ProcessCpuItem<'a> {
    pub pid: u32,
    pub name: &'a str,
    pub username: &'a str,
    pub cpu_pct: f64,
}

#[derive(Serialize)]
pub struct ProcessesCpu<'a> {
    pub v: u32,
    pub t: &'static str,
    pub ts: i64,
    pub items: Vec<ProcessCpuItem<'a>>,
}
