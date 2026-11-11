# Vigil — Design & Branding

This document captures the design language used across **Vigil** (the
dashboard) and its marketing/landing surface. It is framework-agnostic:
the tokens, patterns, and rules below describe *what* the system looks
and feels like, not how a particular CSS framework happens to express
them.

If a future project adopts this language — a sibling tool, a docs site,
a CLI status page — this is the source of truth.

---

## 1. Brand at a glance

> **Vigil — instrument-cluster clarity for live systems.**

The brand sits at the intersection of three sensibilities:

| Sensibility   | What it gives us                                                                                  |
|---------------|---------------------------------------------------------------------------------------------------|
| **SOC / NOC** | Dense numeric readouts, mono digits, status chips, an always-on "LIVE" pulse, gridlined surfaces. |
| **Editorial** | Generous letter-spacing on small caps, restrained type scale, lots of negative space.             |
| **Holographic** | Soft radial glows, gradient text, prismatic sheen, glassy panels with backdrop blur.            |

If a single phrase had to capture the feel, it's **"glass over a dark
control room."** Surfaces are translucent, the chrome behind them is
gridded and dimly lit, and accent color appears only on the things
that matter — status, headings, primary actions, live data.

### Voice & tone

- **Plain, technical, confident.** "Live WebSocket streaming." Not
  "Power up your monitoring journey."
- **Numbers over adjectives.** "~140 KB gzipped" beats "blazingly
  fast."
- **No exclamation marks. No emoji in product UI.**
- **Short paragraphs, declarative sentences.**
- **Operator-friendly.** Copy assumes a reader who reads logs.

---

## 2. The mark

The Vigil logo is an isometric **V** built from cubic blocks:

- The **left arm** is a single smooth prism — gradient flowing from
  pale cyan through teal into emerald along its length.
- The **right arm** is a stack of three cubes: blue/indigo at the
  bottom, purple in the middle, fuchsia/amber on top with a ~25°
  twist around the vertical axis.
- Both arms meet on a **cyan apex cube**, the brand's hero color and
  the literal "point" of the V.
- A soft cyan→blue **radial halo** sits behind the mark.
- A subtle white **rim highlight** picks out the front bevels at small
  sizes so the cubic structure survives down to favicon scale.

**Signature drop-shadow:** `drop-shadow(0 6px 18px rgba(34, 211, 238, 0.35))`
intensifies on hover. The mark **never** appears flat or
single-colored — its gradient stack is the brand.

**Clear space:** at least the width of the apex cube on every side.

**Don't:**
- Don't recolor the mark to a single tint.
- Don't drop it onto a busy photographic background.
- Don't render it on a pure white surface without the halo —
  the whole composition reads as dark-first.

---

## 3. Color system

The palette is built around a **deep navy substrate** and a small set
of **luminous accents** used like indicator LEDs.

### 3.1 Surfaces (dark, primary canvas)

| Token         | Hex       | Role                                    |
|---------------|-----------|-----------------------------------------|
| `--bg-0`      | `#060a14` | Page background, deepest layer          |
| `--bg-1`      | `#0b1220` | Default page surface (landing)          |
| `--bg-page`   | `#0a0e17` | Default page surface (app)              |
| `--bg-surface`| `#0f172a` | Card / panel base                       |
| `--bg-2`      | `#111827` | Header gradient start                   |
| `--bg-3`      | `#1e293b` | Header gradient end / elevated surface  |
| `--bg-elevated`| `#1e293b`| Hovered/active rows, modals             |
| `--bg-hover`  | `rgba(255,255,255,0.04)` | Hover wash                |
| `--bg-input`  | `rgba(255,255,255,0.05)` | Form fields               |

The canonical **header gradient** — used by navbar, footer, every
section header, every stat card header — is:

```
linear-gradient(135deg, #111827 0%, #1e293b 100%)
```

### 3.2 Surfaces (light)

The light theme is intentionally narrower; the brand is dark-first.

| Token       | Hex       | Role                                          |
|-------------|-----------|-----------------------------------------------|
| page bg     | `#eeeeee` | Body                                          |
| brand wash  | `#f5f5f5` | Soft brand background (e.g. inputs)           |
| panel       | `rgba(255,255,255,0.72)` over a glow gradient | Glassy card |

