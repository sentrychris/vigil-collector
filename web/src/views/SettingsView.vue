<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useControlStore } from "@/stores/control";
import { useDocumentTitle } from "@/composables/useDocumentTitle";
import SectionHeader from "@/components/SectionHeader.vue";
import type { Probe, Settings } from "@/interfaces/Control";

useDocumentTitle("Settings");

const control = useControlStore();

/** How often the status and the log are asked for, in milliseconds. */
const POLL_MS = 2000;

/** The names for "this computer only"; the first is the collector's default. */
const LOOPBACK = ["localhost", "127.0.0.1", "::1"];
const EVERYWHERE = "0.0.0.0";
const LOG_LEVELS = ["DEBUG", "INFO", "WARNING", "ERROR"];

type Section = "server" | "connections" | "logs" | "checks" | "about";
type Note = { kind: "ok" | "error"; text: string };

const notes = reactive<Partial<Record<Section, Note>>>({});
const busy = reactive<Partial<Record<Section, boolean>>>({});
const noteTimers: Partial<Record<Section, number>> = {};

/** Vigil stopped answering: it was closed, or is on its way out. */
const lost = ref(false);
const ready = ref(false);

function say(section: Section, kind: Note["kind"], text: string) {
  window.clearTimeout(noteTimers[section]);
  notes[section] = { kind, text };
  if (kind === "ok") {
    noteTimers[section] = window.setTimeout(() => delete notes[section], 4000);
  }
}

/** Runs one change for a section, showing how it went beside its button. */
async function run(section: Section, done: string, work: () => Promise<unknown>) {
  if (busy[section]) return;
  busy[section] = true;
  try {
    await work();
    if (done) say(section, "ok", done);
    else delete notes[section];
  } catch (error) {
    say(section, "error", error instanceof Error ? error.message : String(error));
  } finally {
    busy[section] = false;
  }
}

/* ---------- Server ---------- */

const status = computed(() => control.status);
const serving = computed(() => !!status.value?.serving);

const serverForm = reactive({
  scope: <"local" | "network" | "custom">"local",
  custom: "",
  port: 4500,
});

function scopeOf(address: string) {
  if (LOOPBACK.includes(address)) return "local";
  if (address === EVERYWHERE) return "network";
  return "custom";
}

function fillServerForm() {
  if (!status.value) return;
  serverForm.scope = scopeOf(status.value.address);
  serverForm.custom = serverForm.scope === "custom" ? status.value.address : "";
  serverForm.port = status.value.port;
}

const formAddress = computed(() => {
  if (serverForm.scope === "local") {
    // Whichever loopback name is already in use stays as it is.
    const current = status.value?.address ?? "";
    return LOOPBACK.includes(current) ? current : LOOPBACK[0];
  }
  if (serverForm.scope === "network") return EVERYWHERE;
  return serverForm.custom.trim();
});

const portValid = computed(
  () =>
    Number.isInteger(serverForm.port) && serverForm.port >= 1 && serverForm.port <= 65535,
);

const serverChanges = computed<Partial<Settings>>(() => {
  const changes: Partial<Settings> = {};
  if (!status.value) return changes;
  if (formAddress.value !== status.value.address) changes.address = formAddress.value;
  if (serverForm.port !== status.value.port) changes.port_number = serverForm.port;
  return changes;
});
const serverDirty = computed(() => Object.keys(serverChanges.value).length > 0);
const serverSavable = computed(
  () => serverDirty.value && portValid.value && formAddress.value !== "",
);

/** A start-up flag is deciding the address or port, not the settings file. */
const fromCommandLine = computed(() => {
  const s = status.value;
  if (!s) return "";
  const flags = [
    s.address_from_cli ? `--address ${s.address}` : "",
    s.port_from_cli ? `--port ${s.port}` : "",
  ].filter(Boolean);
  return flags.join(" and ");
});

function saveServer() {
  const moving = serving.value;
  return run(
    "server",
    moving ? "Saved. Vigil is now serving on the new address." : "Saved.",
    async () => {
      await control.save(serverChanges.value);
      fillServerForm();
    },
  );
}

function serverAction(action: "start" | "stop" | "restart") {
  const done = { start: "Serving again.", stop: "Stopped serving.", restart: "Restarted." };
  return run("server", done[action], () => control.server(action));
}

function openInBrowser() {
  return run("server", "", () => control.open("browser"));
}

