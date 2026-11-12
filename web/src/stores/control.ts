import { defineStore } from "pinia";
import { config } from "@/config";
import type {
  ControlStatus,
  LogLine,
  OpenTarget,
  ServerAction,
  Settings,
} from "@/interfaces/Control";

/** How many log lines the page keeps, to match what the collector keeps. */
export const MAX_LOG_LINES = 2000;

const base = `${config.api.urls.http.replace(/\/$/, "")}/control`;

/** What the collector said was wrong, in its own words. */
export class ControlError extends Error {}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${base}${path}`, {
      cache: "no-store",
      ...init,
      headers: { accept: "application/json", ...init?.headers },
    });
  } catch {
    throw new ControlError("Vigil isn't answering. It may have been closed.");
  }
  const text = await response.text();
  let body: unknown = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    // Not JSON: the public server answering instead of the window's own.
  }
  if (!response.ok) {
    const message =
      body && typeof body === "object" && "error" in body
        ? String((body as { error: unknown }).error)
        : `The request failed (${response.status}).`;
    throw new ControlError(message);
  }
  return body as T;
}

function send<T>(path: string, method: "POST" | "PUT", payload?: unknown) {
  return call<T>(path, {
    method,
    headers: payload === undefined ? {} : { "content-type": "application/json" },
    body: payload === undefined ? undefined : JSON.stringify(payload),
  });
}

export const useControlStore = defineStore("control", {
  state: () => ({
    /** Whether this page has a Settings page at all. Null until asked. */
    available: <boolean | null>null,
    status: <ControlStatus | null>null,
    settings: <Settings | null>null,
    logs: <LogLine[]>[],
    newestLog: 0,
  }),
  actions: {
    /**
     * Finds out whether the control API is here. It is only in the Vigil
     * window; a browser gets a 404 and never sees a Settings link.
     */
    async detect(): Promise<boolean> {
      if (this.available !== null) return this.available;
      try {
        const status = await call<ControlStatus | null>("/status");
        this.available = !!status && typeof status.version === "string";
        if (this.available) this.status = status;
      } catch {
        this.available = false;
      }
      return this.available;
    },
    async refreshStatus() {
      this.status = await call<ControlStatus>("/status");
    },
    async loadSettings() {
      this.settings = await call<Settings>("/settings");
    },
    /** Saves the given keys and applies them to the running server. */
    async save(changes: Partial<Settings>) {
      const result = await send<{ settings: Settings; status: ControlStatus }>(
        "/settings",
        "PUT",
        changes,
      );
      this.settings = result.settings;
      this.status = result.status;
    },
    async server(action: ServerAction) {
      try {
        this.status = await send<ControlStatus>(`/server/${action}`, "POST");
      } catch (error) {
        // A failed start leaves a reason in the status worth showing.
        await this.refreshStatus().catch(() => {});
        throw error;
      }
    },
    async pollLogs() {
      const { lines, newest } = await call<{ lines: LogLine[]; newest: number }>(
        `/logs?after=${this.newestLog}`,
      );
      // The collector started again from line 1: start again here too.
      if (newest < this.newestLog) this.logs = [];
      if (lines.length > 0) {
        this.logs = [...this.logs, ...lines].slice(-MAX_LOG_LINES);
      }
      this.newestLog = newest;
    },
    open(target: OpenTarget) {
      return send<null>(`/open/${target}`, "POST");
    },
    quit() {
      return send<null>("/quit", "POST");
    },
  },
});