In light mode the page wears a **fixed radial-glow gradient** behind
all content (see §6). Cards become semi-transparent white with
backdrop blur over that, which gives the same "glass" feel as dark.

### 3.3 Text scale

| Token              | Hex       | Use                                       |
|--------------------|-----------|-------------------------------------------|
| `--text-primary`   | `#f1f5f9` | Headings, primary copy                    |
| `--text-secondary` | `#cbd5e1` | Body copy on dark surfaces                |
| `--text-muted`     | `#94a3b8` | Lede paragraphs, metadata                 |
| `--text-dim`       | `#64748b` | Eyebrows, footnotes, placeholder labels   |

Light mode flips to `#111827` primary, `#6b7280` muted.

### 3.4 Lines & dividers

| Token             | Value                          | Use                       |
|-------------------|--------------------------------|---------------------------|
| `--line`          | `rgba(96, 165, 250, 0.14)`     | Default border            |
| `--border-subtle` | `rgba(148, 163, 184, 0.10)`    | Subtle internal divider   |
| `--border-default`| `rgba(148, 163, 184, 0.16)`    | Card border               |
| `--line-strong` / `--border-accent` | `rgba(96, 165, 250, 0.22–0.32)` | Header underline, accent edge |

**All borders carry a faint blue cast.** Pure neutral borders are
avoided — the blue tint is what keeps the dark theme from feeling
gray.

### 3.5 Accent palette

These are the **indicator LEDs** of the system. Each accent has a
"primary" tone and an implicit secondary/glow used for hover, glow
filters, and chart strokes.

| Name      | Primary   | Secondary | Glow                          | Personality                |
|-----------|-----------|-----------|-------------------------------|----------------------------|
| Blue      | `#60a5fa` | `#93c5fd` | `rgba(96, 165, 250, 0.55)`    | Default. Hyperlinks, info. |
| Cyan      | `#22d3ee` | `#67e8f9` | `rgba(34, 211, 238, 0.55)`    | **Hero brand color.**      |
| Emerald   | `#34d399` | `#10b981` | `rgba(52, 211, 153, 0.55)`    | OK / live / success.       |
| Purple    | `#c084fc` | `#a78bfa` | `rgba(192, 132, 252, 0.55)`   | Secondary highlight.       |
| Amber     | `#fbbf24` | `#f59e0b` | `rgba(251, 191, 36, 0.55)`    | Warning. Inline keywords.  |
| Rose      | `#f43f5e` | `#fb7185` | `rgba(244, 63, 94, 0.55)`     | Critical. Destructive.     |
| Fuchsia   | `#d946ef` | `#a21caf` | —                             | Decorative only (logo).    |

**Status semantics** (locked):

| State        | Token            | Hex       |
|--------------|------------------|-----------|
| OK / live    | `--status-ok`    | `#34d399` |
| Warn / slow  | `--status-warn`  | `#fbbf24` |
| Critical     | `--status-crit`  | `#f87171` |
| Info         | `--status-info`  | `#60a5fa` |

Cyan is reserved for **brand identity** (logo, hero accent gradient,
the cyan apex). Don't use cyan as a status color — that's emerald's
job.

### 3.6 Rules

- **Never** use saturated accents as backgrounds. They appear at
  10–15% opacity for tone backgrounds, ~22–28% opacity for accent
  borders, and 100% only on text/icons/strokes.
- **No more than two accents** in a single component. The hero
  gradient (cyan → blue → purple) is the one exception.
- Pure black (`#000`) and pure white (`#fff`) are not used. The
  closest neutrals are `#060a14` and `#f1f5f9`.

---

## 4. Typography

Vigil uses three font families, each with a defined role.

| Role               | Family                                          | Notes                                          |
|--------------------|-------------------------------------------------|------------------------------------------------|
| **Display / UI**   | `IBM Plex Sans` → `Inter` → system-ui           | Headings, labels, eyebrows. Weights 500–700.   |
| **Body**           | `Lato` → `Montserrat` → system-ui               | Long-form copy. Weights 200–400.               |
| **Mono / data**    | `IBM Plex Mono` → `ui-monospace` → system mono  | All numbers, code, terminals, metric values, technical chips. |

