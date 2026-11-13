<script setup lang="ts">
/**
 * A small line of recent readings, newest at the right edge.
 *
 * It is redrawn when a reading arrives, once a second, and not in between:
 * at this size a line that steps is as readable as one that glides, and it
 * costs next to nothing to draw.
 */
import { computed } from "vue";
import { line as d3Line, area as d3Area, curveMonotoneX } from "d3";
import { HISTORY_LENGTH } from "@/stores/system";

export interface SparkSeries {
  values: number[];
  /** Any CSS colour, a theme variable included. */
  color: string;
}

const props = withDefaults(
  defineProps<{
    series: SparkSeries[];
    /** A fixed top of the scale, for readings that have one (a percent). */
    max?: number;
    /** Otherwise the scale follows the tallest reading, but never drops
     *  below this, so an idle line stays flat instead of magnified noise. */
    floor?: number;
    label: string;
  }>(),
  { max: undefined, floor: 1 },
);

// Drawn in a fixed box and stretched to whatever room the card gives it;
// the stroke keeps its own width (see vector-effect below).
const WIDTH = 100;
const HEIGHT = 28;
const PAD = 2.5;

const uid = `spark-${Math.random().toString(36).slice(2, 9)}`;

const top = computed(() => {
  if (props.max !== undefined) return props.max;
  let tallest = 0;
  for (const s of props.series) for (const v of s.values) if (v > tallest) tallest = v;
  return Math.max(tallest * 1.15, props.floor);
});

const x = (index: number, count: number) =>
  WIDTH - (count - 1 - index) * (WIDTH / (HISTORY_LENGTH - 1));
const y = (value: number) =>
  HEIGHT - PAD - (Math.min(Math.max(value, 0), top.value) / top.value) * (HEIGHT - 2 * PAD);

const paths = computed(() =>
  props.series.map((s, i) => {
    const points = s.values.map((v, index) => [x(index, s.values.length), y(v)] as [number, number]);
    const stroke = d3Line().curve(curveMonotoneX)(points) ?? "";
    const fill =
      d3Area()
        .curve(curveMonotoneX)
        .y0(HEIGHT)(points) ?? "";
    return { id: `${uid}-${i}`, color: s.color, stroke, fill, drawn: points.length > 1 };
  }),
);
</script>

<template>
  <svg
    class="spark"
    :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
    preserveAspectRatio="none"
    role="img"
    :aria-label="label"
  >
    <title>{{ label }}</title>
    <defs>
      <linearGradient v-for="p in paths" :id="p.id" :key="p.id" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" :style="{ stopColor: p.color }" stop-opacity="0.28" />
        <stop offset="100%" :style="{ stopColor: p.color }" stop-opacity="0" />
      </linearGradient>
    </defs>
    <template v-for="p in paths" :key="p.id">
      <template v-if="p.drawn">
        <path :d="p.fill" :fill="`url(#${p.id})`" />
        <path class="spark-line" :d="p.stroke" :style="{ stroke: p.color }" />
      </template>
    </template>
  </svg>
</template>

<style scoped>
.spark {
  display: block;
  width: 100%;
  height: 100%;
  overflow: hidden;
}
.spark-line {
  fill: none;
  stroke-width: 1.5px;
  stroke-linecap: round;
  stroke-linejoin: round;
  vector-effect: non-scaling-stroke;
}
</style>
