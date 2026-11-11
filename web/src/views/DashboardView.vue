<script setup lang="ts">
import { computed, onBeforeMount, ref } from "vue";
import { config } from "@/config";
import { useLoadingStore } from "@/stores/loading";
import { useSystemStore } from "@/stores/system";
import { useDocumentTitle } from "@/composables/useDocumentTitle";
import SectionHeader from "@/components/SectionHeader.vue";
import StatCard from "@/components/stats/StatCard.vue";
import PlatformDetail from "@/components/stats/PlatformDetail.vue";
import CpuDetail from "@/components/stats/CpuDetail.vue";
import UsageDetail from "@/components/stats/UsageDetail.vue";
import GaugeChart from "@/components/charts/GaugeChart.vue";
import RealtimeLineChart from "@/components/charts/RealtimeLineChart.vue";
import NetworkTrafficChart from "@/components/charts/NetworkTrafficChart.vue";
import DiskIoChart from "@/components/charts/DiskIoChart.vue";
import BarChart from "@/components/charts/BarChart.vue";
import PieChart from "@/components/charts/PieChart.vue";
import ServiceStatus from "@/components/ServiceStatus.vue";
import DiskList from "@/components/stats/DiskList.vue";
import DataTable from "@/components/DataTable.vue";

useDocumentTitle("Host Overview");

const loader = useLoadingStore();
const system = useSystemStore();

const processesView = ref<"charts" | "table">("charts");
const processesData = computed(() =>
  system.live && system.realtime.processes.length > 0
    ? system.realtime.processes
    : system.data.processes,
);

onBeforeMount(() => {
  const connectionType = system.connectionType ?? config.api.connectionType;
  system.connect({ websocket: connectionType === "websocket", refresh: false });
});
</script>