**Tabular figures are required for any number that updates in place.**
Set `font-variant-numeric: tabular-nums; font-feature-settings: "tnum";`
on metric values, footers, and table cells so digits don't shift width.

### 4.1 Type scale

A six-step scale, defined as custom properties so components can
reference it consistently.

| Token            | Size      | Use                                        |
|------------------|-----------|--------------------------------------------|
| `--fs-micro`     | 10 px     | Chips, badges, eyebrows, technical labels  |
| `--fs-caption`   | 12 px     | URLs, mono data rows, secondary labels     |
| `--fs-body`      | 14 px     | Primary body copy, list-item names         |
| `--fs-heading`   | 16 px     | Section titles, stat-card titles           |
| `--fs-display`   | 24 px     | Large metric values                        |
| `--fs-hero`      | 30 px     | Primary "instrument" numbers, page titles  |

Marketing hero headings scale fluidly:
`clamp(2.4rem, 5.6vw, 4.4rem)` with `letter-spacing: -0.03em` and
`font-weight: 700`.

### 4.2 Letter-spacing

Letter-spacing is **expressive**, not incidental:

| Context                           | Tracking          |
|-----------------------------------|-------------------|
| Hero / large display              | `-0.03em` (tight) |
| Section titles                    | `-0.02em`         |
| Body copy                         | default (0)       |
| Section / stat-card titles (caps) | `+0.06–0.10em`    |
| Eyebrows (mono, caps)             | `+0.18–0.22em`    |
| Status chips (mono, caps)         | `+0.16em`         |
| Footer pills                      | `+0.06–0.08em`    |

The rule of thumb: **the smaller and more upper-case the text, the
wider its tracking.** This is what gives the small mono labels their
"control panel" feel.

### 4.3 The eyebrow

The eyebrow is a recurring micro-component:

```
font-family: IBM Plex Sans (or Mono on dark)
font-size:   var(--fs-micro)   /* 10–12px */
font-weight: 700
letter-spacing: 0.22em
text-transform: uppercase
color: var(--text-dim) (light) | var(--cyan) | var(--blue) | var(--accent)
```

It introduces a section, names a tag (`BACKEND`, `FRONTEND`), or
labels a hero pulse (`LIVE · 1Hz STREAMING`).

### 4.4 The accent gradient text

Used **once per page**, on the hero key phrase:

```
background: linear-gradient(120deg, var(--cyan) 10%, var(--blue) 50%, var(--purple) 95%);
-webkit-background-clip: text;
background-clip: text;
-webkit-text-fill-color: transparent;
```

Don't apply this to anything other than a single short phrase.
Repetition breaks its impact.

---

## 5. Spacing, radii, elevation

### 5.1 Radii

| Token   | Value | Use                                   |
|---------|-------|---------------------------------------|
| `--r-sm`| 8 px  | Status chips, icon tiles, inputs      |
| `--r-md`| 12 px | Cards, terminals, feature tiles       |
| `--r-lg`| 18 px | Hero browser frame, CTA cards         |
| pill    | 999px | Buttons, hero eyebrow, stack pills    |
| panel   | 14 px | App-side panel cards                  |

### 5.2 Component padding

| Component              | Padding                          |
|------------------------|----------------------------------|
| Section header         | `0.6rem 1rem`                    |
| Stat card header       | `0.55rem 0.9rem`                 |
| Stat card body         | `0.7rem 0.95rem 0.85rem`         |
| Panel card body        | `0.85rem 1rem 0.95rem`           |
| Button (default)       | `0.55rem 1rem`                   |
| Pill / chip            | `0.32rem 0.7rem` (chip), `0.45rem 0.9rem` (pill) |
| Marketing section      | `5rem 0` vertical                |
| Hero                   | `6rem 0 4rem`                    |

Cards and chrome **tighten on mobile** (`<576px`): typically reduce
horizontal padding by `0.1–0.15rem` and shrink icon tiles by 2–4 px.

### 5.3 Elevation

There are **two** shadow recipes. Use them; do not invent new ones.

**Soft glass (panels):**
```
0 1px 0 rgba(255, 255, 255, 0.9) inset,
0 8px 24px -8px rgba(15, 23, 42, 0.18),
0 24px 48px -16px rgba(15, 23, 42, 0.18)
```

