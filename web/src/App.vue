<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import { RouterView, useRoute, useRouter } from "vue-router";
import SiteNavbar from "./components/SiteNavbar.vue";
import SiteFooter from "./components/SiteFooter.vue";
import PageLoading from "@/components/PageLoading.vue";
import { useThemeStore } from "./stores/theme";
import { useControlStore } from "./stores/control";

const theme = useThemeStore();
theme.load();

const route = useRoute();
const router = useRouter();

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
});
onBeforeUnmount(() => window.removeEventListener("vigil:navigate", navigate));
</script>

<template>
  <SiteNavbar />
  <!-- The overlay waits for the dashboard's first data; Settings has none
       to wait for. -->
  <PageLoading v-if="route.name === 'dashboard'" />
  <main class="page-wrapper d-flex flex-column position-relative">
    <RouterView />
  </main>
  <SiteFooter />
</template>
