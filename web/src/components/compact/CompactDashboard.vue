<script setup lang="ts">
/**
 * The dashboard as the Vigil window shows it at its usual, small size: one
 * tight column made to be read at a glance and to fit without scrolling.
 * See useCompactLayout for when it is shown.
 */
import { computed } from "vue";
import { useSystemStore } from "@/stores/system";
import { formatSize } from "@/utilities/format-size";
import { formatRate } from "@/utilities/format-rate";
import { mapTemperatureColor } from "@/utilities/color-map";
import CompactCard from "./CompactCard.vue";
import CompactProcesses from "./CompactProcesses.vue";
import CompactServices from "./CompactServices.vue";
import SparkLine from "./SparkLine.vue";

const system = useSystemStore();

const now = computed(() => (system.live ? system.realtime : system.data));
const platform = computed(() => system.data.platform);

const platformLogo = computed(() => {
  const file = /windows/i.test(platform.value?.distro ?? "") ? "windows.png" : "linux.png";
  return `${import.meta.env.BASE_URL}images/${file}`;
});

// "1 day, 6 hrs, 6 mins, 4 secs" as "1d 6h 6m 4s".
const uptime = computed(() => {
  const text = system.realtime.uptime ?? platform.value?.uptime ?? "";
  return text
    .replace(/\s*days?/g, "d")
    .replace(/\s*hrs?/g, "h")
    .replace(/\s*mins?/g, "m")
    .replace(/\s*secs?/g, "s")
    .replace(/,/g, "");
});

const percent = (value: number | undefined, decimals: number) =>
  typeof value === "number" && Number.isFinite(value) ? value.toFixed(decimals) : "–";

const frequency = computed(() => {
  const mhz = now.value.cpu?.freq ?? 0;
  if (mhz <= 0) return null;
  return mhz >= 1000 ? `${(mhz / 1000).toFixed(2)} GHz` : `${Math.round(mhz)} MHz`;
});

// Used and total in one unit, the total's: "12.4 / 31.8 GiB".
function usage(detail: { used?: number; total?: number; free?: number } | undefined) {
  const total = formatSize(detail?.total, "GiB", 1);
  const scale = { MiB: 1 / 1024, GiB: 1, TiB: 1024 }[total.unit];
  const inTotalUnit = (gib: number | undefined) =>
    typeof gib === "number" && Number.isFinite(gib) ? (gib / scale).toFixed(1) : "–";
  return {
    used: inTotalUnit(detail?.used),
    total: total.value,
    unit: total.unit,
    free: inTotalUnit(detail?.free),
  };
}
const memory = computed(() => usage(now.value.mem));
const disk = computed(() => usage(now.value.disk));

const rate = (bytes: number | undefined) => formatRate(bytes ?? 0);
const diskIo = computed(() =>
  rate((now.value.disk_io?.read_bytes_per_sec ?? 0) + (now.value.disk_io?.write_bytes_per_sec ?? 0)),
);
const rx = computed(() => rate(now.value.network?.rx_bytes_per_sec));
const tx = computed(() => rate(now.value.network?.tx_bytes_per_sec));

const RX = "#22d3ee";
const TX = "#fb7185";
</script>

