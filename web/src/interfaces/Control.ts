// The control API behind the Settings page. It exists only when the page is
// shown in the Vigil window (see src/server/control.rs in the collector).

export interface Probe {
  name: string;
  url: string;
}

/** The settings file, key for key. */
export interface Settings {
  logging_enabled: boolean;
  log_level: string;
  address: string;
  port_number: number;
  max_ws_connections: number;
  ws_push_interval: number;
  probes: Probe[];
  probe_timeout: number;
}

export interface ControlStatus {
  version: string;
  /** Whether the server other devices and browsers reach is running. */
  serving: boolean;
  /** The address being listened on, such as "0.0.0.0:4500". */
  listen: string | null;
  /** Where a browser on this computer reaches it. */
  url: string | null;
  /** The address and port a start would use. */
  address: string;
  port: number;
  /** That value came from the command line rather than the settings file. */
  address_from_cli: boolean;
  port_from_cli: boolean;
  /** Why the server isn't running, when that wasn't asked for. */
  error: string | null;
  connections: { active: number; max: number };
  uptime_secs: number;
  paths: { settings: string; log: string };
  /** RUST_LOG is set, so the log level setting has no effect. */
  log_level_locked: boolean;
}

export interface LogLine {
  seq: number;
  text: string;
}

export type ServerAction = "start" | "stop" | "restart";
export type OpenTarget = "logs" | "settings" | "browser";
