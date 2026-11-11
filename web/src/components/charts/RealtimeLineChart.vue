<script setup lang="ts">
import * as d3 from "d3";
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useResizeObserver } from "@/utilities/resize-observer";
import type { RealtimeDataPoint } from "@/interfaces/ChartRegistry";

type Tone = "blue" | "purple" | "amber" | "green" | "cyan" | "rose";

const props = withDefaults(
  defineProps<{
    dataPoint: number;
    yAxisRange: number[];
    tone?: Tone;
    unit?: string;
    decimals?: number;
  }>(),
  {
    tone: "cyan",
    unit: "",
    decimals: 1,
  },
);

const TONES: Record<
  Tone,
  { primary: string; secondary: string; glow: string }
> = {
  blue:   { primary: "#60a5fa", secondary: "#93c5fd", glow: "rgba(96, 165, 250, 0.55)" },
  purple: { primary: "#a78bfa", secondary: "#c4b5fd", glow: "rgba(167, 139, 250, 0.55)" },
  amber:  { primary: "#f59e0b", secondary: "#fbbf24", glow: "rgba(245, 158, 11, 0.55)" },
  green:  { primary: "#10b981", secondary: "#34d399", glow: "rgba(16, 185, 129, 0.55)" },
  cyan:   { primary: "#22d3ee", secondary: "#67e8f9", glow: "rgba(34, 211, 238, 0.55)" },
  rose:   { primary: "#f43f5e", secondary: "#fb7185", glow: "rgba(244, 63, 94, 0.55)" },
};

const tone = computed(() => TONES[props.tone]);

const svgRef = ref<SVGSVGElement | null>(null);
const { resizeRef, resizeState } = useResizeObserver();

const liveValue = ref(0);
const formattedLive = computed(() => liveValue.value.toFixed(props.decimals));

const uid = `rt-${Math.random().toString(36).slice(2, 9)}`;

const SAMPLE_MS = 500;
const WINDOW_MS = 30_000;

let sampleTimerId: number | null = null;
let rafId = 0;
let stopped = true;
let data: RealtimeDataPoint[] = [];

const teardown = () => {
  stopped = true;
  if (sampleTimerId !== null) clearInterval(sampleTimerId);
  sampleTimerId = null;
  if (rafId) cancelAnimationFrame(rafId);
  rafId = 0;
};

