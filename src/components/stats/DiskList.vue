<script setup lang="ts">
import type { DiskUsageInformation } from "@/interfaces/SystemInformation";
import { formatSize } from "@/utilities/format-size";

defineProps<{
  disks: DiskUsageInformation[];
}>();

const size = (gib: number) => formatSize(gib, "GiB", 1);
</script>

<template>
  <div class="disk-list">
    <div v-for="disk in disks" :key="disk.mountpoint" class="disk-row">
      <div class="disk-head">
        <div class="disk-meta">
          <span class="disk-mount">{{ disk.mountpoint }}</span>
          <span class="disk-sub">
            <span class="disk-device">{{ disk.device }}</span>
            <span class="disk-fs">{{ disk.fstype }}</span>
          </span>
        </div>
        <div class="disk-usage">
          <span class="used">{{ size(disk.used).value }}<span
            v-if="size(disk.used).unit !== size(disk.total).unit"
            class="unit"
          > {{ size(disk.used).unit }}</span></span>
          <span class="separator"> / </span>
          <span class="total">{{ size(disk.total).value }}<span class="unit"> {{ size(disk.total).unit }}</span></span>
          <span
            class="percent"
            :class="{
              'is-warn': disk.percent >= 75 && disk.percent < 90,
              'is-crit': disk.percent >= 90,
            }"
          >{{ disk.percent.toFixed(1) }}%</span>
        </div>
      </div>
      <div class="disk-bar">
        <div
          class="disk-bar-fill"
          :class="{
            'is-warn': disk.percent >= 75 && disk.percent < 90,
            'is-crit': disk.percent >= 90,
          }"
          :style="{ width: Math.min(100, Math.max(2, disk.percent)) + '%' }"
        ></div>
      </div>
    </div>
    <div v-if="!disks.length" class="disk-empty">
      No partitions reported.
    </div>
  </div>
</template>

<style scoped>
.disk-list {
  display: flex;
  flex-direction: column;
  gap: 1.1rem;
}

.disk-row {
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
}

.disk-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 1rem;
  flex-wrap: wrap;
}

.disk-meta {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
}

.disk-mount {
  font-family: "Lato", system-ui, sans-serif;
  font-size: var(--fs-body);
  font-weight: 600;
  color: #111827;
  letter-spacing: 0.01em;
  word-break: break-all;
}

.disk-sub {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro);
  color: #6b7280;
  letter-spacing: 0.04em;
}

.disk-device { word-break: break-all; }

.disk-fs {
  display: inline-block;
  padding: 1px 7px;
  border-radius: 999px;
  background: rgba(15, 23, 42, 0.06);
  color: #4b5563;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

.disk-usage {
  display: inline-flex;
  align-items: baseline;
  gap: 0.4rem;
  font-feature-settings: "tnum";
  font-variant-numeric: tabular-nums;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-caption);
  color: #1f2937;
}
.disk-usage .used { font-weight: 600; }
.disk-usage .separator { color: #9ca3af; }
.disk-usage .total { color: #4b5563; }
.disk-usage .unit { color: #9ca3af; font-size: var(--fs-micro); margin-left: 2px; }

.disk-usage .percent {
  margin-left: 0.55rem;
  padding: 1px 8px;
  border-radius: 999px;
  background: linear-gradient(90deg, rgba(16, 185, 129, 0.18), rgba(52, 211, 153, 0.22));
  color: #047857;
  font-size: var(--fs-micro);
  font-weight: 700;
  font-family: "Lato", system-ui, sans-serif;
}
.disk-usage .percent.is-warn {
  background: linear-gradient(90deg, rgba(245, 158, 11, 0.18), rgba(251, 191, 36, 0.22));
  color: #b45309;
}
.disk-usage .percent.is-crit {
  background: linear-gradient(90deg, rgba(239, 68, 68, 0.2), rgba(248, 113, 113, 0.24));
  color: #b91c1c;
}

.disk-bar {
  height: 6px;
  background: rgba(15, 23, 42, 0.06);
  border-radius: 999px;
  overflow: hidden;
  position: relative;
}
.disk-bar-fill {
  height: 100%;
  background: linear-gradient(90deg, #10b981, #34d399);
  border-radius: 999px;
  transition: width 0.6s ease, background 0.3s ease, box-shadow 0.3s ease;
  box-shadow: 0 0 8px rgba(52, 211, 153, 0.45);
}
.disk-bar-fill.is-warn {
  background: linear-gradient(90deg, #f59e0b, #fbbf24);
  box-shadow: 0 0 10px rgba(251, 191, 36, 0.55);
}
.disk-bar-fill.is-crit {
  background: linear-gradient(90deg, #ef4444, #f87171);
  box-shadow: 0 0 14px rgba(248, 113, 113, 0.65);
}

.disk-empty {
  font-family: "Lato", system-ui, sans-serif;
  font-size: var(--fs-caption);
  color: #6b7280;
  text-align: center;
  padding: 1rem 0;
}

body[data-theme="dark"] .disk-mount { color: var(--text-primary); }
body[data-theme="dark"] .disk-sub   { color: var(--text-muted); }
body[data-theme="dark"] .disk-fs    {
  background: var(--border-default);
  color: var(--text-secondary);
}
body[data-theme="dark"] .disk-usage          { color: var(--text-secondary); }
body[data-theme="dark"] .disk-usage .total   { color: var(--text-muted); }
body[data-theme="dark"] .disk-usage .unit    { color: var(--text-dim); }
body[data-theme="dark"] .disk-usage .separator { color: var(--text-dim); }
body[data-theme="dark"] .disk-bar    { background: var(--border-default); }
body[data-theme="dark"] .disk-empty  { color: var(--text-muted); }
</style>
