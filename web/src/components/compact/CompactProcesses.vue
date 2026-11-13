<script setup lang="ts">
/**
 * The busiest programs, by processor or by memory, one line each.
 */
import { computed, ref } from "vue";
import { useSystemStore } from "@/stores/system";
import { formatSize } from "@/utilities/format-size";
import type { ProcessInformation } from "@/interfaces/SystemInformation";

type SortBy = "cpu" | "mem";

const FEW = 5;

const system = useSystemStore();
const sortBy = ref<SortBy>("cpu");
const showAll = ref(false);

const source = computed(() => (system.live ? system.realtime : system.data));

// The collector sends two lists, the ten busiest by each measure, with
// both figures on every entry.
const listed = computed<ProcessInformation[]>(() => {
  const byMem = source.value.processes ?? [];
  const byCpu = source.value.processes_by_cpu ?? [];
  if (sortBy.value === "cpu" && byCpu.length > 0) return byCpu;
  return byMem;
});

// A process's CPU arrives as a share of one core; shown here as a share
// of the whole machine, the scale the CPU card above uses.
const cores = computed(() => Math.max(1, source.value.cpu?.cores || 1));
const cpuOf = (p: ProcessInformation) =>
  typeof p.cpu === "number" ? p.cpu / cores.value : null;

const rows = computed(() => {
  const all = listed.value;
  const amount = (p: ProcessInformation) =>
    sortBy.value === "cpu" ? (cpuOf(p) ?? 0) : (p.mem ?? 0);
  const most = Math.max(...all.map(amount), sortBy.value === "cpu" ? 1 : 0.001);
  return all.slice(0, showAll.value ? all.length : FEW).map((p) => ({
    key: `${p.name}:${p.pid}`,
    name: p.name,
    title: p.username ? `${p.name} (${p.username})` : p.name,
    share: Math.min(100, Math.max(2, (amount(p) / most) * 100)),
    cpu: cpuText(cpuOf(p)),
    mem: memText(p.mem),
  }));
});

function cpuText(cpu: number | null): string {
  if (cpu === null) return "–";
  return cpu.toFixed(1);
}

function memText(mib: number | undefined): { value: string; unit: string } {
  if (typeof mib !== "number") return { value: "–", unit: "" };
  const f = formatSize(mib, "MiB");
  if (f.unit === "MiB") return { value: mib.toFixed(0), unit: "MiB" };
  return { value: f.value, unit: f.unit };
}

const canShowMore = computed(() => listed.value.length > FEW);
</script>

<template>
  <section class="compact-panel">
    <header class="panel-head">
      <span class="panel-icon"><font-awesome-icon icon="fa-solid fa-list-ul" /></span>
      <h2 class="panel-title">Top processes</h2>
      <button
        v-if="canShowMore"
        type="button"
        class="panel-link"
        :aria-expanded="showAll"
        @click="showAll = !showAll"
      >
        {{ showAll ? "Show fewer" : "View all" }}
        <font-awesome-icon :icon="showAll ? 'fa-solid fa-arrow-up' : 'fa-solid fa-arrow-down'" />
      </button>
    </header>
    <table class="process-table">
      <thead>
        <tr>
          <th class="col-name" scope="col">Name</th>
          <th class="col-bar" aria-hidden="true"></th>
          <th class="col-num" scope="col" :aria-sort="sortBy === 'cpu' ? 'descending' : 'none'">
            <button
              type="button"
              class="sort-button"
              :class="{ active: sortBy === 'cpu' }"
              title="Sort by CPU"
              @click="sortBy = 'cpu'"
            >
              CPU
            </button>
          </th>
          <th class="col-num col-mem" scope="col" :aria-sort="sortBy === 'mem' ? 'descending' : 'none'">
            <button
              type="button"
              class="sort-button"
              :class="{ active: sortBy === 'mem' }"
              title="Sort by memory"
              @click="sortBy = 'mem'"
            >
              Memory
            </button>
          </th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.key">
          <td class="col-name" :title="row.title">{{ row.name }}</td>
          <td class="col-bar">
            <span class="share"><span class="share-fill" :style="{ width: row.share + '%' }"></span></span>
          </td>
          <td class="col-num">{{ row.cpu }}<span class="num-unit">%</span></td>
          <td class="col-num col-mem">{{ row.mem.value }}<span class="num-unit">{{ row.mem.unit }}</span></td>
        </tr>
        <tr v-if="rows.length === 0">
          <td class="empty" colspan="4">Waiting for the first scan…</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>

<style scoped>
.process-table {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
}
th {
  padding: 5px 0 3px;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 9.5px;
  font-weight: 600;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--c-dim);
  text-align: left;
}
td {
  height: 22px;
  padding: 0;
  font-size: 11px;
  color: var(--c-text);
  border-top: 1px solid var(--c-hair);
  white-space: nowrap;
}
.col-name {
  width: 36%;
  padding-left: 10px;
  overflow: hidden;
  text-overflow: ellipsis;
}
td.col-name {
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.col-bar { padding: 0 10px; }
.col-num {
  width: 52px;
  padding-right: 10px;
  text-align: right;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-variant-numeric: tabular-nums;
}
.col-mem { width: 78px; }
th.col-num { padding-right: 10px; text-align: right; }
.num-unit {
  display: inline-block;
  margin-left: 3px;
  font-size: 9.5px;
  color: var(--c-muted);
}
.col-mem .num-unit { min-width: 21px; text-align: left; }

.share {
  display: block;
  height: 4px;
  border-radius: 999px;
  background: var(--c-track);
  overflow: hidden;
}
.share-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--t-green);
}

.sort-button {
  padding: 0;
  border: 0;
  background: none;
  font: inherit;
  letter-spacing: inherit;
  text-transform: inherit;
  color: inherit;
  cursor: pointer;
}
.sort-button:hover { color: var(--c-text); }
.sort-button.active { color: var(--c-accent); }
.sort-button.active::after { content: " ↓"; letter-spacing: 0; }
.sort-button:focus-visible {
  outline: 2px solid var(--c-accent);
  outline-offset: 2px;
  border-radius: 2px;
}
.empty {
  padding-left: 10px;
  color: var(--c-muted);
}
</style>