<template>
  <Transition name="fade">
    <div v-if="loader.loaded" class="container-fluid py-0 py-md-2 dashboard-root">
      <!-- Top stat cards -->
      <section id="statistics" class="page-section mt-0">
        <div class="row g-2">
          <div class="col-sm-12 col-md-6 col-lg-3 d-flex align-items-stretch">
            <StatCard title="Platform" icon="fa-solid fa-server" tone="blue">
              <template #detail>
                <PlatformDetail
                  :detail="system.data.platform"
                  :uptime="system.realtime.uptime"
                />
              </template>
            </StatCard>
          </div>
          <div class="col-sm-12 col-md-6 col-lg-3 d-flex align-items-stretch">
            <StatCard title="CPU" icon="fa-solid fa-microchip" tone="purple">
              <template #detail>
                <CpuDetail
                  :detail="system.live ? system.realtime.cpu : system.data.cpu"
                />
              </template>
            </StatCard>
          </div>
          <div class="col-sm-12 col-md-6 col-lg-3 d-flex align-items-stretch">
            <StatCard title="Memory" icon="fa-solid fa-memory" tone="amber">
              <template #detail>
                <UsageDetail
                  :detail="system.live ? system.realtime.mem : system.data.mem"
                />
              </template>
            </StatCard>
          </div>
          <div class="col-sm-12 col-md-6 col-lg-3 d-flex align-items-stretch">
            <StatCard title="Disk" icon="fa-solid fa-hard-drive" tone="green">
              <template #detail>
                <UsageDetail
                  :detail="system.live ? system.realtime.disk : system.data.disk"
                />
              </template>
            </StatCard>
          </div>
        </div>
      </section>

      <!-- Row 2: services / compact resource gauges -->
      <section id="status-row" class="page-section">
        <div class="row g-2">
          <div class="col-sm-12 col-lg-6 d-flex">
            <ServiceStatus class="flex-fill" />
          </div>
          <div class="col-sm-12 col-lg-6 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="System Resources"
                subtitle="Real-time utilization"
                icon="fa-solid fa-gauge-high"
                tone="blue"
              />
              <div class="panel-body resource-gauges-body">
                <div class="resource-gauge-cell">
                  <GaugeChart
                    title="CPU"
                    id="cpu"
                    :metric="
                      system.live
                        ? system.realtime.cpu.usage
                        : system.data.cpu.usage
                    "
                    format="{y}%"
                  />
                </div>
                <div class="resource-gauge-cell">
                  <GaugeChart
                    title="Memory"
                    id="mem"
                    :metric="
                      system.live
                        ? system.realtime.mem.percent
                        : system.data.mem.percent
                    "
                    format="{y}%"
                  />
                </div>
                <div class="resource-gauge-cell">
                  <GaugeChart
                    title="Disk"
                    id="disk"
                    :metric="
                      system.live
                        ? system.realtime.disk.percent
                        : system.data.disk.percent
                    "
                    format="{y}%"
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- Row 3: realtime line charts (live mode only) -->
      <section id="d3-line-charts" v-if="system.live" class="page-section">
        <div class="row g-2">
          <div class="col-sm-12 col-md-6 col-lg-3">
            <div class="card panel-card border-0 shadow-lg">
              <SectionHeader
                title="Network Traffic"
                subtitle="Live in/out · 30s"
                icon="fa-solid fa-tower-broadcast"
                tone="blue"
              />
              <div class="panel-body p-0">
                <NetworkTrafficChart
                  :rx="system.realtime.network?.rx_bytes_per_sec ?? 0"
                  :tx="system.realtime.network?.tx_bytes_per_sec ?? 0"
                />
              </div>
            </div>
          </div>
          <div class="col-sm-12 col-md-6 col-lg-3">
            <div class="card panel-card border-0 shadow-lg">
              <SectionHeader
                title="Disk I/O"
                subtitle="Live read/write · 30s"
                icon="fa-solid fa-hard-drive"
                tone="green"
              />
              <div class="panel-body p-0">
                <DiskIoChart
                  :read="system.realtime.disk_io?.read_bytes_per_sec ?? 0"
                  :write="system.realtime.disk_io?.write_bytes_per_sec ?? 0"
                  :read-iops="system.realtime.disk_io?.read_iops ?? 0"
                  :write-iops="system.realtime.disk_io?.write_iops ?? 0"
                />
              </div>
            </div>
          </div>
          <div class="col-sm-12 col-md-6 col-lg-3">
            <div class="card panel-card border-0 shadow-lg">
              <SectionHeader
                title="CPU Usage"
                subtitle="Last 60s · %"
                icon="fa-solid fa-wave-square"
                tone="purple"
              />
              <div class="panel-body p-0">
                <RealtimeLineChart
                  :data-point="system.realtime.cpu.usage"
                  :y-axis-range="[0, 100]"
                  tone="purple"
                  unit="%"
                  :decimals="0"
                />
              </div>
            </div>
          </div>
          <div class="col-sm-12 col-md-6 col-lg-3">
            <div class="card panel-card border-0 shadow-lg">
              <SectionHeader
                title="Memory Usage"
                subtitle="System total · 60s · GiB"
                icon="fa-solid fa-wave-square"
                tone="amber"
              />
              <div class="panel-body p-0">
                <RealtimeLineChart
                  :data-point="system.realtime.mem.used"
                  :y-axis-range="[0, 16]"
                  tone="amber"
                  unit="GiB"
                  :decimals="1"
                />
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- Row 4: top processes (wide) + storage breakdown -->
      <section id="processes-and-storage" class="page-section">
        <div class="row g-2">
          <div class="col-sm-12 col-lg-8 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="Top Processes"
                subtitle="Per-process RSS · top 10"
                icon="fa-solid fa-list-ul"
                tone="green"
              >
                <template #right>
                  <div
                    class="view-toggle btn-group btn-group-sm"
                    role="group"
                    aria-label="Top processes view"
                  >
                    <button
                      type="button"
                      class="btn"
                      :class="{ active: processesView === 'charts' }"
                      :aria-pressed="processesView === 'charts'"
                      title="Charts view"
                      @click="processesView = 'charts'"
                    >
                      <font-awesome-icon icon="fa-solid fa-chart-pie" />
                    </button>
                    <button
                      type="button"
                      class="btn"
                      :class="{ active: processesView === 'table' }"
                      :aria-pressed="processesView === 'table'"
                      title="Table view"
                      @click="processesView = 'table'"
                    >
                      <font-awesome-icon icon="fa-solid fa-table-list" />
                    </button>
                  </div>
                </template>
              </SectionHeader>
              <div class="panel-body">
                <div
                  v-if="processesView === 'charts'"
                  class="row align-items-center g-2"
                >
                  <div class="col-sm-12 col-md-6 col-lg-8">
                    <BarChart
                      metric="system"
                      id="processes"
                      title=""
                      :series="
                        system.formatBarChartDataForSystem(
                          processesData,
                          'mem',
                        )
                      "
                      sort-key="data"
                      sort-order="desc"
                      y-axis-text="Memory Used"
                      x-axis-text="System Process"
                    />
                  </div>
                  <div class="col-sm-12 col-md-6 col-lg-4">
                    <PieChart
                      id="system-processes"
                      title=""
                      :series="system.formatPieChartDataForProcesses(processesData)"
                    />
                  </div>
                </div>
                <DataTable
                  v-else
                  type="horizontal"
                  :data="processesData"
                  sort-key="mem"
                  sort-order="desc"
                />
              </div>
            </div>
          </div>
          <div class="col-sm-12 col-lg-4 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="Storage"
                subtitle="Mounted partitions"
                icon="fa-solid fa-hard-drive"
                tone="green"
              />
              <div class="panel-body">
                <DiskList :disks="system.data.disks ?? []" />
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  </Transition>
</template>

<style scoped>
@media (max-width: 575.98px) {
  .dashboard-root { padding-top: 1rem !important; padding-bottom: 1rem !important; }
}

/* Compact gauge row inside the row-2 "System Resources" panel.
   Three gauges share the panel-body horizontally; each cell is a CSS
   container so gauge-value text scales fluidly with the gauge size. */
.resource-gauges-body {
  display: flex;
  flex-direction: row;
  align-items: stretch;
  justify-content: space-around;
  gap: 0.4rem;
  height: 100%;
}
.resource-gauge-cell {
  flex: 1 1 0;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
}
.resource-gauge-cell :deep(.gauge-svg-wrap) {
  max-width: 130px;
}

/* View toggle in the Top Processes panel header */
.view-toggle .btn {
  background: rgba(148, 163, 184, 0.08);
  color: #cbd5e1;
  border: 1px solid rgba(96, 165, 250, 0.22);
  padding: 0.25rem 0.55rem;
  line-height: 1;
  position: relative;
  z-index: 1;
}
.view-toggle .btn + .btn {
  border-left: none;
}
.view-toggle .btn:hover {
  background: rgba(96, 165, 250, 0.14);
  color: #f1f5f9;
}
.view-toggle .btn.active {
  background: rgba(16, 185, 129, 0.18);
  color: #34d399;
  border-color: rgba(16, 185, 129, 0.4);
  box-shadow: inset 0 0 0 1px rgba(16, 185, 129, 0.25);
}
.view-toggle .btn:focus {
  outline: none;
  box-shadow: 0 0 0 2px rgba(96, 165, 250, 0.35);
}
</style>
