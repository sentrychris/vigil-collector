<script setup lang="ts">
import * as d3 from "d3";
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useResizeObserver } from "@/utilities/resize-observer";
import type { RealtimeDataPoint } from "@/interfaces/ChartRegistry";

const props = defineProps<{
  read: number;
  write: number;
  readIops: number;
  writeIops: number;
}>();

const READ_TONE = {
  primary: "#10b981",
  secondary: "#34d399",
  glow: "rgba(16, 185, 129, 0.55)",
};
const WRITE_TONE = {
  primary: "#d946ef",
  secondary: "#e879f9",
  glow: "rgba(217, 70, 239, 0.55)",
};

const KB = 1024;
const MB = 1024 * 1024;
const MIN_Y = 64 * KB;

function formatRate(bps: number): { value: string; unit: string } {
  if (bps >= MB) return { value: (bps / MB).toFixed(2), unit: "MB/s" };
  if (bps >= KB) return { value: (bps / KB).toFixed(1), unit: "KB/s" };
  return { value: String(Math.round(bps)), unit: "B/s" };
}

const liveRead = ref(0);
const liveWrite = ref(0);
const fmtRead = computed(() => formatRate(liveRead.value));
const fmtWrite = computed(() => formatRate(liveWrite.value));

const svgRef = ref<SVGSVGElement | null>(null);
const { resizeRef, resizeState } = useResizeObserver();

const uid = `dio-${Math.random().toString(36).slice(2, 9)}`;

const SAMPLE_MS = 500;
const WINDOW_MS = 30_000;

