<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { config } from "@/config";
import SectionHeader from "@/components/SectionHeader.vue";

type Status = "pending" | "online" | "slow" | "offline";

interface ProbeResult {
  name: string;
  url: string;
  ok: boolean;
  status_code: number | null;
  latency_ms: number;
  error: string | null;
}

interface ServiceState {
  name: string;
  url: string;
  status: Status;
  latency: number | null;
  statusCode: number | null;
  error: string | null;
  checkedAt: number | null;
}

const { pollInterval, thresholds } = config.services;
const probesUrl = `${config.api.urls.http.replace(/\/$/, "")}/probes`;

const LATENCY_SCALE_MS = 1500;

const state = ref<ServiceState[]>([]);
const now = ref(Date.now());

function classify(p: ProbeResult): Status {
  if (!p.ok) return "offline";
  return p.latency_ms >= thresholds.slow ? "slow" : "online";
}

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

const overall = computed(() => {
  if (state.value.length === 0 || state.value.every((s) => s.status === "pending")) {
    return { tone: "pending", label: "Checking" };
  }
  const offline = state.value.filter((s) => s.status === "offline").length;
  const slow = state.value.filter((s) => s.status === "slow").length;
  if (offline > 0) {
    return { tone: "down", label: offline === state.value.length ? "All Down" : `${offline} Down` };
  }
  if (slow > 0) return { tone: "degraded", label: "Degraded" };
  return { tone: "operational", label: "All Operational" };
});

function latencyPercent(ms: number | null): number {
  if (ms == null) return 0;
  return Math.min(100, Math.max(3, (ms / LATENCY_SCALE_MS) * 100));
}

function relTime(ts: number | null): string {
  if (ts == null) return "";
  const sec = Math.max(0, Math.floor((now.value - ts) / 1000));
  if (sec < 5) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  return `${hr}h ago`;
}

const labelFor = (s: Status) =>
  s === "online"
    ? "Online"
    : s === "slow"
      ? "Slow"
      : s === "offline"
        ? "Offline"
        : "Checking";

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
</script>

