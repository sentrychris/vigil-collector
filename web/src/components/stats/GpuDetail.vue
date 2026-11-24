<script setup lang="ts">
import { computed } from "vue";
import type { GpuInformation } from "@/interfaces/SystemInformation";
import { mapTemperatureColor } from "@/utilities/color-map";
import { formatSize } from "@/utilities/format-size";
import { gpuModel } from "@/utilities/hardware";

const props = defineProps<{
  detail: GpuInformation;
}>();

// Used and total in one unit, the total's: "2.6 / 8.0 GiB".
const memory = computed(() => {
  const total = formatSize(props.detail.mem?.total, "GiB", 1);
  const scale = { MiB: 1 / 1024, GiB: 1, TiB: 1024 }[total.unit];
  const used = props.detail.mem?.used;
  return {
    used: typeof used === "number" && Number.isFinite(used) ? (used / scale).toFixed(1) : "–",
    total: total.value,
    unit: total.unit,
  };
});
</script>

<template>
  <div class="metric-block">
    <div class="metric-primary metric-num">
      <span class="metric-value">{{ detail.usage }}</span>
      <span class="metric-unit">%</span>
    </div>
    <div class="metric-label">GPU LOAD</div>
  </div>
  <div class="metric-rows">
    <div class="metric-row">
      <span class="row-label">MODEL</span>
      <!-- Without its maker, which the full name in the tooltip has. -->
      <span class="row-value mono row-name" :title="detail.name">{{ gpuModel(detail.name) }}</span>
    </div>
    <div class="metric-row">
      <span class="row-label">VRAM</span>
      <span class="row-value mono">{{ memory.used }} / {{ memory.total }} <span class="row-unit">{{ memory.unit }}</span></span>
    </div>
    <div class="metric-row">
      <span class="row-label">TEMP</span>
      <!-- 0 means the card's driver doesn't report one. -->
      <span v-if="detail.temp > 0" class="row-value mono">
        <span :style="{ color: mapTemperatureColor(detail.temp) }">{{ detail.temp.toFixed(1) }}</span>
        <span class="row-unit">°C</span>
      </span>
      <span v-else class="row-value mono row-na" title="No temperature sensor available">n/a</span>
    </div>
  </div>
</template>

<style scoped>
.metric-block { margin-bottom: 0.65rem; }
.metric-primary {
  font-feature-settings: "tnum";
  font-variant-numeric: tabular-nums;
  line-height: 1;
}
.metric-value {
  font-size: var(--fs-hero);
  font-weight: 400;
  color: #111827;
}
.metric-unit {
  font-size: var(--fs-body);
  color: #6b7280;
  font-weight: 500;
  margin-left: 4px;
}
.metric-label {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.18em;
  color: #6b7280;
  margin-top: 6px;
}
.row-value.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-caption);
}
.row-label {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro);
  letter-spacing: 0.12em;
}

.metric-rows {
  border-top: 1px solid rgba(15, 23, 42, 0.06);
  padding-top: 0.55rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.metric-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 0.75rem;
  font-size: var(--fs-caption);
}
/* A part's name: cut short rather than wrapped where the card is narrow. */
.row-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row-label { color: #6b7280; }
.row-value {
  color: #1f2937;
  font-weight: 600;
  font-feature-settings: "tnum";
  font-variant-numeric: tabular-nums;
}
.row-na { color: #9ca3af; }
.row-unit { color: #9ca3af; font-weight: 500; font-size: var(--fs-micro); margin-left: 3px; }

body[data-theme="dark"] .metric-value { color: var(--text-primary); }
body[data-theme="dark"] .metric-unit  { color: var(--text-muted); }
body[data-theme="dark"] .metric-label { color: var(--text-muted); }
body[data-theme="dark"] .metric-rows  { border-top-color: var(--border-subtle); }
body[data-theme="dark"] .row-label    { color: var(--text-muted); }
body[data-theme="dark"] .row-value    { color: var(--text-secondary); }
body[data-theme="dark"] .row-unit     { color: var(--text-dim); }
</style>