/* ---------- Connections ---------- */

const connectionsForm = reactive({ max: 100, interval: 1 });

function fillConnectionsForm() {
  if (!control.settings) return;
  connectionsForm.max = control.settings.max_ws_connections;
  connectionsForm.interval = control.settings.ws_push_interval;
}

const connectionsChanges = computed<Partial<Settings>>(() => {
  const changes: Partial<Settings> = {};
  const saved = control.settings;
  if (!saved) return changes;
  if (connectionsForm.max !== saved.max_ws_connections)
    changes.max_ws_connections = connectionsForm.max;
  if (connectionsForm.interval !== saved.ws_push_interval)
    changes.ws_push_interval = connectionsForm.interval;
  return changes;
});
const connectionsValid = computed(
  () =>
    Number.isInteger(connectionsForm.max) &&
    connectionsForm.max >= 0 &&
    connectionsForm.max <= 10000 &&
    typeof connectionsForm.interval === "number" &&
    connectionsForm.interval >= 0.1 &&
    connectionsForm.interval <= 3600,
);
const connectionsSavable = computed(
  () => Object.keys(connectionsChanges.value).length > 0 && connectionsValid.value,
);

const usage = computed(() => {
  const c = status.value?.connections;
  if (!c || c.max === 0) return 0;
  return Math.min(100, Math.round((c.active / c.max) * 100));
});

function saveConnections() {
  return run("connections", "Saved.", async () => {
    await control.save(connectionsChanges.value);
    fillConnectionsForm();
  });
}

/* ---------- Logs ---------- */

const follow = ref(true);
const filter = ref("");
const logBox = ref<HTMLElement | null>(null);

const shownLogs = computed(() => {
  const needle = filter.value.trim().toLowerCase();
  if (!needle) return control.logs;
  return control.logs.filter((line) => line.text.toLowerCase().includes(needle));
});

function levelOf(text: string) {
  const match = /\b(ERROR|WARN|INFO|DEBUG|TRACE)\b/.exec(text.slice(0, 48));
  return match ? match[1].toLowerCase() : "info";
}

function scrollToEnd() {
  const box = logBox.value;
  if (box) box.scrollTop = box.scrollHeight;
}

watch(
  () => [shownLogs.value.length, control.newestLog, follow.value],
  () => {
    if (follow.value) nextTick(scrollToEnd);
  },
);

function setLogging(changes: Partial<Settings>) {
  return run("logs", "Saved.", () => control.save(changes));
}

function openLogs() {
  return run("logs", "", () => control.open("logs"));
}

/* ---------- Service checks ---------- */

const checksForm = reactive({ probes: <Probe[]>[], timeout: 5 });

function fillChecksForm() {
  if (!control.settings) return;
  checksForm.probes = control.settings.probes.map((p) => ({ ...p }));
  checksForm.timeout = control.settings.probe_timeout;
}

