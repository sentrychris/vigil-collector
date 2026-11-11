<p align="center">
  <img src="./logo.svg" alt="Vigil" width="120" />
</p>

<h1 align="center">Vigil dashboard</h1>

The live dashboard for [Vigil Collector](../README.md). It's a Vue 3 +
TypeScript app built with Vite, and the collector embeds the built files
in its binary and serves them at `/`. You don't deploy it separately.

`cargo build` in the repository root builds it for you (see
[build.rs](../build.rs)), so you only need these commands while working on
the dashboard itself.

## Working on it

Requirements: Node.js 18 or newer with npm, and a running collector.

```sh
npm ci
npm run dev          # http://localhost:8888, hot reload
```

The dev server forwards the API (`/system`, `/network`, `/probes`,
`/worker` and the `/connect` WebSocket) to a collector on
`127.0.0.1:4500`. Start one with `cargo run` in the repository root, or
point the dev server at another one in `.env`:

```sh
VIGIL_COLLECTOR_URL="http://192.168.1.20:4500"
```

Checks (CI runs these):

```sh
npm run type-check
npx vitest run --environment jsdom --root src/
npm run build-only   # what build.rs runs; output in dist/
```

## Settings

All optional, read at build time from `.env` (see
[.env.example](.env.example)):

| Variable | Default | What |
|---|---|---|
| `VITE_APP_NAME` | `Vigil` | Small heading above the title in the navbar |
| `VITE_APP_TITLE` | `System Monitor` | Navbar title |
| `VITE_DEPLOY_REGION`, `VITE_DEPLOY_INSTANCE` | empty | Labels shown as chips in the navbar and footer |
| `VITE_CONNECTION_TYPE` | `websocket` | `websocket` for live data, `http` for a snapshot refreshed every 30 s. The navbar switch overrides it per browser |

The dashboard always talks to the collector that served it, so there are
no API URL settings.

## Live and static modes

With a WebSocket connection the dashboard updates every second. In `http`
mode it loads `/system` and polls it every 30 seconds. The switch at the
top right of the navbar flips between the two, and the choice is
remembered in the browser.
