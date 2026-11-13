<script setup lang="ts">
/**
 * The service checks in one line while all is well. Any check that isn't
 * well is listed under it, and a press on the line lists them all.
 */
import { computed, ref } from "vue";
import { serviceLabel, useProbes } from "@/composables/useProbes";

const { state, overall } = useProbes();
const open = ref(false);

const shown = computed(() =>
  open.value ? state.value : state.value.filter((s) => s.status === "offline" || s.status === "slow"),
);
</script>

<template>
  <section v-if="state.length > 0" class="compact-panel">
    <button type="button" class="panel-head head-button" :aria-expanded="open" @click="open = !open">
      <span class="panel-icon tone-red"><font-awesome-icon icon="fa-solid fa-heart-pulse" /></span>
      <span class="panel-title">Services</span>
      <span class="count">{{ state.length }}</span>
      <span class="overall" :class="`tone-${overall.tone}`">
        <span class="overall-dot"></span>{{ overall.label }}
      </span>
      <font-awesome-icon
        class="head-arrow"
        :icon="open ? 'fa-solid fa-arrow-up' : 'fa-solid fa-arrow-down'"
      />
    </button>
    <ul v-if="shown.length > 0" class="service-rows">
      <li v-for="svc in shown" :key="svc.url" class="service-row" :class="`is-${svc.status}`">
        <span class="dot" :title="serviceLabel(svc.status)"></span>
        <a class="name" :href="svc.url" target="_blank" rel="noopener noreferrer" :title="svc.url">{{
          svc.name
        }}</a>
        <span v-if="svc.statusCode != null" class="code">{{ svc.statusCode }}</span>
        <span v-if="svc.status === 'offline'" class="latency error" :title="svc.error ?? 'no response'">
          {{ svc.error ?? "no response" }}
        </span>
        <span v-else-if="svc.status === 'pending'" class="latency">–</span>
        <span v-else class="latency">{{ svc.latency }}<span class="unit">ms</span></span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.head-button {
  width: 100%;
  border: 0;
  background: none;
  text-align: left;
  cursor: pointer;
}
.head-button:focus-visible {
  outline: 2px solid var(--c-accent);
  outline-offset: -2px;
  border-radius: 9px;
}
.count {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 10px;
  color: var(--c-muted);
}
.overall {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 7px;
  border-radius: 999px;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  white-space: nowrap;
  color: var(--pill);
  background: color-mix(in srgb, var(--pill) 12%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--pill) 30%, transparent);
}
.overall-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}
.overall.tone-operational { --pill: var(--c-ok); }
.overall.tone-degraded    { --pill: var(--c-warn); }
.overall.tone-down        { --pill: var(--c-crit); }
.overall.tone-pending     { --pill: var(--c-muted); }
.head-arrow {
  flex: 0 0 auto;
  font-size: 9px;
  color: var(--c-dim);
}

.service-rows {
  margin: 0;
  padding: 0;
  list-style: none;
}
.service-row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 22px;
  padding: 0 10px;
  border-top: 1px solid var(--c-hair);
  font-size: 11px;
  min-width: 0;
}
.dot {
  flex: 0 0 auto;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--c-muted);
}
.is-online .dot  { background: var(--c-ok); }
.is-slow .dot    { background: var(--c-warn); }
.is-offline .dot { background: var(--c-crit); box-shadow: 0 0 6px var(--c-crit); }
.name {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--c-text);
  text-decoration: none;
}
.name:hover { text-decoration: underline; }
.code,
.latency {
  flex: 0 0 auto;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-variant-numeric: tabular-nums;
}
.code {
  padding: 0 5px;
  border-radius: 4px;
  font-size: 9.5px;
  color: var(--c-muted);
  background: var(--c-track);
}
.latency { color: var(--c-text); }
.latency.error {
  flex: 0 1 auto;
  min-width: 0;
  max-width: 55%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--c-crit);
}
.unit {
  margin-left: 2px;
  font-size: 9.5px;
  color: var(--c-muted);
}
</style>
