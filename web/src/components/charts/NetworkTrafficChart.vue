<script setup lang="ts">
import * as d3 from "d3";
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useResizeObserver } from "@/utilities/resize-observer";
import type { RealtimeDataPoint } from "@/interfaces/ChartRegistry";

const props = defineProps<{
  rx: number;
  tx: number;
}>();

const RX_TONE = {
  primary: "#22d3ee",
  secondary: "#67e8f9",
  glow: "rgba(34, 211, 238, 0.55)",
};
const TX_TONE = {
  primary: "#f43f5e",
  secondary: "#fb7185",
  glow: "rgba(244, 63, 94, 0.55)",
};

const KB = 1024;
const MB = 1024 * 1024;
const MIN_Y = 64 * KB; // floor so an idle network doesn't render fake spikes

function formatRate(bps: number): { value: string; unit: string } {
  if (bps >= MB) return { value: (bps / MB).toFixed(2), unit: "MB/s" };
  if (bps >= KB) return { value: (bps / KB).toFixed(1), unit: "KB/s" };
  return { value: String(Math.round(bps)), unit: "B/s" };
}

const liveRx = ref(0);
const liveTx = ref(0);
const fmtRx = computed(() => formatRate(liveRx.value));
const fmtTx = computed(() => formatRate(liveTx.value));

const svgRef = ref<SVGSVGElement | null>(null);
const { resizeRef, resizeState } = useResizeObserver();

const uid = `net-${Math.random().toString(36).slice(2, 9)}`;

const SAMPLE_MS = 500;
const WINDOW_MS = 30_000;