On hover, increase the second and third shadows (~12px / 32px and
32px / 64px) and translate the card `-2px` on Y.

**Dark drama (marketing browser frame):**
```
0 30px 80px -20px rgba(2, 6, 23, 0.7),
0 0 0 1px rgba(96, 165, 250, 0.06) inset
```

On hover, add a cyan rim glow:
```
0 0 60px -10px rgba(34, 211, 238, 0.45)
```

In dark mode, the soft-glass shadow swaps inset highlight for a faint
cyan glow:
```
0 0 0 1px rgba(96, 165, 250, 0.05) inset,
0 8px 24px -8px rgba(0, 0, 0, 0.55),
0 28px 56px -18px rgba(0, 0, 0, 0.6)
```

---

## 6. Atmosphere

Every full-screen surface in the system carries an **atmosphere
layer** — fixed to the viewport, behind everything, pointer-events
none. It's what makes the brand feel like a place rather than a page.

### 6.1 Radial glow gradients

Light mode:
```
radial-gradient(900px  circle at 12% 8%,   rgba(96, 165, 250, 0.22), transparent 65%),
radial-gradient(800px  circle at 88% 22%,  rgba(167, 139, 250, 0.18), transparent 65%),
radial-gradient(1000px circle at 55% 110%, rgba(34, 211, 238, 0.18), transparent 65%),
radial-gradient(700px  circle at 30% 85%,  rgba(16, 185, 129, 0.10), transparent 60%)
```

Dark mode is the same composition with stronger opacities and an
extra warm blob to break the cool palette:
```
radial-gradient(1000px at 10% 6%,   rgba(96, 165, 250, 0.28), …),
radial-gradient(900px  at 92% 18%,  rgba(167, 139, 250, 0.22), …),
radial-gradient(1100px at 55% 115%, rgba(34, 211, 238, 0.22), …),
radial-gradient(750px  at 25% 80%,  rgba(245, 158, 11, 0.12), …),
radial-gradient(700px  at 80% 65%,  rgba(244, 63, 94, 0.10),  …)
```

Marketing surface adds a slow drift animation
(`atmoShift 24s ease-in-out infinite alternate`) — a 2 % vertical
translate, no more. **Always gate motion behind
`@media (prefers-reduced-motion: reduce)`** and disable the
animation there.

### 6.2 Grid overlay

Every dark hero element (navbar, footer, section header, stat card
header, CTA card, atmosphere) is overlaid with a faint blueprint
grid:

```
linear-gradient(rgba(96, 165, 250, 0.04) 1px, transparent 1px),
linear-gradient(90deg, rgba(96, 165, 250, 0.04) 1px, transparent 1px);
background-size: 28–48px;
opacity: 0.5–0.6;
```

On the marketing atmosphere it's masked with a radial fade so the
grid only appears in the upper-center band:

```
mask-image: radial-gradient(ellipse at center top, black 30%, transparent 75%);
```

### 6.3 Corner glows

The same dark-hero elements add a second pseudo-layer with
**corner-radial accents** — a brighter blue blob at the top-left and
a fainter cyan blob at the bottom-right. Stack order is:

1. Base gradient (`#111827 → #1e293b`)
2. Grid overlay (pseudo `::before`)
3. Corner glows (pseudo `::after`)
4. Content (`position: relative; z-index: 1`)

This three-layer recipe is what makes the hero chrome readable as a
single material. Apply it consistently — it is *the* signature.

---

## 7. Iconography

Icons follow a **tone tile** pattern: the icon itself is colored,
sitting inside a translucent tile of the same color, ringed by a
slightly stronger inset border of the same color.

```
.icon-tile {
  width: 26–38px;
  height: 26–38px;
  border-radius: 7–10px;       /* small/medium/large variants */
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.icon-tile.tone-blue {
  background:  rgba(59, 130, 246, 0.12–0.15);
  color:       #60a5fa;
  box-shadow:  inset 0 0 0 1px rgba(59, 130, 246, 0.25–0.28);
}
```

