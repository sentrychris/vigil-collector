<script setup lang="ts">
import { config } from "@/config";
</script>

<template>
  <footer id="site-footer" class="site-footer">
    <div class="container-fluid py-3 footer-inner">
      <span class="footer-text">
        Powered by
        <a class="footer-link" href="https://vigil.edcs.app">{{
          config.app.name
        }}</a>
        <span class="footer-sep d-none">·</span>
        <span class="footer-attr d-none">
          Built by
          <a class="footer-link" href="https://github.com/sentrychris">Chris</a>
        </span>
      </span>
      <span class="footer-meta">
        <span class="footer-pill mono">v{{ config.app.version }}</span>
        <span v-if="config.app.deployment.region" class="footer-pill mono">{{
          config.app.deployment.region
        }}</span>
        <span v-if="config.app.deployment.instance" class="footer-pill mono">{{
          config.app.deployment.instance
        }}</span>
      </span>
    </div>
  </footer>
</template>

<style scoped>
.site-footer {
  background: linear-gradient(135deg, #111827 0%, #1e293b 100%);
  color: #cbd5e1;
  position: relative;
  overflow: hidden;
  border-top: 1px solid rgba(96, 165, 250, 0.22);
}
.site-footer::before {
  content: "";
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(96, 165, 250, 0.04) 1px, transparent 1px),
    linear-gradient(90deg, rgba(96, 165, 250, 0.04) 1px, transparent 1px);
  background-size: 32px 32px;
  pointer-events: none;
  opacity: 0.5;
}
.site-footer::after {
  content: "";
  position: absolute;
  inset: 0;
  background:
    radial-gradient(600px circle at 0% 0%, rgba(59, 130, 246, 0.08), transparent 50%),
    radial-gradient(500px circle at 100% 100%, rgba(34, 211, 238, 0.06), transparent 50%);
  pointer-events: none;
}

.footer-inner {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.footer-text {
  font-size: var(--fs-caption);
  color: #9ca3af;
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  flex-wrap: wrap;
}
.footer-link {
  color: #34d399;
  text-decoration: none;
  font-weight: 600;
  transition: color 0.15s ease;
}
.footer-link:hover {
  color: #6ee7b7;
  text-decoration: underline;
}
.footer-sep {
  color: rgba(96, 165, 250, 0.4);
  font-weight: 700;
}
.footer-attr { color: #9ca3af; }

.footer-meta {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  flex-wrap: wrap;
}

.footer-pill {
  display: inline-flex;
  align-items: center;
  padding: 0.18rem 0.55rem;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  color: #94a3b8;
  font-size: var(--fs-micro);
  font-weight: 600;
  letter-spacing: 0.06em;
  white-space: nowrap;
}
.footer-pill.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-feature-settings: "tnum";
  font-variant-numeric: tabular-nums;
}

/* Mobile: stack neatly, center align so it doesn't look ragged */
@media (max-width: 575.98px) {
  .footer-inner {
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 0.65rem;
  }
  .footer-text { justify-content: center; }
  .footer-meta { justify-content: center; }
  .footer-pill { padding: 0.15rem 0.45rem; }
}
</style>