let sampleTimerId: number | null = null;
let rafId = 0;
let stopped = true;
let rxData: RealtimeDataPoint[] = [];
let txData: RealtimeDataPoint[] = [];

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
  rxData = [];
  txData = [];

  const margin = { top: 10, bottom: 28, left: 54, right: 10 };
  const width = resizeState.dimensions.width - margin.left - margin.right;
  const height = resizeState.dimensions.height - margin.top - margin.bottom;
  if (width <= 0 || height <= 0) return;

  const defs = svg.append("defs");

  const makeStrokeGrad = (id: string, t: typeof RX_TONE) => {
    const g = defs
      .append("linearGradient")
      .attr("id", id)
      .attr("x1", "0").attr("y1", "0").attr("x2", "1").attr("y2", "0");
    g.append("stop").attr("offset", "0%").attr("stop-color", t.secondary);
    g.append("stop").attr("offset", "100%").attr("stop-color", t.primary);
  };
  const makeAreaGrad = (id: string, t: typeof RX_TONE) => {
    const g = defs
      .append("linearGradient")
      .attr("id", id)
      .attr("x1", "0").attr("y1", "0").attr("x2", "0").attr("y2", "1");
    g.append("stop").attr("offset", "0%").attr("stop-color", t.primary).attr("stop-opacity", 0.28);
    g.append("stop").attr("offset", "100%").attr("stop-color", t.primary).attr("stop-opacity", 0);
  };
  makeStrokeGrad(`${uid}-rx-stroke`, RX_TONE);
  makeStrokeGrad(`${uid}-tx-stroke`, TX_TONE);
  makeAreaGrad(`${uid}-rx-area`, RX_TONE);
  makeAreaGrad(`${uid}-tx-area`, TX_TONE);

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

  // Y axis with byte-aware tick formatter
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

  const rxArea = paths.append("path").attr("fill", `url(#${uid}-rx-area)`);
  const txArea = paths.append("path").attr("fill", `url(#${uid}-tx-area)`);

  const rxLine = paths
    .append("path")
    .attr("fill", "none")
    .attr("stroke", `url(#${uid}-rx-stroke)`)
    .attr("stroke-width", 2.25)
    .attr("stroke-linecap", "round")
    .attr("stroke-linejoin", "round")
    .attr("filter", `url(#${uid}-glow)`);
  const txLine = paths
    .append("path")
    .attr("fill", "none")
    .attr("stroke", `url(#${uid}-tx-stroke)`)
    .attr("stroke-width", 2.25)
    .attr("stroke-linecap", "round")
    .attr("stroke-linejoin", "round")
    .attr("filter", `url(#${uid}-glow)`);

  const sample = () => {
    const now = Date.now();
    rxData.push({ time: now, value: props.rx });
    txData.push({ time: now, value: props.tx });
    liveRx.value = props.rx;
    liveTx.value = props.tx;
    const cutoff = now - (WINDOW_MS + SAMPLE_MS * 4);
    while (rxData.length > 0 && rxData[0].time < cutoff) rxData.shift();
    while (txData.length > 0 && txData[0].time < cutoff) txData.shift();
  };

  let lastDomainMax = MIN_Y;

  const render = () => {
    if (stopped) return;
    const now = Date.now();
    timeScale.domain([now - WINDOW_MS, now]);

    let rxd = rxData;
    let txd = txData;
    if (rxd.length > 0 && rxd[rxd.length - 1].time < now - 8) {
      rxd = rxd.concat({ time: now, value: props.rx });
      txd = txd.concat({ time: now, value: props.tx });
    }

    // Auto-scale Y to the observed window max with a sensible floor.
    const visibleStart = now - WINDOW_MS;
    let observedMax = MIN_Y;
    for (const d of rxd) if (d.time >= visibleStart && d.value > observedMax) observedMax = d.value;
    for (const d of txd) if (d.time >= visibleStart && d.value > observedMax) observedMax = d.value;
    const targetMax = observedMax * 1.2;
    // Smooth transitions so the axis doesn't jitter on every sample.
    const newMax = lastDomainMax * 0.85 + targetMax * 0.15;
    if (Math.abs(newMax - lastDomainMax) / lastDomainMax > 0.005) {
      lastDomainMax = newMax;
      valueScale.domain([0, lastDomainMax]);
      renderYAxis();
    }

    if (rxd.length >= 2) {
      rxLine.attr("d", line(rxd));
      rxArea.attr("d", area(rxd));
    }
    if (txd.length >= 2) {
      txLine.attr("d", line(txd));
      txArea.attr("d", area(txd));
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
  <div class="net-chart-wrapper" ref="resizeRef">
    <div class="net-readout">
      <span
        class="net-readout-row"
        :style="{ color: RX_TONE.primary, textShadow: `0 0 14px ${RX_TONE.glow}` }"
      >
        <i class="fa-solid fa-arrow-down" aria-hidden="true"></i>
        <span class="label">IN</span>
        <span class="value">{{ fmtRx.value }}</span>
        <span class="unit">{{ fmtRx.unit }}</span>
      </span>
      <span
        class="net-readout-row"
        :style="{ color: TX_TONE.primary, textShadow: `0 0 14px ${TX_TONE.glow}` }"
      >
        <i class="fa-solid fa-arrow-up" aria-hidden="true"></i>
        <span class="label">OUT</span>
        <span class="value">{{ fmtTx.value }}</span>
        <span class="unit">{{ fmtTx.unit }}</span>
      </span>
    </div>
    <svg ref="svgRef"></svg>
  </div>
</template>

<style scoped>
.net-chart-wrapper {
  position: relative;
  width: 100%;
  /* Fill the panel-body when the card is stretched (flex-fill column);
     otherwise fall back to the 180px floor so non-flex contexts still
     render at a sensible size. */
  height: 100%;
  min-height: 180px;
}
.net-chart-wrapper svg {
  height: 100%;
  width: 100%;
  display: block;
  overflow: visible;
}

.net-readout {
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
.net-readout-row {
  display: inline-flex;
  align-items: baseline;
  gap: 0.45rem;
  font-size: 1.1rem;
}
.net-readout-row i {
  font-size: 0.75rem;
  align-self: center;
}
.net-readout-row .label {
  font-size: 0.65rem;
  font-weight: 700;
  letter-spacing: 0.12em;
  align-self: center;
}
.net-readout-row .value { font-variant-numeric: tabular-nums; }
.net-readout-row .unit {
  font-size: 0.65rem;
  font-weight: 400;
  opacity: 0.7;
  letter-spacing: 0.06em;
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

body[data-theme="dark"] .net-chart-wrapper :deep(.rt-axis line) {
  stroke: rgba(148, 163, 184, 0.28);
}
body[data-theme="dark"] .net-chart-wrapper :deep(.rt-axis text) {
  fill: var(--text-muted);
}
</style>