<template>
  <div class="compact-root">
    <CompactCard :title="platform?.distro || 'Platform'" tone="blue">
      <template #icon><img :src="platformLogo" alt="" /></template>
      <template #sub>
        <span class="k">Kernel</span> {{ platform?.kernel || "–" }}
      </template>
      <template #aside>
        <div class="aside-label">Uptime</div>
        <div class="aside-value">{{ uptime || "–" }}</div>
      </template>
    </CompactCard>

    <CompactCard
      title="CPU"
      icon="fa-solid fa-microchip"
      tone="purple"
      :value="percent(now.cpu?.usage, 1)"
      unit="%"
    >
      <template #sub>
        <template v-if="frequency">{{ frequency }} · </template>{{ now.cpu?.cores ?? "–" }} cores<template
          v-if="(now.cpu?.temp ?? 0) > 0"
        >
          ·
          <span :style="{ color: mapTemperatureColor(now.cpu.temp) }">{{ now.cpu.temp.toFixed(0) }} °C</span>
        </template>
      </template>
      <template #spark>
        <SparkLine
          :series="[{ values: system.history.cpu, color: '#c084fc' }]"
          :floor="25"
          label="CPU use over the last minute"
        />
      </template>
    </CompactCard>

    <CompactCard
      title="Memory"
      icon="fa-solid fa-memory"
      tone="amber"
      :value="percent(now.mem?.percent, 1)"
      unit="%"
      :bar="now.mem?.percent ?? 0"
    >
      <template #sub>
        {{ memory.used }} / {{ memory.total }} {{ memory.unit }} · {{ memory.free }} free
      </template>
      <template #spark>
        <SparkLine
          :series="[{ values: system.history.mem, color: '#fbbf24' }]"
          :max="100"
          label="Memory use over the last minute"
        />
      </template>
    </CompactCard>

    <CompactCard
      title="Disk"
      icon="fa-solid fa-hard-drive"
      tone="green"
      :value="percent(now.disk?.percent, 1)"
      unit="%"
      :bar="now.disk?.percent ?? 0"
    >
      <template #sub>
        {{ disk.used }} / {{ disk.total }} {{ disk.unit }} ·
        <span title="Reads and writes, all disks">{{ diskIo.value }} {{ diskIo.unit }}</span>
      </template>
      <template #spark>
        <SparkLine
          :series="[{ values: system.history.diskIo, color: '#34d399' }]"
          :floor="1024 * 1024"
          label="Disk reads and writes over the last minute"
        />
      </template>
    </CompactCard>

    <CompactCard title="Network" icon="fa-solid fa-tower-broadcast" tone="cyan">
      <template #sub>
        <span :style="{ color: RX }">↓ {{ rx.value }} {{ rx.unit }}</span>
        ·
        <span :style="{ color: TX }">↑ {{ tx.value }} {{ tx.unit }}</span>
      </template>
      <template #spark>
        <SparkLine
          :series="[
            { values: system.history.tx, color: TX },
            { values: system.history.rx, color: RX },
          ]"
          :floor="64 * 1024"
          label="Network traffic in and out over the last minute"
        />
      </template>
    </CompactCard>

    <CompactServices />
    <CompactProcesses />
  </div>
</template>

<style scoped>
.compact-root {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
  min-width: 0;
}
.k {
  color: var(--c-dim);
}
.aside-label {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 9.5px;
  font-weight: 600;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--c-dim);
  line-height: 1.15;
}
.aside-value {
  margin-top: 2px;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
  color: var(--c-text);
  font-variant-numeric: tabular-nums;
  line-height: 1.15;
}
</style>

<!-- Shared by the cards and panels under this root, which are separate
     components. -->
<style>
.compact-root {
  --c-surface: rgba(255, 255, 255, 0.78);
  --c-border: rgba(15, 23, 42, 0.1);
  --c-hair: rgba(15, 23, 42, 0.07);
  --c-track: rgba(15, 23, 42, 0.09);
  --c-text: #111827;
  --c-muted: #6b7280;
  --c-dim: #9ca3af;
  --c-accent: #2563eb;
  --c-ok: #059669;
  --c-warn: #d97706;
  --c-crit: #dc2626;
}
body[data-theme="dark"] .compact-root {
  --c-surface: rgba(15, 23, 42, 0.6);
  --c-border: rgba(148, 163, 184, 0.14);
  --c-hair: rgba(148, 163, 184, 0.09);
  --c-track: rgba(148, 163, 184, 0.14);
  --c-text: #f1f5f9;
  --c-muted: #94a3b8;
  --c-dim: #64748b;
  --c-accent: #60a5fa;
  --c-ok: #34d399;
  --c-warn: #fbbf24;
  --c-crit: #f87171;
}

.compact-root .compact-panel {
  border-radius: 9px;
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  overflow: hidden;
  min-width: 0;
}
.compact-root .panel-head {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 32px;
  padding: 0 10px;
  color: var(--c-text);
}
.compact-root .panel-icon {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: 6px;
  font-size: 10px;
  color: #34d399;
  background: rgba(16, 185, 129, 0.15);
  box-shadow: inset 0 0 0 1px rgba(16, 185, 129, 0.25);
}
.compact-root .panel-icon.tone-red {
  color: #f87171;
  background: rgba(239, 68, 68, 0.15);
  box-shadow: inset 0 0 0 1px rgba(239, 68, 68, 0.25);
}
.compact-root .panel-title {
  margin: 0;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.09em;
  line-height: 1;
  text-transform: uppercase;
  color: var(--c-text);
}
.compact-root .panel-link {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 0;
  border: 0;
  background: none;
  font-size: 10.5px;
  font-weight: 600;
  color: var(--c-accent);
  cursor: pointer;
}
.compact-root .panel-link:hover { text-decoration: underline; }
.compact-root .panel-link:focus-visible {
  outline: 2px solid var(--c-accent);
  outline-offset: 2px;
  border-radius: 3px;
}
.compact-root .panel-link svg { font-size: 9px; }
</style>
