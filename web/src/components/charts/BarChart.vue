<script setup lang="ts">
import { formatSize } from "@/utilities/format-size";
import { computed, ref, watch, onMounted, onUnmounted } from "vue";
import { tween } from "@/utilities/frame-loop";

type Datum = { name: string; color: string; data: number[] };

const valueOf = (d: Datum, key: string): number => {
  if (key === "data") return d.data?.[0] ?? 0;
  return Number((d as Record<string, unknown>)[key] ?? 0);
};

const props = withDefaults(
  defineProps<{
    metric?: string;
    id: string;
    title?: string;
    series: Datum[];
    legend?: boolean;
    yAxisText?: string;
    xAxisText?: string;
    sortKey?: string;
    sortOrder?: "asc" | "desc";
    topN?: number;
    unit?: string;
  }>(),
  { topN: 10, unit: "MiB", sortKey: "data", sortOrder: "desc" },
);

const sorted = computed<Datum[]>(() => {
  const dir = props.sortOrder === "asc" ? 1 : -1;
  const key = props.sortKey ?? "data";
  return [...(props.series ?? [])]
    .sort((a, b) => (valueOf(a, key) - valueOf(b, key)) * dir)
    .slice(0, props.topN);
});

const max = computed(() => {
  const m = Math.max(...sorted.value.map((s) => s.data?.[0] ?? 0), 1);
  return m;
});

const progress = ref(0);
let stopTween: (() => void) | null = null;

const runIntro = () => {
  stopTween?.();
  progress.value = 0;
  stopTween = tween(800, (t) => {
    progress.value = 1 - Math.pow(1 - t, 3);
  });
};

onMounted(() => runIntro());
onUnmounted(() => stopTween?.());

watch(
  () => props.series?.map((s) => s.name).join("|"),
  () => runIntro(),
);

const hexToRgba = (hex: string, alpha: number) => {
  const h = (hex || "#888888").replace("#", "");
  const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
  const v = parseInt(full, 16);
  return `rgba(${(v >> 16) & 255},${(v >> 8) & 255},${v & 255},${alpha})`;
};

// Process memory arrives in MiB; larger values step up to GiB / TiB.
const fmt = (n: number) => {
  const f = formatSize(n, "MiB");
  if (f.unit === "MiB") return n.toFixed(n < 10 ? 1 : 0) + " MiB";
  const v = Number(f.value);
  return v.toFixed(v < 10 ? 2 : 1).replace(/\.?0+$/, "") + " " + f.unit;
};

const hovered = ref<string | null>(null);
</script>

<template>
  <div class="bar-rank" :id="id">
    <div
      v-for="(s, i) in sorted"
      :key="s.name"
      class="bar-row"
      :class="{ dimmed: hovered && hovered !== s.name }"
      @mouseenter="hovered = s.name"
      @mouseleave="hovered = null"
    >
      <div class="bar-rank-num">{{ String(i + 1).padStart(2, "0") }}</div>
      <div class="bar-name" :title="s.name">{{ s.name }}</div>
      <div class="bar-track">
        <div
          class="bar-fill"
          :style="{
            width: `${((s.data?.[0] || 0) / max) * 100 * progress}%`,
            background: `linear-gradient(90deg, ${hexToRgba(s.color, 0.35)}, ${s.color})`,
            boxShadow: `0 0 12px ${hexToRgba(s.color, 0.45)}`,
          }"
        ></div>
        <div
          class="bar-end"
          :style="{
            left: `${((s.data?.[0] || 0) / max) * 100 * progress}%`,
            background: s.color,
            boxShadow: `0 0 8px ${hexToRgba(s.color, 0.7)}`,
            opacity: progress,
          }"
        ></div>
      </div>
      <div
        class="bar-value"
        :style="{ color: hovered === s.name ? s.color : undefined }"
      >
        {{ fmt(s.data?.[0] || 0) }}
      </div>
    </div>

    <div class="bar-empty" v-if="sorted.length === 0">no data</div>
  </div>
</template>

<style scoped>
.bar-rank {
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
  padding: 0.25rem 0.1rem;
}
.bar-row {
  display: grid;
  grid-template-columns: 1.6rem minmax(5rem, 9rem) 1fr 5.5rem;
  align-items: center;
  gap: 0.7rem;
  font-family: "Lato", system-ui, sans-serif;
  transition: opacity 0.2s ease;
}
.bar-row.dimmed {
  opacity: 0.4;
}
.bar-rank-num {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.65rem;
  color: #94a3b8;
  letter-spacing: 0.06em;
  text-align: right;
}
.bar-name {
  font-size: 0.72rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.1em;
  color: #475569;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bar-track {
  position: relative;
  height: 14px;
  background: rgba(15, 23, 42, 0.05);
  border-radius: 3px;
}
.bar-fill {
  height: 100%;
  border-radius: 3px;
  transition: width 0.45s cubic-bezier(0.22, 1, 0.36, 1);
}
.bar-end {
  position: absolute;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 6px;
  height: 18px;
  border-radius: 1px;
  transition: left 0.45s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.3s;
  pointer-events: none;
}
.bar-value {
  font-size: 1rem;
  font-weight: 300;
  text-align: right;
  font-variant-numeric: tabular-nums;
  color: #0f172a;
  letter-spacing: -0.01em;
  transition: color 0.2s ease;
}
.bar-empty {
  text-align: center;
  font-size: var(--fs-caption);
  color: #94a3b8;
  text-transform: uppercase;
  letter-spacing: 0.18em;
  padding: 1.5rem 0;
}

@media (max-width: 575.98px) {
  .bar-row {
    grid-template-columns: 1.4rem minmax(4rem, 7rem) 1fr 4.5rem;
    gap: 0.5rem;
  }
  .bar-value {
    font-size: 0.85rem;
  }
}

body[data-theme="dark"] .bar-rank-num {
  color: var(--text-muted);
}
body[data-theme="dark"] .bar-name {
  color: var(--text-secondary);
}
body[data-theme="dark"] .bar-value {
  color: var(--text-primary);
}
body[data-theme="dark"] .bar-track {
  background: rgba(148, 163, 184, 0.1);
}
</style>