const draw = () => {
  teardown();
  const svg = d3.select(svgRef.value);
  svg.selectAll("*").remove();
  data = [];

  const margin = { top: 10, bottom: 28, left: 34, right: 10 };
  const width = resizeState.dimensions.width - margin.left - margin.right;
  const height = resizeState.dimensions.height - margin.top - margin.bottom;
  if (width <= 0 || height <= 0) return;

  const defs = svg.append("defs");

  const strokeGrad = defs
    .append("linearGradient")
    .attr("id", `${uid}-stroke`)
    .attr("x1", "0")
    .attr("y1", "0")
    .attr("x2", "1")
    .attr("y2", "0");
  strokeGrad
    .append("stop")
    .attr("offset", "0%")
    .attr("stop-color", tone.value.secondary);
  strokeGrad
    .append("stop")
    .attr("offset", "100%")
    .attr("stop-color", tone.value.primary);

  const areaGrad = defs
    .append("linearGradient")
    .attr("id", `${uid}-area`)
    .attr("x1", "0")
    .attr("y1", "0")
    .attr("x2", "0")
    .attr("y2", "1");
  areaGrad
    .append("stop")
    .attr("offset", "0%")
    .attr("stop-color", tone.value.primary)
    .attr("stop-opacity", 0.32);
  areaGrad
    .append("stop")
    .attr("offset", "100%")
    .attr("stop-color", tone.value.primary)
    .attr("stop-opacity", 0);

  const filter = defs
    .append("filter")
    .attr("id", `${uid}-glow`)
    .attr("x", "-50%")
    .attr("y", "-50%")
    .attr("width", "200%")
    .attr("height", "200%");
  filter.append("feGaussianBlur").attr("stdDeviation", "3").attr("result", "blur");
  const merge = filter.append("feMerge");
  merge.append("feMergeNode").attr("in", "blur");
  merge.append("feMergeNode").attr("in", "SourceGraphic");

  defs
    .append("clipPath")
    .attr("id", `${uid}-clip`)
    .append("rect")
    .attr("x", 0)
    .attr("y", 0)
    .attr("width", width)
    .attr("height", height);

  const g = svg
    .append("g")
    .attr("transform", `translate(${margin.left}, ${margin.top})`);

  const timeScale = d3.scaleTime().range([0, width]);
  const valueScale = d3
    .scaleLinear()
    .domain(props.yAxisRange)
    .range([height, 0]);

  const line = d3
    .line<RealtimeDataPoint>()
    .curve(d3.curveMonotoneX)
    .x((d) => timeScale(d.time))
    .y((d) => valueScale(d.value));

  const area = d3
    .area<RealtimeDataPoint>()
    .curve(d3.curveMonotoneX)
    .x((d) => timeScale(d.time))
    .y0(height)
    .y1((d) => valueScale(d.value));

  g.append("g")
    .attr("class", "y axis axis--y rt-axis")
    .call(
      d3
        .axisLeft(valueScale)
        .ticks(4)
        .tickSizeInner(-width)
        .tickSizeOuter(0)
        .tickPadding(8),
    );

  // Custom x-axis: wall-clock-aligned tick times that we reposition every frame
  // for smooth scrolling, with edge-distance opacity fade so ticks don't pop in
  // or out at the boundaries.
  const xAxisG = g
    .append("g")
    .attr("transform", `translate(0, ${height})`)
    .attr("class", "x axis rt-axis");

  const TICK_STEP_MS = width < 320 ? 10_000 : 5_000;
  const TICK_FADE_PX = 32;
  const fmtTick = d3.timeFormat("%H:%M:%S");

  const renderXAxis = (now: number) => {
    const startBound = now - WINDOW_MS - TICK_STEP_MS;
    const endBound = now + TICK_STEP_MS;
    const ticks: number[] = [];
    let t = Math.ceil(startBound / TICK_STEP_MS) * TICK_STEP_MS;
    for (; t <= endBound; t += TICK_STEP_MS) ticks.push(t);

    const sel = xAxisG
      .selectAll<SVGGElement, number>("g.rt-tick")
      .data(ticks, (d) => String(d));

    sel.exit().remove();

    const enter = sel
      .enter()
      .append("g")
      .attr("class", "rt-tick");
    enter.append("line").attr("y1", 0).attr("y2", -height);
    enter
      .append("text")
      .attr("y", 14)
      .attr("text-anchor", "middle")
      .attr("dominant-baseline", "hanging");

    const merged = enter.merge(sel);
    merged.attr(
      "transform",
      (d) => `translate(${timeScale(d)}, 0)`,
    );
    merged.select("text").text((d) => fmtTick(new Date(d)));
    merged.style("opacity", (d) => {
      const x = timeScale(d);
      if (x < TICK_FADE_PX) return Math.max(0, x / TICK_FADE_PX);
      if (x > width - TICK_FADE_PX)
        return Math.max(0, (width - x) / TICK_FADE_PX);
      return 1;
    });
  };

  const paths = g
    .append("g")
    .attr("class", "paths")
    .attr("clip-path", `url(#${uid}-clip)`);

  const areaPath = paths.append("path").attr("fill", `url(#${uid}-area)`);

  const linePath = paths
    .append("path")
    .attr("fill", "none")
    .attr("stroke", `url(#${uid}-stroke)`)
    .attr("stroke-width", 2.25)
    .attr("stroke-linecap", "round")
    .attr("stroke-linejoin", "round")
    .attr("filter", `url(#${uid}-glow)`);

  const headHalo = g
    .append("circle")
    .attr("r", 7)
    .attr("fill", tone.value.primary)
    .attr("opacity", 0.25)
    .style("display", "none");

  const head = g
    .append("circle")
    .attr("r", 3.5)
    .attr("fill", "#ffffff")
    .attr("stroke", tone.value.primary)
    .attr("stroke-width", 1.5)
    .attr("filter", `url(#${uid}-glow)`)
    .style("display", "none");

  const sample = () => {
    const now = Date.now();
    data.push({ time: now, value: props.dataPoint });
    liveValue.value = props.dataPoint;
    const cutoff = now - (WINDOW_MS + SAMPLE_MS * 4);
    while (data.length > 0 && data[0].time < cutoff) data.shift();
  };

  const render = () => {
    if (stopped) return;
    const now = Date.now();
    timeScale.domain([now - WINDOW_MS, now]);

    // Append a synthetic point at "now" so the line always reaches the right
    // edge between samples — eliminates the per-tick snap when a new sample lands.
    let rd: RealtimeDataPoint[] = data;
    if (data.length > 0 && data[data.length - 1].time < now - 8) {
      rd = data.concat({ time: now, value: props.dataPoint });
    }

    if (rd.length >= 2) {
      linePath.attr("d", line(rd));
      areaPath.attr("d", area(rd));

      const last = rd[rd.length - 1];
      const lx = timeScale(last.time);
      const ly = valueScale(last.value);
      head.style("display", null).attr("cx", lx).attr("cy", ly);
      headHalo.style("display", null).attr("cx", lx).attr("cy", ly);
    }

    renderXAxis(now);
    rafId = requestAnimationFrame(render);
  };

  stopped = false;
  sample();
  sampleTimerId = window.setInterval(sample, SAMPLE_MS);
  rafId = requestAnimationFrame(render);
};

