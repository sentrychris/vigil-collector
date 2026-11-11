<script setup lang="ts">
import { computed } from "vue";
import type { PlatformInformation } from "@/interfaces/SystemInformation";

const props = defineProps<{
  detail: PlatformInformation;
  uptime?: string | null;
}>();

const platformLogo = computed(() => {
  const isWindows = /windows/i.test(props.detail?.distro ?? "");
  const file = isWindows ? "windows.png" : "linux.png";
  return `${import.meta.env.BASE_URL}images/${file}`;
});
</script>

<template>
  <div class="metric-block">
    <div class="distro-row">
      <img
        class="platform-logo"
        :src="platformLogo"
        :alt="detail.distro || 'platform'"
      />
      <div class="distro" :title="detail.distro">{{ detail.distro || "—" }}</div>
    </div>
    <div class="metric-label">SYSTEM</div>
  </div>
  <div class="metric-rows">
    <div class="metric-row">
      <span class="row-label">KERNEL</span>
      <span class="row-value mono">{{ detail.kernel || "—" }}</span>
    </div>
    <div class="metric-row">
      <span class="row-label">UPTIME</span>
      <span class="row-value">{{ uptime ?? detail.uptime ?? "—" }}</span>
    </div>
  </div>
</template>

<style scoped>
.metric-block { margin-bottom: 0.65rem; }
.distro-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
}
.platform-logo {
  width: 20px;
  height: 20px;
  object-fit: contain;
  flex-shrink: 0;
}
.distro {
  font-size: var(--fs-heading);
  font-weight: 400;
  color: #111827;
  line-height: 1.2;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
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
.row-label {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro);
  letter-spacing: 0.12em;
}

.metric-rows {
  border-top: 1px solid rgba(15, 23, 42, 0.06);
  padding-top: 0.55rem;
  margin-top: 0.55rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.metric-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  font-size: var(--fs-caption);
  gap: 0.5rem;
}
.row-label { color: #6b7280; flex-shrink: 0; }
.row-value {
  color: #1f2937;
  font-weight: 600;
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}
.row-value.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-caption);
}

body[data-theme="dark"] .distro       { color: var(--text-primary); }
body[data-theme="dark"] .metric-label { color: var(--text-muted); }
body[data-theme="dark"] .metric-rows  { border-top-color: var(--border-subtle); }
body[data-theme="dark"] .row-label    { color: var(--text-muted); }
body[data-theme="dark"] .row-value    { color: var(--text-secondary); }
</style>