Six standard tones — **blue, cyan, purple, amber, emerald, rose** —
plus a **slate** neutral for unclassified items. The same tone names
recur on chart strokes, badge backgrounds, and toggle thumbs, so
prop signatures across components stay consistent.

Icons themselves are **outline-style** SVGs (Font Awesome Solid is
used inside the app for parity with Bootstrap's data conventions; the
marketing surface uses inline outline SVGs, stroke 2, rounded line
caps and joins). Avoid filled glyphs except for chevrons, dots, and
small UI affordances.

---

## 8. Components

Every component below is described as a **recipe** — what it's made
of, not what library renders it.

### 8.1 Section header

The dashboard's most recurring chrome element.

- Header gradient (`#111827 → #1e293b`)
- Grid overlay + corner glows (the §6 recipe)
- Bottom border: `1px solid rgba(96, 165, 250, 0.22)`
- Left side: tone-tile icon + title (caps, +0.06em tracking) +
  optional mono subtitle (caps, +0.08em, dim)
- Right side: optional slot for actions

### 8.2 Panel card

A glassy card used for every dashboard section body:

- Radius `14px`, overflow hidden
- Light: `background: rgba(255, 255, 255, 0.72)`,
  border `rgba(255, 255, 255, 0.85)`,
  `backdrop-filter: blur(18px) saturate(140%)`
- Dark: `background: rgba(15, 23, 42, 0.55)`,
  border `rgba(148, 163, 184, 0.14)`
- Soft-glass shadow recipe (§5.3)
- Hover: lift `-2px`, deepen shadow, on dark add cyan rim
- Children: typically a section header on top, body padding
  `0.85rem 1rem 0.95rem`

### 8.3 Stat card

A **panel card** specialized for a single metric:

- Header gradient + grid overlay + corner glows
- Tone-tile icon + caps title (+0.10em tracking)
- Body is plain white (light) or `--bg-surface` (dark), padded
  `0.7rem 0.95rem 0.85rem`
- Numbers inside use the `metric-num` recipe:
  ```
  font-family: Montserrat / Exo 2 / monospace;
  font-feature-settings: "tnum";
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.01em;
  ```

### 8.4 Chips

Tight, mono, all-caps, semantic-color blocks:

| Variant      | Background                  | Border                         | Color     |
|--------------|-----------------------------|--------------------------------|-----------|
| Status (live)| `rgba(255,255,255,0.04)`    | `rgba(255,255,255,0.08)`       | currentColor (emerald) |
| Status (idle)| same                        | same                           | currentColor (amber)   |
| Node id      | `rgba(255,255,255,0.03)`    | `rgba(255,255,255,0.06)`       | `#cbd5e1`              |

Status chips include a **dot**: 7px round, `background: currentColor`,
`box-shadow: 0 0 10px currentColor`. When the chip is "live" the dot
runs a 1.4s opacity pulse (`0% → 1, 50% → 0.4, 100% → 1`).

### 8.5 Pills

Larger than chips — used in stack lists, version badges, footer meta:

```
padding: 0.45rem 0.9rem;
border-radius: 999px;
background: rgba(255, 255, 255, 0.04);
border: 1px solid var(--line);
font-family: mono;
font-size: 0.74rem;
letter-spacing: 0.06em;
```

### 8.6 Hero eyebrow

A pulsing live badge: pill-shaped, emerald-tinted, with a glowing dot.

```
padding: 0.35rem 0.75rem;
border-radius: 999px;
background: rgba(52, 211, 153, 0.08);
border: 1px solid rgba(52, 211, 153, 0.28);
color: #34d399;
font-family: IBM Plex Mono;
font-size: 0.7rem;
letter-spacing: 0.18em;
```

### 8.7 Buttons

Two variants only.

**Default (ghost)**
```
padding: 0.55rem 1rem;
border-radius: 999px;
border: 1px solid var(--line-strong);
background: rgba(255, 255, 255, 0.03);
color: var(--text);
font-family: IBM Plex Sans;
font-weight: 600;
letter-spacing: 0.02em;
```
On hover: lift `-1px`, brighten background, border → blue.

**Primary**
```
background: linear-gradient(135deg, #60a5fa, #22d3ee);  /* blue → cyan */
color: #0b1220;                                          /* dark text on bright button */
border-color: transparent;
box-shadow:
  0 12px 30px -12px rgba(34, 211, 238, 0.55),
  inset 0 0 0 1px rgba(255, 255, 255, 0.25);
```

