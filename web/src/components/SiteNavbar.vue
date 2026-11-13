<script setup lang="ts">
import { computed } from "vue";
import { config } from "@/config";
import { useThemeStore } from "@/stores/theme";
import { useSystemStore } from "@/stores/system";
import { useControlStore } from "@/stores/control";
import { useWindowFrame } from "@/composables/useWindowFrame";
import { useCompactLayout } from "@/composables/useCompactLayout";
import PageHeader from "@/components/PageHeader.vue";
import SiteLogo from "@/components/SiteLogo.vue";

const theme = useThemeStore();
const system = useSystemStore();
const control = useControlStore();
const frame = useWindowFrame();
const { compact } = useCompactLayout();

// In the Vigil window this bar is the title bar: pressing an empty part of
// it moves the window, and a double press maximises it or puts it back.
function barPressed(event: MouseEvent) {
  if (!frame.framed.value || event.button !== 0) return;
  const target = event.target as Element | null;
  if (target?.closest("a, button, input, label, .window-buttons, .resize-edge")) return;
  frame.ask(event.detail === 2 ? "maximize" : "drag");
}
const node = computed(() => {
  const region = config.app.deployment.region;
  const instance = config.app.deployment.instance;
  if (!region && !instance) return null;
  return [region, instance].filter(Boolean).join(" · ").toUpperCase();
});
</script>

<template>
  <nav
    class="navbar navbar-expand-lg site-navbar shadow-lg"
    :class="{ 'is-window': frame.framed.value, 'is-compact': compact }"
    @mousedown="barPressed"
  >
    <div class="container-fluid py-2 nav-container">
      <RouterLink class="navbar-brand d-flex align-items-center gap-3" to="/">
        <SiteLogo />
        <!-- The Vigil window's bar is a title bar: the mark alone. -->
        <PageHeader
          v-if="!frame.framed.value"
          :decor-title="config.app.name"
          :title="config.app.title"
          compact
        />
      </RouterLink>
      <button
        class="navbar-toggler"
        type="button"
        data-bs-toggle="collapse"
        data-bs-target="#navbarSupportedContent"
        aria-controls="navbarSupportedContent"
        aria-expanded="false"
        aria-label="Toggle navigation"
      >
        <span class="navbar-toggler-icon"></span>
      </button>
      <div class="collapse navbar-collapse" id="navbarSupportedContent">
        <ul class="navbar-nav me-auto mb-0"></ul>

        <div class="nav-controls">
          <!-- Status indicator -->
          <span
            v-if="$route.name !== 'settings'"
            class="status-chip"
            :class="`status-${system.connectionType}`"
          >
            <span class="status-dot"></span>
            <span class="status-text">{{
              system.connectionType === "websocket" ? "LIVE" : "STATIC"
            }}</span>
          </span>

          <!-- Node identifier -->
          <span v-if="node" class="node-chip" :title="node">
            <font-awesome-icon icon="fa-solid fa-server" class="node-icon" />
            <span class="node-text">{{ node }}</span>
          </span>

          <span class="control-divider" aria-hidden="true"></span>

          <!-- Settings: only in the Vigil window -->
          <RouterLink
            v-if="control.available"
            class="nav-button"
            :to="$route.name === 'settings' ? '/' : '/settings'"
            :title="$route.name === 'settings' ? 'Back to the dashboard' : 'Settings'"
            :aria-label="$route.name === 'settings' ? 'Back to the dashboard' : 'Settings'"
          >
            <font-awesome-icon
              :icon="$route.name === 'settings' ? 'fa-solid fa-gauge-high' : 'fa-solid fa-gear'"
            />
          </RouterLink>

          <!-- Theme toggle -->
          <label
            class="nav-toggle"
            title="Toggle dark mode"
            aria-label="Toggle dark mode"
          >
            <input
              type="checkbox"
              @change="theme.toggle"
              :checked="theme.active === 'dark'"
            />
            <span class="nav-toggle-track">
              <font-awesome-icon
                icon="fa-solid fa-sun"
                class="track-icon track-icon-left"
              />
              <font-awesome-icon
                icon="fa-solid fa-moon"
                class="track-icon track-icon-right"
              />
              <span class="nav-toggle-thumb"></span>
            </span>
          </label>

          <!-- Connection mode toggle -->
          <label
            v-if="$route.name !== 'settings'"
            class="nav-toggle"
            title="Toggle live websocket"
            aria-label="Toggle live websocket"
          >
            <input
              type="checkbox"
              @change="system.toggle"
              :checked="system.connectionType === 'websocket'"
            />
            <span class="nav-toggle-track">
              <font-awesome-icon
                icon="fa-solid fa-bolt"
                class="track-icon track-icon-left"
              />
              <font-awesome-icon
                icon="fa-solid fa-pause"
                class="track-icon track-icon-right"
              />
              <span class="nav-toggle-thumb"></span>
            </span>
          </label>
        </div>
      </div>
    </div>

    <!-- The Vigil window's own buttons; it has no title bar but this one. -->
    <template v-if="frame.framed.value">
      <div class="window-buttons">
        <button
          type="button"
          class="window-button"
          title="Minimise"
          aria-label="Minimise"
          @click="frame.ask('minimize')"
        >
          <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
        </button>
        <button
          type="button"
          class="window-button"
          :title="frame.maximized.value ? 'Restore' : 'Maximise'"
          :aria-label="frame.maximized.value ? 'Restore' : 'Maximise'"
          @click="frame.ask('maximize')"
        >
          <svg v-if="frame.maximized.value" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M2.5 2.5v-2h7v7h-2M0.5 2.5h7v7h-7z" />
          </svg>
          <svg v-else viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0.5 0.5h9v9h-9z" />
          </svg>
        </button>
        <button
          type="button"
          class="window-button window-close"
          title="Close"
          aria-label="Close"
          @click="frame.ask('close')"
        >
          <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" /></svg>
        </button>
      </div>
      <!-- The sides and bottom of the window have Windows' own resize
           borders. The top edge is this bar, so it does the same job. -->
      <template v-if="!frame.maximized.value">
        <div class="resize-edge resize-top" @mousedown.left.prevent="frame.ask('resize:top')"></div>
        <div
          class="resize-edge resize-top-left"
          @mousedown.left.prevent="frame.ask('resize:top-left')"
        ></div>
        <div
          class="resize-edge resize-top-right"
          @mousedown.left.prevent="frame.ask('resize:top-right')"
        ></div>
      </template>
    </template>
  </nav>
