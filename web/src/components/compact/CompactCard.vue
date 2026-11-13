<script setup lang="ts">
/**
 * One row of the compact dashboard: what it is on the left, how it has
 * been lately in the middle, what it reads now on the right.
 */
withDefaults(
  defineProps<{
    title: string;
    icon?: string;
    tone?: "blue" | "purple" | "amber" | "green" | "cyan" | "red" | "slate";
    /** The reading, already formatted, and its unit. */
    value?: string;
    unit?: string;
    /** A share from 0 to 100, drawn as a thin bar along the bottom. */
    bar?: number;
  }>(),
  { icon: undefined, tone: "blue", value: undefined, unit: undefined, bar: undefined },
);
</script>

<template>
  <section class="compact-card" :class="`tone-${tone}`">
    <div class="card-row">
      <div class="card-icon">
        <slot name="icon">
          <font-awesome-icon v-if="icon" :icon="icon" />
        </slot>
      </div>
      <div class="card-text">
        <div class="card-title" :title="title">{{ title }}</div>
        <div v-if="$slots.sub" class="card-sub"><slot name="sub" /></div>
      </div>
      <div v-if="$slots.spark" class="card-spark" :class="{ wide: value === undefined }">
        <slot name="spark" />
      </div>
      <div v-if="value !== undefined" class="card-value">
        <span class="value-number">{{ value }}</span>
        <span v-if="unit" class="value-unit">{{ unit }}</span>
      </div>
      <div v-else-if="$slots.aside" class="card-aside"><slot name="aside" /></div>
    </div>
    <div
      v-if="bar !== undefined"
      class="card-bar"
      role="progressbar"
      :aria-label="`${title} used`"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="Math.round(bar)"
    >
      <div class="card-bar-fill" :style="{ width: Math.min(100, Math.max(0, bar)) + '%' }"></div>
    </div>
  </section>
</template>

<style scoped>
.compact-card {
  --tone: #60a5fa;
  --tone-soft: rgba(59, 130, 246, 0.15);
  --tone-edge: rgba(59, 130, 246, 0.25);
  padding: 7px 10px;
  border-radius: 9px;
  background: var(--c-surface);
  border: 1px solid var(--c-border);
  min-width: 0;
}
.tone-purple { --tone: #c084fc; --tone-soft: rgba(168, 85, 247, 0.15); --tone-edge: rgba(168, 85, 247, 0.25); }
.tone-amber  { --tone: #fbbf24; --tone-soft: rgba(245, 158, 11, 0.15); --tone-edge: rgba(245, 158, 11, 0.25); }
.tone-green  { --tone: #34d399; --tone-soft: rgba(16, 185, 129, 0.15); --tone-edge: rgba(16, 185, 129, 0.25); }
.tone-cyan   { --tone: #22d3ee; --tone-soft: rgba(34, 211, 238, 0.14); --tone-edge: rgba(34, 211, 238, 0.25); }
.tone-red    { --tone: #f87171; --tone-soft: rgba(239, 68, 68, 0.15);  --tone-edge: rgba(239, 68, 68, 0.25); }
.tone-slate  { --tone: #cbd5e1; --tone-soft: rgba(148, 163, 184, 0.12); --tone-edge: rgba(148, 163, 184, 0.2); }

.card-row {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
  height: 30px;
}
.card-icon {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: 7px;
  font-size: 12px;
  color: var(--tone);
  background: var(--tone-soft);
  box-shadow: inset 0 0 0 1px var(--tone-edge);
}
.card-icon :deep(img) {
  width: 15px;
  height: 15px;
  object-fit: contain;
}
.card-text {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 2px;
}
.card-title {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.09em;
  line-height: 1.15;
  text-transform: uppercase;
  color: var(--c-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.card-sub {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 10.5px;
  line-height: 1.15;
  color: var(--c-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* Wider in a wider window, and the first thing to give way in a narrow
   one: the figures beside it matter more than its length. */
.card-spark {
  flex: 0 1000 clamp(96px, 26%, 320px);
  min-width: 44px;
  height: 26px;
}
.card-spark.wide { flex-basis: clamp(170px, 42%, 480px); }
.card-value {
  flex: 0 0 auto;
  min-width: 62px;
  text-align: right;
  white-space: nowrap;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}
.value-number {
  font-family: "Lato", system-ui, sans-serif;
  font-size: 19px;
  font-weight: 300;
  letter-spacing: -0.01em;
  color: var(--c-text);
}
.value-unit {
  margin-left: 3px;
  font-size: 10px;
  font-weight: 500;
  color: var(--c-muted);
}
.card-aside {
  flex: 0 0 auto;
  text-align: right;
  white-space: nowrap;
}

.card-bar {
  height: 3px;
  margin-top: 6px;
  border-radius: 999px;
  background: var(--c-track);
  overflow: hidden;
}
.card-bar-fill {
  height: 100%;
  border-radius: inherit;
  background: var(--tone);
}
</style>
