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

  return {
    plugins: [vue()],
    server: {
      port: 8888,
      proxy: {
        ...Object.fromEntries(api.map((path) => [path, collector])),
        "/connect": { target: collector, ws: true },
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
