<script setup lang="ts">
import { RouterView } from "vue-router";
import SiteNavbar from "./components/SiteNavbar.vue";
import SiteFooter from "./components/SiteFooter.vue";
import PageLoading from "@/components/PageLoading.vue";
import { useThemeStore } from "./stores/theme";

const theme = useThemeStore();
theme.load();
</script>

<template>
  <SiteNavbar />
  <PageLoading />
  <main class="page-wrapper d-flex flex-column position-relative">
    <RouterView />
  </main>
  <SiteFooter />
</template>
