/**
 * The service checks: what the collector's probes last found, asked for
 * again every half minute while a component is using them.
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { config } from "@/config";

export type ServiceStatusName = "pending" | "online" | "slow" | "offline";

interface ProbeResult {
  name: string;
  url: string;
  ok: boolean;
  status_code: number | null;
  latency_ms: number;
  error: string | null;
}

export interface ServiceState {
  name: string;
  url: string;
  status: ServiceStatusName;
  latency: number | null;
  statusCode: number | null;
  error: string | null;
  checkedAt: number | null;
}

export interface ServicesOverall {
  tone: "pending" | "operational" | "degraded" | "down";
  label: string;
}

const { pollInterval, thresholds } = config.services;
const probesUrl = `${config.api.urls.http.replace(/\/$/, "")}/probes`;

function classify(p: ProbeResult): ServiceStatusName {
  if (!p.ok) return "offline";
  return p.latency_ms >= thresholds.slow ? "slow" : "online";
}

export function serviceLabel(status: ServiceStatusName): string {
  return status === "online"
    ? "Online"
    : status === "slow"
      ? "Slow"
      : status === "offline"
        ? "Offline"
        : "Checking";
}

export function useProbes() {
  const state = ref<ServiceState[]>([]);
  const now = ref(Date.now());

  async function refresh() {
    try {
      const response = await fetch(probesUrl, { cache: "no-store" });
      if (!response.ok) throw new Error(`probes endpoint returned ${response.status}`);
      const body = (await response.json()) as { probes: ProbeResult[] };
      state.value = body.probes.map((p) => ({
        name: p.name,
        url: p.url,
        status: classify(p),
        latency: p.latency_ms,
        statusCode: p.status_code,
        error: p.error,
        checkedAt: Date.now(),
      }));
    } catch (err) {
      state.value = [
        {
          name: "probe service",
          url: probesUrl,
          status: "offline",
          latency: null,
          statusCode: null,
          error: err instanceof Error ? err.message : "unreachable",
          checkedAt: Date.now(),
        },
      ];
    }
  }

  const overall = computed<ServicesOverall>(() => {
    if (state.value.length === 0 || state.value.every((s) => s.status === "pending")) {
      return { tone: "pending", label: "Checking" };
    }
    const offline = state.value.filter((s) => s.status === "offline").length;
    const slow = state.value.filter((s) => s.status === "slow").length;
    if (offline > 0) {
      return {
        tone: "down",
        label: offline === state.value.length ? "All Down" : `${offline} Down`,
      };
    }
    if (slow > 0) return { tone: "degraded", label: "Degraded" };
    return { tone: "operational", label: "All Operational" };
  });

  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let clockTimer: ReturnType<typeof setInterval> | null = null;

  onMounted(() => {
    refresh();
    pollTimer = setInterval(refresh, pollInterval);
    clockTimer = setInterval(() => (now.value = Date.now()), 1000);
  });

  onBeforeUnmount(() => {
    if (pollTimer) clearInterval(pollTimer);
    if (clockTimer) clearInterval(clockTimer);
  });

  return { state, overall, now };
}
