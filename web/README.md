<p align="center">
  <img src="./logo.svg" alt="Vigil" width="120" />
</p>

<h1 align="center">Vigil</h1>

<p align="center">A lightweight monitoring dashboard for one server, or a fleet of them.</p>

<p align="center">View a <a href="https://status.edcs.app">live example here</a>.</p>

![image](./docs/dashboard.webp)

Vigil ships in **two modes** that share the same UI:

- **Host** — connects directly to one [vigil-collector](https://status.edcs.app/hub/help/fundamentals)
  via HTTP/WebSocket and shows that host's full live dashboard. The
  original mode; what most operators want for a single box.
- **Hub** — connects to a [Vigil Pro](https://vigil.edcs.app#pricing)
  Hub and surfaces a fleet view, per-host detail with sparkline metrics,
  alert state visibility, and rule management.

## Quick Start

### Requirements

- [vigil-collector](https://status.edcs.app/hub/help/fundamentals) — the agent that gathers system samples.
- [vigil-pro](https://vigil.edcs.app#pricing) (optional) — the multi-host hub. Only needed for Hub mode.

1. Clone the repository

   ```
   git clone git@github.com:sentrychris/vigil.git
   ```

2. Install dependencies

   ```
   npm install
   ```

3. Copy `.env.example` to `.env` and configure your environment variables

   ```
   VITE_APP_NAME="Vigil"                             # Visible in the navbar, above page title
   VITE_APP_TITLE="Metric & Monitoring"              # Visible in the navbar, page title

   VITE_APP_DEPLOY_REGION=us-central1-a              # (optional) visible in the footer
   VITE_APP_DEPLOY_INSTANCE=chris-web1               # (optional) visible in the footer

   # ─── Host mode ────────────────────────────────────────────────────
   VITE_API_URL="http://127.0.0.1:4500"              # vigil-collector base URL
   VITE_WORKER_URL="http://127.0.0.1:4500/worker"    # collector's worker endpoint
   VITE_WEBSOCKET_URL="ws://127.0.0.1:4500/connect"  # collector's websocket endpoint
   VITE_CONNECTION_TYPE="websocket"                  # "http" for static data, "websocket" for live

   # ─── Hub mode (optional) ──────────────────────────────────────────
   VITE_HUB_URL="https://hub.vigil.example.com"      # Vigil Pro Hub base URL
   ```

## Host mode

The dashboard works with either static data served from standard HTTP endpoints, or live data served through websocket connections.

- If you set `VITE_CONNECTION_TYPE` to `websocket`, the dashboard will load with a websocket connection configured and will display live data that updates in real time.

- If you set `VITE_CONNECTION_TYPE` to `http`, the dashboard will load with data requested from an HTTP endpoint and will poll for updates every 30 seconds.

There is a control switch provided at the top-right on the navbar to toggle the connection type between HTTP and Websocket.

## Hub mode

Setting `VITE_HUB_URL` at build time enables the hub routes. Everything
behind them is gated by the Vigil Pro admin bearer token.

| Path | What |
|---|---|
| `/hub`              | Fleet overview — host list, fleet alert state, recent activity, ops command popout |
| `/hub/hosts/<id>`   | Host detail — system data sheet, health card, sparkline metrics, source footnote |
| `/hub/rules`        | Alert rules — list, drill into rule definitions, inline delete |
| `/hub/help`         | Alert state-machine docs (firing / breaching / OK + transitions + edge cases) |

The host-detail page renders an "Open dashboard" link when the host's
`collector_url` is set on the hub, deep-linking to that host's own
collector dashboard for full live metrics.

When Hub mode isn't configured, the navbar hides the segmented switcher
and the routes render a "Hub mode disabled" notice.

## Feedback

If you have any questions or feedback, please feel free to raise an [issue here](https://github.com/sentrychris/vigil/issues).

## License

MIT.
