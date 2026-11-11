import type { Configuration } from "./interfaces/Configuration";
import { APP_VERSION } from "./version";

// The dashboard is served by vigil-collector itself, so every request goes
// back to the origin that served the page. In development the Vite dev
// server proxies those paths to a running collector (see vite.config.ts).
const origin = window.location.origin;

export const config: Configuration = {
  app: {
    base_url: import.meta.env.BASE_URL,
    name: import.meta.env.VITE_APP_NAME || "Vigil",
    title: import.meta.env.VITE_APP_TITLE || "System Monitor",
    // Set by the collector's build.rs so the footer shows its version.
    version: import.meta.env.VITE_APP_VERSION || APP_VERSION,
    deployment: {
      region: import.meta.env.VITE_DEPLOY_REGION || "",
      instance: import.meta.env.VITE_DEPLOY_INSTANCE || "",
    },
  },
  api: {
    connectionType: import.meta.env.VITE_CONNECTION_TYPE || "websocket",
    urls: {
      http: origin,
      worker: `${origin}/worker`,
      websocket: `${origin.replace(/^http/, "ws")}/connect`,
    },
  },
  services: {
    pollInterval: 30_000,
    thresholds: {
      slow: 800,
    },
  },
};