</template>

<style scoped>
.site-navbar {
  background: var(--header-grad);
  color: #f1f5f9;
  position: sticky;
  top: 0;
  z-index: 1030;
  overflow: hidden;
  border-bottom: 1px solid rgba(96, 165, 250, 0.22);
}
.site-navbar::before {
  content: "";
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(96, 165, 250, 0.04) 1px, transparent 1px),
    linear-gradient(90deg, rgba(96, 165, 250, 0.04) 1px, transparent 1px);
  background-size: 32px 32px;
  pointer-events: none;
  opacity: 0.6;
}
.site-navbar::after {
  content: "";
  position: absolute;
  inset: 0;
  background:
    radial-gradient(700px circle at 0% 0%, rgba(59, 130, 246, 0.16), transparent 50%),
    radial-gradient(500px circle at 100% 100%, rgba(34, 211, 238, 0.1), transparent 50%);
  pointer-events: none;
}
.nav-container { position: relative; z-index: 1; }

.navbar-toggler {
  border-color: rgba(255, 255, 255, 0.15);
  padding: 0.35rem 0.55rem;
}
.navbar-toggler-icon {
  filter: invert(1);
  opacity: 0.8;
}

/* ---------- Right-side controls ---------- */
.nav-controls {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.control-divider {
  width: 1px;
  height: 22px;
  background: rgba(255, 255, 255, 0.1);
  margin: 0 0.1rem;
}

/* ---------- Status chip ---------- */
.status-chip {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.32rem 0.7rem;
  border-radius: 6px;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.16em;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  white-space: nowrap;
}
.status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 10px currentColor;
}
.status-websocket { color: #34d399; }
.status-http      { color: #fbbf24; }
.status-websocket .status-dot {
  animation: live-pulse 1.4s ease-in-out infinite;
}

/* ---------- Node identifier chip ---------- */
.node-chip {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.32rem 0.7rem;
  border-radius: 6px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-micro);
  font-weight: 600;
  letter-spacing: 0.06em;
  color: #cbd5e1;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
  max-width: 220px;
  overflow: hidden;
}
.node-icon { font-size: var(--fs-caption); color: #60a5fa; flex-shrink: 0; }
.node-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

/* ---------- Settings button ---------- */
.nav-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 24px;
  border-radius: 999px;
  color: #cbd5e1;
  font-size: var(--fs-caption);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  transition: background 0.2s ease, border-color 0.2s ease, color 0.2s ease;
}
.nav-button:hover,
.nav-button:focus-visible {
  color: #ffffff;
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.18);
}

/* ---------- The Vigil window's title bar ---------- */
/* One slim row, as on any desktop program: the mark on the left, then the
   dashboard's controls and the window's own buttons on the right. */
.site-navbar.is-window {
  --bar-height: 40px;
  /* Above the loading overlay: the window must always be movable and
     closable, whatever the page is doing. */
  z-index: 1060;
  flex: 0 0 auto;
  min-height: var(--bar-height);
  padding: 0;
  user-select: none;
  box-shadow: none !important;
}
.is-window .nav-container {
  flex-wrap: nowrap;
  height: var(--bar-height);
  /* The right-hand side is the window buttons'. */
  padding: 0 calc(138px + 0.6rem) 0 0.75rem !important;
}
.is-window .navbar-brand {
  margin: 0;
  padding: 0;
}
.is-window .navbar-brand :deep(.site-logo) {
  width: 24px;
  height: 24px;
  filter: drop-shadow(0 2px 8px rgba(34, 211, 238, 0.35));
}
/* Never folds into a menu: the row is short enough at any window size. */
.is-window .navbar-toggler { display: none; }
.is-window .navbar-collapse {
  display: flex !important;
  flex-basis: auto;
  min-width: 0;
  padding-top: 0;
}
.is-window .nav-controls {
  flex-wrap: nowrap;
  justify-content: flex-end;
  gap: 0.5rem;
  min-width: 0;
}
.is-window .control-divider { display: block; height: 16px; }
.is-window .status-chip,
.is-window .node-chip {
  padding: 0.2rem 0.55rem;
}
.is-window .node-chip { min-width: 0; }

