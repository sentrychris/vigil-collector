<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";

const props = defineProps<{
  id: string;
  title?: string;
  metric: number;
  format: string;
}>();

const SIZE = 200;
const STROKE = 14;
const RADIUS = (SIZE - STROKE) / 2 - 6;
const CENTER = SIZE / 2;
const ARC_DEGREES = 270;
const START_ANGLE = 135;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const ARC_LENGTH = CIRCUMFERENCE * (ARC_DEGREES / 360);

const clamp = (v: number) => Math.max(0, Math.min(100, v));

const animatedValue = ref(0);
let rafId: number | null = null;

const animateTo = (target: number) => {
  if (rafId !== null) cancelAnimationFrame(rafId);
  const start = animatedValue.value;
  const startTime = performance.now();
  const duration = 700;
  const tick = (now: number) => {
    const t = Math.min(1, (now - startTime) / duration);
    const eased = 1 - Math.pow(1 - t, 3);
    animatedValue.value = start + (target - start) * eased;
    if (t < 1) rafId = requestAnimationFrame(tick);
    else rafId = null;
  };
  rafId = requestAnimationFrame(tick);
};

onMounted(() => animateTo(clamp(props.metric)));
onUnmounted(() => {
  if (rafId !== null) cancelAnimationFrame(rafId);
});

watch(
  () => props.metric,
  (next) => animateTo(clamp(next)),
);

const pct = computed(() => animatedValue.value / 100);
const dashOffset = computed(() => ARC_LENGTH * (1 - pct.value));

const palette = computed(() => {
  const v = animatedValue.value;
  if (v >= 90)
    return {
      primary: "#ef4444",
      secondary: "#f87171",
      glow: "rgba(239, 68, 68, 0.55)",
      label: "Critical",
    };
  if (v >= 75)
    return {
      primary: "#f59e0b",
      secondary: "#fbbf24",
      glow: "rgba(245, 158, 11, 0.5)",
      label: "Elevated",
    };
  if (v >= 50)
    return {
      primary: "#22d3ee",
      secondary: "#67e8f9",
      glow: "rgba(34, 211, 238, 0.5)",
      label: "Active",
    };
  return {
    primary: "#10b981",
    secondary: "#34d399",
    glow: "rgba(16, 185, 129, 0.5)",
    label: "Normal",
  };
});

const endRad = computed(() => {
  const angle = START_ANGLE + ARC_DEGREES * pct.value;
  return (angle * Math.PI) / 180;
});
const endX = computed(() => CENTER + RADIUS * Math.cos(endRad.value));
const endY = computed(() => CENTER + RADIUS * Math.sin(endRad.value));

const arcPath = (() => {
  const startRad = (START_ANGLE * Math.PI) / 180;
  const finalRad = ((START_ANGLE + ARC_DEGREES) * Math.PI) / 180;
  const x1 = CENTER + RADIUS * Math.cos(startRad);
  const y1 = CENTER + RADIUS * Math.sin(startRad);
  const x2 = CENTER + RADIUS * Math.cos(finalRad);
  const y2 = CENTER + RADIUS * Math.sin(finalRad);
  const largeArc = ARC_DEGREES > 180 ? 1 : 0;
  return `M ${x1} ${y1} A ${RADIUS} ${RADIUS} 0 ${largeArc} 1 ${x2} ${y2}`;
})();

const ticks = computed(() => {
  const out: { x1: number; y1: number; x2: number; y2: number; on: boolean }[] = [];
  const count = 30;
  for (let i = 0; i <= count; i++) {
    const t = i / count;
    const angle = START_ANGLE + ARC_DEGREES * t;
    const rad = (angle * Math.PI) / 180;
    const inner = RADIUS - STROKE / 2 - 6;
    const outer = RADIUS - STROKE / 2 - 2;
    out.push({
      x1: CENTER + inner * Math.cos(rad),
      y1: CENTER + inner * Math.sin(rad),
      x2: CENTER + outer * Math.cos(rad),
      y2: CENTER + outer * Math.sin(rad),
      on: t <= pct.value,
    });
  }
  return out;
});

const displayValue = computed(() => Math.round(animatedValue.value));
const formatSuffix = computed(() => props.format.replace("{y}", "").trim());
const gradId = computed(() => `gauge-grad-${props.id}`);
</script>