**Don't** use a third button variant. If something needs to feel more
critical, change the iconography or add a status dot — don't recolor
the button.

### 8.8 Toggle switch

Pill-shaped track, `50px × 24px`, with **icons baked in at both ends**
(left = on/light/sun/bolt, right = off/dark/moon/pause). The thumb is
an 18px white circle that slides 26px on activation. The **icons
change color** to indicate state:

- Off state: left icon = amber `#fbbf24`, right icon = slate `#94a3b8`
- On state:  left icon = dim `#6b7280`,  right icon = purple `#c084fc`

Track background: `rgba(255, 255, 255, 0.06)` ↑ to `0.10` on hover.
Thumb transition: `transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1)`
— a slight overshoot is part of the brand.

### 8.9 Browser frame (marketing)

Used to present screenshots:

- Outer radius `--r-lg`, border `--line-strong`, dark drama shadow
- Header bar `0.7rem 1rem` with three colored dots
  (rose, amber, emerald, all 50% opacity) and a faux URL bar with a
  green lock dot
- On hover: `perspective(1400px)` + cursor-tracked `rotateX/rotateY`
  (max ~10°), holographic diagonal sheen tracks the cursor, plus a
  ~210px white-radial spotlight glare. All pointer effects are
  disabled under `prefers-reduced-motion: reduce`.

### 8.10 Terminal block

Used to present commands. Same radius / shadow as the browser frame,
slightly darker background (`#0a0e18`), header bar with three dots
plus a mono label (`~/path — bash`).

Code coloring (consistent across the brand):

| Class           | Color            | Use                  |
|-----------------|------------------|----------------------|
| `term-prompt`   | cyan `#22d3ee`   | `$`                  |
| `term-cmd`      | `#f1f5f9`        | the command          |
| `term-flag`     | amber `#fbbf24`  | `--flag` / inline literals |
| `term-string`   | emerald `#34d399`| quoted strings, URLs |
| `term-comment`  | dim slate, italic| `# comments`         |

### 8.11 Architecture node

Two boxes connected by a glowing arrow (used to depict a system
topology):

- Each node: `panel-card` styling + an `arch-tag` eyebrow
  (mono, +0.18em, dim) + a heading + a bulleted list with **5px round
  bullets** in the tone color
- The connector: an inline SVG arrow in cyan with
  `filter: drop-shadow(0 0 8px var(--cyan))`, plus a tiny mono caption
  beneath (`JSON · 1 Hz`)

This pattern generalizes — anywhere you need to show "A talks to B."

---

## 9. Data visualization

Charts are first-class brand surfaces. The rules:

1. **One tone per chart.** Each chart picks a tone (`blue / cyan /
   purple / amber / green / rose`) and uses its primary, secondary,
   and glow colors only.
2. **Glow lines, not flat lines.** Stroke widths are typically
   1.5–2 px, with an SVG `feGaussianBlur` filter on a duplicated
   stroke beneath, tinted to the tone's `glow` color.
3. **Stroke gradients along the X axis.** Lines fade from primary →
   secondary as new data arrives, suggesting movement.
4. **Subtle axes.** Axis lines and ticks: `stroke: #94a3b8` (dark) /
   grey (light), `opacity: 0.2`. Never bold the axis.
5. **Tabular numerics on every label.** No exceptions for tick text,
   tooltips, or legends.
6. **Rolling windows have a fixed range.** A 30-second realtime
   window with a Y-axis that smoothly transitions when bounds
   change is the canonical layout.

Tone palette for charts (matches §3.5; this is the canonical
ChartTone set referenced in code):

```ts
{
  blue:   { primary: "#60a5fa", secondary: "#93c5fd", glow: "rgba(96, 165, 250, 0.55)" },
  purple: { primary: "#a78bfa", secondary: "#c4b5fd", glow: "rgba(167, 139, 250, 0.55)" },
  amber:  { primary: "#f59e0b", secondary: "#fbbf24", glow: "rgba(245, 158, 11, 0.55)" },
  green:  { primary: "#10b981", secondary: "#34d399", glow: "rgba(16, 185, 129, 0.55)" },
  cyan:   { primary: "#22d3ee", secondary: "#67e8f9", glow: "rgba(34, 211, 238, 0.55)" },
  rose:   { primary: "#f43f5e", secondary: "#fb7185", glow: "rgba(244, 63, 94, 0.55)" },
}
```

