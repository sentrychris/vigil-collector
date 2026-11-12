<script setup lang="ts">
import { formatSize } from "@/utilities/format-size";
import { computed, ref, watch, onMounted, onUnmounted } from "vue";
import { tween } from "@/utilities/frame-loop";

type Slice = { name: string; color: string; y: number };

const props = withDefaults(
  defineProps<{
    id: string;
    title?: string;
    series: Slice[];
    unit?: string;
  }>(),
  { unit: "MiB" },
);

const SIZE = 220;
const STROKE = 22;
const RADIUS = (SIZE - STROKE) / 2 - 6;
const CENTER = SIZE / 2;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const GAP_PX = 2;

const sorted = computed(() =>
  [...(props.series ?? [])].sort((a, b) => b.y - a.y),
);

const segments = computed<Slice[]>(() => {
  const arr = sorted.value;
  if (arr.length <= 7) return arr;
  const top = arr.slice(0, 6);
  const rest = arr.slice(6);
  const otherTotal = rest.reduce((s, x) => s + x.y, 0);
  return [...top, { name: "Other", color: "#94a3b8", y: otherTotal }];
});

const total = computed(() =>
  segments.value.reduce((sum, s) => sum + s.y, 0),
);

type Arc = Slice & {
  fraction: number;
  segLength: number;
  dashOffset: number;
};

const arcs = computed<Arc[]>(() => {
  let cumulative = 0;
  return segments.value.map((s) => {
    const fraction = total.value > 0 ? s.y / total.value : 0;
    const dashOffset = -cumulative * CIRCUMFERENCE;
    cumulative += fraction;
    const segLength = Math.max(0, fraction * CIRCUMFERENCE - GAP_PX);
    return { ...s, fraction, segLength, dashOffset };
  });
});

const progress = ref(0);
let stopTween: (() => void) | null = null;

const runIntro = () => {
  stopTween?.();
  progress.value = 0;
  stopTween = tween(900, (t) => {
    progress.value = 1 - Math.pow(1 - t, 3);
  });
};

onMounted(() => runIntro());
onUnmounted(() => stopTween?.());

watch(
  () => (props.series ?? []).map((s) => s.name).join("|"),
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
  if (f.unit === "MiB") return n.toFixed(0) + " MiB";
  const v = Number(f.value);
  return v.toFixed(v < 10 ? 2 : 1).replace(/\.?0+$/, "") + " " + f.unit;
};

const hovered = ref<string | null>(null);
const activeArc = computed(
  () => arcs.value.find((a) => a.name === hovered.value) ?? null,
);
</script>

<template>
  <div class="donut-wrap" :id="id">
    <div class="donut-chart">
      <svg
        :viewBox="`0 0 ${SIZE} ${SIZE}`"
        preserveAspectRatio="xMidYMid meet"
        class="donut-svg"
      >
        <circle
          :cx="CENTER"
          :cy="CENTER"
          :r="RADIUS"
          fill="none"
          class="donut-track"
          :stroke-width="STROKE"
        />

        <g :transform="`rotate(-90 ${CENTER} ${CENTER})`">
          <circle
            v-for="a in arcs"
            :key="a.name"
            :cx="CENTER"
            :cy="CENTER"
            :r="RADIUS"
            fill="none"
            :stroke="a.color"
            :stroke-width="STROKE"
            stroke-linecap="butt"
            :stroke-dasharray="`${a.segLength * progress} ${CIRCUMFERENCE}`"
            :stroke-dashoffset="a.dashOffset"
            :style="{
              filter: `drop-shadow(0 0 6px ${hexToRgba(a.color, 0.55)})`,
              opacity: hovered && hovered !== a.name ? 0.32 : 1,
              transition: 'opacity 0.2s ease',
            }"
            class="donut-arc"
            @mouseenter="hovered = a.name"
            @mouseleave="hovered = null"
          />
        </g>
      </svg>

      <div class="donut-center">
        <div class="donut-center-label">
          {{ activeArc ? activeArc.name : "Total" }}
        </div>
        <div
          class="donut-center-value"
          :style="
            activeArc
              ? {
                  color: activeArc.color,
                  textShadow: `0 0 14px ${hexToRgba(activeArc.color, 0.55)}`,
                }
              : {}
          "
        >
          {{ activeArc ? fmt(activeArc.y) : fmt(total) }}
        </div>
        <div class="donut-center-sub">
          {{
            activeArc
              ? (activeArc.fraction * 100).toFixed(1) + "%"
              : segments.length + " items"
          }}
        </div>
      </div>
    </div>

    <ul class="donut-legend">
      <li
        v-for="a in arcs"
        :key="`l-${a.name}`"
        class="legend-item"
        :class="{ active: hovered === a.name, dimmed: hovered && hovered !== a.name }"
        @mouseenter="hovered = a.name"
        @mouseleave="hovered = null"
      >
        <span
          class="legend-dot"
          :style="{
            background: a.color,
            boxShadow: `0 0 6px ${hexToRgba(a.color, 0.55)}`,
          }"
        ></span>
        <span class="legend-name" :title="a.name">{{ a.name }}</span>
        <span class="legend-pct">{{ (a.fraction * 100).toFixed(1) }}%</span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.donut-wrap {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.75rem;
  padding: 0.25rem 0.1rem;
}
.donut-chart {
  position: relative;
  width: 100%;
  max-width: 240px;
  aspect-ratio: 1 / 1;
}
.donut-svg {
  width: 100%;
  height: 100%;
  overflow: visible;
}
.donut-track {
  stroke: rgba(15, 23, 42, 0.06);
}
.donut-arc {
  cursor: pointer;
}
.donut-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  text-align: center;
}
.donut-center-label {
  font-size: var(--fs-micro);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.18em;
  color: #94a3b8;
  max-width: 9rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.donut-center-value {
  font-family: "Lato", system-ui, sans-serif;
  font-weight: 200;
  font-size: 1.55rem;
  line-height: 1.1;
  color: #0f172a;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
  margin-top: 0.1rem;
  transition: color 0.2s ease, text-shadow 0.2s ease;
}
.donut-center-sub {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.65rem;
  color: #94a3b8;
  letter-spacing: 0.08em;
  margin-top: 0.15rem;
}

.donut-legend {
  list-style: none;
  margin: 0;
  padding: 0;
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}
.legend-item {
  display: grid;
  grid-template-columns: 0.6rem 1fr auto;
  align-items: center;
  gap: 0.55rem;
  padding: 0.15rem 0.35rem;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.15s ease, opacity 0.2s ease;
  font-family: "Lato", system-ui, sans-serif;
}
.legend-item:hover,
.legend-item.active {
  background: rgba(15, 23, 42, 0.04);
}
.legend-item.dimmed {
  opacity: 0.45;
}
.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}
.legend-name {
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: #475569;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.legend-pct {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-variant-numeric: tabular-nums;
  font-size: 0.7rem;
  color: #64748b;
}

body[data-theme="dark"] .donut-track {
  stroke: rgba(148, 163, 184, 0.1);
}
body[data-theme="dark"] .donut-center-label,
body[data-theme="dark"] .donut-center-sub {
  color: var(--text-muted);
}
body[data-theme="dark"] .donut-center-value {
  color: var(--text-primary);
}
body[data-theme="dark"] .legend-name {
  color: var(--text-secondary);
}
body[data-theme="dark"] .legend-pct {
  color: var(--text-muted);
}
body[data-theme="dark"] .legend-item:hover,
body[data-theme="dark"] .legend-item.active {
  background: rgba(148, 163, 184, 0.08);
}
</style>