.window-buttons {
  position: absolute;
  top: 0;
  right: 0;
  z-index: 2;
  display: flex;
}
.window-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  height: var(--bar-height);
  padding: 0;
  border: 0;
  background: transparent;
  color: #cbd5e1;
  transition: background 0.12s ease, color 0.12s ease;
}
.window-button svg {
  width: 10px;
  height: 10px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1;
  shape-rendering: crispEdges;
}
.window-close svg { shape-rendering: geometricPrecision; }
.window-button:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #ffffff;
}
.window-button:active { background: rgba(255, 255, 255, 0.16); }
.window-button:focus-visible {
  outline: 2px solid #60a5fa;
  outline-offset: -2px;
}
.window-close:hover {
  background: #c42b1c;
  color: #ffffff;
}
.window-close:active { background: #b3271a; }

.resize-edge {
  position: absolute;
  top: 0;
  z-index: 3;
}
.resize-top {
  left: 0;
  right: 0;
  height: 4px;
  cursor: ns-resize;
}
.resize-top-left,
.resize-top-right {
  width: 10px;
  height: 10px;
}
.resize-top-left {
  left: 0;
  cursor: nwse-resize;
}
.resize-top-right {
  right: 0;
  cursor: nesw-resize;
}

/* ---------- The bar of the small window ---------- */
/* Everything a size down, so the bar is in proportion to the compact
   dashboard beneath it. */
.site-navbar.is-compact { --bar-height: 32px; }
.is-compact .nav-container {
  padding: 0 calc(120px + 0.4rem) 0 0.6rem !important;
}
.is-compact .navbar-brand :deep(.site-logo) {
  width: 20px;
  height: 20px;
}
.is-compact .nav-controls { gap: 0.35rem; }
.is-compact .control-divider { display: none; }
.is-compact .status-chip {
  gap: 0.35rem;
  padding: 0.12rem 0.45rem;
  font-size: 9px;
}
.is-compact .status-dot {
  width: 6px;
  height: 6px;
}
.is-compact .node-chip { display: none; }
.is-compact .nav-button {
  width: 26px;
  height: 20px;
  font-size: 10px;
}
.is-compact .nav-toggle-track {
  width: 40px;
  height: 20px;
  padding: 0 5px;
}
.is-compact .track-icon { font-size: 8px; }
.is-compact .nav-toggle-thumb {
  width: 14px;
  height: 14px;
}
.is-compact .nav-toggle input:checked ~ .nav-toggle-track .nav-toggle-thumb {
  transform: translateX(20px);
}
.is-compact .window-button { width: 40px; }
/* Narrower than this, the word goes and the dot says it. */
@media (max-width: 379.98px) {
  .is-compact .status-text { display: none; }
}

/* ---------- Toggle switches ---------- */
.nav-toggle {
  position: relative;
  display: inline-flex;
  align-items: center;
  cursor: pointer;
  margin: 0;
}
.nav-toggle input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.nav-toggle-track {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  width: 50px;
  height: 24px;
  padding: 0 6px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  transition: background 0.2s ease, border-color 0.2s ease;
}
.nav-toggle:hover .nav-toggle-track {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.18);
}

.track-icon {
  font-size: var(--fs-micro);
  z-index: 1;
  transition: color 0.2s ease;
}
.track-icon-left  { color: #fbbf24; }
.track-icon-right { color: #94a3b8; }

.nav-toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 18px;
  height: 18px;
  background: #f9fafb;
  border-radius: 50%;
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.45);
  transition: transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
}
.nav-toggle input:checked ~ .nav-toggle-track .nav-toggle-thumb {
  transform: translateX(26px);
}
.nav-toggle input:checked ~ .nav-toggle-track .track-icon-left  { color: #6b7280; }
.nav-toggle input:checked ~ .nav-toggle-track .track-icon-right { color: #c084fc; }

@keyframes live-pulse {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.4; }
}

/* ---------- Mobile ---------- */
@media (max-width: 991.98px) {
  .navbar-collapse { padding-top: 0.75rem; }
  .nav-controls {
    justify-content: flex-start;
    gap: 0.5rem;
  }
  .control-divider { display: none; }
  .node-chip { max-width: 100%; }
}

@media (max-width: 575.98px) {
  .nav-container { padding: 0.5rem !important; }
  .navbar-brand { gap: 0 !important; }
  .status-chip,
  .node-chip {
    padding: 0.28rem 0.55rem;
    letter-spacing: 0.1em;
  }
}
</style>