<template>
  <div class="card panel-card border-0 shadow-lg">
    <SectionHeader
      title="Service Status"
      :subtitle="`${state.length} monitored`"
      icon="fa-solid fa-heart-pulse"
      tone="red"
    >
      <template #right>
        <span class="overall-pill" :class="`tone-${overall.tone}`">
          <span class="overall-pill-dot"></span>
          {{ overall.label }}
        </span>
      </template>
    </SectionHeader>

    <div class="service-list">
      <div
        v-for="svc in state"
        :key="svc.url"
        class="service-row"
        :class="`is-${svc.status}`"
      >
        <span class="indicator-bar" :title="labelFor(svc.status)"></span>

        <div class="service-info">
          <div class="d-flex align-items-center gap-2 flex-wrap">
            <span class="status-dot" :class="`dot-${svc.status}`"></span>
            <span class="service-name">{{ svc.name }}</span>
            <span
              v-if="svc.statusCode != null"
              class="code-badge"
              :class="`code-${Math.floor(svc.statusCode / 100)}xx`"
              >{{ svc.statusCode }}</span
            >
            <span class="status-label">{{ labelFor(svc.status) }}</span>
          </div>
          <a
            :href="svc.url"
            target="_blank"
            rel="noopener noreferrer"
            class="service-url"
            >{{ svc.url }}</a
          >
        </div>

        <div class="service-metrics">
          <div class="latency-readout">
            <template v-if="svc.status === 'pending'">
              <span class="latency-pending">—</span>
            </template>
            <template v-else-if="svc.status === 'offline'">
              <span class="latency-error" :title="svc.error ?? 'no response'">
                {{ svc.error ?? "no response" }}
              </span>
            </template>
            <template v-else>
              <span class="latency-value">{{ svc.latency }}</span>
              <span class="latency-unit">ms</span>
            </template>
          </div>
          <div
            v-if="svc.status === 'online' || svc.status === 'slow'"
            class="latency-bar"
          >
            <div
              class="latency-bar-fill"
              :style="{ width: latencyPercent(svc.latency) + '%' }"
            ></div>
          </div>
          <div class="checked-at" v-if="svc.checkedAt">
            {{ relTime(svc.checkedAt) }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ---------- Card fills column height; rows distribute evenly ---------- */
.card.panel-card {
  display: flex;
  flex-direction: column;
}
.service-list {
  flex: 1 1 auto;
  display: flex;
  flex-direction: column;
  /* Container query context — rows restack based on the card's own width,
     not the viewport width, so the layout adapts whether the card is in a
     col-lg-4 slot or full-width on mobile. */
  container-type: inline-size;
  container-name: services;
}
.service-list .service-row {
  /* Basis = content size, so a row never shrinks below what it needs to
     render. flex-grow is still 1 so if the card is taller than the sum of
     content heights, the surplus is distributed evenly across rows. */
  flex: 1 1 auto;
}

/* Narrow card: stack the row vertically so info/metrics don't squeeze. */
@container services (max-width: 480px) {
  .service-list .service-row {
    /* Override the equal-share flex from the wide layout so each row sizes
       to its (now taller) content instead of clipping. */
    flex: 0 0 auto;
    flex-direction: column;
    align-items: stretch;
    gap: 0.6rem;
    padding: 0.85rem 1rem 0.85rem 1.25rem;
  }
  .service-metrics {
    text-align: left;
    min-width: 0;
  }
  .latency-bar {
    margin-left: 0;
    width: 100%;
  }
  .latency-value { font-size: var(--fs-heading); }
  .service-url { margin-left: 0; }
}

/* ---------- Overall pill (header right) ---------- */
.overall-pill {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.4rem 0.85rem;
  border-radius: 999px;
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
}
.overall-pill-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 10px currentColor;
}
.tone-operational { color: #34d399; }
.tone-degraded    { color: #fbbf24; }
.tone-down        { color: #f87171; }
.tone-pending     { color: #9ca3af; }

/* ---------- Rows ---------- */
.service-list {
  background: #ffffff;
}

.service-row {
  position: relative;
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 1rem 1.4rem 1rem 1.65rem;
  border-bottom: 1px solid rgba(15, 23, 42, 0.06);
  transition: background 0.18s ease;
}
.service-row:last-child { border-bottom: none; }
.service-row:hover { background: rgba(15, 23, 42, 0.02); }

.indicator-bar {
  position: absolute;
  left: 0; top: 0; bottom: 0;
  width: 4px;
  background: var(--indicator);
  transition: background 0.3s ease, box-shadow 0.3s ease;
}
.service-row.is-online   { --indicator: #10b981; }
.service-row.is-slow     { --indicator: #f59e0b; }
.service-row.is-offline  { --indicator: #ef4444; }
.service-row.is-pending  { --indicator: #9ca3af; }

.service-row.is-offline .indicator-bar {
  box-shadow: 0 0 18px rgba(239, 68, 68, 0.45);
}

.service-info { flex: 1 1 auto; min-width: 0; }

.status-dot {
  width: 10px; height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}
.dot-online {
  background: #10b981;
  box-shadow: 0 0 0 0 rgba(16, 185, 129, 0.55);
  animation: pulse-online 2.2s infinite;
}
.dot-slow {
  background: #f59e0b;
  animation: pulse-slow 1.6s infinite;
}
.dot-offline {
  background: #ef4444;
  box-shadow: 0 0 8px rgba(239, 68, 68, 0.55);
}
.dot-pending { background: #9ca3af; }

@keyframes pulse-online {
  0%   { box-shadow: 0 0 0 0   rgba(16, 185, 129, 0.55); }
  70%  { box-shadow: 0 0 0 10px rgba(16, 185, 129, 0); }
  100% { box-shadow: 0 0 0 0   rgba(16, 185, 129, 0); }
}
@keyframes pulse-slow {
  0%, 100% { opacity: 1; transform: scale(1); }
  50%      { opacity: 0.55; transform: scale(0.85); }
}

.service-name { font-weight: 600; font-size: var(--fs-body); color: #1f2937; }

.code-badge {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro); font-weight: 600;
  padding: 2px 7px; border-radius: 5px;
  letter-spacing: 0.04em;
}
.code-2xx { background: rgba(16, 185, 129, 0.12); color: #047857; }
.code-3xx { background: rgba(59, 130, 246, 0.12); color: #1d4ed8; }
.code-4xx { background: rgba(245, 158, 11, 0.14); color: #b45309; }
.code-5xx { background: rgba(239, 68, 68, 0.12);  color: #b91c1c; }

.status-label {
  font-size: var(--fs-micro); font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: #6b7280;
}
.is-online   .status-label { color: #047857; }
.is-slow     .status-label { color: #b45309; }
.is-offline  .status-label { color: #b91c1c; }

.service-url {
  display: inline-block;
  margin-top: 4px;
  margin-left: 18px;
  font-size: var(--fs-caption);
  color: #6b7280;
  text-decoration: none;
  word-break: break-all;
  transition: color 0.15s ease;
}
.service-url:hover { color: #2563eb; text-decoration: underline; }

.service-metrics { flex: 0 0 auto; text-align: right; min-width: 130px; }

.latency-readout {
  font-feature-settings: "tnum";
  font-variant-numeric: tabular-nums;
  line-height: 1;
}
.latency-value { font-size: var(--fs-display); font-weight: 400; color: #111827; }
.latency-unit { font-size: var(--fs-caption); color: #6b7280; font-weight: 500; margin-left: 3px; }
.latency-pending { font-size: var(--fs-heading); color: #9ca3af; font-weight: 600; }
.latency-error {
  font-size: var(--fs-caption); color: #b91c1c; font-weight: 500;
  display: inline-block; max-width: 180px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.latency-bar {
  margin-top: 8px; height: 4px; width: 130px;
  margin-left: auto;
  background: rgba(15, 23, 42, 0.06);
  border-radius: 999px; overflow: hidden;
}
.latency-bar-fill {
  height: 100%; border-radius: 999px;
  transition: width 0.5s ease, background 0.3s ease;
}
.is-online .latency-bar-fill { background: linear-gradient(90deg, #10b981, #34d399); }
.is-slow   .latency-bar-fill { background: linear-gradient(90deg, #f59e0b, #fbbf24); }

.checked-at {
  margin-top: 6px;
  font-size: var(--fs-micro);
  color: #9ca3af;
  letter-spacing: 0.04em;
}

@media (max-width: 575.98px) {
  .service-row { padding: 0.85rem 1rem 0.85rem 1.25rem; gap: 0.75rem; }
  .service-metrics { min-width: 100px; }
  .latency-bar { width: 100px; }
  .latency-value { font-size: var(--fs-heading); }
}

/* ---------- Dark theme ---------- */
body[data-theme="dark"] .service-list { background: var(--bg-surface); }
body[data-theme="dark"] .service-row { border-bottom-color: var(--border-subtle); }
body[data-theme="dark"] .service-row:hover { background: var(--bg-hover); }
body[data-theme="dark"] .service-name { color: var(--text-primary); }
body[data-theme="dark"] .service-url { color: var(--text-muted); }
body[data-theme="dark"] .service-url:hover { color: var(--accent-blue); }

body[data-theme="dark"] .status-label              { color: var(--text-muted); }
body[data-theme="dark"] .is-online   .status-label { color: var(--status-ok); }
body[data-theme="dark"] .is-slow     .status-label { color: var(--status-warn); }
body[data-theme="dark"] .is-offline  .status-label { color: var(--status-crit); }

body[data-theme="dark"] .code-2xx { background: rgba(52, 211, 153, 0.16); color: var(--status-ok); }
body[data-theme="dark"] .code-3xx { background: rgba(96, 165, 250, 0.18); color: var(--accent-blue); }
body[data-theme="dark"] .code-4xx { background: rgba(251, 191, 36, 0.18); color: var(--status-warn); }
body[data-theme="dark"] .code-5xx { background: rgba(248, 113, 113, 0.18); color: var(--status-crit); }

body[data-theme="dark"] .latency-value   { color: var(--text-primary); }
body[data-theme="dark"] .latency-unit    { color: var(--text-muted); }
body[data-theme="dark"] .latency-pending { color: var(--text-dim); }
body[data-theme="dark"] .latency-error   { color: var(--status-crit); }
body[data-theme="dark"] .latency-bar     { background: var(--border-default); }
body[data-theme="dark"] .checked-at      { color: var(--text-dim); }

body[data-theme="dark"] .service-row.is-offline .indicator-bar {
  box-shadow: 0 0 22px rgba(248, 113, 113, 0.5);
}
body[data-theme="dark"] .dot-online {
  box-shadow: 0 0 0 0 rgba(52, 211, 153, 0.55);
  animation: pulse-online-dark 2.2s infinite;
}
@keyframes pulse-online-dark {
  0%   { box-shadow: 0 0 0 0   rgba(52, 211, 153, 0.55); }
  70%  { box-shadow: 0 0 0 10px rgba(52, 211, 153, 0); }
  100% { box-shadow: 0 0 0 0   rgba(52, 211, 153, 0); }
}
</style>
