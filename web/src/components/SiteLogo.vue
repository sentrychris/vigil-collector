<template>
  <!--
    Vigil mark: a hybrid V — the right arm is a stack of three cubic
    blocks with the top one twisted ~25° around its vertical axis. The
    left arm is a single smooth prism that runs from upper-left down
    into the apex. Both arms meet on a shared cyan apex cube.

    Painter's order is back-to-front so blocks correctly overlap.
  -->
  <svg
    class="site-logo"
    viewBox="12 12 40 40"
    xmlns="http://www.w3.org/2000/svg"
    role="img"
    aria-label="Vigil"
  >
    <defs>
      <!-- ─── Smooth left arm: gradients flow along the arm's length ─── -->
      <linearGradient
        id="vigil-smooth-top"
        x1="15.44" y1="14.5" x2="29" y2="39"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset="0%" stop-color="#a5f3fc" />
        <stop offset="45%" stop-color="#22d3ee" />
        <stop offset="100%" stop-color="#34d399" />
      </linearGradient>
      <linearGradient
        id="vigil-smooth-outer"
        x1="15.44" y1="14.5" x2="25.94" y2="46"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset="0%" stop-color="#22d3ee" />
        <stop offset="55%" stop-color="#0e7490" />
        <stop offset="100%" stop-color="#075985" />
      </linearGradient>
      <linearGradient
        id="vigil-smooth-inner"
        x1="21.5" y1="14.5" x2="32" y2="42.5"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset="0%" stop-color="#164e63" />
        <stop offset="100%" stop-color="#0c4a6e" />
      </linearGradient>

      <!-- ─── Apex (cyan/blue) ─── -->
      <linearGradient id="vigil-apex-top" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#67e8f9" />
        <stop offset="100%" stop-color="#22d3ee" />
      </linearGradient>
      <linearGradient id="vigil-apex-l" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#22d3ee" />
        <stop offset="100%" stop-color="#0891b2" />
      </linearGradient>
      <linearGradient id="vigil-apex-r" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#0e7490" />
        <stop offset="100%" stop-color="#075985" />
      </linearGradient>

      <!-- ─── Right arm 1 — bottom block (blue/indigo) ─── -->
      <linearGradient id="vigil-r1-top" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#93c5fd" />
        <stop offset="100%" stop-color="#60a5fa" />
      </linearGradient>
      <linearGradient id="vigil-r1-l" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#60a5fa" />
        <stop offset="100%" stop-color="#3b82f6" />
      </linearGradient>
      <linearGradient id="vigil-r1-r" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#1e40af" />
        <stop offset="100%" stop-color="#1e3a8a" />
      </linearGradient>

      <!-- ─── Right arm 2 — middle block (purple) ─── -->
      <linearGradient id="vigil-r2-top" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#e9d5ff" />
        <stop offset="100%" stop-color="#c084fc" />
      </linearGradient>
      <linearGradient id="vigil-r2-l" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#c084fc" />
        <stop offset="100%" stop-color="#a855f7" />
      </linearGradient>
      <linearGradient id="vigil-r2-r" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#7e22ce" />
        <stop offset="100%" stop-color="#581c87" />
      </linearGradient>

      <!-- ─── Right arm 3 — TWISTED top block (fuchsia → amber) ─── -->
      <linearGradient id="vigil-r3-top" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stop-color="#fbbf24" />
        <stop offset="100%" stop-color="#d946ef" />
      </linearGradient>
      <linearGradient id="vigil-r3-l" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#d946ef" />
        <stop offset="100%" stop-color="#a21caf" />
      </linearGradient>
      <linearGradient id="vigil-r3-r" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="#86198f" />
        <stop offset="100%" stop-color="#4a044e" />
      </linearGradient>

      <!-- Soft halo behind the mark -->
      <radialGradient id="vigil-halo" cx="0.5" cy="0.55" r="0.55">
        <stop offset="0%" stop-color="#22d3ee" stop-opacity="0.25" />
        <stop offset="60%" stop-color="#60a5fa" stop-opacity="0.08" />
        <stop offset="100%" stop-color="#0b1220" stop-opacity="0" />
      </radialGradient>
    </defs>

    <rect x="12" y="12" width="40" height="40" fill="url(#vigil-halo)" />

    <!--
      ── Smooth left arm prism ──
      Top corners (parallelogram):
        BO = back-outer  (15.44, 14.5? no — 18)
        BI = back-inner  (21.5, 14.5)
        FI = front-inner (32, 35.5)   — joins apex N
        FO = front-outer (25.94, 39)  — joins apex W
      Sides extend straight down by 7 (cube edge length).
    -->
    <g>
      <!-- Inner side (shadow) -->
      <polygon
        points="21.5,14.5 32,35.5 32,42.5 21.5,21.5"
        fill="url(#vigil-smooth-inner)"
      />
      <!-- Outer side (lit, faces camera-left) -->
      <polygon
        points="15.44,18 25.94,39 25.94,46 15.44,25"
        fill="url(#vigil-smooth-outer)"
      />
      <!-- Top face (brightest, lit from above) -->
      <polygon
        points="15.44,18 21.5,14.5 32,35.5 25.94,39"
        fill="url(#vigil-smooth-top)"
      />
    </g>

    <!-- ── Top-right block (TWISTED ~25° around vertical axis) ─── -->
    <!-- Original top corners about centre (42.5, 18):
         N(42.5,14.5) E(48.56,18) S(42.5,21.5) W(36.44,18)
         Rotated +25° clockwise on screen:
         N'(43.98, 14.83)  E'(47.99, 20.56)
         S'(41.02, 21.17)  W'(37.01, 15.44)
         Bottoms remain where the un-twisted cube would have them. -->
    <g>
      <polygon
        points="37.01,15.44 41.02,21.17 42.5,28.5 36.44,25"
        fill="url(#vigil-r3-l)"
      />
      <polygon
        points="47.99,20.56 41.02,21.17 42.5,28.5 48.56,25"
        fill="url(#vigil-r3-r)"
      />
      <polygon
        points="43.98,14.83 47.99,20.56 41.02,21.17 37.01,15.44"
        fill="url(#vigil-r3-top)"
      />
    </g>

    <!-- ── Mid-right block (purple) ─── -->
    <g>
      <polygon points="32.94,25 39,28.5 39,35.5 32.94,32" fill="url(#vigil-r2-l)" />
      <polygon points="45.06,25 39,28.5 39,35.5 45.06,32" fill="url(#vigil-r2-r)" />
      <polygon points="39,21.5 45.06,25 39,28.5 32.94,25" fill="url(#vigil-r2-top)" />
    </g>

    <!-- ── Lower-right block (blue/indigo) ─── -->
    <g>
      <polygon points="29.44,32 35.5,35.5 35.5,42.5 29.44,39" fill="url(#vigil-r1-l)" />
      <polygon points="41.56,32 35.5,35.5 35.5,42.5 41.56,39" fill="url(#vigil-r1-r)" />
      <polygon points="35.5,28.5 41.56,32 35.5,35.5 29.44,32" fill="url(#vigil-r1-top)" />
    </g>

    <!-- ── Apex (front-most, cyan) ─── -->
    <g>
      <polygon points="25.94,39 32,42.5 32,49.5 25.94,46" fill="url(#vigil-apex-l)" />
      <polygon points="38.06,39 32,42.5 32,49.5 38.06,46" fill="url(#vigil-apex-r)" />
      <polygon points="32,35.5 38.06,39 32,42.5 25.94,39" fill="url(#vigil-apex-top)" />
    </g>

    <!-- ── Edge highlights ──
         Smooth arm gets a single rim sheen down its outer-top edge.
         Cubic blocks each get a small front-rim highlight that picks out
         the bevel at small sizes. -->
    <g
      stroke="#ffffff"
      stroke-opacity="0.22"
      stroke-width="0.4"
      fill="none"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <!-- Smooth arm outer-top crease -->
      <polyline points="15.44,18 25.94,39" stroke-opacity="0.32" stroke-width="0.5" />
      <polyline points="21.5,14.5 32,35.5" stroke-opacity="0.18" />

      <!-- Cubic block front rims -->
      <polyline points="29.44,32 35.5,35.5 41.56,32" />
      <polyline points="32.94,25 39,28.5 45.06,25" />
      <polyline points="37.01,15.44 41.02,21.17 47.99,20.56" />
      <polyline points="25.94,39 32,42.5 38.06,39" />
    </g>
  </svg>
</template>

<style scoped>
.site-logo {
  display: block;
  width: 45px;
  height: 45px;
  filter: drop-shadow(0 6px 18px rgba(34, 211, 238, 0.35));
  transition: filter 220ms ease, transform 320ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
.site-logo:hover {
  filter: drop-shadow(0 8px 22px rgba(34, 211, 238, 0.55));
  transform: translateY(-1px) rotate(-2deg);
}
</style>
