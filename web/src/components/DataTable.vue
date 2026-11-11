<script setup lang="ts">
import { ref, watch, computed, onMounted, type Ref } from "vue";
import { orderBy } from "lodash";
import { sizeText } from "@/utilities/format-size";

const props = defineProps<{
  type: string;
  data: any;
  nested?: boolean;
  sortKey?: string;
  sortOrder?: string;
  excludeColumns?: Array<string>;
}>();

const tableData = ref(props.data);
const tableSortKey = ref("mem");
const tableSortOrder: Ref<"asc" | "desc"> = ref("desc");

// Process memory (``mem``) arrives in MiB; show it in MiB / GiB / TiB.
const formatCell = (key: string, value: unknown) =>
  key === "mem" && isNumeric(value) ? sizeText(Number(value), "MiB") : value;

const formatHeader = (header: string) => {
  header = header.charAt(0).toUpperCase() + header.slice(1);
  return header.replace("_", " ");
};

const isNumeric = (value: unknown): boolean => {
  if (typeof value === "number") return Number.isFinite(value);
  if (typeof value === "string" && value.trim() !== "")
    return Number.isFinite(Number(value));
  return false;
};

const getFilteredHeaders = (headers: string[]) => {
  if (props.excludeColumns && props.excludeColumns.length > 0) {
    return headers.filter((header) => !props.excludeColumns?.includes(header));
  }
  return headers;
};

const changeOrder = (key: string) => {
  tableSortKey.value = key;
  tableSortOrder.value = tableSortOrder.value === "asc" ? "desc" : "asc";
  tableData.value = orderBy(tableData.value, key, tableSortOrder.value);
};

const transpose = () => {
  const response: Array<{ header: string; value: string }> = [];

  for (const key in tableData) {
    let value = tableData[key as keyof typeof tableData];

    if (Number.isInteger(value)) {
      value = parseFloat(tableData[key as keyof typeof tableData]).toFixed(2);
    }

    response.push({
      header: formatHeader(key),
      value,
    });
  }

  return response;
};

const verticalData = computed(() => {
  const data = transpose();
  return orderBy(data, props.sortKey, tableSortOrder.value);
});

onMounted(() => {
  watch(
    () => props.data,
    () => {
      tableData.value = props.data;
    },
  );
});
</script>

<template>
  <table
    v-if="type === 'vertical'"
    class="data-table table table-sm table-borderless table-responsive"
  >
    <tr v-for="(content, key) in verticalData" :key="key">
      <th>{{ content.header }}</th>
      <td :class="{ mono: isNumeric(content.value) }">{{ content.value }}</td>
    </tr>
  </table>

  <table
    v-else
    class="data-table table table-sm table-striped table-responsive-sm"
  >
    <thead>
      <tr v-if="nested">
        <th
          class="cursor-pointer"
          v-for="(header, key) in getFilteredHeaders(Object.keys(tableData))"
          :key="key"
          @click="changeOrder(header)"
        >
          <span class="me-1 indicators">
            <font-awesome-icon
              v-if="tableSortOrder === 'asc' && tableSortKey === header"
              :class="{ active: tableSortKey === header }"
              icon="fa-solid fa-caret-up"
            />
            <font-awesome-icon
              v-else
              :class="{ active: tableSortKey === header }"
              icon="fa-solid fa-caret-down"
            />
          </span>
          {{ formatHeader(header) }}
        </th>
      </tr>
      <tr v-else>
        <th
          class="cursor-pointer"
          v-for="(header, key) in getFilteredHeaders(Object.keys(tableData[0]))"
          :key="key"
          @click="changeOrder(header)"
        >
          <span class="me-1 indicators">
            <font-awesome-icon
              v-if="tableSortOrder === 'asc' && tableSortKey === header"
              :class="{ active: tableSortKey === header }"
              icon="fa-solid fa-caret-up"
            />
            <font-awesome-icon
              v-else
              :class="{ active: tableSortKey === header }"
              icon="fa-solid fa-caret-down"
            />
          </span>
          {{ formatHeader(header) }}
        </th>
      </tr>
    </thead>
    <tbody v-if="nested">
      <tr>
        <td
          v-for="(object, key) in getFilteredHeaders(Object.keys(tableData))"
          :key="key"
          :class="{ mono: isNumeric(tableData[object]) }"
        >
          {{ tableData[object] }}
        </td>
      </tr>
    </tbody>
    <tbody v-else>
      <tr v-for="(object, key) in tableData" :key="key">
        <td
          v-for="(content, index) in getFilteredHeaders(Object.keys(object))"
          :key="index"
          :class="{ mono: isNumeric(object[content]) }"
        >
          {{ formatCell(content, object[content]) }}
        </td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