let sampleTimerId: number | null = null;
let rafId = 0;
let stopped = true;
let readData: RealtimeDataPoint[] = [];
let writeData: RealtimeDataPoint[] = [];

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
  readData = [];
  writeData = [];

  const margin = { top: 10, bottom: 28, left: 54, right: 10 };
  const width = resizeState.dimensions.width - margin.left - margin.right;
  const height = resizeState.dimensions.height - margin.top - margin.bottom;
  if (width <= 0 || height <= 0) return;

  const defs = svg.append("defs");

  const makeStrokeGrad = (id: string, t: typeof READ_TONE) => {
    const g = defs
      .append("linearGradient")
      .attr("id", id)
      .attr("x1", "0").attr("y1", "0").attr("x2", "1").attr("y2", "0");
    g.append("stop").attr("offset", "0%").attr("stop-color", t.secondary);
    g.append("stop").attr("offset", "100%").attr("stop-color", t.primary);
  };
  const makeAreaGrad = (id: string, t: typeof READ_TONE) => {
    const g = defs
      .append("linearGradient")
      .attr("id", id)
      .attr("x1", "0").attr("y1", "0").attr("x2", "0").attr("y2", "1");
    g.append("stop").attr("offset", "0%").attr("stop-color", t.primary).attr("stop-opacity", 0.28);
    g.append("stop").attr("offset", "100%").attr("stop-color", t.primary).attr("stop-opacity", 0);
  };
  makeStrokeGrad(`${uid}-read-stroke`, READ_TONE);
  makeStrokeGrad(`${uid}-write-stroke`, WRITE_TONE);
  makeAreaGrad(`${uid}-read-area`, READ_TONE);
  makeAreaGrad(`${uid}-write-area`, WRITE_TONE);

  const filter = defs
    .append("filter")
    .attr("id", `${uid}-glow`)
    .attr("x", "-50%").attr("y", "-50%")
    .attr("width", "200%").attr("height", "200%");
  filter.append("feGaussianBlur").attr("stdDeviation", "3").attr("result", "blur");
  const merge = filter.append("feMerge");
  merge.append("feMergeNode").attr("in", "blur");
  merge.append("feMergeNode").attr("in", "SourceGraphic");

  defs
    .append("clipPath")
    .attr("id", `${uid}-clip`)
    .append("rect")
    .attr("x", 0).attr("y", 0).attr("width", width).attr("height", height);

  const g = svg
    .append("g")
    .attr("transform", `translate(${margin.left}, ${margin.top})`);

  const timeScale = d3.scaleTime().range([0, width]);
  const valueScale = d3.scaleLinear().domain([0, MIN_Y]).range([height, 0]);

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

  const yAxisG = g.append("g").attr("class", "y axis axis--y rt-axis");

  const fmtAxisVal = (v: d3.NumberValue) => {
    const n = Number(v);
    if (n >= MB) return `${(n / MB).toFixed(n >= 10 * MB ? 0 : 1)} MB/s`;
    if (n >= KB) return `${(n / KB).toFixed(0)} KB/s`;
    return `${n}`;
  };

  const renderYAxis = () => {
    yAxisG.call(
      d3
        .axisLeft(valueScale)
        .ticks(4)
        .tickFormat(fmtAxisVal)
        .tickSizeInner(-width)
        .tickSizeOuter(0)
        .tickPadding(8),
    );
  };
  renderYAxis();

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

    const enter = sel.enter().append("g").attr("class", "rt-tick");
    enter.append("line").attr("y1", 0).attr("y2", -height);
    enter
      .append("text")
      .attr("y", 14)
      .attr("text-anchor", "middle")
      .attr("dominant-baseline", "hanging");

    const merged = enter.merge(sel);
    merged.attr("transform", (d) => `translate(${timeScale(d)}, 0)`);
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

  const readArea = paths.append("path").attr("fill", `url(#${uid}-read-area)`);
  const writeArea = paths.append("path").attr("fill", `url(#${uid}-write-area)`);

  const readLine = paths
    .append("path")
    .attr("fill", "none")
    .attr("stroke", `url(#${uid}-read-stroke)`)
    .attr("stroke-width", 2.25)
    .attr("stroke-linecap", "round")
    .attr("stroke-linejoin", "round")
    .attr("filter", `url(#${uid}-glow)`);
  const writeLine = paths
    .append("path")
    .attr("fill", "none")
    .attr("stroke", `url(#${uid}-write-stroke)`)
    .attr("stroke-width", 2.25)
    .attr("stroke-linecap", "round")
    .attr("stroke-linejoin", "round")
    .attr("filter", `url(#${uid}-glow)`);

  const sample = () => {
    const now = Date.now();
    readData.push({ time: now, value: props.read });
    writeData.push({ time: now, value: props.write });
    liveRead.value = props.read;
    liveWrite.value = props.write;
    const cutoff = now - (WINDOW_MS + SAMPLE_MS * 4);
    while (readData.length > 0 && readData[0].time < cutoff) readData.shift();
    while (writeData.length > 0 && writeData[0].time < cutoff) writeData.shift();
  };

  let lastDomainMax = MIN_Y;

  const render = () => {
    if (stopped) return;
    const now = Date.now();
    timeScale.domain([now - WINDOW_MS, now]);

    let rd = readData;
    let wd = writeData;
    if (rd.length > 0 && rd[rd.length - 1].time < now - 8) {
      rd = rd.concat({ time: now, value: props.read });
      wd = wd.concat({ time: now, value: props.write });
    }

    const visibleStart = now - WINDOW_MS;
    let observedMax = MIN_Y;
    for (const d of rd) if (d.time >= visibleStart && d.value > observedMax) observedMax = d.value;
    for (const d of wd) if (d.time >= visibleStart && d.value > observedMax) observedMax = d.value;
    const targetMax = observedMax * 1.2;
    const newMax = lastDomainMax * 0.85 + targetMax * 0.15;
    if (Math.abs(newMax - lastDomainMax) / lastDomainMax > 0.005) {
      lastDomainMax = newMax;
      valueScale.domain([0, lastDomainMax]);
      renderYAxis();
    }

    if (rd.length >= 2) {
      readLine.attr("d", line(rd));
      readArea.attr("d", area(rd));
    }
    if (wd.length >= 2) {
      writeLine.attr("d", line(wd));
      writeArea.attr("d", area(wd));
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
  <div class="dio-chart-wrapper" ref="resizeRef">
    <div class="dio-readout">
      <span
        class="dio-readout-row"
        :style="{ color: READ_TONE.primary, textShadow: `0 0 14px ${READ_TONE.glow}` }"
      >
        <i class="fa-solid fa-arrow-down" aria-hidden="true"></i>
        <span class="label">READ</span>
        <span class="value">{{ fmtRead.value }}</span>
        <span class="unit">{{ fmtRead.unit }}</span>
        <span class="iops">{{ readIops }} IOPS</span>
      </span>
      <span
        class="dio-readout-row"
        :style="{ color: WRITE_TONE.primary, textShadow: `0 0 14px ${WRITE_TONE.glow}` }"
      >
        <i class="fa-solid fa-arrow-up" aria-hidden="true"></i>
        <span class="label">WRITE</span>
        <span class="value">{{ fmtWrite.value }}</span>
        <span class="unit">{{ fmtWrite.unit }}</span>
        <span class="iops">{{ writeIops }} IOPS</span>
      </span>
    </div>
    <svg ref="svgRef"></svg>
  </div>
</template>

<style scoped>
.dio-chart-wrapper {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 180px;
}
.dio-chart-wrapper svg {
  height: 100%;
  width: 100%;
  display: block;
  overflow: visible;
}

.dio-readout {
  position: absolute;
  top: 0.6rem;
  right: 0.9rem;
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  align-items: flex-end;
  z-index: 1;
  pointer-events: none;
  font-family: "Lato", system-ui, sans-serif;
  font-weight: 200;
  letter-spacing: -0.01em;
  line-height: 1;
}
.dio-readout-row {
  display: inline-flex;
  align-items: baseline;
  gap: 0.45rem;
  font-size: 1.1rem;
}
.dio-readout-row i {
  font-size: 0.75rem;
  align-self: center;
}
.dio-readout-row .label {
  font-size: 0.65rem;
  font-weight: 700;
  letter-spacing: 0.12em;
  align-self: center;
}
.dio-readout-row .value { font-variant-numeric: tabular-nums; }
.dio-readout-row .unit {
  font-size: 0.65rem;
  font-weight: 400;
  opacity: 0.7;
  letter-spacing: 0.06em;
}
.dio-readout-row .iops {
  font-size: 0.6rem;
  font-weight: 600;
  letter-spacing: 0.08em;
  opacity: 0.65;
  margin-left: 0.25rem;
}

:deep(.rt-axis path.domain) { stroke: none; }
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

body[data-theme="dark"] .dio-chart-wrapper :deep(.rt-axis line) {
  stroke: rgba(148, 163, 184, 0.28);
}
body[data-theme="dark"] .dio-chart-wrapper :deep(.rt-axis text) {
  fill: var(--text-muted);
}
</style>