<template>
  <div class="gauge-wrap">
    <div class="gauge-title" v-if="title">{{ title }}</div>

    <div class="gauge-svg-wrap">
      <svg
        :viewBox="`0 0 ${SIZE} ${SIZE}`"
        class="gauge-svg"
        preserveAspectRatio="xMidYMid meet"
      >
        <defs>
          <linearGradient :id="gradId" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" :stop-color="palette.secondary" />
            <stop offset="100%" :stop-color="palette.primary" />
          </linearGradient>
        </defs>

        <line
          v-for="(t, i) in ticks"
          :key="i"
          :x1="t.x1"
          :y1="t.y1"
          :x2="t.x2"
          :y2="t.y2"
          :stroke="t.on ? palette.primary : 'currentColor'"
          :stroke-opacity="t.on ? 0.85 : 0.18"
          stroke-width="1.25"
          stroke-linecap="round"
          class="gauge-tick"
        />

        <path
          :d="arcPath"
          fill="none"
          class="gauge-track"
          :stroke-width="STROKE"
          stroke-linecap="round"
        />

        <path
          :d="arcPath"
          fill="none"
          :stroke="`url(#${gradId})`"
          :stroke-width="STROKE"
          stroke-linecap="round"
          :style="{ filter: `drop-shadow(0 0 10px ${palette.glow})` }"
          :stroke-dasharray="`${ARC_LENGTH} ${CIRCUMFERENCE}`"
          :stroke-dashoffset="dashOffset"
          class="gauge-arc"
        />

        <circle
          v-if="animatedValue > 0.5"
          :cx="endX"
          :cy="endY"
          :r="STROKE / 2 - 1"
          fill="#ffffff"
          :style="{ filter: `drop-shadow(0 0 6px ${palette.primary})` }"
        />
      </svg>

      <div class="gauge-readout">
        <div
          class="gauge-value"
          :style="{
            color: palette.primary,
            textShadow: `0 0 18px ${palette.glow}`,
          }"
        >
          {{ displayValue
          }}<span class="gauge-suffix">{{ formatSuffix }}</span>
        </div>
        <div class="gauge-status" :style="{ color: palette.primary }">
          <span
            class="gauge-status-dot"
            :style="{
              background: palette.primary,
              boxShadow: `0 0 8px ${palette.glow}`,
            }"
          ></span>
          {{ palette.label }}
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.gauge-wrap {
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 100%;
}
.gauge-title {
  font-size: var(--fs-micro);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.16em;
  color: #6b7280;
  margin-bottom: 0.25rem;
}
.gauge-svg-wrap {
  position: relative;
  width: 100%;
  max-width: 240px;
  aspect-ratio: 1 / 1;
  margin: 0 auto;
  color: #0f172a;
  /* Container query so gauge text scales sensibly when used in compact
     contexts (3-up inside a col-lg-4 panel) vs full-size standalone. */
  container-type: inline-size;
}
.gauge-svg {
  width: 100%;
  height: 100%;
  overflow: visible;
}
.gauge-track {
  stroke: rgba(15, 23, 42, 0.07);
}
.gauge-arc {
  transition: stroke 0.3s ease;
}
.gauge-readout {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.gauge-value {
  font-family: "Lato", system-ui, sans-serif;
  font-weight: 200;
  /* Scales with the gauge's own width (5-50% range) so a 140px compact
     gauge gets ~28px text while a 240px hero gauge still hits ~48px. */
  font-size: clamp(1.25rem, 22cqi, 3rem);
  line-height: 1;
  letter-spacing: -0.02em;
  transition: color 0.3s ease, text-shadow 0.3s ease;
  display: inline-flex;
  align-items: baseline;
}
.gauge-suffix {
  font-size: clamp(0.65rem, 8cqi, 1.1rem);
  font-weight: 400;
  margin-left: 0.15rem;
  opacity: 0.8;
}
.gauge-status {
  margin-top: 0.35rem;
  display: flex;
  align-items: center;
  gap: 0.3rem;
  font-size: clamp(0.55rem, 5cqi, 0.625rem);
  font-weight: 700;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  opacity: 0.9;
  transition: color 0.3s ease;
}
.gauge-status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  display: inline-block;
}

body[data-theme="dark"] .gauge-title {
  color: var(--text-muted);
}
body[data-theme="dark"] .gauge-svg-wrap {
  color: #ffffff;
}
body[data-theme="dark"] .gauge-track {
  stroke: rgba(148, 163, 184, 0.13);
}
</style>