/* Brand-aligned data table.
   Reuses Vigil tokens (--bg-*, --text-*, --border-*, --fs-*) from the global
   theme stylesheets; falls back to sensible light-mode values when tokens
   aren't defined. Spacing follows Bootstrap's table-sm rhythm — keep as-is. */
.data-table {
  --bs-table-bg: transparent;
  --bs-table-color: var(--text-primary, #1f2937);
  --bs-table-striped-bg: rgba(148, 163, 184, 0.05);
  --bs-table-striped-color: var(--text-primary, #1f2937);
  --bs-table-border-color: var(--border-subtle, rgba(148, 163, 184, 0.1));

  margin-bottom: 0;
  font-family: "Lato", system-ui, -apple-system, "Segoe UI", sans-serif;
  font-size: var(--fs-body);
  color: var(--text-primary, #1f2937);
  border-collapse: separate;
  border-spacing: 0;
}

.data-table thead th {
  font-family: "IBM Plex Sans", "Inter", system-ui, sans-serif;
  font-size: var(--fs-micro);
  font-weight: 600;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--text-muted, #6b7280);
  background: transparent;
  border-bottom: 1px solid var(--border-accent, rgba(96, 165, 250, 0.22));
  white-space: nowrap;
  user-select: none;
  vertical-align: middle;
}

.data-table thead th.cursor-pointer {
  transition: color 0.15s ease;
}
.data-table thead th.cursor-pointer:hover {
  color: var(--accent-blue, #2563eb);
}

.data-table thead th .indicators {
  display: inline-block;
  width: 0.7em;
  color: var(--text-dim, #94a3b8);
  opacity: 0.45;
  transition: color 0.15s ease, opacity 0.15s ease;
}
.data-table thead th .indicators .active {
  color: var(--accent-blue, #2563eb);
  opacity: 1;
}

.data-table tbody td {
  vertical-align: middle;
  border-bottom: 1px solid
    var(--border-subtle, rgba(148, 163, 184, 0.08));
  color: var(--text-primary, #1f2937);
}
.data-table tbody tr:last-child td {
  border-bottom: none;
}

.data-table tbody tr {
  transition: background 0.12s ease;
}
.data-table tbody tr:hover td {
  background: rgba(96, 165, 250, 0.07);
  color: var(--text-primary, #0f172a);
}

/* Numeric cells: IBM Plex Mono with tabular figures (brand rule #6) */
.data-table td.mono {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas,
    monospace;
  font-variant-numeric: tabular-nums;
  font-feature-settings: "tnum";
  font-size: var(--fs-caption);
  color: var(--text-secondary, #475569);
  white-space: nowrap;
}

/* Vertical (key/value) variant — header column reads as an eyebrow label */
.data-table.table-borderless th {
  font-family: "IBM Plex Sans", "Inter", system-ui, sans-serif;
  font-size: var(--fs-micro);
  font-weight: 600;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--text-muted, #6b7280);
  border: none;
  white-space: nowrap;
  width: 1%;
  padding-right: 1rem;
}
.data-table.table-borderless td {
  border: none;
}
</style>
