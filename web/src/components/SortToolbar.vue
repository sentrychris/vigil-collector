<script setup lang="ts">
/**
 * Generic, reusable sort selector.
 *
 * Renders a small inline toolbar of pill buttons, one per sort option.
 * Selecting the active option flips its direction; selecting an inactive
 * one switches to that field with its `defaultDirection`.
 *
 * Usage:
 *   <SortToolbar
 *     v-model="sort"
 *     :options="[
 *       { value: 'status', label: 'Status', defaultDirection: 'asc',
 *         tooltip: 'Unhealthy first' },
 *       { value: 'name',   label: 'Name',   defaultDirection: 'asc' },
 *     ]"
 *   />
 *
 * The component is field-agnostic — sort comparators live in the caller.
 */
export type SortDirection = "asc" | "desc";

export interface SortState {
  field: string;
  direction: SortDirection;
}

export interface SortOption {
  value: string;
  label: string;
  defaultDirection?: SortDirection;
  /** Tooltip — useful when ascending/descending semantics aren't obvious
   *  for the field (e.g. status: "ascending = unhealthy first"). */
  tooltip?: string;
}

const props = defineProps<{
  modelValue: SortState;
  options: SortOption[];
  /** Eyebrow label rendered to the left of the toolbar. Pass empty
   *  string to suppress. */
  eyebrow?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [SortState];
}>();

function pick(opt: SortOption): void {
  if (props.modelValue.field === opt.value) {
    emit("update:modelValue", {
      field: opt.value,
      direction: props.modelValue.direction === "asc" ? "desc" : "asc",
    });
  } else {
    emit("update:modelValue", {
      field: opt.value,
      direction: opt.defaultDirection ?? "asc",
    });
  }
}
</script>

<template>
  <div class="sort-toolbar" role="toolbar" :aria-label="`Sort by ${eyebrow ?? 'field'}`">
    <span v-if="eyebrow !== ''" class="st-eyebrow" aria-hidden="true">
      {{ eyebrow ?? "SORT" }}
    </span>
    <button
      v-for="opt in options"
      :key="opt.value"
      type="button"
      class="st-btn"
      :class="{ 'is-active': modelValue.field === opt.value }"
      :aria-pressed="modelValue.field === opt.value"
      :title="opt.tooltip || opt.label"
      @click="pick(opt)"
    >
      <span class="st-label">{{ opt.label }}</span>
      <span v-if="modelValue.field === opt.value" class="st-arrow" aria-hidden="true">
        <font-awesome-icon
          :icon="modelValue.direction === 'asc'
            ? 'fa-solid fa-caret-up'
            : 'fa-solid fa-caret-down'"
        />
      </span>
    </button>
  </div>
</template>

<style scoped>
.sort-toolbar {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  flex-wrap: nowrap;
}
.st-eyebrow {
  font-family: "IBM Plex Mono", ui-monospace, monospace;
  font-size: 0.6rem;
  font-weight: 700;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: #94a3b8;
  margin-right: 0.15rem;
  user-select: none;
}

.st-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.28rem 0.6rem;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  color: #cbd5e1;
  cursor: pointer;
  font-family: "IBM Plex Sans", "Inter", system-ui, sans-serif;
  font-size: 0.7rem;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  line-height: 1;
  transition: background 140ms ease, border-color 140ms ease, color 140ms ease;
}
.st-btn:hover {
  background: rgba(255, 255, 255, 0.07);
  border-color: rgba(96, 165, 250, 0.32);
  color: #f1f5f9;
}
.st-btn:focus-visible {
  outline: 2px solid #60a5fa;
  outline-offset: 2px;
}
.st-btn.is-active {
  background: rgba(96, 165, 250, 0.16);
  border-color: rgba(96, 165, 250, 0.55);
  color: #60a5fa;
}
.st-btn.is-active:hover {
  background: rgba(96, 165, 250, 0.22);
  color: #93c5fd;
}

.st-label { line-height: 1; }
.st-arrow {
  display: inline-flex;
  font-size: 0.78rem;
  margin-left: -0.05rem;
  line-height: 1;
}

@media (max-width: 575.98px) {
  .st-eyebrow { display: none; }
  .st-btn { padding: 0.24rem 0.5rem; font-size: 0.66rem; }
}
</style>
