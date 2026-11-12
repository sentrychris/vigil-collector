<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { RouterView, useRoute, useRouter } from "vue-router";
import SiteNavbar from "./components/SiteNavbar.vue";
import SiteFooter from "./components/SiteFooter.vue";
import PageLoading from "@/components/PageLoading.vue";
import { useThemeStore } from "./stores/theme";
import { useControlStore } from "./stores/control";
import { useWindowFrame } from "@/composables/useWindowFrame";

const theme = useThemeStore();
theme.load();

const route = useRoute();
const router = useRouter();

// In the Vigil window the navbar is the window's title bar and the page
// scrolls beneath it (see "The Vigil window" in main.scss). Set before
// anything is drawn so the layout doesn't jump.
const { framed } = useWindowFrame();
if (framed.value) document.documentElement.classList.add("vigil-window");
const body = ref<HTMLElement | null>(null);

// In the Vigil window the tray menu's "Settings" asks the page that is
// already showing to go there, rather than loading it again.
function navigate(event: Event) {
  const path = (event as CustomEvent<unknown>).detail;
  if (typeof path === "string" && path.startsWith("/")) router.push(path);
}

onMounted(() => {
  window.addEventListener("vigil:navigate", navigate);
  // Shows the Settings link if this is the Vigil window.
  useControlStore().detect();
  // The page itself no longer scrolls there, so the part that does takes
  // the keyboard: Page Down and the arrow keys work without a click first.
  if (framed.value) body.value?.focus();
});
onBeforeUnmount(() => window.removeEventListener("vigil:navigate", navigate));
</script>

<template>
  <SiteNavbar />
  <!-- The overlay waits for the dashboard's first data; Settings has none
       to wait for. -->
  <PageLoading v-if="route.name === 'dashboard'" />
  <div ref="body" class="app-body" :tabindex="framed ? -1 : undefined">
    <main class="page-wrapper d-flex flex-column position-relative">
      <RouterView />
    </main>
    <SiteFooter />
  </div>
</template>