onMounted(() => {
  requestAnimationFrame(() => draw());
});

watch(
  () => [resizeState.dimensions.width, resizeState.dimensions.height],
  () => draw(),
);

onUnmounted(teardown);
</script>

<template>
  <div class="realtime-line-wrapper" ref="resizeRef">
    <div
      class="rt-readout"
      :style="{
        color: tone.primary,
        textShadow: `0 0 14px ${tone.glow}`,
      }"
    >
      <span
        class="rt-pulse"
        :style="{ background: tone.primary, boxShadow: `0 0 8px ${tone.glow}` }"
      ></span>
      <span class="rt-value">{{ formattedLive }}</span>
      <span class="rt-unit">{{ unit }}</span>
    </div>
    <svg ref="svgRef"></svg>
  </div>
</template>

<style scoped>
.realtime-line-wrapper {
  position: relative;
  width: 100%;
  /* Fill the panel-body when the card is stretched (flex-fill column);
     otherwise fall back to the 180px floor so non-flex contexts still
     render at a sensible size. */
  height: 100%;
  min-height: 180px;
}
.realtime-line-wrapper svg {
  height: 100%;
  width: 100%;
  display: block;
  overflow: visible;
}

.rt-readout {
  position: absolute;
  top: 0.6rem;
  right: 0.9rem;
  display: inline-flex;
  align-items: baseline;
  gap: 0.45rem;
  font-family: "Lato", system-ui, sans-serif;
  font-weight: 200;
  font-size: 1.4rem;
  line-height: 1;
  letter-spacing: -0.01em;
  z-index: 1;
  pointer-events: none;
}
.rt-pulse {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  align-self: center;
  animation: rt-pulse 1.6s ease-in-out infinite;
}
.rt-value {
  font-variant-numeric: tabular-nums;
}
.rt-unit {
  font-size: 0.7rem;
  font-weight: 400;
  opacity: 0.7;
  letter-spacing: 0.06em;
}

@keyframes rt-pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50%      { opacity: 0.55; transform: scale(0.7); }
}

/* d3 axis styling — subtle, no spine, dashed grid */
:deep(.rt-axis path.domain) {
  stroke: none;
}
:deep(.rt-axis line) {
  stroke: rgba(15, 23, 42, 0.18);
  stroke-dasharray: 2 4;
  shape-rendering: crispEdges;
}
:deep(.rt-axis text) {
  fill: #94a3b8;
  font-family: "Lato", system-ui, sans-serif;
  font-size: 10px;
  letter-spacing: 0.04em;
}

body[data-theme="dark"] .realtime-line-wrapper :deep(.rt-axis line) {
  stroke: rgba(148, 163, 184, 0.28);
}
body[data-theme="dark"] .realtime-line-wrapper :deep(.rt-axis text) {
  fill: var(--text-muted);
}
</style>