const checksChanges = computed<Partial<Settings>>(() => {
  const changes: Partial<Settings> = {};
  const saved = control.settings;
  if (!saved) return changes;
  const probes = checksForm.probes.map((p) => ({ name: p.name.trim(), url: p.url.trim() }));
  if (JSON.stringify(probes) !== JSON.stringify(saved.probes)) changes.probes = probes;
  if (checksForm.timeout !== saved.probe_timeout) changes.probe_timeout = checksForm.timeout;
  return changes;
});
const checksProblem = computed(() => {
  for (const probe of checksForm.probes) {
    if (!probe.name.trim()) return "Every check needs a name.";
    if (!/^https?:\/\//i.test(probe.url.trim()))
      return "Every address must start with http:// or https://.";
  }
  if (!(checksForm.timeout >= 0.1 && checksForm.timeout <= 120))
    return "The timeout must be between 0.1 and 120 seconds.";
  return "";
});
const checksDirty = computed(() => Object.keys(checksChanges.value).length > 0);

function addCheck() {
  checksForm.probes.push({ name: "", url: "https://" });
}

function saveChecks() {
  return run("checks", "Saved. The dashboard uses the new list from its next check.", async () => {
    await control.save(checksChanges.value);
    fillChecksForm();
  });
}

/* ---------- About ---------- */

const confirmingQuit = ref(false);

const uptime = computed(() => {
  const total = status.value?.uptime_secs ?? 0;
  const days = Math.floor(total / 86400);
  const hours = Math.floor((total % 86400) / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h ${minutes}m`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m`;
  return "Less than a minute";
});

function openSettingsFile() {
  return run("about", "", () => control.open("settings"));
}

async function quit() {
  await run("about", "", () => control.quit());
  if (!notes.about) lost.value = true;
  confirmingQuit.value = false;
}

/* ---------- Keeping up to date ---------- */

let timer: number | undefined;

async function poll() {
  if (document.hidden) return;
  try {
    await Promise.all([control.refreshStatus(), control.pollLogs()]);
    lost.value = false;
  } catch {
    lost.value = true;
  }
}

onMounted(async () => {
  try {
    await Promise.all([control.refreshStatus(), control.loadSettings(), control.pollLogs()]);
    fillServerForm();
    fillConnectionsForm();
    fillChecksForm();
    ready.value = true;
    nextTick(scrollToEnd);
  } catch {
    lost.value = true;
  }
  timer = window.setInterval(poll, POLL_MS);
});

onBeforeUnmount(() => {
  window.clearInterval(timer);
  Object.values(noteTimers).forEach((t) => window.clearTimeout(t));
});

// The tray menu can stop, start or move the server too. Follow it unless
// the form has an unsaved edit in it.
watch(
  () => [status.value?.address, status.value?.port],
  (_now, before) => {
    if (!status.value || before[0] === undefined) return;
    const untouched =
      formAddress.value === before[0] && serverForm.port === before[1];
    if (untouched) fillServerForm();
  },
);
</script>

<template>
  <div class="container-fluid py-0 py-md-2 settings-root">
    <div v-if="lost" class="settings-banner" role="alert">
      <font-awesome-icon icon="fa-solid fa-triangle-exclamation" />
      <span>
        Vigil isn't answering. If you quit it, you can close this window. Otherwise
        this page carries on as soon as it is back.
      </span>
    </div>

    <template v-if="ready && status && control.settings">
      <section class="page-section mt-0">
        <div class="row g-2">
          <!-- Server -->
          <div class="col-sm-12 col-lg-7 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="Server"
                subtitle="What other devices and browsers connect to"
                icon="fa-solid fa-server"
                tone="blue"
              >
                <template #right>
                  <span
                    class="state-pill"
                    :class="serving ? 'state-on' : status.error ? 'state-failed' : 'state-off'"
                  >
                    <span class="state-dot"></span>
                    {{ serving ? "Serving" : status.error ? "Not serving" : "Stopped" }}
                  </span>
                </template>
              </SectionHeader>
              <div class="panel-body">
                <div class="server-summary">
                  <div class="server-where">
                    <template v-if="serving">
                      <div class="eyebrow">Listening on</div>
                      <div class="mono-value">{{ status.listen }}</div>
                    </template>
                    <template v-else>
                      <div class="eyebrow">Not serving</div>
                      <div class="summary-text">
                        Nothing else can connect right now. This window keeps
                        monitoring.
                      </div>
                    </template>
                    <div v-if="status.error && !serving" class="field-error mt-1">
                      {{ status.error }}
                    </div>
                  </div>
                  <div class="button-row">
                    <button
                      v-if="serving"
                      type="button"
                      class="s-btn"
                      :disabled="busy.server"
                      @click="serverAction('stop')"
                    >
                      <font-awesome-icon icon="fa-solid fa-stop" /> Stop
                    </button>
                    <button
                      v-else
                      type="button"
                      class="s-btn s-btn-primary"
                      :disabled="busy.server"
                      @click="serverAction('start')"
                    >
                      <font-awesome-icon icon="fa-solid fa-play" /> Start
                    </button>
                    <button
                      type="button"
                      class="s-btn"
                      :disabled="busy.server || !serving"
                      @click="serverAction('restart')"
                    >
                      <font-awesome-icon icon="fa-solid fa-arrow-rotate-right" /> Restart
                    </button>
                    <button
                      type="button"
                      class="s-btn"
                      :disabled="busy.server || !serving"
                      title="Open the dashboard in your browser"
                      @click="openInBrowser"
                    >
                      <font-awesome-icon icon="fa-solid fa-up-right-from-square" />
                      Browser
                    </button>
                  </div>
                </div>

                <hr class="s-rule" />

                <form class="s-form" novalidate @submit.prevent="serverSavable && saveServer()">
                  <fieldset class="s-field">
                    <legend class="s-label">Who can connect</legend>
                    <label class="s-choice">
                      <input v-model="serverForm.scope" type="radio" value="local" />
                      <span>
                        <span class="choice-title">This computer only</span>
                        <span class="choice-help">Nothing else on the network can reach it.</span>
                      </span>
                    </label>
                    <label class="s-choice">
                      <input v-model="serverForm.scope" type="radio" value="network" />
                      <span>
                        <span class="choice-title">Any device on my network</span>
                        <span class="choice-help">
                          Listens on 0.0.0.0. Your firewall may ask to allow it.
                        </span>
                      </span>
                    </label>
                    <label class="s-choice">
                      <input v-model="serverForm.scope" type="radio" value="custom" />
                      <span>
                        <span class="choice-title">One address of this computer</span>
                        <span class="choice-help">
                          For a machine with several network adapters.
                        </span>
                      </span>
                    </label>
                    <input
                      v-if="serverForm.scope === 'custom'"
                      v-model="serverForm.custom"
                      type="text"
                      class="s-input mono mt-1"
                      placeholder="192.168.1.20"
                      aria-label="Address to listen on"
                      spellcheck="false"
                      autocomplete="off"
                    />
                  </fieldset>

                  <div class="s-field s-field-narrow">
                    <label class="s-label" for="settings-port">Port</label>
                    <input
                      id="settings-port"
                      v-model.number="serverForm.port"
                      type="number"
                      class="s-input mono"
                      min="1"
                      max="65535"
                      step="1"
                      :aria-invalid="!portValid"
                    />
                    <div v-if="!portValid" class="field-error">Between 1 and 65535.</div>
                    <div v-else class="field-help">The default is 4500.</div>
                  </div>

                  <div class="s-actions">
                    <button
                      type="submit"
                      class="s-btn s-btn-primary"
                      :disabled="!serverSavable || busy.server"
                    >
                      Save
                    </button>
                    <button
                      v-if="serverDirty"
                      type="button"
                      class="s-btn s-btn-quiet"
                      :disabled="busy.server"
                      @click="fillServerForm"
                    >
                      Discard
                    </button>
                    <span
                      v-if="notes.server"
                      class="s-note"
                      :class="`s-note-${notes.server.kind}`"
                      role="status"
                    >
                      {{ notes.server.text }}
                    </span>
                    <span v-else-if="serverDirty && serving" class="s-note">
                      Saving moves the server. Anyone connected reconnects.
                    </span>
                  </div>
                  <div v-if="fromCommandLine" class="field-help">
                    Vigil was started with <code>{{ fromCommandLine }}</code
                    >, which is used instead of the saved setting until you save
                    a different one here.
                  </div>
                </form>
              </div>
            </div>
          </div>

          <!-- Connections -->
          <div class="col-sm-12 col-lg-5 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="Connections"
                subtitle="Live viewers and how often they update"
                icon="fa-solid fa-users"
                tone="purple"
              />
              <div class="panel-body">
                <div class="eyebrow">Live connections</div>
                <div class="usage-line">
                  <span class="usage-count">{{ status.connections.active }}</span>
                  <span class="usage-of">of {{ status.connections.max }} in use</span>
                </div>
                <div
                  class="usage-bar"
                  role="progressbar"
                  aria-label="Live connections in use"
                  :aria-valuenow="status.connections.active"
                  aria-valuemin="0"
                  :aria-valuemax="status.connections.max"
                >
                  <div class="usage-fill" :style="{ width: `${usage}%` }"></div>
                </div>
                <div class="field-help">
                  This window isn't counted and can't be locked out.
                </div>

                <hr class="s-rule" />

                <form class="s-form" novalidate @submit.prevent="connectionsSavable && saveConnections()">
                  <div class="s-pair">
                    <div class="s-field">
                      <label class="s-label" for="settings-max">Connection limit</label>
                      <input
                        id="settings-max"
                        v-model.number="connectionsForm.max"
                        type="number"
                        class="s-input mono"
                        min="0"
                        max="10000"
                        step="1"
                      />
                      <div class="field-help">
                        Further viewers are turned away. 0 allows none.
                      </div>
                    </div>
                    <div class="s-field">
                      <label class="s-label" for="settings-interval">Update every</label>
                      <div class="s-suffixed">
                        <input
                          id="settings-interval"
                          v-model.number="connectionsForm.interval"
                          type="number"
                          class="s-input mono"
                          min="0.1"
                          max="3600"
                          step="0.1"
                        />
                        <span>seconds</span>
                      </div>
                      <div class="field-help">How often readings are taken and sent.</div>
                    </div>
                  </div>
                  <div v-if="!connectionsValid" class="field-error">
                    The limit is 0 to 10000 and the interval 0.1 to 3600 seconds.
                  </div>
                  <div class="s-actions">
                    <button
                      type="submit"
                      class="s-btn s-btn-primary"
                      :disabled="!connectionsSavable || busy.connections"
                    >
                      Save
                    </button>
                    <span
                      v-if="notes.connections"
                      class="s-note"
                      :class="`s-note-${notes.connections.kind}`"
                      role="status"
                    >
                      {{ notes.connections.text }}
                    </span>
                    <span v-else class="s-note">
                      Takes effect straight away. Nobody is disconnected.
                    </span>
                  </div>
                </form>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- Logs -->
      <section class="page-section">
        <div class="card panel-card border-0 shadow-lg">
          <SectionHeader
            title="Logs"
            subtitle="What Vigil has written since it started"
            icon="fa-solid fa-list-ul"
            tone="slate"
          />
          <div class="panel-body">
            <div class="log-toolbar">
              <label class="s-switch">
                <input
                  type="checkbox"
                  class="form-check-input"
                  :checked="control.settings.logging_enabled"
                  :disabled="busy.logs"
                  @change="
                    setLogging({
                      logging_enabled: ($event.target as HTMLInputElement).checked,
                    })
                  "
                />
                <span>Keep a log</span>
              </label>
              <label class="s-inline">
                <span>Level</span>
                <select
                  class="s-input s-select"
                  :value="control.settings.log_level.toUpperCase()"
                  :disabled="busy.logs || status.log_level_locked"
                  @change="
                    setLogging({ log_level: ($event.target as HTMLSelectElement).value })
                  "
                >
                  <option v-for="level in LOG_LEVELS" :key="level" :value="level">
                    {{ level }}
                  </option>
                </select>
              </label>
              <input
                v-model="filter"
                type="search"
                class="s-input log-filter"
                placeholder="Filter"
                aria-label="Filter log lines"
              />
              <span class="toolbar-space"></span>
              <span
                v-if="notes.logs"
                class="s-note"
                :class="`s-note-${notes.logs.kind}`"
                role="status"
              >
                {{ notes.logs.text }}
              </span>
              <label class="s-switch">
                <input v-model="follow" type="checkbox" class="form-check-input" />
                <span>Follow</span>
              </label>
              <button type="button" class="s-btn" :disabled="busy.logs" @click="openLogs">
                <font-awesome-icon icon="fa-solid fa-folder-open" /> Open log file
              </button>
            </div>
            <div v-if="status.log_level_locked" class="field-help mb-2">
              The level is set by the <code>RUST_LOG</code> environment variable, so the
              choice here has no effect until Vigil is started without it.
            </div>
            <div
              ref="logBox"
              class="log-view"
              tabindex="0"
              role="log"
              aria-label="Log"
            >
              <div v-if="shownLogs.length === 0" class="log-empty">
                {{
                  control.logs.length > 0
                    ? "No lines match."
                    : control.settings.logging_enabled
                      ? "Nothing logged yet."
                      : "Logging is off."
                }}
              </div>
              <div
                v-for="line in shownLogs"
                :key="line.seq"
                class="log-line"
                :class="`log-${levelOf(line.text)}`"
              >
                {{ line.text }}
              </div>
            </div>
          </div>
        </div>
      </section>

      <section class="page-section">
        <div class="row g-2">
          <!-- Service checks -->
          <div class="col-sm-12 col-lg-7 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="Service checks"
                subtitle="Addresses the dashboard checks are up"
                icon="fa-solid fa-heart-pulse"
                tone="green"
              />
              <div class="panel-body">
                <form class="s-form" novalidate @submit.prevent="!checksProblem && checksDirty && saveChecks()">
                  <div v-if="checksForm.probes.length === 0" class="field-help">
                    No checks. The Services panel on the dashboard stays empty.
                  </div>
                  <div
                    v-for="(probe, index) in checksForm.probes"
                    :key="index"
                    class="check-row"
                  >
                    <input
                      v-model="probe.name"
                      type="text"
                      class="s-input check-name"
                      placeholder="Name"
                      :aria-label="`Name of check ${index + 1}`"
                      maxlength="60"
                    />
                    <input
                      v-model="probe.url"
                      type="url"
                      class="s-input mono check-url"
                      placeholder="https://example.com/health"
                      :aria-label="`Address of check ${index + 1}`"
                      spellcheck="false"
                    />
                    <button
                      type="button"
                      class="s-btn s-btn-quiet s-btn-icon"
                      :title="`Remove ${probe.name || 'this check'}`"
                      :aria-label="`Remove ${probe.name || 'this check'}`"
                      @click="checksForm.probes.splice(index, 1)"
                    >
                      <font-awesome-icon icon="fa-solid fa-xmark" />
                    </button>
                  </div>
                  <div class="check-footer">
                    <button
                      type="button"
                      class="s-btn"
                      :disabled="checksForm.probes.length >= 50"
                      @click="addCheck"
                    >
                      <font-awesome-icon icon="fa-solid fa-plus" /> Add a check
                    </button>
                    <label class="s-inline">
                      <span>Give up after</span>
                      <input
                        v-model.number="checksForm.timeout"
                        type="number"
                        class="s-input mono s-input-short"
                        min="0.1"
                        max="120"
                        step="0.1"
                      />
                      <span>seconds</span>
                    </label>
                  </div>
                  <div class="s-actions">
                    <button
                      type="submit"
                      class="s-btn s-btn-primary"
                      :disabled="!checksDirty || !!checksProblem || busy.checks"
                    >
                      Save
                    </button>
                    <button
                      v-if="checksDirty"
                      type="button"
                      class="s-btn s-btn-quiet"
                      :disabled="busy.checks"
                      @click="fillChecksForm"
                    >
                      Discard
                    </button>
                    <span
                      v-if="notes.checks"
                      class="s-note"
                      :class="`s-note-${notes.checks.kind}`"
                      role="status"
                    >
                      {{ notes.checks.text }}
                    </span>
                    <span v-else-if="checksDirty && checksProblem" class="s-note s-note-error">
                      {{ checksProblem }}
                    </span>
                  </div>
                </form>
              </div>
            </div>
          </div>

          <!-- About -->
          <div class="col-sm-12 col-lg-5 d-flex">
            <div class="card panel-card border-0 shadow-lg flex-fill">
              <SectionHeader
                title="About"
                subtitle="This copy of Vigil"
                icon="fa-solid fa-circle-info"
                tone="amber"
              />
              <div class="panel-body">
                <dl class="about-list">
                  <dt>Version</dt>
                  <dd class="mono">{{ status.version }}</dd>
                  <dt>Running for</dt>
                  <dd>{{ uptime }}</dd>
                  <dt>Settings file</dt>
                  <dd>
                    <span class="mono path">{{ status.paths.settings }}</span>
                    <button
                      type="button"
                      class="s-btn s-btn-quiet s-btn-small"
                      :disabled="busy.about"
                      @click="openSettingsFile"
                    >
                      Open
                    </button>
                  </dd>
                  <dt>Log file</dt>
                  <dd>
                    <span class="mono path">{{ status.paths.log }}</span>
                  </dd>
                </dl>

                <hr class="s-rule" />

                <div v-if="!confirmingQuit" class="s-actions">
                  <button type="button" class="s-btn s-btn-danger" @click="confirmingQuit = true">
                    <font-awesome-icon icon="fa-solid fa-power-off" /> Quit Vigil
                  </button>
                  <span
                    v-if="notes.about"
                    class="s-note"
                    :class="`s-note-${notes.about.kind}`"
                    role="status"
                  >
                    {{ notes.about.text }}
                  </span>
                  <span v-else class="s-note">
                    Closing this window leaves Vigil running in the tray.
                  </span>
                </div>
                <div v-else class="quit-confirm" role="alertdialog" aria-label="Quit Vigil">
                  <div>
                    Quit Vigil? Monitoring stops, the tray icon goes, and anyone
                    connected is disconnected.
                  </div>
                  <div class="s-actions">
                    <button
                      type="button"
                      class="s-btn s-btn-danger"
                      :disabled="busy.about"
                      @click="quit"
                    >
                      Quit
                    </button>
                    <button type="button" class="s-btn" @click="confirmingQuit = false">
                      Keep running
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
/* The palette tokens exist only in the dark theme (see dark.scss); each
   one here falls back to its light counterpart. */
.settings-root {
  --s-text: var(--text-primary, #1e293b);
  --s-text-2: var(--text-secondary, #334155);
  --s-muted: var(--text-muted, #64748b);
  --s-border: var(--border-default, rgba(15, 23, 42, 0.16));
  --s-rule: var(--border-subtle, rgba(15, 23, 42, 0.08));
  --s-input: var(--bg-input, #f8fafc);
  --s-hover: var(--bg-hover, rgba(15, 23, 42, 0.05));
  --s-accent: var(--accent-blue, #2563eb);
  --s-ok: var(--status-ok, #047857);
  --s-warn: var(--status-warn, #b45309);
  --s-crit: var(--status-crit, #b91c1c);
  font-size: var(--fs-body);
}
/* Bootstrap's card sets its own text colour, which is the light theme's. */
.panel-body,
.settings-banner {
  color: var(--s-text);
}

.mono {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

/* ---------- Banner ---------- */
.settings-banner {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  margin-bottom: 0.5rem;
  padding: 0.6rem 0.9rem;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--s-warn) 45%, transparent);
  background: color-mix(in srgb, var(--s-warn) 12%, transparent);
}
.settings-banner svg { color: var(--s-warn); flex-shrink: 0; }

/* ---------- Header pill ---------- */
.state-pill {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.4rem 0.85rem;
  border-radius: 999px;
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  white-space: nowrap;
}
.state-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 10px currentColor;
}
.state-on { color: #34d399; }
.state-off { color: #94a3b8; }
.state-failed { color: #f87171; }

/* ---------- Text ---------- */
.eyebrow,
.s-label {
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--s-muted);
  margin-bottom: 0.3rem;
}
legend.s-label { float: none; width: auto; }
.mono-value {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-display);
  font-weight: 600;
  line-height: 1.2;
  overflow-wrap: anywhere;
}
.summary-text { color: var(--s-text-2); max-width: 38ch; }
.field-help {
  font-size: var(--fs-caption);
  color: var(--s-muted);
  margin-top: 0.3rem;
}
.field-error {
  font-size: var(--fs-caption);
  color: var(--s-crit);
  margin-top: 0.3rem;
}
code {
  font-size: inherit;
  color: var(--s-text-2);
  background: var(--s-hover);
  padding: 0.05rem 0.3rem;
  border-radius: 4px;
}
.s-rule {
  margin: 0.9rem 0;
  border: 0;
  border-top: 1px solid var(--s-rule);
  opacity: 1;
}

/* ---------- Server summary ---------- */
.server-summary {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 0.9rem;
  flex-wrap: wrap;
}
.server-where { min-width: 0; }
.button-row {
  display: flex;
  gap: 0.4rem;
  flex-wrap: wrap;
}

/* ---------- Buttons ---------- */
.s-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.4rem 0.8rem;
  border-radius: 6px;
  border: 1px solid var(--s-border);
  background: var(--s-input);
  color: var(--s-text);
  font-size: var(--fs-body);
  font-weight: 500;
  line-height: 1.3;
  white-space: nowrap;
  transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.s-btn svg { font-size: var(--fs-caption); }
.s-btn:hover:not(:disabled) {
  background: var(--s-hover);
  border-color: var(--s-muted);
}
.s-btn:focus-visible,
.s-input:focus-visible,
.log-view:focus-visible {
  outline: 2px solid var(--s-accent);
  outline-offset: 1px;
}
.s-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.s-btn-primary {
  background: #2563eb;
  border-color: #2563eb;
  color: #ffffff;
}
.s-btn-primary:hover:not(:disabled) {
  background: #1d4ed8;
  border-color: #1d4ed8;
}
.s-btn-danger {
  color: var(--s-crit);
  border-color: color-mix(in srgb, var(--s-crit) 45%, transparent);
  background: transparent;
}
.s-btn-danger:hover:not(:disabled) {
  background: color-mix(in srgb, var(--s-crit) 12%, transparent);
  border-color: var(--s-crit);
}
.s-btn-quiet {
  background: transparent;
  border-color: transparent;
  color: var(--s-text-2);
}
.s-btn-icon { padding: 0.4rem 0.55rem; }
.s-btn-small {
  padding: 0.15rem 0.5rem;
  font-size: var(--fs-caption);
  border-color: var(--s-border);
}

/* ---------- Forms ---------- */
.s-form {
  display: flex;
  flex-direction: column;
  gap: 0.8rem;
}
.s-field {
  margin: 0;
  padding: 0;
  border: 0;
  min-width: 0;
}
.s-field-narrow { max-width: 11rem; }
.s-pair {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
  gap: 0.8rem;
}
.s-input {
  display: block;
  width: 100%;
  padding: 0.4rem 0.6rem;
  border-radius: 6px;
  border: 1px solid var(--s-border);
  background: var(--s-input);
  color: var(--s-text);
  font-size: var(--fs-body);
  line-height: 1.3;
}
.s-input::placeholder { color: var(--s-muted); opacity: 0.7; }
.s-input:disabled { opacity: 0.5; }
.s-input[aria-invalid="true"] { border-color: var(--s-crit); }
.s-select {
  width: auto;
  padding-right: 1.6rem;
}
.s-select option { color: #0f172a; background: #ffffff; }
.s-input-short { width: 5.5rem; }
.s-suffixed {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  color: var(--s-muted);
}
.s-choice {
  display: flex;
  align-items: flex-start;
  gap: 0.6rem;
  padding: 0.3rem 0;
  cursor: pointer;
}
.s-choice input {
  margin-top: 0.2rem;
  accent-color: #2563eb;
  flex-shrink: 0;
}
.choice-title { display: block; font-weight: 500; }
.choice-help {
  display: block;
  font-size: var(--fs-caption);
  color: var(--s-muted);
}
.s-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
}
.s-note {
  font-size: var(--fs-caption);
  color: var(--s-muted);
}
.s-note-ok { color: var(--s-ok); }
.s-note-error { color: var(--s-crit); }
.s-switch,
.s-inline {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  margin: 0;
  color: var(--s-text-2);
  white-space: nowrap;
}
.s-switch { cursor: pointer; }
.s-switch .form-check-input { margin: 0; }

/* ---------- Connections ---------- */
.usage-line {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
}
.usage-count {
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-hero);
  font-weight: 600;
  line-height: 1.1;
}
.usage-of { color: var(--s-muted); }
.usage-bar {
  height: 6px;
  margin-top: 0.5rem;
  border-radius: 999px;
  background: var(--s-hover);
  overflow: hidden;
}
.usage-fill {
  height: 100%;
  border-radius: 999px;
  background: #a855f7;
  transition: width 0.3s ease;
}

/* ---------- Logs ---------- */
.log-toolbar {
  display: flex;
  align-items: center;
  gap: 0.8rem;
  flex-wrap: wrap;
  margin-bottom: 0.6rem;
}
.toolbar-space { flex: 1 1 auto; }
.log-filter { width: 12rem; }
.log-view {
  height: 18rem;
  overflow: auto;
  padding: 0.6rem 0.75rem;
  border-radius: 8px;
  border: 1px solid rgba(148, 163, 184, 0.16);
  /* A terminal in both themes. */
  background: #0a0e17;
  color: #cbd5e1;
  font-family: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: var(--fs-caption);
  line-height: 1.55;
}
.log-line {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.log-error { color: #f87171; }
.log-warn { color: #fbbf24; }
.log-debug,
.log-trace { color: #64748b; }
.log-empty { color: #64748b; }

/* ---------- Service checks ---------- */
.check-row {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}
.check-name { flex: 0 1 12rem; }
.check-url { flex: 1 1 14rem; min-width: 0; }
.check-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.8rem;
  flex-wrap: wrap;
}

/* ---------- About ---------- */
.about-list {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 0.45rem 1rem;
  margin: 0;
}
.about-list dt {
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--s-muted);
  padding-top: 0.2rem;
}
.about-list dd {
  margin: 0;
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
  min-width: 0;
}
.path {
  font-size: var(--fs-caption);
  color: var(--s-text-2);
  overflow-wrap: anywhere;
}
.quit-confirm {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  padding: 0.7rem 0.8rem;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--s-crit) 40%, transparent);
  background: color-mix(in srgb, var(--s-crit) 8%, transparent);
}

@media (max-width: 575.98px) {
  .check-row { flex-wrap: wrap; }
  .check-name { flex: 1 1 100%; }
  .log-filter { width: 100%; }
  .toolbar-space { display: none; }
}
</style>
