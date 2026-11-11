<script setup lang="ts">
import { useLoadingStore } from "@/stores/loading";

withDefaults(
  defineProps<{
    title: string;
    icon?: string;
    tone?: "blue" | "purple" | "amber" | "green" | "red" | "slate";
  }>(),
  {
    tone: "blue",
  },
);

const loader = useLoadingStore();
</script>

<template>
  <div class="card stat-card panel-card border-0 shadow-lg flex-fill">
    <div class="stat-header">
      <div class="d-flex align-items-center gap-3">
        <div v-if="icon" class="stat-icon" :class="`tone-${tone}`">
          <font-awesome-icon :icon="icon" />
        </div>
        <div class="stat-title">{{ title }}</div>
      </div>
    </div>
    <div class="stat-body">
      <slot v-if="loader.loaded" name="detail" />
      <div v-else class="stat-loading">Loading…</div>
    </div>
  </div>
</template>

<style scoped>
.stat-card {
  display: flex;
  flex-direction: column;
}

.stat-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.55rem 0.9rem;
  background: linear-gradient(135deg, #111827 0%, #1e293b 100%);
  color: #f1f5f9;
  position: relative;
  overflow: hidden;
  border-bottom: 1px solid rgba(96, 165, 250, 0.22);
}
.stat-header::before {
  content: "";
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(96, 165, 250, 0.04) 1px, transparent 1px),
    linear-gradient(90deg, rgba(96, 165, 250, 0.04) 1px, transparent 1px);
  background-size: 28px 28px;
  pointer-events: none;
  opacity: 0.5;
}
.stat-header::after {
  content: "";
  position: absolute;
  inset: 0;
  background:
    radial-gradient(500px circle at 0% 0%, rgba(59, 130, 246, 0.12), transparent 50%),
    radial-gradient(400px circle at 100% 100%, rgba(34, 211, 238, 0.08), transparent 50%);
  pointer-events: none;
}
.stat-header > * { position: relative; z-index: 1; }

.stat-icon {
  width: 26px;
  height: 26px;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--fs-caption);
  flex-shrink: 0;
}
.stat-icon.tone-blue   { background: rgba(59, 130, 246, 0.15);  color: #60a5fa; box-shadow: inset 0 0 0 1px rgba(59,130,246,0.25); }
.stat-icon.tone-purple { background: rgba(168, 85, 247, 0.15);  color: #c084fc; box-shadow: inset 0 0 0 1px rgba(168,85,247,0.25); }
.stat-icon.tone-amber  { background: rgba(245, 158, 11, 0.15);  color: #fbbf24; box-shadow: inset 0 0 0 1px rgba(245,158,11,0.25); }
.stat-icon.tone-green  { background: rgba(16, 185, 129, 0.15);  color: #34d399; box-shadow: inset 0 0 0 1px rgba(16,185,129,0.25); }
.stat-icon.tone-red    { background: rgba(239, 68, 68, 0.15);   color: #f87171; box-shadow: inset 0 0 0 1px rgba(239,68,68,0.25); }
.stat-icon.tone-slate  { background: rgba(148, 163, 184, 0.12); color: #cbd5e1; box-shadow: inset 0 0 0 1px rgba(148,163,184,0.2); }

.stat-title {
  font-family: "IBM Plex Sans", "Inter", system-ui, sans-serif;
  font-weight: 600;
  font-size: var(--fs-body);
  letter-spacing: 0.1em;
  text-transform: uppercase;
}

.stat-body {
  flex: 1;
  padding: 0.7rem 0.95rem 0.85rem;
  background: #ffffff;
}
@media (max-width: 575.98px) {
  .stat-header { padding: 0.45rem 0.75rem; }
  .stat-icon { width: 24px; height: 24px; font-size: var(--fs-micro); }
  .stat-title { font-size: var(--fs-caption); }
  .stat-body { padding: 0.6rem 0.8rem 0.7rem; }
}

.stat-loading {
  color: #9ca3af;
  font-size: var(--fs-caption);
}

body[data-theme="dark"] .stat-body {
  background: var(--bg-surface);
}
body[data-theme="dark"] .stat-loading {
  color: var(--text-dim);
}
</style>
