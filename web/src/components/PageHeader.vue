<script setup lang="ts">
withDefaults(
  defineProps<{
    decorTitle: string;
    title: string;
    compact?: boolean;
  }>(),
  { compact: false },
);
</script>

<template>
  <div class="page-header" :class="{ 'is-compact': compact }">
    <div class="eyebrow">{{ decorTitle }}</div>
    <h1 class="page-title">{{ title }}</h1>
  </div>
</template>

<style scoped>
.page-header {
  padding: 0;
  line-height: 1;
}

.eyebrow {
  font-family: "IBM Plex Sans", "Inter", system-ui, sans-serif;
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.22em;
  text-transform: uppercase;
  color: #60a5fa;
}

.page-title {
  font-family: "IBM Plex Sans", "Inter", system-ui, sans-serif;
  font-weight: 600;
  font-size: var(--fs-heading);
  letter-spacing: 0.04em;
  color: #f9fafb;
  margin: 0.25rem 0 0;
  line-height: 1.1;
}

/* Default (non-compact) page header for use outside the navbar */
.page-header:not(.is-compact) {
  padding: 0.75rem 0 0.25rem;
}
.page-header:not(.is-compact) .eyebrow {
  color: #6b7280;
}
.page-header:not(.is-compact) .page-title {
  font-weight: 300;
  font-size: var(--fs-hero);
  letter-spacing: -0.01em;
  color: #111827;
}
body[data-theme="dark"] .page-header:not(.is-compact) .page-title {
  color: var(--text-primary);
}
body[data-theme="dark"] .page-header:not(.is-compact) .eyebrow {
  color: var(--text-muted);
}

@media (max-width: 575.98px) {
  .page-title { font-size: var(--fs-body); }
  .page-header:not(.is-compact) .page-title { font-size: var(--fs-display); }
}
</style>
