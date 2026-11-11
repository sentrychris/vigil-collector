<script setup lang="ts">
import { useLoadingStore } from "@/stores/loading";
const loader = useLoadingStore();
</script>

<template>
  <div
    class="d-flex flex-column align-items-center justify-content-center"
    id="loading"
    v-if="!loader.loaded"
  >
    <div class="mask">
      <font-awesome-icon
        v-if="loader.error"
        icon="fa-solid fa-exclamation-triangle"
        class="text-danger fa-4x"
      />
      <div class="loader" v-else>
        <div class="ring ring-1"></div>
        <div class="ring ring-2"></div>
        <div class="ring ring-3"></div>
      </div>
    </div>
    <p class="mt-4 loading-message">{{ loader.message }}</p>
  </div>
</template>

<style scoped>
#loading {
  position: fixed;
  inset: 0;
  z-index: 1050;
  background: #eeeeee;
}
body[data-theme="dark"] #loading {
  background: var(--bg-page);
}

.mask {
  display: flex;
  align-items: center;
  justify-content: center;
}

.loader {
  position: relative;
  width: 64px;
  height: 64px;
}

.ring {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  border: 2px solid transparent;
}
.ring-1 {
  border-top-color: #34d399;
  animation: spin 1.4s linear infinite;
}
.ring-2 {
  inset: 8px;
  border-right-color: #60a5fa;
  animation: spin 1.8s linear infinite reverse;
}
.ring-3 {
  inset: 16px;
  border-bottom-color: #c084fc;
  animation: spin 2.2s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to   { transform: rotate(360deg); }
}

.loading-message {
  font-size: var(--fs-body);
  letter-spacing: 0.06em;
  color: #6b7280;
  font-weight: 500;
}

body[data-theme="dark"] .loading-message {
  color: #8b8d8f;
}
</style>
