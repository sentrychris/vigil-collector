import { fileURLToPath, URL } from "node:url";

import { defineConfig, loadEnv } from "vite";
import vue from "@vitejs/plugin-vue";

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => {
  // ``npm run dev`` serves the dashboard on :8888 and forwards the API to a
  // collector, by default one running locally on its default port.
  const env = loadEnv(mode, process.cwd(), "");
  const collector = env.VIGIL_COLLECTOR_URL || "http://127.0.0.1:4500";
  const api = ["/system", "/network", "/probes", "/worker"];

  // The Settings page talks to the collector's private listener, which only
  // the Vigil window normally reaches. To work on it, start a collector with
  // VIGIL_CONTROL_PORT and VIGIL_CONTROL_TOKEN set and give the same two to
  // ``npm run dev``; the token is added here so the page needs no session.
  const control =
    env.VIGIL_CONTROL_PORT && env.VIGIL_CONTROL_TOKEN
      ? {
          "/control": {
            target: `http://127.0.0.1:${env.VIGIL_CONTROL_PORT}`,
            changeOrigin: true,
            headers: { authorization: `Bearer ${env.VIGIL_CONTROL_TOKEN}` },
          },
        }
      : {};

  return {
    plugins: [vue()],
    server: {
      port: 8888,
      proxy: {
        ...Object.fromEntries(api.map((path) => [path, collector])),
        "/connect": { target: collector, ws: true },
        ...control,
      },
    },
    build: {
      // Embedded into the collector binary; keep the output small and
      // without source maps.
      sourcemap: false,
      chunkSizeWarningLimit: 1500,
    },
    resolve: {
      alias: [
        {
          find: "@",
          replacement: fileURLToPath(new URL("./src", import.meta.url)),
        },
        {
          find: /^~(.*)$/,
          replacement: "$1",
        },
      ],
    },
  };
});
