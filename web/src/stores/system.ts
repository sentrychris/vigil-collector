import type { SystemResponse, RealtimeSystemResponse } from "@/interfaces/SystemResponse";
import type { ConnectionTypes } from "@/interfaces/types/ConnectionTypes";
import type { ProcessInformation } from "@/interfaces/SystemInformation";
import type { ProcessMetric } from "@/interfaces/types/SystemTypes";
import { defineStore } from "pinia";
import { useLoadingStore } from "./loading";
import { config } from "@/config";
import { diskThroughput, shownDisks } from "@/utilities/hardware";

/** How many readings the compact window's sparklines keep: a minute's
 *  worth at the collector's usual one a second. */
export const HISTORY_LENGTH = 60;

function remember(series: number[], value: number) {
  series.push(Number.isFinite(value) ? value : 0);
  if (series.length > HISTORY_LENGTH) series.splice(0, series.length - HISTORY_LENGTH);
}

export const useSystemStore = defineStore("system", {
  state: () => ({
    data: { disks: [] } as unknown as SystemResponse,
    realtime: <RealtimeSystemResponse>{
      cpu: {
        freq: 0,
        temp: 0,
        usage: 0,
        cores: 0,
      },
      gpu: null,
      mem: {
        total: 0,
        used: 0,
        free: 0,
        percent: 0,
      },
      disk: {
        total: 0,
        used: 0,
        free: 0,
        percent: 0,
      },
      disks: [],
      network: {
        rx_bytes_per_sec: 0,
        tx_bytes_per_sec: 0,
      },
      disk_io: {
        read_bytes_per_sec: 0,
        write_bytes_per_sec: 0,
        read_iops: 0,
        write_iops: 0,
      },
      uptime: null,
      processes: [],
      processes_by_cpu: [],
      processes_by_gpu: [],
    },
    // The last minute of live readings, oldest first, for sparklines.
    // Kept here rather than in a chart so it survives a change of view.
    history: {
      cpu: [] as number[],
      gpu: [] as number[],
      mem: [] as number[],
      diskIo: [] as number[],
      // Each shown disk's own reads and writes, by its mount point.
      disks: {} as Record<string, number[]>,
      rx: [] as number[],
      tx: [] as number[],
    },
    connection: <WebSocket | null>null,
    connectionType: <ConnectionTypes | null>null,
    live: false,
    connected: false,
    poll: <any>null,
  }),
  actions: {
    async connect({
      websocket = false,
      refresh = false,
    }: {
      websocket: boolean;
      refresh?: boolean;
    }) {
      this.connectionType = websocket ? "websocket" : "http";
      if (!this.connected) {
        const loader = useLoadingStore();

        if (!refresh) {
          loader.toggle(false);
          loader.setMessage("Connecting to system monitor...");
        }

        this.http.get<SystemResponse>("system")
          .then((response) => {
            const { data } = response;
            this.staticUpdate(data);
            this.connected = true;
            if (websocket) {
              this.stopPollingApi();
              this.websocket();
            } else {
              // Static mode re-reads /system on a timer.
              this.startPollingApi(config.services.pollInterval);
              if (!refresh) loader.toggle(true);
            }
          })
          .catch(() => {
            if (!refresh) {
              loader.setError("An unexpected error has occurred");
            }
          });
      }
    },
    async websocket() {
      const loader = useLoadingStore();
      loader.setMessage("Opening websocket connection...");

      const response = await fetch(config.api.urls.worker, {
        method: "POST",
        body: JSON.stringify({ connection: "monitor" }),
      });

      const worker = await response.json();

      const url = `${config.api.urls.websocket}?id=${worker.id}`;

      this.connection = this.connection ?? new WebSocket(url);

      this.connection.onopen = () => {
        loader.setMessage("Websocket connected, loading dashboard...");
        setTimeout(() => {
          loader.toggle(true);
          this.live = true;
        }, 1000);
      };
      this.connection.onerror = () => {
        loader.toggle(false);
        loader.setMessage("Error! Unable to connect to websocket...");
      };
      this.connection.onmessage = (response) => {
        try {
          const data = JSON.parse(response.data) as RealtimeSystemResponse;
          this.liveUpdate(data);
        } catch (error) {
          console.log({ error });
        }
      };
      this.connection.onclose = () => {
        this.live = false;
        this.connection = null;
        console.log("websocket connection closed");
      };
    },
    async startPollingApi(interval: number = 30000) {
      if (!this.poll) {
        this.poll = setInterval(() => {
          this.refresh(false);
        }, interval);
      }
    },
    async stopPollingApi() {
      if (this.poll) {
        clearInterval(this.poll);
      }
      this.poll = null;
    },
    async toggle() {
      const type = this.connectionType === "websocket" ? "http" : "websocket";

      this.setConnectionType(type);
      this.refresh(this.connectionType === "websocket");
    },
    async refresh(websocket: boolean) {
      if (this.connection instanceof WebSocket) {
        this.connection.close();
      }

      this.connected = false;
      this.connect({ websocket, refresh: true });
    },
    async reconnect() {
      if (this.connection instanceof WebSocket) {
        this.connection.close();
        this.connect({ websocket: true });
      }
    },
    staticUpdate(data: SystemResponse) {
      this.$patch({ data });
    },
    liveUpdate(realtime: RealtimeSystemResponse) {
      this.$patch({ realtime });
      this.realtime.processes = realtime.processes;
      this.realtime.processes_by_cpu = realtime.processes_by_cpu ?? [];
      this.realtime.processes_by_gpu = realtime.processes_by_gpu ?? [];
      this.realtime.disks = realtime.disks ?? [];
      remember(this.history.cpu, realtime.cpu?.usage);
      remember(this.history.gpu, realtime.gpu?.usage ?? 0);
      remember(this.history.mem, realtime.mem?.percent);
      remember(
        this.history.diskIo,
        (realtime.disk_io?.read_bytes_per_sec ?? 0) + (realtime.disk_io?.write_bytes_per_sec ?? 0),
      );
      // A disk keeps its line while it is listed; one that is taken away
      // loses it, and starts again if it comes back.
      const shown = shownDisks(realtime.disks);
      const disks: Record<string, number[]> = {};
      for (const disk of shown) {
        const series = this.history.disks[disk.mountpoint] ?? [];
        remember(series, diskThroughput(disk, shown, realtime.disk_io) ?? 0);
        disks[disk.mountpoint] = series;
      }
      this.history.disks = disks;
      remember(this.history.rx, realtime.network?.rx_bytes_per_sec ?? 0);
      remember(this.history.tx, realtime.network?.tx_bytes_per_sec ?? 0);
    },
    setConnectionType(type: ConnectionTypes) {
      this.connectionType = type;
    },
    seriesToColor(str: string) {
      const map = {
        mysqld: "#00758f",
        "next-server (v14.2.6)": "#d9534f",
        "php-fpm8.2": "#8892bf",
        node: "#5cb85c",
        "PM2 v5.4.2: God": "#5D3FD3",
        php: "#8892bf",
        "systemd-journald": "#f0ad4e",
        "npm start": "#5cb85c",
      };

      const colour = map[str as keyof typeof map];
      if (!colour) {
        let hash = 0;
        for (let i = 0; i < str.length; i++) {
          hash = str.charCodeAt(i) + ((hash << 5) - hash);
        }
        const hex = (hash & 0x00ffffff).toString(16).toUpperCase();
        return "#" + "00000".substring(0, 6 - hex.length) + hex;
      }

      return colour;
    },
    formatBarChartDataForSystem(series: Array<ProcessInformation>, key: ProcessMetric) {
      const response: Array<{ name: string; color: string; data: Array<number> }> = [];

      series.forEach((point) => {
        const dp = response.find((dp) => {
          return dp.name === point.name;
        });

        if (dp) {
          dp.data[0] += point.mem;
        } else {
          response.push({
            name: point.name,
            color: this.seriesToColor(point.name),
            data: [(<unknown>point[key]) as number],
          });
        }
      });

      return response;
    },
    formatPieChartDataForProcesses(series: Array<ProcessInformation>) {
      const response: Array<{ name: string; color: string; y: number }> = [];
      series.forEach((point) => {
        const dp = response.find((dp) => {
          return dp.name === point.name;
        });

        if (dp) {
          dp.y += point.mem;
        } else {
          response.push({
            name: point.name,
            color: this.seriesToColor(point.name),
            y: (<unknown>point.mem) as number,
          });
        }
      });

      return response;
    },
  },
  persist: {
    storage: localStorage,
    pick: ["connectionType"],
  },
});