---

## 10. Motion

Motion in Vigil is **slow, soft, and informative.** Three patterns
recur:

| Pattern              | Use                              | Recipe                                                            |
|----------------------|----------------------------------|-------------------------------------------------------------------|
| **Live pulse**       | Status dots, hero eyebrow dot    | 1.4–1.6s opacity oscillation, optional 0.9–1.1 scale              |
| **Lift on hover**    | Cards, buttons                   | `-1` to `-3px` translateY, 180–250ms ease, deepen shadow          |
| **Reveal-on-scroll** | Marketing sections               | `opacity 0 → 1`, `translateY 18px → 0`, 700ms `cubic-bezier(.2,.8,.2,1)`, fired by IntersectionObserver |

Plus:

- **Atmosphere drift** (24s, marketing only).
- **Heartbeat**: 2.4s ease-in-out, scale `1 → 1.18 → 0.95 → 1` on
  the rose-colored heart in the footer signature line.
- **Toggle thumb overshoot**: 0.25s `cubic-bezier(0.34, 1.56, 0.64, 1)`.
- **Holographic sheen + spotlight**: only on hover-capable inputs,
  cursor-tracked.

**Every animation is gated behind**
`@media (prefers-reduced-motion: reduce)`. The browser-frame tilt
and atmosphere drift are disabled there, the heart stops, and reveal
elements skip the transition (visible immediately, no transform).

---

## 11. Layout

- Marketing wrap: `width: min(1180px, 92%)`, centered.
- App layout: full-bleed, `container-fluid`-style with horizontal
  gutters of `0.75rem` on mobile.
- Dashboard sections are flexbox rows of equal-height
  `panel-card`s (`flex-fill`).
- Sticky navbar, sticky page header on long views, footer always at
  the bottom of a `min-height: 100vh` flex column.
- A **section** in marketing is `5rem 0` vertical with an eyebrow,
  title, and lede stack as the opener.

Breakpoints follow a standard 4-tier scale:
`576px / 768px / 992px / 1200px`. The two we actually customize for
are `<576px` (chrome shrinks, gutters tighten) and `<992px`
(navbar collapses, controls left-justify).

---

## 12. Accessibility

- **Color contrast.** Body text on dark surfaces (`#cbd5e1` on
  `#0f172a`) clears WCAG AA. Eyebrow / dim text colors clear AA Large
  only — reserve them for non-essential metadata.
- **Status is never color alone.** The status chip pairs the dot
  color with the word `LIVE` / `STATIC`. Charts that swap tones to
  signal change always also change shape, label, or value.
- **`prefers-reduced-motion: reduce`** disables atmosphere drift,
  browser-frame tilt + sheen + spotlight, the live pulse, the
  heartbeat, and reveal-on-scroll.
- **Focus.** Buttons, toggles, and links must show a visible focus
  ring; default browser outline is acceptable, custom rings should
  use the blue accent at full opacity with `2px` offset.
- **Aria.** Decorative SVGs (logo halo, atmosphere) carry
  `aria-hidden="true"`. The logo carries `role="img"
  aria-label="Vigil"`. Toggles use `<input type="checkbox">` with
  `aria-label`.

---

## 13. Adapting to a new surface

If you're applying this language to a fresh project, the minimum
viable identity is:

1. **The mark** — or a derivative cube-stack mark in the same
   palette, drop-shadowed in cyan.
2. **The atmosphere** — fixed multi-radial glow + masked grid.
3. **The header gradient** + grid overlay + corner glows recipe on
   any dark hero chrome.
4. **The type triumvirate** — IBM Plex Sans / Lato / IBM Plex Mono,
   used by role.
5. **The accent palette as LEDs**, never as backgrounds.
6. **Tabular figures everywhere a number can change.**

Skip any of those and the brand starts to feel like something else.
Get all six and a reader will recognize it as Vigil before they
read a single word.
