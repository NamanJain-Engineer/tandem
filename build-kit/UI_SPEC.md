# UI_SPEC: Tandem desktop UI and UX

**Status:** binding for implementation.
**Owner of truth:** this file (visual/UX), `PRODUCT_SPEC.md` (behavior), `DECISIONS.md` (identity and stack). If they conflict, the kit-wide precedence applies: DECISIONS > PRODUCT_SPEC > ARCHITECTURE (IPC, data, protocols) > this file > MILESTONES. The keyboard map in §5 and the tokens in §1.2 are canonical and PRODUCT_SPEC references them.
**Implementation target:** `packages/ui` (tokens, primitives, composites) and `apps/desktop/src` (screens). React 19, TypeScript, Tailwind 4, Radix primitives (via shadcn-style local components), lucide-react icons, xterm.js for terminals.

This is Tandem's own design. It covers the same jobs as other agent workspaces, but its layout rhythm, colour, names and copy are original. Don't import other products' names, artwork or phrasing.

---

## 0. How to use this spec (for the implementing agent)

1. **Build the tokens first**, as the exact CSS in §1. Then build every primitive in §2 with Storybook-less "gallery" routes (`/__gallery` in dev builds) showing each state, and write a vitest snapshot for each state.
2. **Build the shell** (§3) with placeholder content. Then build screens in the order of MILESTONES.md.
3. **Every screen in §4 has:**
   - an **ID** (`UI-xx`) and **feature codes** (map to PRODUCT_SPEC IDs);
   - an ASCII wireframe;
   - its labels (use them verbatim);
   - its states;
   - its interactions;
   - its acceptance checks.
4. **ASCII wireframes are layout intent, not pixel geometry.** Use the dimensions stated in the text.
5. **Microcopy in quotes is final.** Placeholders like `{name}` are interpolated.
6. **Everything must work** in both dark and light themes, with keyboard only, and at 200% zoom.

**Feature-area codes:**

| Code | Area | Code | Area |
|---|---|---|---|
| SH | Shell, window, navigation | PL | Plugins and gateway |
| CD | Code mode, panes, terminals | AU | Auto supervisor |
| TH | Thread mode | DI | Dictate (push-to-talk) |
| EN | Engines (CLI detect/install/launch) | CN | Conductor (voice agent) |
| AC | Accounts and usage | RM | Remote (SSH, phone) |
| DB | Dashboard and notifications | BE | Backend-backed account/auth |
| AG | Teammates (Agent mode) | BL | Billing, credits, paywall |
| TK | Tasks | WB | Website/docs (separate app) |
| RT | Routines | TL | Telemetry, logs, updates, diagnostics |

---

## 1. Design tokens

### 1.1 Principles
- **Calm surfaces, readable status.** Neutrals carry 95% of the UI. Colour is reserved for status, the accent (focus, primary action, selection) and diff/syntax.
- **Density:** a compact desktop density with a 4px base grid. The default control height is 32px, and 28px in dense contexts (pane headers, tables).
- **Two themes, one set of tokens.** Components only reference semantic tokens (`--tn-*`), never raw palette values.
- **Terminals stay terminals.** Terminal colours come from a separate ANSI palette per theme (§1.9).

### 1.2 CSS: paste into `packages/ui/src/styles/tokens.css`

```css
@import "tailwindcss";

/* ---------- Raw palette (do not use directly in components) ---------- */
:root {
  --slate-0:   #ffffff;
  --slate-25:  #f8fafc;
  --slate-50:  #f1f5f9;
  --slate-100: #e2e8f0;
  --slate-200: #cbd5e1;
  --slate-300: #94a3b8;
  --slate-400: #64748b;
  --slate-500: #475569;
  --slate-600: #334155;
  --slate-700: #1e293b;
  --slate-750: #172033;
  --slate-800: #111827;
  --slate-850: #0d1320;
  --slate-900: #0a0f1a;
  --slate-950: #060a12;

  --teal-50:  #effcf9;
  --teal-100: #ccfbf1;
  --teal-200: #99f6e4;
  --teal-300: #5eead4;
  --teal-400: #2dd4bf;   /* brand accent */
  --teal-500: #14b8a6;
  --teal-600: #0d9488;
  --teal-700: #0f766e;
  --teal-800: #115e59;
  --teal-900: #134e4a;

  --amber-300: #fcd34d;  --amber-400: #fbbf24; --amber-500: #f59e0b; --amber-600: #d97706; --amber-700: #b45309;
  --sky-300:   #7dd3fc;  --sky-400:   #38bdf8; --sky-500:   #0ea5e9; --sky-600:   #0284c7; --sky-700: #0369a1;
  --green-300: #86efac;  --green-400: #4ade80; --green-500: #22c55e; --green-600: #16a34a; --green-700: #15803d; --green-800: #166534;
  --rose-300:  #fda4af;  --rose-400:  #fb7185; --rose-500:  #f43f5e; --rose-600:  #e11d48; --rose-700:  #be123c; --rose-800: #9f1239;
  /* Ink: the neutral near-black scale behind every surface (D-1894, D-2600). */
  --ink-0:   #ffffff;
  --ink-25:  #fafafa;
  --ink-50:  #f4f4f5;
  --ink-100: #e8e8eb;
  --ink-200: #d4d4d8;
  --ink-300: #a1a1aa;
  --ink-400: #71717a;
  --ink-500: #52525b;
  --ink-600: #3f3f46;
  --ink-700: #2a2a2f;
  --ink-750: #232327;
  --ink-775: #1c1c20;
  --ink-800: #17171a;
  --ink-850: #121214;
  --ink-900: #0e0e10;
  --ink-950: #09090b;

  --violet-400:#a78bfa;  --violet-500:#8b5cf6;
  --orange-400:#fb923c;
}

/* ---------- Semantic tokens: DARK (default) ---------- */
:root, [data-theme="dark"] {
  color-scheme: dark;

  /* surfaces (elevation increases upward): neutral near-black, each step a
     little lighter, so panes lift off the window without hard lines (D-2600) */
  --tn-bg:            var(--ink-950);  /* window background, behind panes */
  --tn-surface-1:     var(--ink-900);  /* rails, sidebars */
  --tn-surface-2:     var(--ink-850);  /* panes, cards */
  --tn-surface-3:     var(--ink-775);  /* raised: menus, popovers, sheets */
  --tn-surface-4:     var(--ink-750);  /* hover on surface-3, inputs */
  --tn-overlay:       rgb(0 0 0 / 0.62);/* modal scrim */

  /* borders: soft, so elevation does the separating */
  --tn-border:        rgb(255 255 255 / 0.08);
  --tn-border-strong: rgb(255 255 255 / 0.15);
  --tn-divider:       rgb(255 255 255 / 0.055);

  /* text */
  --tn-text:          #ededef;
  --tn-text-muted:    #a3a3ac;
  --tn-text-subtle:   #8b8b95;
  --tn-text-disabled: #55555e;
  --tn-text-inverse:  var(--ink-950);

  /* accent */
  --tn-accent:        var(--teal-400);
  --tn-accent-hover:  var(--teal-300);
  --tn-accent-press:  var(--teal-500);
  --tn-accent-fg:     #042f2c;                 /* text on accent */
  --tn-accent-soft:   rgb(45 212 191 / 0.14);  /* selected row, chip bg */
  --tn-accent-ring:   rgb(45 212 191 / 0.55);  /* focus ring */

  /* status: agent/session lifecycle */
  --tn-status-working:      var(--sky-400);
  --tn-status-working-soft: rgb(56 189 248 / 0.14);
  --tn-status-working-text: var(--sky-400);   /* status as text (Badges, labels) */
  --tn-status-waiting-text: var(--amber-400);
  --tn-status-done-text:    var(--green-400);
  --tn-status-waiting:      var(--amber-400);
  --tn-status-waiting-soft: rgb(251 191 36 / 0.14);
  --tn-status-done:         var(--green-400);
  --tn-status-done-soft:    rgb(74 222 128 / 0.14);
  --tn-status-idle:         var(--ink-400);
  --tn-status-idle-soft:    rgb(113 113 122 / 0.16);
  --tn-status-error:        var(--rose-400);
  --tn-status-error-soft:   rgb(251 113 133 / 0.14);

  /* feedback */
  --tn-success: var(--green-400);
  --tn-warning: var(--amber-400);
  --tn-warning-text: var(--amber-400); /* warning as text (meters at 80%, cautions) */
  --tn-danger:  var(--rose-400);
  --tn-danger-hover: var(--rose-300);
  --tn-danger-fg: #2b0710;
  --tn-info:    var(--sky-400);

  /* diff */
  --tn-diff-add-bg:   rgb(34 197 94 / 0.12);
  --tn-diff-add-fg:   var(--green-300);
  --tn-diff-del-bg:   rgb(244 63 94 / 0.12);
  --tn-diff-del-fg:   var(--rose-300);
  --tn-diff-hunk-bg:  rgb(56 189 248 / 0.08);

  /* misc */
  --tn-scrollbar:     rgb(255 255 255 / 0.16);
  --tn-selection:     rgb(45 212 191 / 0.28);
  --tn-kbd-bg:        var(--ink-775);
}

/* ---------- Semantic tokens: LIGHT ---------- */
[data-theme="light"] {
  color-scheme: light;

  --tn-bg:            var(--ink-50);
  --tn-surface-1:     var(--ink-25);
  --tn-surface-2:     var(--ink-0);
  --tn-surface-3:     var(--ink-0);
  --tn-surface-4:     var(--ink-50);
  --tn-overlay:       rgb(9 9 11 / 0.32);

  --tn-border:        rgb(9 9 11 / 0.09);
  --tn-border-strong: rgb(9 9 11 / 0.17);
  --tn-divider:       rgb(9 9 11 / 0.06);

  --tn-text:          #18181b;
  --tn-text-muted:    #4f4f58;
  --tn-text-subtle:   #62626c;
  --tn-text-disabled: #a1a1aa;
  --tn-text-inverse:  #ffffff;

  --tn-accent:        var(--teal-700);   /* #0f766e: 5.5:1 on white (§7.3; teal-600 was 3.74:1) */
  --tn-accent-hover:  var(--teal-800);
  --tn-accent-press:  var(--teal-900);
  --tn-accent-fg:     #ffffff;
  --tn-accent-soft:   rgb(15 118 110 / 0.10);
  --tn-accent-ring:   rgb(15 118 110 / 0.85);  /* §5.4: 3.6:1 on every surface (0.50 was 2.1:1) */

  --tn-status-working:      var(--sky-600);
  --tn-status-working-soft: rgb(2 132 199 / 0.10);
  --tn-status-working-text: var(--sky-700);    /* 5.9:1 on white (§7.3) */
  --tn-status-waiting-text: var(--amber-700);  /* 5.0:1 */
  --tn-status-done-text:    var(--green-700);  /* 5.0:1 */
  --tn-status-waiting:      var(--amber-700);  /* §5.4: 3:1 as a dot (amber-600 was 2.9:1 on bg) */
  --tn-status-waiting-soft: rgb(217 119 6 / 0.12);
  --tn-status-done:         var(--green-700);  /* 3:1 as a dot on the ink-50 bg */
  --tn-status-done-soft:    rgb(22 163 74 / 0.10);
  --tn-status-idle:         var(--ink-400);
  --tn-status-idle-soft:    rgb(113 113 122 / 0.12);
  --tn-status-error:        var(--rose-700);  /* AA as text too: it labels errors across the app */
  --tn-status-error-soft:   rgb(225 29 72 / 0.10);

  --tn-success: var(--green-600);
  --tn-warning: var(--amber-600);
  --tn-warning-text: var(--amber-700); /* AA as text on every light surface */
  --tn-danger:  var(--rose-700);   /* §5.4: 4.5:1 as text (rose-600 was 4.3:1 on bg) */
  --tn-danger-hover: var(--rose-800);
  --tn-danger-fg: #ffffff;
  --tn-info:    var(--sky-600);

  --tn-diff-add-bg:   rgb(22 163 74 / 0.10);
  --tn-diff-add-fg:   var(--green-800);  /* §5.4: green-700 was 4.49:1 on its tint */
  --tn-diff-del-bg:   rgb(225 29 72 / 0.08);
  --tn-diff-del-fg:   var(--rose-700);
  --tn-diff-hunk-bg:  rgb(2 132 199 / 0.06);

  --tn-scrollbar:     rgb(9 9 11 / 0.20);
  --tn-selection:     rgb(15 118 110 / 0.20);
  --tn-kbd-bg:        var(--ink-50);
}

/* ---------- Theme mapping for Tailwind 4 ---------- */
@theme inline {
  --color-bg: var(--tn-bg);
  --color-surface-1: var(--tn-surface-1);
  --color-surface-2: var(--tn-surface-2);
  --color-surface-3: var(--tn-surface-3);
  --color-surface-4: var(--tn-surface-4);
  --color-overlay: var(--tn-overlay);
  --color-border: var(--tn-border);
  --color-border-strong: var(--tn-border-strong);
  --color-divider: var(--tn-divider);
  --color-fg: var(--tn-text);
  --color-fg-muted: var(--tn-text-muted);
  --color-fg-subtle: var(--tn-text-subtle);
  --color-fg-disabled: var(--tn-text-disabled);
  --color-fg-inverse: var(--tn-text-inverse);
  --color-accent: var(--tn-accent);
  --color-accent-hover: var(--tn-accent-hover);
  --color-accent-press: var(--tn-accent-press);
  --color-accent-fg: var(--tn-accent-fg);
  --color-accent-soft: var(--tn-accent-soft);
  --color-working: var(--tn-status-working);
  --color-working-soft: var(--tn-status-working-soft);
  --color-working-text: var(--tn-status-working-text);
  --color-waiting-text: var(--tn-status-waiting-text);
  --color-done-text: var(--tn-status-done-text);
  --color-waiting: var(--tn-status-waiting);
  --color-waiting-soft: var(--tn-status-waiting-soft);
  --color-done: var(--tn-status-done);
  --color-done-soft: var(--tn-status-done-soft);
  --color-idle: var(--tn-status-idle);
  --color-idle-soft: var(--tn-status-idle-soft);
  --color-error: var(--tn-status-error);
  --color-error-soft: var(--tn-status-error-soft);
  --color-success: var(--tn-success);
  --color-warning: var(--tn-warning);
  --color-warning-text: var(--tn-warning-text);
  --color-danger: var(--tn-danger);
  --color-danger-hover: var(--tn-danger-hover);
  --color-danger-fg: var(--tn-danger-fg);
  --color-info: var(--tn-info);
  --color-diff-add-bg: var(--tn-diff-add-bg);
  --color-diff-add-fg: var(--tn-diff-add-fg);
  --color-diff-del-bg: var(--tn-diff-del-bg);
  --color-diff-del-fg: var(--tn-diff-del-fg);
  --color-diff-hunk-bg: var(--tn-diff-hunk-bg);

  --font-sans: "Inter", ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace;

  /* type scale (size / line-height) */
  --text-2xs: 10px;  --text-2xs--line-height: 14px;
  --text-xs:  11px;  --text-xs--line-height: 16px;
  --text-sm:  12px;  --text-sm--line-height: 18px;
  --text-base:13px;  --text-base--line-height: 20px;   /* default UI text */
  --text-md:  14px;  --text-md--line-height: 22px;     /* chat body, composer */
  --text-lg:  16px;  --text-lg--line-height: 24px;
  --text-xl:  20px;  --text-xl--line-height: 28px;
  --text-2xl: 24px;  --text-2xl--line-height: 32px;
  --text-3xl: 30px;  --text-3xl--line-height: 38px;    /* onboarding hero only */

  --font-weight-regular: 400;
  --font-weight-medium: 500;
  --font-weight-semibold: 600;

  --tracking-tight: -0.01em;
  --tracking-caps: 0.06em;   /* for 10–11px uppercase section labels */

  /* spacing: Tailwind default 4px scale is used as-is (spacing-1 = 4px) */
  --spacing: 4px;

  /* radii: controls 6, panels 10, dialogs 12 (D-1894) */
  --radius-xs: 4px;    /* chips, kbd, badges */
  --radius-sm: 6px;    /* controls: buttons, inputs, menu items */
  --radius-md: 10px;   /* panels, panes, cards, popovers (D-1894) */
  --radius-lg: 12px;   /* dialogs, sheets */
  --radius-full: 9999px;

  /* elevation: one set, brand.css's e1–e3 (D-1891, D-1894) */
  --shadow-1: var(--tn-elev-1);
  --shadow-2: var(--tn-elev-2);
  --shadow-3: var(--tn-elev-3);
  --shadow-focus: 0 0 0 2px var(--tn-bg), 0 0 0 4px var(--tn-accent-ring);

  /* motion */
  --ease-standard: cubic-bezier(0.2, 0, 0, 1);
  --ease-enter:    cubic-bezier(0, 0, 0.2, 1);
  --ease-exit:     cubic-bezier(0.4, 0, 1, 1);
  --ease-spring:   cubic-bezier(0.34, 1.36, 0.64, 1);  /* only for toasts/HUD pop-in */
}

:root {
  /* durations (not mapped into Tailwind; use via arbitrary values or utilities below) */
  --dur-instant: 60ms;   /* hover colour */
  --dur-fast:    120ms;  /* press, toggle, tooltip */
  --dur-base:    180ms;  /* menus, popovers, tabs */
  --dur-slow:    260ms;  /* sheets, dialogs, sidebar slide */
  --dur-deliberate: 400ms; /* onboarding steps, splash fade */

  /* z-index scale */
  --z-base: 0;
  --z-pane-overlay: 10;   /* drag handles, pane drop targets */
  --z-rail: 20;
  --z-titlebar: 30;
  --z-sidebar: 40;
  --z-sheet: 200;
  --z-dialog: 300;
  --z-popover: 350;       /* menus, selects, tooltips anchored: above the sheet or dialog they open from (D-109) */
  --z-toast: 400;
  --z-hud: 500;           /* Dictate/Conductor HUD */
  --z-command: 600;       /* command palette sits above everything but HUD? no: above HUD */
  --z-max: 9999;          /* crash/restore splash, drag ghost */

  /* layout constants */
  --titlebar-h: 40px;
  --rail-w: 248px;        --rail-min: 246px; --rail-max: 360px; --rail-collapsed: 52px;
  --sidebar-w: 470px;     --sidebar-min: 320px; --sidebar-max: 900px;
  --pane-header-h: 30px;
  --statusbar-h: 22px;
  --composer-min-h: 88px; --composer-max-h: 320px;
}

/* reduced motion: collapse all durations */
@media (prefers-reduced-motion: reduce) {
  :root { --dur-fast: 0ms; --dur-base: 0ms; --dur-slow: 0ms; --dur-deliberate: 0ms; }
}
[data-motion="reduced"] { --dur-fast: 0ms; --dur-base: 0ms; --dur-slow: 0ms; --dur-deliberate: 0ms; }

/* base */
html, body, #root { height: 100%; background: var(--tn-bg); color: var(--tn-text); }
body { font-family: var(--font-sans); font-size: 13px; line-height: 20px; -webkit-font-smoothing: antialiased; font-feature-settings: "cv01", "cv09", "cv11", "ss03"; }
::selection { background: var(--tn-selection); }
:focus-visible { outline: none; box-shadow: var(--shadow-focus); border-radius: inherit; }
* { scrollbar-width: thin; scrollbar-color: var(--tn-scrollbar) transparent; }
```

**Z-index note:** the command palette is `--z-command` (600), above the HUD (500). When the palette opens, the Dictate/Conductor HUD remains visible behind it with 40% opacity.

### 1.2a Developer-first refresh (M18 · `app-ui`, D-2600 to D-2601)
These replace the slate-on-navy look the app shipped with up to 0.8.0. The direction is the design lead's (D-1894, docs/DESIGN.md §4.2, §4.5, §4.7); D-2600 and D-2601 built it. The CSS above is already the result.
- **Neutral near-black (D-2600).** A new raw `--ink-*` scale (zinc-like, no blue cast: no channel more than 4 apart) backs every app surface. Dark: bg `#09090b`, rails `#0e0e10`, panes `#121214`, raised `#1c1c20`, inputs and hover `#232327`. Light: bg `#f4f4f5`, rails `#fafafa`, panes and raised `#ffffff`, inputs `#f4f4f5`. Slate stays only in the raw palette until the site's T8 moves its last semantic uses to ink. The accent stays teal (DECISIONS).
- **Gentle elevation, soft borders (D-2600).** Borders are white (dark) or ink (light) at 8% / 15%, dividers 5.5–6%. Each surface step is lighter than the one below (a test checks the order). Depth and the lit top edge are one set, `brand.css`'s e1–e3 (D-1891): `--shadow-1…3` point at `--tn-elev-1…3`.
- **Radii (D-1894):** xs 4px (chips, kbd, badges), sm 6px (controls), md 10px (panes, panels, cards, popovers), lg 12px (dialogs, sheets). A 6px control nests in a 10px panel with 4px of padding.
- **Spacing rhythm:** the 4px grid stays. Panels 12px dense or 16px default, dialogs 24px; rows 30px; pane gutters 6px.
- **Focus rings:** unchanged in shape: 2px of the window background, then 2px of `accent-ring`, at 3:1 or better on every surface in both themes.
- **Type:** Inter with the site's alternates cv01, cv09, cv11 and ss03 (DESIGN §4.3), so the app and the site draw the same figures; the scale in §1.4 is unchanged. Counts, times and meters are tabular.
- **Terminal palette:** Night's background is the pane surface `#121214` with neutral blacks and whites, so a terminal sits flush in its pane; Day's greys are the ink scale (§1.9). The window's first paint and the PTY's OSC 11 answer use the same values.
- **Quiet status primitives (D-2601):**
  - `StatusDot`: working is a filled dot whose halo breathes (still under reduced motion); waiting a diamond, done a check, idle a hollow ring, error a triangle, offline a dashed ring. Every status still has its own shape (§1.3).
  - `Badge quiet`: a dot and sentence-case coloured text with no box ("● Needs you" in `waiting-text`), for rows that must stay calm. The boxed badge stays for rare flags.
  - `Count`: a 16px tabular pill for unread and queued counts, "99+" above 99, tones neutral, accent and strong.
  - `TimeAgo` / `compactAgo`: "now", "4m", "2h", "3d" in a `<time>` whose tooltip is the full date; labels come from the caller's `en.json`.
- **Contrast:** every text token passes 4.5:1 and every status mark and the focus ring 3:1 on all five surfaces in both themes (`contrast.test.ts`). Light `status-done` moved to green-700 for that, and light `status-error` to rose-700, because `text-error` labels errors as text across the app (rose-600 was 4.27:1 on the ink-50 background). `warning-text` is a token too.

### 1.3 Colour usage rules
| Use | Token | Never |
|---|---|---|
| Primary button, focus ring, selected tab underline, active mode segment, link | accent | Status meaning |
| Working (agent producing output) | `status-working` (sky) | Accent |
| Waiting for you (approval, question, input needed) | `status-waiting` (amber) | Red |
| Done (turn finished, ready for review) | `status-done` (green) | |
| Idle (session open, nothing happening) | `status-idle` (slate) | |
| Error, crashed, auth failure | `status-error` (rose) | Warning-type info |
| Destructive button (Delete, Discard, Disconnect) | danger | Plain accent |
| Status as **text** (Badge labels, "Working 3", "Waiting on you") | `status-{working,waiting,done}-text`, `status-error`; idle labels use `text-muted` | The shape-only `status-*` tokens as light-theme text (they fail AA, §7.3) |
| Usage and context meters: <80% | fg-muted fill | |
| Usage and context meters: 80–94% | warning fill | |
| Usage and context meters: ≥95% (plan windows only; context meters stop at warning) | danger fill | |

Every status indicator pairs colour with a **shape or text label** (never colour alone): working = a filled dot with a breathing halo (D-2601), waiting = filled diamond, done = check, idle = hollow circle, error = triangle.

### 1.4 Typography rules
- UI default: `text-base` (13/20) regular, `text-fg`.
- Section labels in rails and settings: `text-2xs` semibold uppercase, `tracking-caps`, `text-fg-subtle`.
- Screen titles (page header): `text-lg` semibold.
- Dialog titles: `text-lg` semibold. Onboarding hero: `text-3xl` semibold, `tracking-tight`.
- Chat/transcript body: `text-md` (14/22). Code in chat: `font-mono text-sm`.
- Terminal: `font-mono` 13px by default (Settings › Appearance: 11–18px), line-height 1.25.
- Numbers in meters, counts and timers: `font-variant-numeric: tabular-nums`.
- Truncation: single-line labels use `truncate` with a native tooltip (`title`) carrying the full text.

### 1.5 Spacing and density
- Base grid 4px.
- Panel padding 12px (dense) or 16px (default); dialogs 24px.
- Vertical rhythm in forms: label→control 6px; field→field 16px; group→group 24px.
- Rail rows: 30px tall, 8px horizontal padding, 6px icon-to-text gap.
- Pane gutters (between panes): 6px, showing `--tn-bg`.

### 1.6 Iconography
- lucide-react, 16px in controls, 14px in dense rows and pane headers, 20px in empty states. Stroke width 1.75.
- Engine marks (D-1895, D-2602; DESIGN §4.6): our own monogram (C=Claude Code, X=Codex, Cu=Cursor, G=Gemini, Co=Copilot, Gk=Grok, O=OpenCode, D=Droid, A=Amp, Ai=Aider, K=Kimi, Ag=Antigravity, Ki=Kiro) in Inter semibold, colour `fg`, on a 4px-radius tile in the engine's quiet tint: `--tn-engine-<id>` in `brand.css`, dark `oklch(0.34 0.05 h)`, light `oklch(0.93 0.035 h)`, with hues Claude Code 275, Codex 10, Gemini CLI 85, Copilot CLI 120, Cursor Agent 300, Grok 235, OpenCode 55, Droid 140, Amp 205, Aider 330, Kimi 25, Antigravity CLI 315, Kiro CLI 165. Shell, plugins and unknown ids sit on `surface-4`. 16px by default, 14px in pane headers and dense rows; 80% opacity at rest, 100% on the focused pane. `fg` passes 4.5:1 on every tile in both themes (`EngineGlyph.test.tsx`, 26 pairs). The status mark sits beside the tile, never on it, and the engine's name is always beside it. **Never a vendor logo, shape, colour or gradient**; no hue on text, borders or the pane frame.

### 1.7 Avatars (Teammates)
- Rounded square (`radius-md`), sizes 20 / 24 / 32 / 48 / 72px.
- Content: up to two initials from the teammate name (the first letters of the first two words, or the first two letters of one word), uppercase, semibold, white text at 42% of the size.
- Background: a linear gradient at 135°, `hsl(h 65% 48%)` → `hsl(h+40 70% 38%)`, where `h = fnv1a32(teammate.id) % 360`. Skip hues 0–15 and 345–360 (too close to the error red) by adding 30.
- Status badge: a 30%-size `StatusDot` overlaid at bottom-right with a 2px ring of the surface colour.

### 1.8 Motion
| Element | Enter | Exit |
|---|---|---|
| Tooltip | fade 120ms, delay 500ms (0ms if another tooltip was open within 800ms) | fade 60ms |
| Menu / popover / select | fade + scale 0.98→1, 180ms `ease-enter`, origin = anchor side | fade 120ms |
| Sheet (right) | slide from 24px + fade, 260ms `ease-enter` | 180ms `ease-exit` |
| Dialog | fade + scale 0.97→1, 260ms | 180ms |
| Toast | slide up 12px + fade, 260ms `ease-spring` | fade 180ms |
| Sidebar open/close | width transition 260ms `ease-standard`; content fades in after 120ms | |
| Mode switch | cross-fade of main area 180ms; the rail content swaps with no animation | |
| Pane split | new pane grows from 0 → target over 180ms; no animation during drag | |
| Status change | dot colour transitions over 180ms; "waiting" gets a single 600ms attention pulse (scale 1→1.35→1) | |
| WaveformRing | continuous; see §2.30 | |

`prefers-reduced-motion` (or Settings > Appearance > Reduce motion) removes all transforms and pulses. Opacity changes remain at ≤120ms, and WaveformRing becomes a static ring whose thickness reflects level.

### 1.9 Terminal palettes (xterm `ITheme`)
**Dark ("Tandem Night"):**
- background `#121214`, foreground `#e4e4e7`, cursor `#2dd4bf`, cursorAccent `#121214`, selection `rgba(45,212,191,0.28)`
- black `#26262b`, red `#f87171`, green `#4ade80`, yellow `#fbbf24`, blue `#60a5fa`, magenta `#c084fc`, cyan `#2dd4bf`, white `#d4d4d8`
- bright: black `#5c5c66`, red `#fca5a5`, green `#86efac`, yellow `#fde68a`, blue `#93c5fd`, magenta `#d8b4fe`, cyan `#5eead4`, white `#fafafa`

**Light ("Tandem Day"):**
- background `#ffffff`, foreground `#27272a`, cursor `#0d9488`, selection `rgba(13,148,136,0.20)`
- black `#27272a`, red `#dc2626`, green `#16a34a`, yellow `#b45309`, blue `#2563eb`, magenta `#9333ea`, cyan `#0d9488`, white `#71717a`
- bright: black `#52525b`, red `#ef4444`, green `#22c55e`, yellow `#d97706`, blue `#3b82f6`, magenta `#a855f7`, cyan `#14b8a6`, white `#18181b`

Setting "Terminal theme: Follow app / Night / Day / High contrast" (high contrast: pure `#000`/`#fff` with saturated ANSI).

---

## 2. Component library (`packages/ui`)

**Conventions:**
- All components take `className` and forward refs.
- Sizes are `"sm" | "md"`: 28px / 32px heights.
- Interactive components expose `disabled`, `loading` where relevant, and `aria-*` passthrough.
- Every component has a gallery entry covering all its variants × states × both themes.
- Visual state vocabulary: **default, hover, active (pressed), focus-visible, selected, disabled, loading, error**.

### 2.1 Button
| Prop | Type | Default |
|---|---|---|
| `variant` | `"primary" \| "secondary" \| "ghost" \| "danger" \| "link"` | `"secondary"` |
| `size` | `"sm" \| "md" \| "lg"` (28/32/40px) | `"md"` |
| `iconLeft`, `iconRight` | ReactNode | — |
| `loading` | boolean | false |
| `kbd` | string (e.g. `"⌘↩"`), rendered as a trailing `Kbd` | — |
| `fullWidth` | boolean | false |

**Visuals:**
- *primary:* bg accent, fg accent-fg; hover accent-hover; pressed accent-press.
- *secondary:* bg surface-4, border `border`, fg `fg`; hover border-strong.
- *ghost:* transparent; hover surface-4.
- *danger:* bg danger, fg danger-fg.
- *link:* accent text, underline on hover.

**Loading:** replaces `iconLeft` with a 14px spinner, keeps the label, sets `aria-busy`, and blocks clicks.
**Disabled:** 40% opacity, `cursor: not-allowed`, no hover change; the tooltip explains why if `disabledReason` is passed.

### 2.2 IconButton
Square 28/32px, `variant` ghost|secondary|primary. Takes `icon` and `label` (**required**; used for the `aria-label` and the tooltip). Optional `badge` (number or dot) at top-right, and `pressed` (toggle state → `aria-pressed`, bg accent-soft, icon accent).

### 2.3 Input / Textarea
- Height 32; bg surface-4; border `border`; radius sm; padding-x 10.
- States: hover border-strong; focus border accent + focus ring; error border danger + helper text in danger with an `aria-describedby` link.
- Props: `label`, `hint`, `error`, `prefix`/`suffix` (icon or text), `clearable`, `monospace`.
- Textarea: autosize between `minRows` and `maxRows`.
- `SecretInput`: masked, with a show/hide toggle; the value is never logged.

### 2.4 Select / Combobox
- Built on Radix Select (Combobox via cmdk).
- Trigger looks like an Input with a chevron. Options support `icon`, `description` (second line, fg-subtle), `disabled` + `disabledReason`, and grouped headings.
- Combobox adds type-to-filter and an "Add {query}…" creatable row when `onCreate` is given.

### 2.5 Checkbox, Radio, Switch
- Checkbox: 16px, radius xs, accent when checked, supports indeterminate.
- Switch: 28×16 track, accent when on, with a label to the left in settings rows.
- **All have visible labels; clicking the label toggles.**

### 2.6 Tabs
- `variant="underline"` (page-level: 2px accent underline on the active tab, 32px tall) or `variant="pill"` (inside panels: the active tab gets bg surface-4).
- Props: `items[{id,label,icon?,count?,disabled?}]`, `value`, `onChange`, `overflow` ("scroll" | "menu").
- Keyboard: arrows move, Home/End jump; manual activation for heavy tabs (`activation="manual"`).

### 2.7 SegmentedControl
Used for the **mode switcher** and small toggles.
- Container bg surface-1, border, radius sm, 2px inner padding.
- The selected segment is bg surface-3 with a shadow-1 and fg text; unselected segments are fg-muted.
- Props: `items[{id,label,icon?,kbd?}]`, `value`, `size`.
- The mode switcher variant (`modeSwitch`) shows a 1px accent underline under the selected label.

### 2.8 Tooltip
- Surface-3, text-sm, padding 6×8, radius sm, max-width 280.
- Shows the label plus an optional `Kbd` combo aligned right.
- Delay 500ms; never on touch; never on disabled elements (wrap them in a span).

### 2.9 Menu (dropdown) and ContextMenu
- Surface-3, border, radius md, shadow-2, min-width 200, padding 4.
- Items are 28px tall with radius sm.
- Props per item: `icon`, `label`, `kbd`, `danger`, `disabled`, `checked` (checkbox/radio items), `submenu`.
- Separators and group labels (2xs caps).
- The ContextMenu is the same visual, triggered by right-click or Shift+F10 / the Menu key.
- **Destructive items go last**, after a separator, in danger colour.

### 2.10 Dialog
- Centred, width `sm` 400 | `md` 520 | `lg` 720; surface-3; radius lg; shadow-3; scrim `overlay`.
- Header: title plus an optional description.
- Body scrolls. The footer is right-aligned with the primary action rightmost.
- Esc closes unless `dismissable=false` (used for crash restore and force update).
- Focus is trapped; it returns to the trigger on close.
- `ConfirmDialog` preset: `title`, `body`, `confirmLabel`, `danger`, `requireText` (typed confirmation, for deleting a workspace with worktrees).

### 2.11 Sheet
- Right-side panel over content, width 480 (`md`) or 640 (`lg`); full height below the title bar.
- Header with title, subtitle and a close IconButton; a sticky footer holds actions.
- Used for: New session, New teammate, Routine editor, Plugin connect, Task detail.
- Esc closes, but asks "Discard changes?" if the form is dirty.

### 2.12 Toast
- Bottom-right, stacked max 3 (older ones collapse into "+2 more").
- Width 360; surface-3; left 3px bar in the tone colour.
- `tone`: info | success | warning | error; `action` (one button); auto-dismiss after 5s, with hover pausing the timer. Errors persist until dismissed.
- `aria-live="polite"` (errors: `assertive`).

### 2.13 Badge / Chip
- **Badge:** 18px tall, radius xs, text-2xs medium, sentence case (T3; no capitals or caps tracking). Variants neutral | accent | working | waiting | done | idle | error | outline. Optional leading dot.
- **Chip:** 24px, radius full, removable (×), used for attachments, @mentions, filters and action results.

### 2.14 Avatar
- Props: `name`, `id` (seed), `size`, `status?`, `imageUrl?` (a user-uploaded photo overrides the gradient).
- Implements §1.7.
- `AvatarStack` shows up to 3 overlapped avatars, then "+n".

### 2.15 StatusDot
- 8px by default (6 / 8 / 10). `status`: working | waiting | done | idle | error | offline.
- Shapes per §1.3, as refreshed in §1.2a: working is a filled dot with a halo that breathes over 1.6s; waiting gets a single-shot pulse on transition.
- Always paired with an `aria-label`, e.g. "Waiting for you".

### 2.16 ProgressBar and UsageMeter
- **ProgressBar:** 4px track (surface-4), fill accent; indeterminate = a sliding 30% segment over 1.2s.
- **UsageMeter:** compact meter for the title bar and settings.
  - Props: `label` ("5h" | "Week" | "Context"), `value` 0–1, `resetAt?`, `detail?`, `size` ("xs" is 36×6px in the title bar; "sm" is full width).
  - Fill colour follows the §1.3 thresholds.
  - Tooltip: "{label}: {pct}% used · resets {relative time}".

### 2.17 EmptyState
- Centred column, max-width 420: a 20px icon in a 40px rounded tile (surface-4), a title (text-md semibold), a body (fg-muted), then primary and secondary actions, then an optional "Learn more" link.
- Used in every list/screen with no data. The copy for each lives with its screen in §4.

### 2.18 Skeleton and Spinner
- **Skeleton:** blocks in surface-4 with a shimmer (disabled under reduced motion).
- **Spinner:** 14/16/20px ring; accent in buttons, fg-muted elsewhere.
- **Rule:** show a skeleton after 150ms of loading and never flash one for faster loads.

### 2.19 Kbd
- Inline key caps: bg kbd-bg, border, radius xs, `font-mono text-2xs`, padding 1×4, min-width 16.
- `keys={["⌘","K"]}` renders each separately.
- **Platform aware:** `Kbd.Mod` renders ⌘ on mac and Ctrl elsewhere; Alt/⌥ likewise.

### 2.20 PaneChrome (header for every pane)
> **D-2603 (developer-first):** an agent that is working or waiting shows how long it has been at it ("4m", amber while it waits; tooltip "Working · 4m" or "Needs you · 4m"; it ticks only while its tab is on screen) before the badges, from its status record's `since`. The maximize, menu and close buttons sit at half opacity until the header is hovered or holds focus; their tooltips carry their shortcuts. Panes without a title are named for their engine or shell and their folder: "Claude Code · shop", "Shell · api", "Shell · shop 2" (the pane's cwd, else the workspace's name); with no folder known, "{Engine} · n" as before.
Height 30px, bg surface-2, bottom divider. Layout (left→right):
```
[drag grip ⋮⋮][StatusDot][EngineGlyph] Title (editable on dbl-click) · branch chip · account chip   ...   [ctx meter][⤢ maximize][⋯ menu][× close]
```
- **Props:** `title`, `status`, `engine`, `account?`, `branch?`, `context?` (0–1), `badges?` (e.g. "Auto", "Remote"), `onClose`, `onMaximize`, `menuItems`, `focused` (a 1px accent top border when focused), `dirty?`.
- **Drag grip:** the whole header is draggable except its buttons; the grip icon only appears on hover.
- **Close:** closing a pane with a running agent opens a confirm: "Stop {title}?" with body "The agent is still working. Its conversation is saved; the terminal process will end." and actions "Keep running" / "Stop and close".

### 2.21 SplitView / PaneGrid
- Built on react-resizable-panels with nested groups.
- **Gutter** is 6px; its hit area is 10px (with a -2px margin overlap). On hover, a 2px accent line appears centred in the gutter. Double-clicking a gutter equalizes its siblings.
- **Min pane size:** 240×140px. Below that, a new split is refused and a toast appears: "Not enough room to split. Try Arrange or close a pane."
- **Drop targets while dragging a pane:** a translucent accent-soft overlay splits the target pane into five zones (top, right, bottom, left, centre). Centre = swap; edges = split in that direction.
- **Presets** (layout snapping): 1, 2 columns, 2 rows, 2×2, 1+2 (main left), 3 columns, 3×2, 4×2. The implementation must keep the layout tree serializable (see ARCHITECTURE).

### 2.22 CommandPalette
- Built on cmdk. Opens with Mod+K.
- Width 640, top offset 18vh, surface-3, radius lg, shadow-3.
- An input with the placeholder "Type a command or search…".
- **Groups:** "Suggestions", "Go to", "Sessions", "Teammates", "Workspaces", "Actions", "Settings". Each row has an icon, label, optional description and right-aligned Kbd.
- **Prefixes:** `>` actions only, `@` teammates, `#` tasks, `/` workspaces, `?` help.
- Empty result: "No matches for "{q}"". Recent commands are persisted locally (last 10).

### 2.23 Composer (shared by Thread, Teammate chat, Task comment)
```
┌───────────────────────────────────────────────────────────────┐
│ [chip: screenshot.png ×] [chip: @src/auth.ts ×]                │
│ Ask Claude Code to…                                            │
│                                                                │
├───────────────────────────────────────────────────────────────┤
│ [Plan|Act] [Engine ▾] [Model ▾] [Effort ▾] [Access: Edits ▾]  [📎] [🎙] [Send ⌘↩] │
└───────────────────────────────────────────────────────────────┘
```
- **Props:** `placeholder`, `engine`, `models`, `value`, `onSend`, `onStop`, `running`, `attachments`, `mentionsProvider`, `allowedModes`, `accessLevels`, `disabledReason?`, `dictation` (boolean).
- **Textarea:** text-md, autosize from 88px to 320px, then it scrolls.
  - Enter sends, Shift+Enter inserts a newline. Setting "Send with Mod+Enter" swaps them.
  - Up arrow in an empty composer recalls the last message for editing.
- **Toolbar (32px, pill controls in size sm):**
  - *Plan | Act* SegmentedControl. Plan = the agent proposes without editing. Act = it may change files. Tooltip: "Plan: propose a plan, no file changes. Act: make changes."
  - *Engine* select (engine glyph plus name, with a disabled row for engines that aren't installed: "Not installed · Set up").
  - *Model* select (list from the engine adapter; shows "Default" first).
  - *Effort* select: "Low", "Medium", "High", "Max" (hidden when the engine doesn't support it).
  - *Access* select: "Read only", "Edits", "Full access". Full access shows a warning-tone badge and a one-time confirm: "Full access lets the agent run any command without asking. Use it only in folders you trust."
  - Attach (📎): files and images; also drag-drop and paste. Images become chips with thumbnails.
  - Mic (🎙): Dictate into the composer (see UI-60). Press-and-hold or click to toggle.
  - Send, a primary IconButton. While running it becomes **Stop** (a square icon, danger-ghost) with `Esc` as its shortcut.
- **Mentions:** typing `@` opens a popover of workspace files (fuzzy), teammates (in Teammate chats) and open sessions. Selecting one inserts a chip.
- **Slash commands:** `/` at the start opens the engine's command list, where the adapter supports it.
- **Disabled state:** the composer is dimmed with the `disabledReason` shown inline, e.g. "Claude Code isn't signed in. Sign in" (link).

### 2.24 ApprovalCard
Appears inline in a transcript and is mirrored in the Dashboard and Notifications.
```
┌─◆ Waiting for you ─────────────────────────────────────────── 2m ─┐
│ Claude Code wants to run a command                                  │
│ ┌───────────────────────────────────────────────────────────────┐   │
│ │ $ pnpm prisma migrate reset                                   │   │
│ └───────────────────────────────────────────────────────────────┘   │
│ In: ~/code/shop · Branch: tandem/fix-auth                            │
│ ⚠ This can delete data.                                             │
│ [Allow once] [Allow for this thread] [Deny]       [Deny and explain…]│
└─────────────────────────────────────────────────────────────────────┘
```
- **Props:** `kind` (`ApprovalKind`, ARCHITECTURE §6.9: command | file_write | file_delete | network | mcp | plugin_action | spend | question | other), `title`, `detail` (code block, diff preview or key/value list), `risk` (low | medium | high → none, warning or danger banner), `context` (workspace, branch, teammate, plugin account), `cost?` (credits or money), `actions` (the default set per kind; see below), `expiresAt?`, `resolved?` ({by, decision, at}).
- **Border:** 1px `status-waiting` while pending. After resolution it collapses to a single line: "✓ Allowed once · you · 10:42" or "✕ Denied · you · 10:42".
- **Default actions per kind:**

  | Kind | Actions |
  |---|---|
  | command | Allow once / Allow for this thread / Deny |
  | file_write / file_delete | View diff / Allow / Deny |
  | plugin_action (write, publish) | Approve / Deny, plus a checkbox "Allow {action} for {teammate} for the rest of this chat" (= Allow for this conversation) |
  | spend | Approve {amount} / Deny; never has an "always" option |
  | question | A free-text reply box plus suggested answers from the agent |
  | network / mcp / other | Allow once / Allow for this thread / Deny (the detail shows the host, or the MCP server, tool and arguments) |

- **"Deny and explain…"** opens a small textarea whose text is sent back to the agent.
- **Keyboard:** when the card is focused, `A` or `Mod+↩` = primary allow (Allow once), `Mod+Shift+↩` = Allow for this thread (commands), `D` = deny, `E` = Deny and explain…, `Esc` = focus Deny (§5).

### 2.25 DiffView
- Unified by default, split when the pane width is at least 900px (with a toggle).
- A line-number gutter, syntax highlighting (Shiki, lazy-loaded) and word-level intraline highlights.
- A header per file: path, +/- counts, collapse caret, and a "Open in editor" icon.
- Large files (>2k lines) render collapsed with "Show diff (2,431 lines)".
- Binary: "Binary file changed".

### 2.26 FileTree
- Virtualized (react-arborist or custom).
- Rows are 24px with indent guides every 12px. Folders show a chevron; files show a type icon.
- **Git status letter** right-aligned: M (warning), A (success), D (danger), U (info), R (info).
- Context menu: New file, New folder, Rename, Delete, Copy path, Copy relative path, Reveal in Finder/Explorer, Open in terminal here, Attach to composer.
- Drag a row out onto a terminal (inserts the quoted path) or onto a composer (attaches).
- Filter box at the top: "Filter files".

### 2.27 TerminalView (xterm wrapper)
- **Props:** `sessionId`, `theme`, `fontSize`, `onTitle`, `onLink`, `onFileLink(path,line)`, `webglPreferred`, `searchOpen`, `readOnly`.
- **WebGL:** handles the WebGL context pool from DECISIONS (it requests a lease from `useWebglPool()`; if none is free it uses the DOM renderer, and a "Lite" badge appears in PaneChrome with the tooltip "Rendering in lite mode to save GPU contexts").
- **Search bar** (Mod+F, top-right inside the pane): input, match counter "3 of 12", ↑ ↓, case/regex toggles, ×.
- **Links:** URLs underline on hover; Mod+click opens them. File paths (`path:line:col`) do the same and open in the Files sidebar at that line.
- **Drop:** dropped files insert quoted paths at the cursor (`'…'` on POSIX, `"…"` on Windows).
- **Overlays:**
  - Restoring: "Reconnecting…" with a spinner.
  - Exited: a bar at the bottom with "Process exited ({code}). [Restart] [Open shell here] [Close]".
- **Copy:** Mod+C copies when there's a selection; Ctrl+C with no selection is sent to the process.

### 2.28 EngineGlyph and EngineBadge
- **EngineGlyph:** a 14/16/20/24/32px tile with the engine's letters on its tint (see §1.6); props `letters`, `engine` (the id that picks the tint), `active` (full opacity), `selected` (a 2px accent ring), `label`.
- **EngineBadge:** the glyph plus the name, plus an optional account label ("Claude Code · Work").

### 2.29 AccountChip
- A 20px pill: the provider glyph plus the account nickname ("Personal", "Work", "Client A").
- Shown only when its engine has more than one account (D-2620): with one, the pane header and the Dashboard card name none, since there's nothing to tell apart and its name is usually "Default".
- Clicking opens a popover listing all accounts for that engine with usage meters and a "Switch to…" action, and a footer "Manage accounts…".

### 2.30 WaveformRing (Conductor and Dictate visual)
- **Geometry:** an SVG ring, diameter 28 (HUD compact), 56 (HUD expanded) or 120 (onboarding). 48 radial bars evenly spaced around the circle, 2px wide with rounded caps.
- **Bar length** = base (8% of the radius) + audio level × per-bar weight (from a smoothed FFT band, 60fps via requestAnimationFrame).
- **States:**

  | State | Look |
  |---|---|
  | idle | slate bars at base length, no motion |
  | listening | accent bars driven by mic level |
  | thinking | bars at base length with a single accent highlight rotating clockwise, 1.2s per revolution |
  | speaking | bars driven by output audio level, in sky |
  | error | rose static ring with a gap at the top |

- **Reduced motion:** no rotation; the ring's stroke thickness reflects level, updated at most every 150ms.
- **Props:** `state`, `level` (0–1, or an audio node), `size`, `label` (aria-live text: "Listening", "Thinking", "Speaking").

### 2.31 Other composites
| Component | Purpose / key props |
|---|---|
| `ListRow` | 30px row: leading slot (icon/avatar/dot), title, secondary text, trailing slot (badge/kbd/menu); selectable, draggable |
| `PageHeader` | Title, subtitle, actions, optional tabs row beneath |
| `SettingsRow` | Label + description on the left, control on the right, 56px min height, divider between |
| `Banner` | Full-width inline banner in info/warning/error/success with an action; used at the top of panes and pages |
| `Callout` | Boxed note inside forms |
| `KeyValue` | Two-column definition list for details panels |
| `CodeBlock` | Mono block with a copy button and optional language label |
| `MarkdownView` | Renders agent messages (GFM, code blocks via CodeBlock, tables, task lists); sanitizes HTML; links open externally after confirmation for non-https |
| `ToolCallBlock` | Collapsible block for an agent tool call: icon by tool type, one-line summary ("Edited src/auth.ts (+12 −3)", "Ran pnpm test (exit 0, 14s)"), and expandable detail (stdout, diff) |
| `Timeline` | Vertical list of run events (routines, tasks) with time and status |
| `CronInput` | Schedule builder (see UI-50) |
| `Stepper` | Onboarding steps indicator |
| `HotkeyRecorder` | Captures a key combo; shows conflicts |
| `DeviceSelect` | Microphone picker with a live level meter |
| `QRCode` | For phone pairing |
| `CreditMeter` | Credits remaining with a 30-day spend sparkline |


---

## 3. App shell

### 3.1 Window anatomy

```
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●  Tandem ▾ │  [ Agent | Code | Thread ]  │         ⌕ Search or run (⌘K)        │ 5h ▰▱ Wk ▰▰ │ ◈ 8,420 │ 🔔3 │ ▣ │ (AV) │  ← Title bar 40px
├──────────────┬───────────────────────────────────────────────────────────┬───────────────────┤
│ LEFT RAIL    │ MAIN AREA                                                 │ RIGHT SIDEBAR     │
│ 248px        │ (mode-specific)                                           │ 470px (toggle)    │
│ (collapsible │                                                           │ tabs: Dashboard · │
│  to 52px)    │                                                           │ Browser · Files · │
│              │                                                           │ Tasks             │
│              │                                                           │                   │
├──────────────┴───────────────────────────────────────────────────────────┴───────────────────┤
│ status bar 22px: ⎇ main ↑2 ↓0 · 3 agents working · 1 waiting · Dictate: Fn · Offline ✓ ·  v0.4.2 │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Minimum window:** 960×600. **Default** on first launch: 1440×900, centred (or 90% of the screen if smaller). Window state persists through tauri-plugin-window-state.

### 3.2 Title bar (UI-01) · SH, AC, BL, DB, CN
- **Height 40px.** macOS uses `titleBarStyle: Overlay` with traffic lights inset 12px; the left cluster starts at 80px. Windows and Linux use a custom title bar with min/max/close at the far right (Windows-style 46×40 buttons; Linux follows the same layout).
- The whole bar is a drag region (`data-tauri-drag-region`) except its controls. Double-click maximizes (or zooms on mac).
- **Left→right:**
  1. **App menu button:** "Tandem ▾" (Windows/Linux only; on mac the native menu bar is used). Items: Settings (Mod+,), Check for updates…, Keyboard shortcuts (Mod+/), Help & docs, Report a problem…, Sign out, Quit.
  1a. **Left rail toggle:** IconButton (Mod+\), pressed when the rail is expanded (F-SH-01 item 3).
  2. **Mode switcher:** a SegmentedControl with "Agent" (Mod+1), "Code" (Mod+2) and "Thread" (Mod+3). The selected mode persists across launches (Tandem has one main window, F-SH-01). Tooltip: "{Mode} (⌘1)".
  2a. **Workspace menu** (Code mode only; F-SH-01 item 5 and AC6): a Menu button with the workspace name. Items: "Auto-arrange panes" (Mod+Shift+T), "Card view", "Show terminal sessions" (toggle), "Usage…" (UI-37).
  3. **Omnibox:** centred, 360px wide (it shrinks to an icon under 1180px of window width). It shows "⌕ Search or run" plus a Kbd hint, and clicking it opens the CommandPalette.
  4. **Usage cluster** (the F-AC-04 plan-usage chip): for the active pane's engine account, or the default engine when nothing is focused. It shows the window closest to exhaustion as text ("Claude · 82% · resets 4:10 PM", coloured per the §1.3 meter thresholds) and, when the window is ≥1280px wide, two xs UsageMeters labelled "5h" and "Wk". Hidden if the engine doesn't report usage. Hovering shows a popover: "Claude Code · Work: 5-hour window 62% · resets in 1h 12m; Weekly 38% · resets Mon 09:00", with a "Manage accounts…" link.
  5. **Credits:** "◈ 8,420" (the credits balance). A tooltip explains what credits pay for ("Credits pay for cloud dictation, prompt Enhance, Conductor and Auto. Coding uses your own AI accounts."). Clicking opens Settings › Account (UI-114). Shown only signed in to a hosted build. Below 10% of the monthly grant it turns warning-coloured; at 0 it turns danger-coloured with "0".
  6. **Notifications:** a bell IconButton with a count badge (waiting items count separately in amber: "3" amber if any are waiting, else neutral). Opens the Notifications popover (UI-40).
  6a. **Update pill:** shown only when an update is staged ("Restart to update") or failed ("Update failed — Retry"); opens UI-130 (F-SH-01 item 9, F-SH-06).
  7. **Sidebar toggle:** IconButton "▣" (Mod+Shift+B), pressed when the sidebar is open.
  8. **Conductor button:** hidden unless Conductor is enabled. A 28px WaveformRing (idle state). Click toggles a call; press-and-hold is push-to-talk (UI-61).
  9. **Account avatar:** the user's initials. Menu: name/email, "Account", "Settings", "What's new", "Sign out". No plan badge (D-869).
- **Offline:** a thin 2px warning line appears along the bottom of the title bar, and the status bar says "Offline · local features still work".

### 3.3 Left rail · SH
> **D-2603:** a workspace row's status is quiet: amber "Needs you", or a working dot with "n working", or "Failed" in rose, never a boxed chip. The name keeps at least 10 characters; the branch gives way to a status, and live panes are a `Count` pill. Teammate rows (UI-70) are two lines: name with Failed > Needs you > n working > age, then the latest activity.

> **D-2611:** a worktree's row (F-CD-15) is named as its workspace is, `coupon-rules` rather than `tandem/coupon-rules`: a worktree's branch is always `tandem/` and that name, so the prefix said nothing and took the room. The branch is in the row's tooltip, before the path, and in the pane header's chip and the status bar. Beside a status the name keeps at least 10 characters, as a workspace row's does, and ⋯ takes no room: it covers the row's end while the row is hovered or holds focus. It covers whole parts, never the end of a mark (D-2700, D-2623): the row ends with its agents' count, which ⋯ takes the place of, or with a blank as wide as a one-digit count when there's none. A click on what ⋯ covers is the row's.

> **D-2701:** a row never runs past the rail. While a worktree row's contents are wider than the row, it folds them, in order: the Preview button's word, the radar mark's symbol, the checks badge's words, then its status words to their dot (the `StatusDot` of the state, with the words as its accessible name and tooltip; the unread dot sits on it, as on a pane row's dot). A workspace row folds its status words the same way. The name keeps its 10 characters, so the rail's minimum is a fully folded worktree row at the deepest indent: the name's 10 characters, the status dot, the checks and radar icons and a two-digit count.
- **Width:** default 248px, drag-resizable between a fully folded worktree row's width and 360px (F-SH-03); the width persists per mode. The minimum is that row at the deepest indent, its text parts measured in the app's own engine: 246px in Chromium, and more in WebKitGTK, which draws a name's 10 characters 80px wide where Chromium draws 75.7 (D-2701, D-2626). While a row the rail shows has a jj badge or a running Preview, the minimum adds the widest such row's; a rail narrower than its minimum widens to it over --dur-base with --ease-standard, never mid-drag, and keeps that width when they go (D-2703). Where scrollbars take room (Windows), the minimum adds a scrollbar's width.
- **Collapse** with Mod+\ (§5), or by dragging below 160px. Collapsed it is a 52px icon rail showing avatars/glyphs with tooltips.
- **Background** surface-1, with a 1px right border.
- **Structure (all modes):** a header row (section title plus a "+" action), a scrollable list, and a footer row (mode-specific quick actions).
- **Per-mode content:**

  | Mode | Header | List | Footer |
  |---|---|---|---|
  | Agent | "TEAMMATES" + "+" (New teammate) | Pinned "Lead" teammate first, then teammates A–Z (Avatar, name, status dot, unread count). Under each expanded teammate: its chats (up to 5, "Show all…") | "Tasks", "Routines", "Skills library", "Plugins" nav rows |
  | Code | "WORKSPACES" + "+" (Add folder…) | Each workspace: folder icon, name, branch chip, a count of running sessions. Expanded, it lists its open panes (StatusDot + EngineGlyph + title) and "Saved sessions (n)" | "Launch…" (Mod+T), "Presets", "Remote hosts" |
  | Thread | "THREADS" + "+" (New thread) | Grouped by folder ("No folder" group first), threads sorted by last activity; each row shows the StatusDot, title, relative time and engine glyph | "Search threads" |

- **Row interactions:**
  - Single click selects or opens.
  - Double click renames (inline input).
  - Right click opens the ContextMenu.
  - Dragging rows reorders workspaces and teammates; the order is persisted.
  - Rows show a hover menu (⋯) at the right.
- **Keyboard:** Up/Down move, Right expands, Left collapses, Enter opens, F2 renames, Delete opens a confirm.

### 3.4 Main area · SH
- Background `bg`. Content is padded 6px from the rail, sidebar and status bar, so panes float as cards (radius md, border).
- Mode content is kept mounted: switching modes hides the others with `display:none` so terminals don't re-mount. xterm instances detach their WebGL context when hidden, to free pool slots.

### 3.5 Right sidebar · SH, DB, CD
- **Width:** default 470px, resizable between 320 and 900px, or full-window (F-SH-10). Toggle with Mod+Shift+B. It stays open across mode switches and remembers its tab and width.
- **Tab strip (32px, underline tabs):**
  - "Dashboard" (with a count of waiting items)
  - "Browser"
  - "Files"
  - "Tasks"
  - a "+" menu with "New browser tab", "Open file…", "Scratchpad"
- **Responsive:** below 1280px of window width, the sidebar overlays the main area (floating with shadow-3) instead of pushing it, and an outside click closes it unless it's pinned (a pin IconButton in the strip).

### 3.6 Status bar (22px) · SH, TL
- Surface-1, text-2xs, fg-muted.
- **D-2603 (DESIGN §5.4.6):** quiet by default: the branch (with ahead/behind and its pull request's CI state: "CI passed", "CI running", "CI cancelled", "CI failed (2)", opening the checks; both follow the Files tab's context) and the version. Then, each only while non-zero: agents "1 waiting" (amber, first) and "2 working", each opening the Dashboard filtered to it; the listening ports (":5173" or "3 ports", opening the Ports tab; read every 15 s while the window has focus); and the fullest usage window of the title bar chip's account ("5h 42%", "Wk 42%", warning or danger past the meter thresholds). Dictate and the connection keep their places.
- **Left segment:** the git branch of the focused workspace, with ahead/behind counts. Clicking opens the Files tab's Changes view.
- **Centre:** "3 working · 1 waiting · 5 idle" (each clickable; it filters the Dashboard).
- **Right segment:**
  - "Dictate: Fn": the current hotkey; clicking opens Dictate settings.
  - The connection state: "Online" / "Offline".
  - "Update ready" (accent, when applicable).
  - The version.

### 3.7 Responsive rules summary
| Window width | Behaviour |
|---|---|
| ≥ 1440 | Rail, main and sidebar all docked |
| 1280–1439 | Sidebar docked; rail may auto-collapse if main would drop below 720px |
| 1024–1279 | Sidebar overlays; rail docked |
| 960–1023 | Rail auto-collapses to 52px; sidebar overlays; omnibox becomes an icon |

---

## 4. Screens

The format for each screen is: ID · title · feature codes, then the wireframe, labels, states, interactions and acceptance checks.

### 4.1 Launch, sign-in and onboarding

#### UI-02 · Launch / restore splash · SH, TL
```
┌──────────────────────────────────────────┐
│                                          │
│              ◯◯  Tandem                  │
│                                          │
│        Restoring your workspace…         │
│        ▰▰▰▰▰▰▱▱▱▱   6 of 9 sessions       │
│                                          │
│        [Skip restore]                    │
└──────────────────────────────────────────┘
```
- Shown only when restore takes longer than 400ms. It is centred in the main window over `bg`.
- **Labels:** "Restoring your workspace…", "{n} of {m} sessions", "Skip restore".
- **Crash variant**, when the previous run ended unexpectedly: the title is "Tandem closed unexpectedly". The body is "We saved {m} sessions from your last run. Restore them?". Actions: "Restore all" (primary), "Choose…" (opens a checklist of sessions with workspace, engine and title), "Start fresh". A checkbox reads "Send a crash report" (checked only if telemetry is on; the report never includes prompts or file contents).
- **Error per session:** failed restores are listed at the end: "Couldn't restore 2 sessions" plus a "Details" disclosure (the reason per session, e.g. "Folder not found: ~/code/old").
- **Acceptance:** killing the app mid-session and relaunching shows the crash variant. "Restore all" returns panes to their prior layout.

#### UI-03 · Sign-in · BE, BL
```
┌──────────────────────────────── 520 ────────────────────────────────┐
│  Welcome to Tandem                                                   │
│  Run your coding agents side by side.                                │
│                                                                      │
│  [ Continue with Google ]                                            │
│  [ Continue with email  ]                                            │
│                                                                      │
│  ───────────────  or  ───────────────                                │
│  [ Use Tandem offline ]  (limited: 4 panes, local Dictate)           │
│                                                                      │
│  By continuing you agree to the Terms and Privacy Policy.            │
└──────────────────────────────────────────────────────────────────────┘
```
- **Flow:** "Continue with…" opens the system browser (PKCE). The app shows "Finish signing in in your browser…" with a "Cancel" link and "Didn't open? Copy link". A deep link returns to the app, which shows "Signed in as {email}" and proceeds.
- **Email path:** the browser shows the magic-link or password flow; the app shows the same waiting state.
- **Offline path:** skips the account; an "Offline mode" chip appears in the account menu with "Sign in" as its action.
- **Errors:**
  - "Sign-in timed out. Try again." (after 5 minutes)
  - "That link has expired."
  - "Couldn't reach Tandem. Check your connection or continue offline."
- **Acceptance:** the PKCE loopback and deep link both work. Cancel returns to this screen. The offline path lands in onboarding with everything local unlimited (D-2900).

#### UI-04 · Onboarding wizard · EN, AC, DI, SH
A 4-step Stepper, full-window, content width 720.

1. **"Pick your theme"**: Dark and Light preview cards (selecting one applies it live), plus "Follow system". Button: "Continue".
2. **"Set up your coding agents"** (the Engine setup component, UI-05, embedded).
3. **"Try Dictate"** (optional):
   - Shows a 120px WaveformRing and the text: "Hold **Fn** (or your chosen key) and say something. Release to insert."
   - A test field: "Your words appear here".
   - A microphone select.
   - "Download local model (Parakeet · {size from the model manifest, 670 MB})" with a progress bar, and "Use cloud instead".
   - "Skip for now".
4. **"Open your first workspace"**:
   - A folder drop zone: "Drop a project folder here, or [Choose folder…]".
   - Recent git repos (scanned from `~/code`, `~/projects`, `~/dev` and `~/src`, up to 8).
   - "Finish" opens Code mode with that workspace and the Launch sheet.

- Each step has a "Back" and "Continue/Skip" footer, and completion persists (the wizard never shows twice). The wizard can be reopened from Settings > General > "Run setup again".
- **Acceptance:** keyboard-only completion is possible. Skipping every step still lands the user in an empty Code mode.

#### UI-05 · Engine setup · EN, AC
```
Set up your coding agents
Tandem runs the agents you already use. Nothing is bundled, and your AI accounts stay yours.

┌──────────────────────────────────────────────────────────────────────────────┐
│ (C) Claude Code     ✓ Installed 2.4.1 · Signed in: Work          [Accounts…] │
│ (X) Codex           ✓ Installed 0.52.0 · Not signed in           [Sign in]   │
│ (Cu) Cursor Agent   Not installed                                [Install]   │
│ (G) Gemini CLI      Not installed                                [Install]   │
│ (Gk) Grok           ✓ Installed · Signed in                      [Accounts…] │
│ (O) OpenCode        Not installed                                [Install]   │
│ ▸ More agents (Copilot, Droid, Amp, Aider, Kimi)                              │
└──────────────────────────────────────────────────────────────────────────────┘
[Rescan]                                              You can change this later in Settings › Engines.
```
- **Row states:** Checking… (spinner) · Installed {version} · Not installed · Update available {v} · Install failed (error) · Installed but not found in your shell PATH (warning, with "Fix PATH…").
- **Install:**
  - Opens a Dialog "Install Cursor Agent" showing the exact command from the vendor's official installer (read only, in a CodeBlock).
  - Text: "This runs the vendor's installer in a terminal. Review it first."
  - Buttons: "Run in terminal" / "Copy command" / "Cancel".
  - Running it opens a temporary terminal pane in the dialog with live output. On success the row updates; on failure it shows "Install failed (exit {code})" plus "View output".
- **Sign in:** launches the engine's own login flow in a terminal pane inside a Dialog, under that account's provider home. Text: "Sign in with {Engine}'s own login. Tandem never sees your token." It detects completion by polling the adapter's auth-status check.
- **Accounts…** opens UI-35.
- **Acceptance:** with none installed, all rows show Install. After a manual install and Rescan, the row flips to Installed.

### 4.2 Code mode

#### UI-10 · Code mode: empty workspace · CD, EN
> **D-2603:** "Start an agent in {name}" over "It runs in a real terminal in this folder." (DESIGN §9.4). Engine tiles are e1 surfaces with the engine's 20px tile (§1.6), "Default · v{version}" on the default engine and "v{version}" on the rest, and a key cap 1–9; the number keys launch while focus is in the grid. Engines not installed list this OS's install command in mono with a copy button beside Install. Under the grid, "Launch several…" is a secondary button and "New worktree…" (repositories on this machine) a ghost button beside it. A tab still called "Tab n" reads as its first agent's task (five words) or its workspace ("shop", "shop 2"); a name the user gave is kept.
```
┌───────────────────────────────── main ─────────────────────────────────┐
│                          ⌂ shop (~/code/shop)  ⎇ main                    │
│                                                                          │
│   Start something                                                        │
│   ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐            │
│   │ (C)        │ │ (X)        │ │ (Gk)       │ │ >_          │            │
│   │ Claude Code│ │ Codex      │ │ Grok       │ │ Shell       │            │
│   └────────────┘ └────────────┘ └────────────┘ └────────────┘            │
│   [Launch several…  ⇧⌘L]  [Open a preset ▾]                              │
│                                                                          │
│   Saved sessions (3)                                                     │
│   ◯ Fix checkout redirect · Claude Code · 2h ago        [Resume]         │
│   ◯ Write e2e for billing · Codex · yesterday           [Resume]         │
└──────────────────────────────────────────────────────────────────────────┘
```
- **Tiles:** installed engines only, plus "Shell" (always present). Not-installed engines appear in "More…" as disabled with "Set up". Clicking a tile opens one pane in the workspace root.
- **Saved sessions:** conversations from engines that support resume (Claude Code, Codex). Resume relaunches with a resume flag in a new pane.
- **No workspace selected:** EmptyState with the title "Add a project folder", body "Tandem works on folders on your disk. Agents' changes land right there.", and actions "Add folder…" / "Clone repository…".

#### UI-11 · Code mode: pane grid · CD, DB, AU
```
┌─◇(C) Fix auth guard · ⎇tandem/fix-auth · Work  ▰▰▱ 41%  ⤢ ⋯ ×┐┌─● (X) e2e billing · ⎇tandem/e2e · Personal ⤢ ⋯ ×┐
│ (live xterm)                                           ││ (live xterm)                                   │
│                                                        ││                                                │
│                                                        ││                                                │
├─ command bar ──────────────────────────────────────────┤└────────────────────────────────────────────────┘
│ ▸ Send to Fix auth guard…                    🎙  ⌘↩   │┌─○ >_ shell · ⎇main ─────────────────── ⤢ ⋯ ×┐
└────────────────────────────────────────────────────────┘│ $ pnpm dev                                     │
                                                          └────────────────────────────────────────────────┘
```
- **Grid** uses PaneGrid (§2.21). Each pane has PaneChrome (§2.20) plus a TerminalView.
- **Pane title defaults:** "{Engine} · {n}". The first user prompt (truncated to 40 chars) auto-renames the pane unless the user renamed it manually.
- **Command bar** (optional per pane; toggle in the ⋯ menu, default on for agent panes): a single-line composer below the terminal that sends text plus Enter into the PTY. It supports Dictate (🎙), history (↑/↓), and multi-line entry via Shift+Enter (sent as bracketed paste).
- **Pane ⋯ menu:** Rename · Duplicate · Split right (Mod+D) · Split down (Mod+Shift+D) · Move to new tab · Hand off to… ▸ (engine/account list) · Auto ▸ (Off / Finish / Stress-test / Review / Explore / Custom…) · Show command bar ✓ · Open in Thread · Copy session ID · Reveal worktree · Restart · ─ · Stop agent (danger) · Close pane (danger).
- **Worktree badge:** if the pane runs in a worktree, the header shows the branch chip "⎇ tandem/{slug}". Hovering shows the path. When the header is short of room the chip gives way first, down to its icon, and the title keeps its length; the title is never cut to fewer than 10 characters, as a worktree row's name isn't (D-2611). The ⋯ menu adds "Merge into {base}…", "Open diff", and "Discard worktree…" (danger; requires typing the branch name).
- **Auto badge:** when Auto is on, the header shows an "Auto: Stress-test · 3/15" Badge (turn / F-AU-01 ceiling) in accent-outline. Clicking opens a popover with the mode, turn count, credits used and "Stop Auto".
- **Arrange:** a toolbar button at the top-right of the main area ("Arrange") rebalances into the nearest preset for the number of panes. Mod+Shift+T (§5).
- **Maximize** (⤢ or Mod+Shift+Enter on a focused pane, §5): the pane fills the main area; the others stay alive. Esc or ⤢ restores.
- **Focus:** clicking a pane focuses it (accent top border). Mod+[ and Mod+] cycle focus (Ctrl+Shift+[ / ] on Windows/Linux, §5); Mod+Alt+Arrow moves focus spatially.
- **States:**
  - Pane restoring: overlay "Reconnecting…".
  - Engine missing: a Banner in the pane, "Claude Code isn't installed on this machine. [Set up]".
  - Engine auth failed: Banner "Claude Code needs you to sign in. [Sign in]".
  - Usage limit hit: see UI-36.
  - Process exited: a bottom bar per §2.27.
- **Acceptance:**
  - 16 panes stay usable, with at most 8 WebGL contexts and the rest in lite mode.
  - Drag-to-split works in all four directions.
  - Layout persists across relaunch.

#### UI-12 · Launch sheet ("Launch several…") · CD, EN, AC
A Sheet, md 480. Title "Launch agents"; subtitle: the workspace name.
```
Preset:   [ Solo | Pair | Bench | Fleet | Custom ]

Lineup
 1  (C) Claude Code ▾   Account: Work ▾     Role: Builder ▾    ☑ Own worktree
 2  (X) Codex ▾         Account: Personal ▾ Role: Reviewer ▾   ☑ Own worktree
 [+ Add seat ▾]  (Agent · Shell command · Browser URL)   How many [ 1 ▴▾ ]

Starting prompt (optional)
┌──────────────────────────────────────────────────────────────┐
│ Describe the job. Each agent gets this plus its role brief.   │
└──────────────────────────────────────────────────────────────┘
☐ Send the same prompt to every agent   ☑ Arrange panes automatically

This opens 2 panes and 2 worktrees (tandem/fix-auth-1, tandem/fix-auth-2).
                                               [Cancel]  [Launch 2 agents  ⌘↩]
```
- **Presets (Tandem's own):**

  | Preset | Seats (F-CD-14) |
  |---|---|
  | Solo | 1 agent (Builder) |
  | Pair | agent A builds + agent B reviews, same tree |
  | Bench | 1 agent + 1 shell (tests/git) |
  | Fleet | 4 agents on parallel tasks |
  | Custom | Starts from the current lineup |

  Users can save the current lineup as a preset ("Save as preset…").
- **Roles** are editable text briefs (Role ▾ → "Edit roles…" in this sheet; stored with the `presets` data, F-CD-14). Defaults:
  - Planner: "Break the job into steps and write them to TASKS.md. Don't edit code."
  - Builder: "Implement the assigned step. Keep changes small and run tests."
  - Reviewer: "Review the diff on your branch against the goal. List problems; fix only obvious ones."
  - Tester: "Write and run tests for the change. Report failures with repro steps."
  - Lead: "Coordinate the others through the task board; don't write code yourself."
- **Worktree toggle:** default on when there's more than one agent. The branch naming pattern is `tandem/{slug}-{n}`, where the slug comes from the prompt's first words (or "session").
- **Validation:**
  - Engines that aren't installed can't be selected (the row shows "Set up").
  - If the workspace isn't a git repo, the worktree checkbox is disabled with the tooltip "Worktrees need a git repository".
- **Summary line** updates live. Launch opens the panes, arranges them, and sends the prompt (with the role brief prepended) to each.
- **Acceptance:**
  - A Pair launch creates two worktrees and two panes, each on its own branch.
  - Cancel creates nothing.

#### UI-13 · Hand off dialog · CD, TH, AC
Dialog md. Title "Hand off to another agent".
- Body: "Tandem will ask {current engine} to write a handoff summary, then start {target} with it."
- Fields:
  - "Target": an engine + account select.
  - "Where": "Same folder" / "Same worktree" / "New worktree".
  - "Summary": an editable textarea, generated on open with a skeleton and "Writing summary…" while loading. The user can edit it before sending.
  - A checkbox: "Close the original pane after handoff".
- Actions: "Cancel" / "Start {target}".
- **Error:** "Couldn't generate a summary. You can write one yourself." (the textarea becomes empty and enabled).

#### UI-14 · Remote hosts (SSH workspaces) · RM, CD
Opened from the Code rail footer "Remote hosts". It's a page in the main area with the header "Remote hosts" and a "+ Add host" button.
- **List rows:** name, `user@host:port`, state (Connected / Disconnected / Installing helper… / Error), and the number of workspaces. Actions: Connect / Disconnect / Edit / Remove.
- **Add host Sheet:**
  - Fields: "Name", "Host", "User", "Port (22)", "Authentication": "Use my SSH agent / config" (default) or "Key file…".
  - Button "Test connection" shows "Connected. {os} {arch}. Helper not installed." then "Install helper" (copies `tandem-host` and shows progress).
- **Remote workspace in the rail:** a cloud glyph plus the host label; its panes show a "Remote · {host}" badge.
- **Errors:** "Host key changed for {host}. Tandem won't connect until you verify it." with "Show fingerprint" / "Trust new key…" (danger confirm).



#### Worktree workflow (UI-150–153, F-WT-01…04)

- **UI-150 Worktree setup** (dialog; "Worktree setup…" in a repository's rail menu and every worktree's ⋯ menu). Description: "What every new worktree of {folder} gets. Saved in .vibezeo/worktree.json in the repository; commit it to share it with your team." Fields: "Copy into each new worktree" (monospace `Textarea`, one gitignore pattern per line), "Setup command" and "Teardown command" (`Input`, with the hints in the catalog), then the note "Saving trusts these commands on this computer. …". Footer: Cancel / **Save**.
  - Loading: three skeletons. Error: an error `Banner` "Couldn't read the worktree setup: {reason}" with Retry. Empty (no file): "No setup yet. New worktrees start with only the files git tracks." above empty fields. A malformed file: a warning `Banner` "The current file can't be used ({reason}). Saving replaces it." with empty fields. Populated: the file's values.
- **UI-151 Review worktree setup** (dialog, opened after a worktree is made when the file's commands aren't trusted here). Body: "This repository's .vibezeo/worktree.json wants to run these commands in {the new worktree | # new worktrees}. Run them only if you trust this repository. Nothing has run yet." Then "Setup" and "Teardown, when a worktree is discarded", each in a `CodeBlock`, verbatim. Footer: **Not now** (secondary) / **Trust and run** (primary). Several worktrees made together with the same commands share one review. A failure shows an error `Banner` in the dialog.
- Toasts: "Copied {n} files into the worktree" (success; "Stopped at the copy limit (2,000 files or 512 MB)." as the body when partial), "Couldn't copy files into the worktree: {reason}", "The worktree setup can't be used: {reason}", "Couldn't start the setup: {reason}". Discarding adds the teardown's line as the toast body ("Teardown finished", "Teardown exited with code {code}", "Teardown was stopped after 2 minutes", "Teardown skipped: its command isn't trusted on this computer"), with the warning tone when it didn't run cleanly.
- UI-152 (review comments on a worktree's diff, F-WT-02) and UI-153 (overlap check, F-WT-03) are reserved for their features.

- **UI-153 Overlap** (dialog; "Check overlap…" in a worktree's ⋯ menu; F-WT-03, F-WT-10). Title "Overlap: {branch}". Body: "{n} files changed since this worktree started", then **Other worktrees**: each sibling with shared files or symbols, "{branch} also changed {n} files" and the files (monospace), then "Shared symbols" with each symbol (monospace) and what each side did: "changed here and there", "renamed or removed here, used there", "used here, renamed or removed there". "No overlap with other worktrees" when none. Then **Merging into {base}**: the forecast verdict. A line under the symbols says what wasn't parsed: "{n} files weren't checked for symbols (other languages or too large)". Loading: "Checking for conflicts…". Error: an error `Banner` with Retry.

- **Code intelligence** (the Files editor and worktree diffs; F-CD-23). In the editor: F12 or Mod+click goes to the definition; Shift+F12 opens **References** (a panel under the editor: "{n} references to {name}", one row per reference with its path, line and text in monospace; Enter or click opens it; Escape closes; "No references found" when empty; a `Spinner` while asked); hovering a name shows a tooltip with the server's text (monospace, at most 12 lines, scrolls); diagnostics are wavy underlines in the error or warning tone with the message on hover. The editor's status line (right of the tabs) shows "{server}", "Starting the language server…" (with a `Spinner`, before the first answer), "2 errors, 1 warning", "{server} is off until you trust this repository" (with a ghost **Trust repository…**, which asks with the native confirm "Trust {repo} for language servers?"), or for a worktree of sandboxed agents "{server} is off until you trust this worktree" (with **Trust worktree…**; the confirm says "The agent's code will run outside its sandbox."), a file outside the folder "Outside this folder: read only, without code intelligence", "No language server for {language}" (its tooltip "Install {server} to get definitions and references"), or "{server} stopped: {reason}". In a diff (the worktree diff and the review queue's), clicking a name on a new-side line opens a small menu: **Go to definition** / **Find references**; the diff closes and the Files editor shows the result (references in the panel under the clicked line's file). Go to definition with no answer says "No definition found for {name}" in the status line.

- **Multi-repo rows** (F-CD-22). A multi-repo folder's rail row shows "{n} repositories" where a repository's shows its branch; its worktrees list the same. Disabled menu entries on a multi-repo worktree say "Not available for multi-repo worktrees yet". A folder with more than 12 repositories isn't multi-repo; its row's tooltip says "This folder holds {n} repositories, more than 12, so its worktrees aren't made across them. Add the repositories you work in on their own."

- **jj workspaces** (the New worktree dialog and the rail; F-CD-24). In a repository co-located with jj, the New worktree dialog adds a `SegmentedControl` "Make" with **jj workspace** (preselected, described "jj {version}") and **git worktree**; the choice is remembered for the repository. A repository with `.jj` and no `.git` shows an info `Banner` "This repository uses jj without git. Vibezeo works with jj in repositories co-located with git." and makes no jj workspace; an SSH or WSL folder shows nothing about jj. A jj workspace's rail row carries an outline `Badge` "jj" beside its name. What isn't available for jj workspaces yet is disabled with "Not available for jj workspaces yet", as F-CD-22's entries are.

- **Multi-repo diff** (the worktree diff dialog; F-CD-22). A line per repository above the files, "{repo} · {n} files" (or "{repo} · No changes"); the files follow grouped by repository, each path starting with its name, all in one review bar. Second opinion isn't offered.

- **UI-158 Create pull requests** (dialog; "Create pull requests" in a multi-repo worktree's ⋯ menu, after the native confirm; F-CD-22). Title "Pull requests for {branch}". One row per repository: its name and step ("Pushing…", "Opening the pull request…", "Linking…", "Opened" with the link, "Skipped: no new commits", "Skipped: GitLab isn't supported yet", "Failed: {reason}"). "Open" (ghost) opens a pull request in the system browser. Footer: Close. Loading: the rows in their waiting state with a `Spinner`. Error: an error `Banner` "Couldn't read the pull requests' progress: {reason}"; spinners stop. No run (the app restarted): "This run isn't running any more. Create pull requests again to finish it; open pull requests are kept."

- **Radar mark** (a worktree's rail row; F-WT-10). A `Badge` (`waiting`, amber) while the worktree shares symbols with a sibling; it opens UI-153. It leads with the radar icon, then the symbol in code type (`applyCoupon`), or "{n} symbols" for several (D-2700); a long symbol is cut at its end, the count never. Its accessible name, and its title, begin with its words (D-2701, WCAG 2.5.3): "{symbol}, shared with another worktree", or "{n} symbols shared with another worktree: {symbols}" with every symbol. Beside the row's status ("● 1 working", "Needs you", "Failed") it's the icon alone, so the row's name keeps its room (D-2613, as D-2603 hides the branch). A rail row's marks show their words whole or not at all: while the row's contents are wider than the row, its marks fold to their icons, the Preview button first, then the radar mark, then the checks badge, and last the row's status words fold to their dot (D-2700, D-2701).

- **Radar notice** (a toast; F-WT-10). Title "{branchA} and {branchB} both touch {symbols}". Body: "Their rail rows show it; Check overlap… has the details." Action: **Warn both agents** (types the note into each worktree's agent, then "Warned both agents"; when a worktree has no agent, "No agent is running in {branch}"). Warning tone.
- **UI-160 Instructions** (dialog, `lg`; "Instructions…" in a local folder's rail menu; F-EN-07). Title "Instructions"; description "Which instructions file each engine reads in {folder}, and how they differ from AGENTS.md."
  - **Engines:** one row per installed engine: its label and what it reads ("Reads AGENTS.md", "Reads AGENTS.md and {files}", "Reads {files}, not AGENTS.md", "Nothing yet", "Unknown").
  - **Files:** one row per file that exists: the path in mono, a `Badge` for git ("Committed", "Not committed", "Ignored by git"), "Points to AGENTS.md", or why it's never read or written. An ignored file that new worktrees don't get says "Not in new worktrees" with **Copy into worktrees**. A file that differs from AGENTS.md has **Show the difference**, which opens a `DiffView`.
  - With no AGENTS.md, a `Select` "Start AGENTS.md from" lists the files that can become it.
  - Footer: **Close** / **Use one file for all** (primary). The plan view lists each file it would create ("New") or replace ("Replaced"), the new text in a `CodeBlock`, the old text under "What it holds now", "Its text isn't in AGENTS.md and will be replaced. A copy is kept for Undo." when that's so, and the files left as they are and why. Footer: **Back** / **Write # files** (primary). A success toast has **Undo**.
  - States: loading (`Skeleton`s), error (`Banner` with Try again), empty ("No instructions files yet"), populated, and the plan.
- **UI-161 Engine versions** (in Settings › Engines; F-EN-08). Each installed engine's row reads "{version} · {npm | Homebrew | its own installer | installed another way}", with an outline `Badge` "Updates itself" when it does and isn't pinned. Its details add a `Switch` "Pin this version" (or "This engine has no documented way to stop it updating itself."), **Check for updates** (secondary, loading while npm answers), then "{version} is the newest", "{version} is available" or "Update it with" and the command in mono; **Roll back to {version}…** (ghost) when an earlier version is known. Errors show inline under the buttons.
  - A version that threads have failed on since an update shows a warning `Banner` at the top of the page and in the thread: "{engine} updated from {from} to {to}, and threads have failed since." with **Roll back to {from}…**, which opens the Install sheet titled "Roll back {engine} to {version}" with the command shown before anything runs.
  - **Running in the background:** one row per process (engine, its command in mono, **Stop**), "No engine is running outside a terminal." when empty, a `Spinner` while loading, and the reason when the platform can't list them. Stop opens a danger `ConfirmDialog` "Stop this {engine} process?" / "Process {pid} has no terminal and runs outside Vibezeo. Stopping asks it to end."
- **UI-162 Session library** (dialog, `lg`; "Session library…" in the command palette; F-TH-17). Title "Session library"; description "Claude Code and Codex sessions on this computer, including ones started outside Vibezeo. Vibezeo only reads them."
  - An `Input` "Filter by title or folder" narrows the list as you type. With search on, **Search session text** (secondary) runs the full-text search, and Enter does the same. With search off, the line reads "Turn on Search session text in Settings › Privacy to search what sessions say." It needs no account (D-2900); limits that turn it off show "Searching what sessions say is turned off for this account."
  - Groups per repository (its folder name, the path as its tooltip; "No folder" last). Each row has the engine's glyph, the title (or "Untitled session"), and "{engine} · {account | Outside Vibezeo} · {age} · {size}". Its buttons are **Open** (ghost), **Resume** (secondary) and **Continue with another engine…** (ghost). A gone folder disables Resume and Continue, with "This session's folder isn't on this computer any more" as their tooltip; an engine that isn't installed disables Resume.
  - "# files couldn't be read" under the filter when any were skipped.
  - **Open** replaces the list with the transcript (`role="log"`): prompts on a `surface-3` block, replies plain, tools as "Used {name}" in `fg-subtle`. **Back** returns to the list.
  - **Continue** closes the library and opens UI-13 with the session as its source. Its body is "Vibezeo will write a summary from this {engine} session's last messages, then start {target} in its folder with it." Where offers "The session's folder" only, and there's no "Close the original pane".
  - States: loading (`Skeleton`s), error (`Banner` with Try again), empty (`EmptyState` "No sessions found" / "Sessions you run with Claude Code or Codex show up here, in Vibezeo or in any terminal."), no match ("No sessions match."), populated, and the transcript.
  - Settings › Privacy: "Search session text", a `Switch` (disabled while off only when limits turn search off). Its description: "Keeps a full-text index of your Claude Code and Codex sessions' prompts and replies on this computer, for the Session library. Nothing is sent anywhere. Turning it off deletes the index."

- **UI-155 Land worktrees** (dialog; "Land worktrees…" in a repository's rail menu and every local worktree's ⋯ menu; F-WT-07). Description: "Merges each ticked worktree into {base}, one at a time, after merging {base} into it and running the check. Nothing is pushed." Body: a `Checkbox` per candidate (its branch, "{n} commits", and the forecast: "No conflicts expected", "Conflicts in {n} files" or "No forecast"), in landing order; one with uncommitted changes is disabled with "Commit or stash its changes first". Then **Check command** (`Input`, monospace, the last one used here), with "Without a check, worktrees land untested." when empty. Loading: `Skeleton` rows. Empty: "No worktrees have commits that {base} doesn't." Errors: an error `Banner` (a dirty or detached base says so and disables Land). Footer: Cancel / **Land** (primary, "Land {n}").

  While landing, the rows become steps: waiting, "Merging {base} in…", "Checking…", "Landing…", Landed (with the commit), Failed (exit code), Conflict (the files). A failed step shows its output's tail in a `CodeBlock` with **Send failure to agent** (an agent `Select` when several run there) or **Copy** when none does. Footer: **Stop** while running; Close after. **Undo landing** (secondary, so Close is the landed state's one filled button) shows whenever the last run landed something; it asks with the native confirm "Undo landing? Moves {base} back to where it was before the landing. The worktrees keep their commits, and each keeps what landed before it, since {base} was merged into it." A landing interrupted by quitting shows a warning `Banner` the next time: "The last landing was interrupted. Vibezeo aborted its unfinished merge in {folder}."

- **UI-156 Check pull requests?** (dialog; "Check pull request" in a local worktree's ⋯ menu while its repository isn't allowed; F-WT-08). Title "Check pull requests for this repository?". Body: "Vibezeo will run your GitHub CLI (gh) in {folder}'s worktrees to read their pull requests, checks and review comments. gh signs itself in; Vibezeo never sees its token." Footer: **Not now** / **Allow** (primary).

- **UI-157 Pull request checks** (dialog; the checks badge on a worktree's rail row; F-WT-08). Title "Checks for {branch}", description the pull request's number as a link. One row per check: its name, workflow and state ("Passed", "Failed", "Running", "Skipped", "Cancelled"). A failed one has **Send failure to agent** (an agent `Select` when several run in the worktree) or **Copy failure** when none does. Loading: `Skeleton` rows. Empty: "No checks have reported yet." Error: an error `Banner` with Retry. Footer: **Refresh** / Close.

  The rail badge leads with its icon: a `Spinner` while checks run, a crossed circle in the error tone when one failed, a check mark when they passed, a no-entry sign when they were cancelled. Its words are the status bar's, whole: "CI running", "CI failed", "CI passed", "CI cancelled" (D-2700). Its title is the checks' summary ("{n} checks failed: {names}", "Checks running", "Checks passed", "Checks cancelled"); its accessible name starts with its words and, for a failure, says which ("CI failed: 1 check failed: test"). Beside the row's status it's the icon alone, as the radar mark is (D-2613, D-2614), and it folds to its icon after the radar mark when the row's contents don't fit (D-2700). Nothing shows without a pull request, before the repository is allowed, or on GitLab.

- **UI-154 Compare previews** (dialog; "Compare previews…" in every local worktree's ⋯ menu, with the worktree ticked, and "Side by side…" in Best of N's Compare, with both lanes ticked when there are two; F-WT-09). Description: "Choose two worktrees. Their previews open side by side in a new tab." Body: a `Checkbox` per worktree of the repository whose preview exists, labelled with its branch, described "Preview running" or "Its preview starts when you open them"; ticking a third unticks the oldest. Loading: two `Skeleton` rows while the previews are read. Empty: "Side by side needs two worktrees whose repository has a preview." Footer: Cancel / **Open side by side** (enabled with two ticked; "Waiting for the previews…" while it waits). A failure shows an error `Banner`; an untrusted preview opens UI-151 instead. The result is a new tab in the repository, named "<branch> | <branch>", with two browser panes side by side, each named after its branch.
- **UI-163 ACP agents** (a section of Settings › Engines, under the engine list; F-EN-10). Heading "ACP agents" with **Add an ACP agent…** (secondary, `Plus`).
  - Rows: the label, then the program and arguments in `font-mono` `fg-muted` (truncated, full as the tooltip), and **Remove** (ghost). Remove confirms (`ConfirmDialog`, danger): "Remove {name}?" / "Its threads stay readable but won't run again. An agent you add later is a new engine."
  - States: loading (`Spinner`), error (`Banner` with Try again), empty ("Add any agent that speaks the Agent Client Protocol to use it in Thread mode.").
  - **Add an ACP agent** (dialog, `md`): description "Vibezeo starts the command and asks it to introduce itself. The agent signs in its own way; Vibezeo never sees its keys."
    - "Fill in:" then **Goose** (ghost).
    - Fields: Name, Command (hint "A command on your PATH, or its full path"), Arguments (the extra-arguments parser; an open quote is an error), and Environment variables: rows of name and value with a remove button, **Add a variable**, and the hint "Settings the agent reads, not keys: keep keys in the agent's own sign-in."
    - Footer: Cancel, **Test** (secondary), **Add** (primary). Both are disabled until Name and Command are filled and the arguments parse.
  - In a thread on an added agent, **Sign in** on a sign-in error doesn't open an account's login: an info toast says "Sign in to {label} with its own command in a terminal, then try again. Vibezeo never holds its sign-in." (D-2514). The error itself names the methods the agent offers.
  - The launch sheet never lists added agents: they run in threads only (D-2513).
    - A passed test shows a success `Banner`: "{name} answered, version {version}." plus "It signs in with: {methods}." A failure or a refused value shows an error `Banner` with the core's message ("Not an ACP agent: …"). Any change clears both.
  - In the Thread engine picker, an added agent reads "{label} · via ACP".

**ID allocation for this part.** Thread UI-20–25, Engines & accounts UI-33–37, Routines UI-50–53 (UI-50 is already referenced by `CronInput`), Agent mode UI-70–76, Tasks UI-80–84, Plugins UI-90–94. UI-40–49 (Dashboard, Notifications) and UI-60–69 (Dictate, Conductor) are reserved for later parts.

**Shared vocabulary (applies to every screen below).**
- Worktree paths and branches follow DECISIONS: `<repo>/.tandem/worktrees/{slug}` on `tandem/{slug}`.
- The Composer's mode segments are **"Plan" | "Act"** (§2.23), and its access levels are **"Read only" / "Edits" / "Full access"**. "Act" is used instead of "Agent" so it never collides with Agent mode.
- **Unattended work** (task runs, routine runs, teammate mail) never inherits interactive Full access. It gets Full access only where a screen offers an explicit, per-item opt-in (routines, UI-51).

---

### 4.3 Thread mode

#### UI-20 · Thread mode layout · TH, SH
```
┌ THREADS  + ┐┌─ ⌂ shop · Fix checkout redirect ▾  ⎇ tandem/fix-redirect  (C) Claude Code · Work · Sonnet ─┐┌ CHANGES (3) ──── × ┐
│ ⌕ Search   ││   ◆ Waiting for you · 2m                  ▰▰▱ 41%   [Changes ⌘⌥C] [⫿ Split] [⋯]           ││ M src/checkout.ts  │
│ PINNED     ││──────────────────────────────────────────────────────────────────────────────────────────── ││   +12 −3           │
│ ✓ Release…  ││                                                      ┌───────────────────────────────┐   ││ A src/redirect.ts  │
│ SHOP       ││                                                      │ The checkout redirect loops … │   ││   +40              │
│ ◆ Fix chec… ││                                                      └───────────────────────────────┘   ││ M test/checkout.…  │
│ ● Add rate… ││  › Reasoned for 9s                                                                         ││   +18 −1           │
│ ○ Explain … ││  ▸ Read src/checkout.ts                                               ✓                     ││                    │
│ NO FOLDER  ││  ▸ Ran pnpm test checkout (exit 1, 6s)                                ✓                     ││ [Open diff] [⋯]    │
│ ○ Regex he… ││  The loop comes from …                                                                     ││                    │
│            ││  ┌─◆ Waiting for you ─ Claude Code wants to edit 2 files ─────────────────────────────────┐ ││                    │
│            ││  │ [View diff] [Allow] [Deny]                                                            │ ││                    │
│            ││  └───────────────────────────────────────────────────────────────────────────────────────┘ ││                    │
│ ⌕ Search   ││  ┌ Composer (§2.23) ───────────────────────────────────────────────────────────────────────┐ ││                    │
│  threads   ││  └──────────────────────────────────────────────────────────────────────────────────────────┘ ││                    │
└────────────┘└──────────────────────────────────────────────────────────────────────────────────────────────┘└────────────────────┘
```
- **Rail (§3.3 Thread row).** Header "THREADS" + "+" ("New thread", Mod+N). A "⌕ Search" filter box under the header ("Filter threads"), then "PINNED", then one group per folder (uppercase folder name), with "NO FOLDER" first. Rows: StatusDot, title (truncate), relative time ("2m", "yesterday"), EngineGlyph at the far right. Folder group header hover shows "+" ("New thread in {folder}") and ⋯ (Collapse, Reveal folder, Remove folder from Thread…). Footer: "Search threads" (opens the palette pre-filtered with the thread scope).
- **Row ContextMenu:** Rename (F2) · Pin / Unpin · Open in split · Open in Code mode · Duplicate thread · Copy thread link · Archive · ─ · Delete… (danger).
- **Thread header (40px, surface-2):** folder chip "⌂ {folder}" (or "No folder · Add folder…"), title (click ▾ to rename or pick a recent thread), branch chip (when the folder is a git repo), EngineBadge with AccountChip, model name, then the status line ("● Working 12s", "◆ Waiting for you · 2m", "✓ Done · 3m ago", "○ Idle", "▲ Failed"), a context UsageMeter xs ("Context"), and buttons "Changes" (toggle, count badge), "Split" (Mod+D), ⋯.
- **Header ⋯ menu:** Hand off… (Mod+Shift+H) · Switch account ▸ · Plan versions (n) · Open in Code mode (opens the same session in a terminal pane, when the engine supports resume) · Copy session ID · Export as Markdown… · Archive · ─ · Delete thread… (danger).
- **Multi-thread grid:** drag rail rows into the main area or use Split to show up to 6 threads side by side, with no account (D-2900). It uses PaneGrid (§2.21) with PaneChrome carrying the thread title. Closing a grid pane only hides the thread. Each pane keeps its own draft and Changes panel. Over a smaller limit a token carries: toast "This account shows {n} threads at a time", with nothing to buy.
- **Keyboard:** Mod+N new thread · Mod+L focus composer · Mod+D split · Mod+[ / Mod+] cycle grid panes · Mod+Alt+C toggle Changes · Mod+F find in thread · Alt+↑ / Alt+↓ previous/next message · Mod+Shift+H hand off · F2 rename · Mod+Backspace archive (asks first).

#### UI-21 · Thread view: transcript items · TH
> **D-2604 (scannable turns):** three or more consecutive tool calls fold to "+n previous tool calls" with the latest call still shown beneath. A command whose output ends in a test summary (vitest, jest, cargo, pytest) shows "18 passed · 0 failed" as its meta. A finished turn that edited files or ran tests closes with one line: "Changed 2 files +14 −6 · 18 passed · 0 failed". All of it comes from the tool events the engines send; nothing is inferred beyond their diffs and output. Claude Code sends no diffs, so its edits count the lines its result's patch says, or else what its Edit, MultiEdit and Write inputs describe (D-2608). A file whose lines can't be told (without the patch, a Write over an existing file or a `replace_all`) counts as changed, and then the turn says only "Changed 2 files", never a partial total. "+n −m" reads out as "n lines added, m removed".
| Item | Rendering | Labels |
|---|---|---|
| User message | Right-aligned block, surface-3, radius md, max 72% width, attachment chips above text | Hover: "Edit and resend", "Copy" |
| Reasoning | Collapsed one-liner, fg-subtle, chevron | "› Reasoned for {n}s"; while streaming "› Reasoning…" |
| Tool call | `ToolCallBlock`: icon by tool type, verb + mono arg, trailing ✓ / spinner / ▲ | "Read {path}", "Edited {path} (+a −d)", "Ran {cmd} (exit {c}, {t}s)", "Searched for "{q}" ({n} files)", "Fetched {host}", "Called {plugin} · {action}" |
| Tool group | 3+ consecutive calls fold into one row | "{n} tool calls" / expanded "Collapse" |
| Diff block | Inline DiffView (unified, collapsed to 12 lines) for edits in the finished turn | "Show all {n} lines", "Open in Changes" |
| Assistant | MarkdownView, no bubble, text-md | Hover: "Copy", "Copy as Markdown" |
| Plan version | Callout with accent left bar, title "Plan · version {n}", numbered steps | Actions "Implement this plan", "Revise…", "Copy" |
| Note | Centered fg-subtle line with icon | "Switched to Codex · Personal", "Resumed after restart", "Handed off from Claude Code" |
| Error | Banner (error) inside the transcript | See states below |

- **Streaming:** only the last block re-renders. A running footer under the last item shows "● Working {elapsed}" with the three-dot pulse. Auto-scroll sticks to the bottom until the user scrolls up; then a floating pill "↓ New activity" appears (click or End returns).
- **Plan → implement:** in Plan the agent ends its turn with a Plan version. "Implement this plan" switches the composer to Act (keeping the chosen access level), sends "Implement plan version {n} exactly." with the plan attached, and keeps any unsent draft text in the composer. "Revise…" pre-fills the composer with "Revise the plan: ". "Plan versions (n)" in the header ⋯ lists all versions (time, step count) with "View", "Implement", "Copy".
- **States:**
  - Loading a thread: 4 Skeleton message blocks (after 150ms).
  - Empty thread (with folder): EmptyState centred above the composer. Title "Where do you want to start in {folder}?" Body "Describe the work in plain words, or @ a file to start from." Suggestion chips: "Explain this repo", "Find and fix a failing test", "Plan a new feature". Clicking a chip fills the composer; it never sends by itself.
  - Empty thread (no folder): title "Start without a folder". Body "The agent can answer and plan, but it can't read or change files until you add one." Action "Add folder…". Adding a folder keeps the draft and attachments.
  - No threads at all: rail shows "No threads yet" + "New thread"; main shows the no-folder empty state.
  - No chat engine installed: EmptyState "Thread mode needs a chat engine". Body "Install Claude Code, Codex, Cursor Agent, Grok or OpenCode to read sessions as conversations." Actions "Set up engines" (UI-33), "Use Code mode instead".
  - Turn errors (error Banner, each with one next step):
    - "The conversation is too long for this model. Start a new thread from a summary." + "Continue in new thread".
    - "{Engine} stopped responding while {stage}." (stage: "starting", "sending your message", "streaming the reply", "running a tool") + "Retry turn", "View log".
    - "{n} tool calls were blocked by this thread's access level ({level})." + "Change access…".
    - "{Engine} needs you to sign in again." + "Sign in".
    - Usage limit: UI-36 card.
  - Cancelled turn (Stop): Note "Stopped by you" (never styled as an error).
  - Crash recovery: a thread left Working by a crash opens as Idle with Note "Tandem restarted. The last turn didn't finish." + "Retry turn".
- **Acceptance:** a 2,000-item transcript scrolls at 60fps (virtualized); Stop during streaming leaves a "Stopped by you" note and a usable composer within 500ms.

#### UI-22 · Thread composer · TH, EN, AC, DI
Uses Composer (§2.23) with these thread-specific rules:
```
┌──────────────────────────────────────────────────────────────────────────────────┐
│ [▣ error.png ×] [@ src/checkout.ts ×] [@ test/ ×]                                 │
│ Ask Claude Code about shop — @ a file or folder, paste a screenshot…              │
├──────────────────────────────────────────────────────────────────────────────────┤
│ [Plan|Act] [(C) Claude Code ▾] [Sonnet ▾] [Effort: Med ▾] [Access: Edits ▾]  12.4k │ [📎] [🎙] [➤] │
└──────────────────────────────────────────────────────────────────────────────────┘
```
- **Placeholders:** "Ask {engine} about {folder} — @ a file or folder, paste a screenshot…"; no folder: "Ask {engine} anything…"; running: "Working… you can type your next message, or Stop."
- **Engine select:** chat engines only (Claude Code, Codex, Cursor Agent, Grok, OpenCode). Terminal-only engines appear in a disabled group "Terminal only · open in Code mode". Changing engine mid-thread takes effect on the next turn and adds a Note "Switched to {engine} · {account}. Tandem will send a summary of this thread with your next message." Every chat engine is enabled with no account (D-2900); limits that list engines disable the others with "Not available to threads here".
- **Model select:** popover titled "{Engine} models" with "Search models…". First row "Default ({resolved model})". Rows carry a one-line description from the engine's catalog. Engines that don't report models show only "Default". A saved model that's gone shows a warning row "{model} is no longer offered. Using Default."
- **Effort select:** "Low", "Medium", "High", "Max" (engine-supported values only; hidden when unsupported).
- **Access select:** "Read only" (the agent can read and search, no edits, no commands), "Edits" (it can change files; commands ask first), "Full access" (warning badge; one-time confirm per folder, §2.23).
- **Token readout:** "{n}" tokens this thread in fg-subtle tabular-nums (k/M suffixes), tooltip "Tokens used in this thread · {input} in / {output} out".
- **Defaults:** a new thread uses the last engine, and each engine remembers its own last model, effort and access.
- **Attachments:** 📎 menu "Add files…", "Add folder…", "Paste image". Dragging anywhere over the thread shows an accent-soft overlay "Drop to attach". Over 20 MB per file: chip in error "Too large (max 20 MB)".
- **@mentions:** workspace folders first, then files (fuzzy), then "Open threads". Selected items become chips. There are no teammate mentions in Thread mode.
- **Dictate (🎙):** hold to talk or click to toggle; text inserts at the caret. Settings can hide the button.
- **Send/Stop:** Send is disabled when empty ("Type a message first"). While running, the right button becomes Stop (Esc). Sending while running queues the message as a chip above the input: "Queued · sends when this turn finishes" with ×.
- **IME:** Enter during composition never sends.
- **Keyboard:** Enter send · Shift+Enter newline · Esc stop (when running) · ↑ in empty composer edits last message · Mod+Shift+P toggle Plan/Act · Mod+Shift+O model picker · Mod+U attach · `@` mention · `/` engine commands.

#### UI-23 · Inline approvals · TH, PL
- ApprovalCard (§2.24) renders **in the transcript at the point of the request** and a compact copy is **pinned above the composer** while pending, so it's reachable without scrolling. The thread status becomes "◆ Waiting for you" and the rail row gets the waiting diamond.
- Multiple pending requests stack; only the oldest is expanded, then "+{n} more waiting" expands the rest.
- **File edits:** "View diff" expands a DiffView inside the card. "Allow" / "Deny" / "Deny and explain…".
- **Commands:** "Allow once" / "Allow for this thread" / "Deny". The session scope label is "this thread".
- **Plugin actions:** ApprovalCard kind plugin_action (see UI-93 for fields).
- **Expired:** "This request expired after {n} minutes. The agent was told it was denied." with "Ask again" (sends "Please retry the last action.").
- **Resolved:** collapses to one line per §2.24.
- **Keyboard:** Mod+Shift+A focuses the oldest pending card; then A or Mod+↩ allow once, Mod+Shift+↩ allow for this thread, D deny, E deny and explain, Esc focuses Deny (§5).

#### UI-24 · Changes panel · TH, CD
- A right panel inside the thread view (not the right sidebar), 360px, resizable 280–560. Toggle with "Changes" or Mod+Alt+C. Below 1100px window width it floats over the transcript (shadow-3).
- **Header:** "CHANGES ({n})", scope Select "This thread" / "All uncommitted", ⋯ (Refresh, Copy file list, Open in Files).
- **List:** FileTree-style rows with status letter, path (middle-truncated), +/− counts. Selecting a row shows its DiffView beneath (split handle) or in a Dialog lg when the panel is under 420px.
- **Footer actions:** "Open diff" (full DiffView in main area), "Commit…" (message field, pre-filled by the agent's last summary; "Commit" / "Commit and push"), "Discard all…" (danger, ConfirmDialog "Discard {n} changes? This can't be undone.").
- Refreshes when a turn finishes and on file-watch events (debounced 500ms).
- **States:** empty "Nothing's changed on disk yet." · not a repo "This folder isn't a git repository, so Tandem can't track changes. [Initialize git]" · loading skeleton rows · error "Couldn't read git status: {reason}" + "Retry".

#### UI-25 · Hand off / switch account in a thread · TH, AC
- **Hand off…** opens UI-13 with Thread-specific "Where" options: "Continue in this thread" (default) / "New thread in {folder}" / "New worktree thread". On start the thread shows Note "Handed off from {engine} · {account}" followed by the summary as a collapsed Callout "Handoff summary".
- **Switch account ▸** lists the engine's accounts with 5h/Week meters (as in AccountChip). Choosing one adds Note "Now using {account}. Earlier turns stay linked to {old account}."

---

### 4.4 Engines & accounts

#### UI-33 · Settings › Engines · EN
```
Engines                                                            [Rescan]
Coding agents Tandem found on this machine. Nothing is bundled.
┌────────────────────────────────────────────────────────────────────────────┐
│ (C)  Claude Code   2.4.1 · Thread ✓ · Teammates ✓   2 accounts   [Manage ▸] │
│ (X)  Codex         0.52.0 · Update available 0.53 [Update]  1 acct [Manage ▸] │
│ (Gk) Grok          1.1.0 · Thread ✓               Signed in     [Manage ▸] │
│ (G)  Gemini CLI    ⚠ Installed but not on your shell PATH    [Fix PATH…]    │
│ (Cu) Cursor Agent  Not installed                            [Install]      │
│ ▸ 6 more                                                                   │
└────────────────────────────────────────────────────────────────────────────┘
Detection  Asked zsh (login) in 0.8s · checked 9 folders   [Show details]
```
- Extends the UI-05 rows. Capability tags: "Thread ✓" (chat adapter), "Teammates ✓" (can hold teammate chats with memory and skills), "Terminal only".
- **Manage ▸** opens an engine detail page: KeyValue ("Path", "Version", "Installed with", "Provider homes"), toggle "Show in launch tiles", "Default model" select, "Default access" select ("Read only" / "Edits" / "Full access"), then the engine's accounts (UI-35) and "Remove from Tandem" (hides it; never uninstalls).
- **Show details** lists each shell asked (with timing) and each folder checked, plus a copyable fix: "Add this to ~/.zprofile: export PATH=\"{dir}:$PATH\"". A slow shell shows "Your shell took over 5s to start. Tandem stopped waiting." with "Try again".
- **States:** checking (spinner per row) · none found (EmptyState "No coding agents found" / "Install one below, or tell Tandem where it lives." / "Choose binary…") · remote host scope (a Select at the top "This Mac / {host}" lists engines per SSH host).
- **Keyboard:** Mod+R rescan (while on this page).

#### UI-34 · Install and update flow · EN
- Uses the UI-05 Install Dialog. Added states: "Downloading installer…" (indeterminate ProgressBar), live terminal output, "Checking that {engine} runs…", success "{Engine} {version} is ready." with "Sign in now" (primary) / "Later".
- **Update:** Dialog "Update {engine} to {v}?" body "Running sessions keep the old version until they restart." Actions "Update" / "Cancel". Rows with running sessions show "Restart sessions to use {v}".
- **Failure:** "Install failed (exit {code})" + "View output", "Copy command", "Open install guide". Platforms with no scripted installer show only "Copy command" and "Open install guide".

#### UI-35 · Accounts · AC, EN
```
Accounts                                                     [+ Add account]
Each account signs in with the engine's own login, in its own folder.
CLAUDE CODE
│ Work       Default   5h ▰▰▰▱ 62% · resets 1h 12m   Wk ▰▱▱ 38%   [⋯] │
│ Personal             5h ▱▱▱▱  4%                   Wk ▰▱▱ 21%  [Make default] [⋯] │
CODEX
│ Studio     Default   Week ▰▰▰▰ 91% · resets Mon 09:00          [⋯] │
OVERRIDES
│ Workspace "shop" → Claude Code: Personal                      [Edit] [×] │
│ Teammate "Rhea"  → Codex: Studio                              [Edit] [×] │
```
- Reached from Settings › Accounts, UI-33 "Manage", AccountChip "Manage accounts…" and the title-bar usage popover.
- **Row:** nickname (inline rename), "Default" Badge (accent), UsageMeters "5h" and "Wk" (whatever windows the engine reports; "Usage not reported" otherwise), sign-in state ("Signed in" / "Signed out" warning / "Checking…"). ⋯: Rename · Make default · Sign in again · Open provider folder · Sign out · ─ · Remove account… (danger; "Remove {name}? Its sign-in folder is deleted. Threads and chats that used it stay, but can't continue on it.").
- **Add account Sheet (md):** "Engine" select, "Nickname" (placeholder "Work, Personal, Client A"), then "Sign in" launches the engine's login in a terminal inside the sheet under a new provider home. Copy: "You're signing in with {Engine}'s own login. Tandem never sees your token." Completion flips to "Signed in as {nickname}" + "Done". Cancel stops the login process. Accounts per engine have no limit with no account (D-2900); a token's limit shows the core's reason, with nothing to buy.
- **Default vs override:** the default applies to new sessions everywhere. Overrides (workspace or teammate) win over the default; existing sessions keep the account they started on. "Edit" opens a Dialog with "Scope" and "Account" selects.
- **States:** empty group "No {engine} accounts yet. [Add account]" · sign-in failed "Sign-in didn't finish. {engine output line}" + "Try again".

#### UI-36 · Usage-limit card and dialog · AC, TH, CD, AG
```
┌─▲ Work reached its 5-hour limit · resets 4:12 PM (in 1h 48m) ───────────────┐
│ Your thread and changes are safe. Choose how to continue.                    │
│ ( ) Pause and resume at reset   Tandem sends "continue" at 4:12 PM           │
│ ( ) Continue on another account [Personal · 5h 4% ▾]                         │
│     Sends a summary: recent messages and finished steps. Full tool output    │
│     stays here.                                                              │
│ ( ) Stop here                                                                │
│                                                    [Cancel]  [Continue]      │
└──────────────────────────────────────────────────────────────────────────────┘
```
- Appears inline in Threads and teammate chats; in a Code pane it shows as a Banner with "Choose…" opening the same content as a Dialog md.
- Waiting state (after "Pause and resume at reset"): thread status "◆ Paused · resumes 4:12 PM" with "Resume now" and "Cancel auto-resume". A Notification fires on resume.
- Switching shows Note "Continuing on {account} with a summary…". There is **never a silent switch**; every account change is a user click.
- No other account available: option 2 is disabled with "No other {engine} account. [Add account]".
- **Keyboard:** ↑/↓ choose option, Mod+↩ continue, Esc cancel (keeps the card).

#### UI-37 · Usage page · AC
- Settings › Accounts › "Usage" tab. Range SegmentedControl "Today | 7 days | 30 days". Stacked daily bars by engine (tokens), a table per account (tokens, sessions, top workspace), and "Built from your engines' local logs on this machine." Empty: "No usage found in the last {range}."

---

### 4.5 Agent mode

#### UI-70 · Teammate rail · AG
> **D-2603:** Agent mode's first run (no teammates) shows "Hand work to a teammate", one primary "New teammate" and the local templates as cards (avatar, name, the brief's first line); a card opens New teammate filled in. Loading shows two skeleton cards; if the templates can't load, the primary action stands alone.
```
┌ TEAMMATES      + ⋯ ┐
│ ┌──┐ Lead      ● 2 │   ← pinned Lead: 32px avatar, status summary
│ │LD│ "Asked Rhea…" │
│ └──┘               │
│ (RH) Rhea     ◆    │   ← waiting diamond = "Waiting for you"
│   ├ ● Weekly deps  │
│   ├ ○ Test flake…  │
│   └ Show all (7)…  │
│ (OB) Ops Bot   2h  │
│ (Te) Tess  ▲ Failed│
├────────────────────┤
│ ☐ Tasks            │
│ ⟳ Routines      3  │
│ ✦ Skills library   │
│ ⚡ Plugins          │
└────────────────────┘
```
- Per §3.3: Lead pinned first (avatar 32, a one-line "last said" preview), then teammates A–Z or dragged order (Avatar 24 with status badge, name, trailing slot). Trailing slot priority: "▲ Failed" > "◆" (waiting for you) > "● {n}" (working chats) > relative time.
- Header ⋯: "Pause teammate mail" (toggle; when on, the header shows a "Mail paused" Badge and queued messages wait) · "Sort A–Z" / "Custom order" · "Show terminal-only teammates".
- Row hover: "New chat with {name}" (+), "Pin as Lead", ⋯ (Open settings, Duplicate, Delete…).
- Footer nav rows open pages in the main area: Tasks (UI-80), Routines (UI-50, count = active routines), Skills library (UI-74), Plugins (UI-90).
- **Keyboard:** standard rail keys (§3.3); Mod+N new chat with the selected teammate; "New teammate" is in the rail header "+" and the palette (no default shortcut: `Mod+Shift+N` is Notifications, §5).

#### UI-71 · Teammate page · AG
```
┌──────────────────────────────────────────────────────────────────────────────┐
│ (RH) Rhea                                   ● 1 working  [+ New chat ⌘N] [⋯] │
│      Keeps our dependencies healthy · Claude Code · Work                     │
│ [Chats 7]  [Memory 12]  [Skills 4]  [Settings]                               │
├──────────────────────────────────────────────────────────────────────────────┤
│ ◆ Weekly deps │ ● Test flake │ ○ Upgrade React │ +                          │
│ (transcript per UI-21)                                                       │
│ ✉ Lead → Rhea: "Can you check why CI is red on main?"          [Open]        │
│ (Composer §2.23, placeholder "Message Rhea…")                                │
└──────────────────────────────────────────────────────────────────────────────┘
```
- PageHeader: Avatar 48, name, first line of the brief (fg-muted), EngineBadge + account. Status pill: "Ready" (idle), "● {n} working", "◆ Waiting for you".
- Tabs (underline): "Chats {n}", "Memory {n notes}", "Skills {n}", "Settings". Mod+Alt+1…4 switch tabs.
- **Chats tab:** a pill strip of chats (StatusDot + title; drag to reorder; right-click Rename / Archive / Delete), "+" new chat. The chat body is the UI-21 transcript with the composer. Each chat shows its place chip ("⌂ Home" or "⌂ {place}") in a thin header; changing place applies to the next turn.
- **Teammate mail rows:** one line "✉ {from} → {to}: "{first 80 chars}"" with "Open" (Dialog with the full message, time, delivery state "Delivered" / "Queued · mail is paused" / "Refused: {name} can't receive mail" + "Retry"). `@{teammate}` in this composer sends mail when the sender has "Can message teammates".
- **Terminal-only teammate** (engine without a chat adapter): the Chats tab is replaced by "Open in Code mode" plus Banner "{Engine} runs as a terminal. Memory and skills need a chat engine."
- **Empty chats:** EmptyState title "Give {name} something to do". Body "Chats with {name} use its brief, memory and skills." Chips from the brief (up to 3, e.g. "Check for outdated packages").

#### UI-72 · New teammate sheet · AG, BL
```
New teammate                                                         ×
Someone you can hand work to. Name it, describe the job, pick the engine.
┌ ✦ Draft with AI ─────────────────────────────────────────── 10 ◈ ┐
│ Describe the job in a sentence or two…                     🎙 [Draft] │
└────────────────────────────────────────────────────────────────────┘
Name        [Release notes writer          ] 21/40
Avatar      (RN) Generated from its ID.  [Upload photo…]
Engine      [(C) Claude Code ▾]   Account [Default (Work) ▾]
Brief       ┌───────────────────────────────────────────────┐
            │ What it does, how well, in what voice, and     │
            │ what it must never do.                         │
            └───────────────────────────────────────────────┘
Home        ~/.tandem/teammates/release-notes-writer   [Change…]
                                        [Cancel]  [Create teammate ⌘↩]
```
- Sheet md. **Draft with AI** (hosted; comes with Vibezeo accounts): type or Dictate a description, then "Draft". States: "Drafting a name and brief…" (skeletons over Name and Brief) → fields fill and get a subtle accent-soft highlight → footnote "Review and edit anything before you create it." Error: "Couldn't draft right now. Fill in the fields yourself." With no account the card isn't shown.
- **Engine select:** chat engines first with description "Chats, memory and skills"; terminal-only engines in a second group with "Terminal only". Not installed: disabled "Not installed · Set up". Helper: "The engine does the work. The brief decides what the work is."
- **Validation:** name required, unique (error "You already have a teammate called {name}."), max 40. Create is disabled while the engine isn't installed ("Install {engine} first" + "Recheck").
- **Limits:** none with no account (D-2900); a token's teammate limit comes back from the core as an error, with nothing to buy.
- **Result:** creates the Home folder with `NOTES.md` and `ABOUT_YOU.md`, opens the teammate page on its Chats tab.

#### UI-73 · Memory tab · AG
```
[Notes | About you]         [List | Raw]        ▰▰▰▰▰▰▱▱▱▱ 1,840 / 3,000 characters
SUGGESTED AFTER "Weekly deps" (2)
│ ✎ "Prefers pnpm over npm in every repo."          [Keep] [Edit] [Discard] │
NOTES
│ • Release branch is cut on Thursdays.                          [Edit] [🗑] │
│ • Never bump major versions without asking.                    [Edit] [🗑] │
┌──────────────────────────────────────────────────────────────── [Add note] ┐
│ Add a fact or preference for Rhea…                                            │
└────────────────────────────────────────────────────────────────────────────┘
Memory is for lasting facts and preferences. Don't store secrets here.
```
- Pill Tabs "Notes" (`NOTES.md`) and "About you" (`ABOUT_YOU.md`, "What {name} knows about you"). SegmentedControl "List | Raw".
- **Budget meter:** a ProgressBar with the §1.3 usage thresholds, label "{used} / {cap} characters". Caps come from Settings (UI-76).
- **Suggestions** appear only in Learn mode "Ask". In "Auto" they are applied and shown in a collapsible "Added automatically (n)" group with "Undo" per item.
- **Raw:** mono Textarea of the file with "Save" / "Revert". If the file changed on disk while editing: Banner "{file} changed outside Tandem." + "Reload" / "Keep mine".
- **States:** empty "{name} hasn't saved any notes. It adds them as it learns, or you can add one." · over budget "Over the budget by {n} characters. Trim a note or raise the budget in Settings." (save disabled) · likely secret detected "This looks like a secret. Memory isn't encrypted; store it in a plugin or your keychain instead." (save disabled) · loading Skeleton rows.
- **Keyboard:** Mod+↩ add note · Mod+S save (Raw) · Delete on a focused row removes it (with Undo toast).

#### UI-74 · Skills tab and Skills library · AG
- **Skills tab list:** ListRow per skill: name (mono), description, source Badge ("By {name}", "By you", "From library"), usage "Used {n}× · last {time}", Switch "On". ⋯: Edit · Duplicate · Copy to teammate… · Reveal folder · ─ · Delete… (danger). Header actions: "+ New skill", "Add from library".
- **Skill editor (Sheet lg):** "Name" (lowercase-dashes, validated "Use lowercase letters, numbers and dashes."), "When to use" (one sentence; this is the description the engine sees), body Markdown editor with Preview toggle, and a live checklist: "Says when to use it", "Steps are concrete", "Says how to check the result", "Says what not to do". Items are advisory, except "No secrets", which blocks Save. Footer "Cancel" / "Save skill".
- **Skills library page:** grid of starter skills (name, description, "Add to…" teammate multi-select) plus "Sync to installed engines" (copies the skill folder into each chat engine's skills directory; per-engine result chips "Claude Code ✓", "Codex ✓", "Cursor ▲ folder not writable").
- **Empty:** "No skills yet. {name} writes a skill when it finds a procedure worth repeating, or you can add one."

#### UI-75 · Teammate settings: identity, engine, places · AG, AC
SettingsRows in groups, with a sticky save bar ("Unsaved changes" · "Revert" · "Save") that appears only when dirty.
- **IDENTITY:** "Name", "Avatar" (Change photo / Use generated), "Brief" (Textarea, 8 rows, hint "Leads every chat with {name}.").
- **ENGINE:** "Engine" select. Changing it shows a warning Callout: "Existing chats keep their transcripts but restart on {new engine}. Memory and skills carry over, since they're files." Save label becomes "Save and switch to {engine}". "Account": "Use default ({account})" or a specific account (creates a UI-35 override).
- **PLACES:** "Home" (path, "Reveal"). Places list: path, access Select "Read only" / "Read and write", ×. "+ Add place…" (folder picker). Helper: "{name} can only open Home and the places listed here."

#### UI-76 · Teammate settings: permissions, learning, danger zone · AG, PL, RT
- **PERMISSIONS:**
  - "Can message teammates" Switch; when on, "Who" Select "Any teammate" / "Only…" (multi-select). Helper: "Mail can start a chat with the recipient. It never carries Full access."
  - "Can create and edit its own routines" Switch. Helper: "It can schedule work for itself. You'll see each routine on the Routines page."
  - "Access for unattended work" Select "Read only" / "Edits" (default). Full access is not offered here; it's per routine (UI-51).
  - "Plugins": one row per connected plugin: glyph, name, account, Switch "Enabled for {name}", "Policy…" (opens UI-93 scoped to this teammate). Not-connected plugins aren't listed; footer link "Connect more plugins…". Helper: "Only enabled plugins are visible to {name}. Risky actions still ask you."
- **LEARNING:** "Learn after each turn" SegmentedControl "Ask | Auto | Off". Descriptions: Ask "Suggests memory updates for you to keep or discard." Auto "Updates memory itself; you can undo." Off "Only changes memory when you or {name} edit it in a chat." Footnote: "Learning runs a short follow-up on {name}'s own engine account. It never gets plugin access." "Notes budget" SegmentedControl "3,000 | 6,000 | 12,000 | 24,000" characters (About you is fixed at 1,500).
- **DANGER ZONE:** "Delete teammate…" (danger). ConfirmDialog with `requireText` = name: "Delete {name}? Its chats, memory and skills move to the Trash for 30 days. Its routines are deleted." The Lead can't be deleted, only renamed ("Lead can't be deleted. Pin another teammate as Lead first.").
- **Acceptance:** toggling a plugin off removes its tools from the next turn; a teammate without "Can message teammates" gets "Refused" on mail sends.

---

### 4.6 Tasks

Tasks live in a local tracker that humans and agents share (agents use the `tandem-tasks` MCP). **Creating or assigning a task never starts work.** A run always starts from an explicit "Run…".

#### UI-80 · Tasks page: board and list · TK
```
Tasks  ⌂ shop ▾                         Runs ●● ○○ 2/4   [Board|List]  [+ New task N]
⌕ Filter tasks   [Assignee ▾] [Label ▾] [Project ▾]  ☐ Show archived
BACKLOG 4      TO DO 3          IN PROGRESS 2           IN REVIEW 1          DONE 12
┌──────────┐  ┌──────────────┐  ┌───────────────────┐  ┌──────────────────┐  ┌────────┐
│SHOP-18   │  │SHOP-21       │  │SHOP-12            │  │SHOP-09           │  │SHOP-07 │
│Dark mode │  │Rate limit API│  │Fix checkout loop  │  │Upgrade to Vite 7 │  │ …      │
│ ui       │  │ api  (RH)    │  │ ● Running · 12m   │  │ ◆ Ready to review│  │        │
└──────────┘  └──────────────┘  │ ⎇ tandem/shop-12  │  └──────────────────┘  └────────┘
                                └───────────────────┘
```
- Opened from the Agent rail footer "Tasks", the right-sidebar "Tasks" tab (compact list only) and `#` in the CommandPalette.
- **Header:** workspace Select (tasks are per workspace; "All workspaces" option), **concurrency indicator** "Runs {active}/{max}" with 4 slot dots (filled = active run). Clicking it opens a popover listing active and queued runs (task key, runner, elapsed, "Stop") and the note "Up to 4 runs at once. More runs wait in line."
- **Board:** columns "Backlog", "To do", "In progress", "In review", "Done" (and "Canceled", hidden unless filtered). Drag cards between columns (dnd-kit; keyboard: Space pick up, arrows move, Space drop). Cards: key (mono, fg-subtle), title (2 lines max), labels (Chips), assignee Avatar (teammate or "You"), priority icon, and a run Badge: "● Running · {elapsed}", "◆ Waiting for you", "◆ Ready to review", "▲ Failed", "■ Stopped", "⋯ Queued ({n} ahead)".
- **List:** a table with columns Key, Title, Status, Assignee, Labels, Updated, Run. Sortable headers; multi-select with Shift/Mod-click; bulk bar "Move to…", "Assign…", "Archive".
- **Archive:** hover × on a card or row archives it; toast "Archived SHOP-18" + "Undo". "Show archived" reveals them dimmed with "Restore".
- **Agent edits:** when an agent changes a task, the card flashes accent-soft once and shows "Updated by {name} · just now" in its tooltip.
- **States:** empty workspace: EmptyState "No tasks yet" / "Write down work for you or your teammates. Nothing runs until you choose Run." / "New task" · no match "No tasks match these filters." + "Clear filters" · loading Skeleton cards · store error "Couldn't read tasks from {path}." + "Retry" / "Open folder".
- **Keyboard (page focused):** N new task · / filter · B / L board or list · J/K or arrows move selection · Enter open · R run selected · E edit title · Mod+Backspace archive · 1–5 set status.

#### UI-81 · Task detail · TK
```
SHOP-12 · Fix checkout loop                                  [Run… R] [⋯] ×
Status [In progress ▾]  Assignee [(RH) Rhea ▾]  Priority [High ▾]  Labels [bug ×][+]
Project [Checkout ▾]    Workspace ⌂ shop
DESCRIPTION                                                        [Edit]
  Users bounce between /cart and /pay after the coupon step…
RUNS
  ● Run 2 · Rhea · Claude Code · started 12m ago · ⎇ tandem/shop-12   [Open] [Stop]
  ▲ Run 1 · Codex · failed after 4m: tests didn't pass                [View]
COMMENTS · HISTORY
  (RH) Rhea · 3m  "Found the cause in redirect.ts; writing a test."
  (Composer, placeholder "Comment, or @ a teammate…")
```
- Sheet lg (§2.11). Title edits inline. Properties use Select/Combobox with "Add {query}…" for labels and projects.
- **Description:** Markdown view; "Edit" swaps to a Textarea with explicit "Save" / "Cancel".
- **Concurrent edits:** if an agent changes a field you're editing, your unsaved value stays and a Banner shows "{name} changed {field} while you were editing." + "Use theirs" / "Keep mine". Unchanged fields update silently.
- **Tabs under the description:** "Comments" (threaded, Markdown; agent comments show the teammate avatar) and "History" (Timeline of every field and status change, with who and when).
- **Narrow (sheet width < 560):** properties stack in one column above the description.
- **Keyboard:** Esc close · R run · Mod+↩ save description/comment · Mod+. focus properties.

#### UI-82 · Run in worktree dialog · TK, CD, AC
```
Run SHOP-12 in its own worktree                                  Dialog md
Runner       [(RH) Rhea · Claude Code ▾]      Account [Default (Work) ▾]
Model        [Default ▾]                      Access  [Edits ▾]
Worktree     ⎇ tandem/shop-12-fix-checkout-loop   from [main ▾]
Review       [Second pass by (X) Codex ▾]     ☐ Skip review
Extra instructions (optional)
┌──────────────────────────────────────────────────────────────┐
│ Anything the runner should know beyond the task description. │
└──────────────────────────────────────────────────────────────┘
Creates .tandem/worktrees/shop-12-fix-checkout-loop. Your main checkout isn't touched.
                                                   [Cancel]  [Start run ⌘↩]
```
- **Runner:** teammates (chat engines) or a bare engine + account.
- **Access:** "Read only" / "Edits". "Full access" is shown disabled with the reason "Task runs are unattended, so they can't use Full access."
- **Review:** default is a second pass by a different engine when one is installed; the reviewer reads the diff against the task and writes a verdict. "Skip review" moves the task straight to "In review" for you.
- **Queueing:** when all 4 slots are busy, the primary label becomes "Queue run" and the summary adds "4 of 4 runs are active. This one starts when a slot frees up."
- **Blocked states:** not a git repo: "Task runs need a git repository so each run gets its own worktree." (Start disabled) · limits that turn runs off: "Running tasks is turned off for this account. You can still track tasks." (runs need no account or sign-in, D-2902) · branch exists: inline error "Branch {name} already exists." + "Use a new name".
- **On start:** the task moves to "In progress", the run appears in RUNS and the Dashboard, and a Note is posted to Comments ("Run {n} started by you").

#### UI-83 · Review state · TK, CD
- A task enters **"In review"** when a run finishes. Detail shows a REVIEW section above RUNS:
  - "Result" (the runner's summary, MarkdownView), "Reviewer verdict" Badge ("✓ Looks good" done tone / "◆ {n} concerns" waiting tone / "Skipped"), and the concerns as a list.
  - "Changes ({n} files, +a −d)" with an embedded DiffView of the worktree against its base.
  - Actions: "Merge into {base}…" (primary; Dialog with "Squash and merge" / "Merge commit", commit message, "Delete worktree after merge" checked) · "Request changes" (opens a comment box; sending resumes the same run with the comment as its next instruction) · "Open in Code mode" (opens a pane in the worktree) · ⋯ "Discard run…" (danger; typed branch-name confirm).
- Merge success: status "Done", toast "Merged SHOP-12 into main" + "View commit". Merge conflict: Banner "Merging hit conflicts in {n} files." + "Resolve in Code mode" / "Cancel merge".

#### UI-84 · Stop and resume · TK
- "Stop" on a run (card, RUNS row, concurrency popover) asks "Stop run {n}? The worktree and progress so far are kept." → "Keep running" / "Stop run". Stopping during setup cancels cleanly with "Stopped before it started".
- Stopped run row: "■ Stopped by you · {time}" with "Resume" (continues the same conversation and worktree) and "Retry from scratch" (new worktree, same instructions; the previous instructions are pre-filled and editable).
- Failed run row: "▲ Failed after {t}: {reason}" (reasons: "engine not signed in", "usage limit reached", "tests didn't pass", "worktree couldn't be created", "you signed out") + "Resume" / "Retry" / "View log".
- Signing out or plan lapse stops active runs with reason "you signed out" / "plan changed"; records stay.

---

### 4.7 Routines

#### UI-50 · Routines list · RT, AG
```
Routines                                                          [+ New routine]
Recurring jobs your teammates run on their own.     ✓ Background runner on
TODAY  00 ─────06────── ▲09 ──── 12 ─────── ▲15 ──── 18 ───── ▲21 ── 24   (now ┃ 11:40)
┌───────────────────────────────────────────────────────────────────────────────┐
│ ● Morning summary   (LD) Lead   Weekdays 09:00   Next in 21h   ✓ New result · 2h│ [▶] [⋯] │
│ ○ Dependency check  (RH) Rhea   Every 6 hours    Next 15:00    ✓ Done · 5h ago  │ [▶] [⋯] │
│ ▲ Weekly update     (OB) Ops    Fridays 17:00    Next Fri      ✕ Failed · Fri   │ [▶] [⋯] │
│     Codex isn't signed in on account Studio.            [Sign in] [Open run]     │
│ ‖ Research brief    (Te) Tess   Mondays 08:00    Paused        — Never run      │ [▶] [⋯] │
└───────────────────────────────────────────────────────────────────────────────┘
```
- Opened from the Agent rail footer "Routines", the teammate page ⋯ "Routines", and the palette.
- **Today strip:** a 24h track with ▲ markers for today's runs (filled = done, hollow = upcoming, rose = failed) and a playhead at the current time. Hover a marker for "{routine} · {time} · {result}". Hidden under 900px width.
- **Row:** StatusDot (working while a run is in progress; "‖" paused, idle), name, teammate Avatar + name, schedule in words, "Next …" (relative, exact time in the tooltip), last result Badge: "✓ New result · {time}" (unread output), "✓ Done", "✕ Failed · {time}", "— Never run". Failed rows show a second line with the error and one fix action.
- **Error lines (with actions):** "{Engine} isn't signed in on account {account}." [Sign in] · "{Teammate} was deleted." [Choose teammate] · "{Engine} isn't installed." [Set up] · "This routine needs Edits, but {teammate} only allows Read only for unattended work." [Change access] · "Tandem was closed at {time}; the run was skipped." [Run now].
- **Row actions:** "▶ Run now" · ⋯: Edit · Pause / Resume · Open last run · Run history · Duplicate · ─ · Delete… (danger; "Delete {name}? Past run chats are kept.").
- **Header chip:** "✓ Background runner on" or "Runs only while Tandem is open" + "Turn on background runs".
- **States:** empty EmptyState "No routines yet" / "Have a teammate check something every morning, or draft a weekly update." + "New routine" + suggestion chips ("Morning project summary", "Weekly update draft", "Dependency health check") that open the editor pre-filled · limits that turn routines off (none with no account, D-2900): EmptyState "Routines are turned off" / "This account doesn't run routines. Your teammates still chat as usual." · paused by such limits: Banner "Your routines are paused because this account turned routines off. They resume when routines are back on." · loading Skeleton rows.
- **Keyboard:** N new · Enter edit · Space run now (asks first if a run is already going) · P pause/resume · H history · Delete delete.

#### UI-51 · Routine editor · RT, AG, PL
```
New routine                                                              ×
Teammate     [(LD) Lead ▾]
Name         [Morning summary                     ]
Instruction  ┌────────────────────────────────────────────────────────┐
             │ Summarize yesterday's merged PRs and open issues in     │
             │ shop. Output a 5-bullet digest. Don't comment on PRs.   │
             └────────────────────────────────────────────────────────┘
             Write it so it works on its own: what to produce, where to
             look, which time window, and the output format.
Schedule     (CronInput, UI-52)
Access       [Edits ▾]          Plugins  [Same as Lead (2) ▾]
If missed    (•) Skip it  ( ) Run once when Tandem opens
☐ Run in the background when Tandem is closed
                               [Cancel]  [Save and run now]  [Save ⌘↩]
```
- Sheet md. "Teammate" lists chat-capable teammates; if none: "Routines need a teammate on a chat engine." + "New teammate".
- **Instruction:** required, min 20 characters ("Add a bit more detail so the run can stand on its own.").
- **Access:** "Read only", "Edits" (default), "Full access". Choosing Full access expands a warning Callout with a required checkbox "Let this routine run any command without asking, even when I'm away." Save is disabled until it's checked. The row then shows a warning "Full" Badge in the list.
- **Plugins:** "Same as {teammate} ({n})" or "Choose…" (a subset of the teammate's enabled plugins; never more). Plugin writes in unattended runs are **held for approval** and appear on the Dashboard.
- **Background runs:** installs an OS scheduler entry (launchd on macOS, Task Scheduler on Windows, systemd user timer on Linux). The state line shows "Background runner installed" or "Couldn't install the background runner: {reason}" + "Retry".
- Dirty close asks "Discard changes?" (§2.11).

#### UI-52 · Schedule builder (CronInput) · RT
```
[ Daily | Weekly | Interval | Cron ]
Weekly:   [M][T][W][T][F] [S][S]   at [09:00]   Time zone [Europe/London ▾]
┌ Weekdays at 09:00 (Europe/London) ─────────────────────────────────┐
│ Next: Mon 29 Sep 09:00 · Tue 30 Sep 09:00 · Wed 1 Oct 09:00        │
│ Clocks go back on 26 Oct; runs stay at 09:00 local time.           │
└────────────────────────────────────────────────────────────────────┘
```
- **Daily:** "at {time}". **Weekly:** day toggles plus "at {time}"; shortcut chips "Weekdays", "Weekends". **Interval:** "Every [n] [minutes | hours]" (minimum 5 minutes), optional "Only between [08:00] and [20:00]". **Cron:** mono Input with the hint "minute hour day month weekday" and a "Cheat sheet" popover.
- **Resolved preview** (always visible, updates as you type): a plain-words summary, the next 3 run times in the chosen time zone, and a DST note when a clock change falls within the next 60 days.
- **Validation (inline error, preview hidden):** "Cron needs 5 fields; this has {n}." · "{value} isn't valid for {field}." · "Every {n} minutes is too often. The minimum is 5." · "This schedule never runs." Switching from Cron to a builder tab keeps the expression when it maps exactly, else asks "Replace your cron expression?".
- **Keyboard:** arrows move between segments and day toggles, Space toggles a day.

#### UI-53 · Run history · RT
- A Sheet lg opened from "Run history" or by clicking a row's result Badge. PageHeader shows the routine name, schedule and "Run now" / "Pause".
- **Timeline** rows: time, trigger ("Scheduled", "Run now", "Caught up after Tandem opened"), duration, result Badge, and "Open chat" (the fresh chat created for that run in the teammate's Chats tab). Failed rows expand to show the error and the failing stage ("starting the engine", "running", "plugin call held").
- Filter SegmentedControl "All | Failed". Retention note: "Keeps the last 100 runs. Chats stay until you delete them."
- **Empty:** "This routine hasn't run yet." + "Run now".

---

### 4.8 Plugins

Plugins connect once, app-wide, through `tandem-gateway`. Each teammate gets only the plugins you enable for it. Engines never see credentials; they live in the OS keychain.

#### UI-90 · Plugin catalog · PL
```
Plugins                                                        ⌕ Search plugins
Connect a service once. Then choose which teammates can use it.
[All] [Development] [Business] [Communication] [Research] [Media] [Growth]
CONNECTED (2)
┌ (Gh) GitHub ─────────────── ● Connected ┐ ┌ (Se) Sentry ─────────────── ● Connected ┐
│ Issues, PRs and code search.            │ │ Errors and releases.                    │
│ octo-shop · 3 teammates                 │ │ shop-prod · 1 teammate                  │
└─────────────────────────────────────────┘ └─────────────────────────────────────────┘
BROWSE
┌ (Li) Linear ────────────── [Connect] ┐ ┌ (No) Notion ──────────── [Connect] ┐ ┌ (Gm) Gmail ────── Preview ┐
│ Issues and projects.                 │ │ Pages and databases.               │ │ Read and draft email.     │
└──────────────────────────────────────┘ └────────────────────────────────────┘ └───────────────────────────┘
Credentials stay in your system keychain. Teammates reach plugins only through Tandem.
```
- Opened from the Agent rail footer "Plugins", Settings › Plugins, and UI-76 "Connect more plugins…".
- **Card (radius md, surface-2, 2–3 columns by width):** a letter glyph tile (like EngineGlyph; no vendor logos), name, one-line description, category, and a status: "● Connected" (done tone, with account label and "{n} teammates"), "Connect" (secondary Button), "Preview" (outline Badge; card dimmed; tooltip "Not ready yet. We'll let you know when it is."), "▲ Needs attention" (error tone; sign-in expired or revoked).
- Clicking a Connected card opens UI-92; a Connect card opens UI-91.
- **States:** search empty "No plugins match "{q}"." · a connection limit a token carries: Connect buttons show "At limit" with "This account's plugin connections are used up." · gateway down: Banner "The plugin gateway isn't running, so teammates can't use plugins." + "Restart gateway" / "View log" · loading Skeleton cards.
- **Keyboard:** / search · arrows move between cards · Enter open · 1–7 choose a category.

#### UI-91 · Connect flow · PL
Sheet md, title "Connect {service}". The body depends on the method.
```
OAuth                                           API key
① Find {service}'s sign-in      ✓               API key   [••••••••••••••••  👁]
② Register Tandem               ✓               Create a key at {service} › Settings ›
③ Sign in in your browser       ◌ waiting…      API keys. It needs {scope} access.
④ Save to keychain                              [Cancel]  [Test and connect]
⑤ Check the connection
[Cancel]  Didn't open? [Copy link]
```
- **OAuth:** a vertical step list with live state per step. The browser opens on start. A deep link returns to the app. Timeouts: "Still waiting after 5 minutes." + "Try again".
- **API key:** SecretInput (never logged; cleared on Cancel), vendor-specific hint, "Test and connect". An empty field cancels a previous attempt.
- **Local service** (e.g. a desktop app's MCP port): "Tandem looks for {service} on localhost:{port}." with state "Found" / "Nothing listening on {port}" + "Check again". If another program holds the port: "Port {port} is used by a different program ({name})."
- **Failure:** the failing step turns rose and a Banner shows "Couldn't connect: {step} failed." with the vendor's message untruncated in a CodeBlock, plus "Try again" and "Copy details". An existing connection is never changed by a failed reconnect.
- **Success:** "Connected as {account}" → step "Choose teammates": a checklist of teammates (none checked by default) with the note "You can change this any time." → "Done".

#### UI-92 · Connected plugin detail · PL, AG
```
(Gh) GitHub                                   ● Connected   [Reconnect] [⋯]
Signed in as octo-shop · OAuth · connected 12 Sep · 34 tools
[Teammates]  [Approvals]  [Activity]
TEAMMATES                                          Enabled for 3 of 5
│ (LD) Lead      ■ On                                                   │
│ (RH) Rhea      ■ On    Custom policy                                   │
│ (OB) Ops Bot   □ Off                                                   │
```
- Page in the main area (Sheet lg when opened from a teammate's settings). KeyValue: "Signed in as", "Method" ("Browser sign-in" / "API key" / "Local"), "Connected", "Tools", "Credentials" ("In your system keychain").
- **Teammates tab:** a Switch per teammate. Turning one off takes effect on its next turn; a turn preparing access is cancelled with Note "{service} was turned off for {name}."
- **Approvals tab:** UI-93 at plugin level.
- **Activity tab:** the last 50 gateway calls: time, teammate, action, risk class Badge, decision ("Ran", "Approved by you", "Denied", "Held"). Arguments are shown only when you expand a row, and payloads are never stored beyond 7 days.
- **⋯ menu:** Reconnect · Copy account ID · ─ · Disconnect… (danger). Confirm: "Disconnect {service}? All teammates lose access right away, turns using it will fail, and remembered approvals are cleared. The credential is removed from your keychain." → "Cancel" / "Disconnect".
- **Expired/revoked:** Banner "{service} sign-in expired. Teammates can't use it until you reconnect." + "Reconnect".

#### UI-93 · Approval policy by risk class · PL, AG, RT, TK
```
APPROVAL POLICY                                   Applies to: [All teammates ▾]
Read           Look things up                    [Run without asking ▾]
Write          Create, change or send            [Ask once per chat ▾]
Publish        Posts publicly or emails others   [Ask once per chat ▾]
Uses credits   Spends {service} credits          [Ask every time ▾]
Spend          Charges money or changes budgets  Ask every time  🔒
Delete         Deletes or revokes                Ask every time  🔒
Unknown        Actions Tandem can't classify     Ask every time  🔒
☑ Let Full access chats run writes without asking
☐ Let Full access chats publish without asking
Unattended runs (routines, tasks, mail) hold writes for you on the Dashboard.
```
- Rows map to the F-PL-06 risk classes: Read = `read`, Write = `write`, Publish = `external_publish`, Uses credits = `credit_consuming`, Spend = `spend`, Delete = `destructive`, Unknown = `unclassified`. The defaults are the ARCHITECTURE §11.7 decision table; a user may only make a class stricter. "Let Full access chats run writes without asking" is **on** by default (F-PL-06: write runs without asking in interactive Full when granted); "Let Full access chats publish without asking" is the per-plugin "Allow publish in Full" (off).
- Options per class: "Run without asking", "Ask once per chat" (= "Allow for this conversation", offered only for Write and Publish), "Ask every time". Defaults: Read = Run without asking; Write and Publish = Ask once per chat; Uses credits = Ask every time. **Spend**, **Delete** and **Unknown** are locked to "Ask every time" (🔒 tooltip: "Spending, deleting and unknown actions always ask."). Some connectors mark every write as "Ask every time"; their Write row is then locked with "{service} requires approval for every write."
- "Applies to" lets a teammate override the plugin-level policy (only stricter or equal for Spend/Unknown). Overridden teammates show "Custom policy" in UI-92.
- "Ask once per chat" remembers the exact action for the rest of that chat and is cleared on quit, sign-out or reconnect.
- **Plugin ApprovalCard (§2.24, kind plugin_action):**
  ```
  ┌─◆ Waiting for you ─ Rhea wants to use GitHub ──────────────────── 1m ─┐
  │ Action   Create pull request            Account  octo-shop               │
  │ Target   octo-shop/web · tandem/shop-12 → main                           │
  │ Cost     None                                                            │
  │ ☐ Allow "Create pull request" for Rhea for the rest of this chat        │
  │ [Approve] [Deny]                                    [Deny and explain…]  │
  └──────────────────────────────────────────────────────────────────────────┘
  ```
  [Approve] = Allow once; ticking the checkbox first = "Allow for this conversation" (F-PL-07; shown only for `write` and `external_publish`, cleared on quit, sign-out or reconnect). Spend cards add "Cost {amount} {currency}" in danger tone, re-check the amount before running ("The amount changed from {a} to {b}. Review again."), and never offer the "for the rest of this chat" checkbox. Held drafts from unattended runs render as the same card on the Dashboard with "Held · from routine {name}".

#### UI-94 · Plugins in the composer · PL, AG
- When a teammate chat mentions work a connected-but-disabled plugin could do, or the user types `@{plugin}`, a strip appears above the composer: "{name} can't use {service} yet." + "Allow {service} for {name}" (applies from the next message) + ×. Dismissals are remembered per chat.
- Plugin tool calls render as ToolCallBlock "Called {service} · {action}" with the risk class Badge. Denied calls render "Denied by you" in fg-subtle, never as errors.


---

<!-- ui_part4.md: continues UI_SPEC after ui_part3 (§4.3–4.8). Same format and rules as ui_part2: IDs, wireframes, labels (verbatim), states, interactions, acceptance. Components and tokens are from §1–§2 only. -->

### 4.9 Dashboard

One live wall of every agent in every mode: Code panes, Threads and Teammate chats. It is fed by the status bus (one store, one status per session) and never by old notifications: a notification can't override the current status.

**Status model** (shared by the Dashboard, rail, status bar and Notifications):

| Status | Meaning | Dot (§1.3) | Sort inside group |
|---|---|---|---|
| Waiting | An approval, question or input is pending | ◆ amber, single pulse on entry | Oldest first (longest wait on top) |
| Working | The agent is producing output or running a tool | ●●● sky, pulsing | Longest running first |
| Done | A turn finished and you haven't looked yet | ✓ green | Newest first |
| Idle | Open, nothing happening, already seen | ○ slate | Last activity, newest first |
| Failed | Crashed, auth failure, limit hit | ▲ rose | Newest first; shown only when n > 0 |

"Done" becomes "Idle" when the user focuses that session for 2 s, or chooses "Mark as seen".

#### UI-42 · Dashboard (sidebar tab) · DB, CD, TH, AG
```
┌ Dashboard ◆2 │ Browser │ Files │ Tasks │ +              [⤢] [📌] ┐
│ 14 agents                                                          │
│ ◆ 2 waiting · ●●● 5 working · ✓ 3 done · ○ 4 idle                 │
│ ▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰  (stacked status bar)             │
│ [⌕ Search agents…                    ]  Group: [Status|Project] │
│ (All 14) (Waiting 2) (Working 5) (Done 3) (Idle 4)                 │
│ WAITING FOR YOU · 2                                                │
│ ┌◆─(C) Fix auth guard ───────────────────── Waiting · 2m ─ ⋯ ×┐   │
│ │ Claude Code · shop · ⎇ tandem/fix-auth · Work                      │   │
│ │ Wants to run: pnpm prisma migrate reset                        │   │
│ │ [Allow once] [Deny] [Open]                                     │   │
│ └────────────────────────────────────────────────────────────────┘   │
│ WORKING · 5                                                        │
│ ┌●──(X) e2e billing ─────────────────────── Working · 14m ─ ⋯ ×┐   │
│ │ Codex · shop · ⎇ tandem/e2e · Personal                             │   │
│ │ Ran pnpm test (exit 1, 22s)                                    │   │
│ └────────────────────────────────────────────────────────────────┘   │
│ IDLE · 4                                       [Close idle (4)]    │
└────────────────────────────────────────────────────────────────────┘
```
- **Header:** the total count in `text-xl` semibold ("{n} agents", "1 agent"), the subtitle with the four counts (each is a button that sets the filter), and a 6px stacked bar in the four status colours (proportional widths, each segment with an `aria-label` like "2 waiting"). Failed appears in the subtitle and bar only when n > 0.
- **Controls:** a search Input "Search agents…", a SegmentedControl "Status | Project", and filter Chips with counts ("All", "Waiting", "Working", "Done", "Idle", "Failed"). One chip is active at a time.
- **Agent card** (`AgentCard`, surface-2, radius md, 1px border; waiting cards use a 1px `status-waiting` border):
  - Line 1: StatusDot, EngineGlyph or Avatar (teammate chats), title (truncate), then right-aligned "{Status} · {elapsed}" in tabular-nums, the ⋯ menu and × (hover or focus only).
  - Line 2 (fg-muted, text-sm): "{Engine} · {workspace or teammate} · ⎇ {branch} · {account}". Omit missing parts; the account only when its engine has more than one (D-2620).
  - Lines 3–4 (optional, fg-subtle, up to 2 lines, passed through the shared secret redactor): the latest activity: the pending request ("Wants to run: {cmd}", "Asks: {question}"), the last ToolCallBlock summary, or the start of the last message. The setting "Show status previews on Dashboard cards" (on) controls it.
  - Waiting cards show compact ApprovalCard actions for kinds `command`, `file_write` and `question` ("Allow once" / "Deny" / "Open"; questions show only "Answer…", which jumps). High-risk requests show only "Open", so the full card is reviewed in context.
  - Badges: "Auto" (accent-outline) and "Remote · {host}". The "Lite" badge is pane-only and never appears on cards.
- **Grouping:** by Status (group order Waiting, Working, Done, Failed, Idle), or by Project (workspaces in rail order, "Teammates" and "No folder" last, each group header showing its own mini counts). Group headers are collapsible 2xs caps rows; collapse state persists.
- **Search:** matches title, teammate, engine, workspace, branch, account and the last activity line. Every word must match (AND), case- and accent-insensitive. Esc clears the query first, then blurs. Counts in the header stay global; chip counts show matches ("Idle 2 of 4").
- **Close idle:** a secondary sm Button in the Idle group header, "Close idle ({n})". When a search or filter is active it acts on matches only and reads "Close {n} matching idle".
  - ConfirmDialog: title "Close {n} idle agents?", body "Their conversations are saved and can be resumed from Saved sessions. Working, waiting and done agents stay open.", actions "Cancel" / "Close {n} agents" (primary, not danger: nothing is lost).
  - Result toast: "Closed {n} idle agents" with action "Show saved sessions". Plain shells aren't agents and are never closed by this.
- **Card ⋯ menu:** "Open" · "Open in Thread" (chat engines) · "Hand off to…" (UI-13) · "Mark as seen" (done) · "Copy session ID" · "Stop agent" (working; danger) · ─ · "Close" (danger).
- **Close (×):** idle/done closes at once with the toast "Closed {title}" and a "Resume" action; working or waiting opens the PaneChrome confirm "Stop {title}?" (§2.20).
- **Jump to pane** (click a card, Enter, or "Open"):
  1. Switch to the owning mode, workspace and tab; restore a maximized sibling if it hides the target; expand its rail row.
  2. Focus the pane, thread or chat and scroll the transcript to the pending item if waiting.
  3. **Flash:** a 2px accent outline pulses twice over 600ms on the pane (reduced motion: a static outline for 1s). Screen readers hear "Focused {title}".
  - The sidebar stays open unless it is floating (<1280px), in which case it closes.
- **States:**
  - Loading (first 150ms+): 3 Skeleton cards.
  - Empty: EmptyState icon `layout-dashboard`, title "No agents running", body "Start one in Code, Thread or Agent mode.", actions "Launch agents…" (opens UI-12) / "New thread".
  - Filter empty: "Nothing matches "{q}"" + "Clear search".
  - Status bus disconnected: Banner warning "Live status paused. Reconnecting…"; cards keep their last status, dimmed, with "as of {time}".
- **Acceptance:** status changes reach the card within 500ms of the bus event (F-DB-01); "Close idle" with a search active closes only matching idle agents; jumping to a pane in a hidden mode switches mode and focuses it in one action; a resolved approval removes the card from Waiting even if its notification is unread.

#### UI-43 · Dashboard full view · DB
```
┌ Dashboard ── 14 agents · ◆2 ●5 ✓3 ○4 ── [⌕ Search agents…] [Status|Project] ── [Close idle (4)] [Exit full view Esc] ┐
│ WAITING · 2                     WORKING · 5                                                                          │
│ ┌card──────────┐ ┌card──────────┐ ┌card──────────┐ ┌card──────────┐                                                   │
│ └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘                                                   │
│ DONE · 3 ...                                                                                                         │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```
- Opened by [⤢] in the tab strip, `Mod+Alt+Shift+D`, or Conductor. It is the sidebar's full-screen mode (F-SH-10): it overlays the main area without resizing it, so no PTY resizes.
- Cards flow in a responsive grid, min 280px, max 4 columns at ≥1440px; groups become full-width rows. The header collapses to one 48px row.
- Keyboard: arrows move between cards spatially, Enter jumps (and exits full view), Esc exits full view (first clearing search).
- Same states and actions as UI-42.

---

### 4.10 Notifications

#### UI-40 · Notifications popover · DB
```
┌ Waiting on you ───────────────── [Mark all read] [⚙] ┐   400 × ≤560
│ (Waiting on you 2) (All)                     Snooze ▾            │
│ TODAY                                                       │
│ ◆ Fix auth guard needs approval                  2m  •      │
│   Claude Code · shop — pnpm prisma migrate reset            │
│   [Allow once] [Deny] [Jump to pane]                        │
│ ✓ e2e billing finished                          14m  •      │
│   Codex · shop — 3 files changed                            │
│ ⟳ Nightly deps check is ready for review         1h         │
│   Routine · Rex                                             │
│ YESTERDAY                                                   │
│ ▲ Plugin GitHub needs you to reconnect          21h         │
└─────────────────────────────────────────────────────────────┘
```
- Titled "Waiting on you" (F-DB-02). Opens from the title-bar bell (anchored, surface-3, shadow-2) or `Mod+Shift+N`. The badge counts unread items; it is amber when any unread item needs you, neutral otherwise, and shows "99+" above 99.
- **Tabs (pill):** "Waiting on you" (unresolved waiting items only; the default when n > 0) and "All" (the last 7 days, max 200, older ones pruned).
- **Item row:** a type icon (StatusDot shape where applicable), title (text-base medium), a source line (fg-muted), relative time, and an unread dot. Up to 2 inline actions from the source (approvals use the §2.24 defaults). Hover/focus reveals "Mark read" and "Dismiss".
- **Click / Enter** marks the item read and jumps to its source using the Dashboard jump (UI-42). An item whose source no longer exists shows "This session was closed" and offers "Resume" when possible.
- **Resolved items:** an approval answered elsewhere collapses to "✓ Allowed · you · 10:42" and leaves "Waiting on you" immediately.
- **Coalescing:** more than 3 items of the same type within 10s merge into one row ("4 agents finished") that expands inline.
- **Snooze ▾:** "30 minutes", "1 hour", "Until tomorrow 9:00", "Until I turn it back on". While snoozed, the bell shows a small crescent and system notifications and sounds stop; in-app items still collect. Waiting items still badge.
- **Footer link:** "Notification settings" opens UI-41.
- **Empty:** icon `bell`, title "You're all caught up", body "Approvals, finished turns and routine results show up here."
- **Item types:**

  | Type | Title pattern | Source line | Actions | Default channels |
  |---|---|---|---|---|
  | Approval | "{title} needs approval" | "{Engine} · {workspace} — {summary}" | per kind + "Jump to pane" | In-app, system, sound |
  | Question | "{title} has a question" | first line of the question | "Answer…" | In-app, system, sound |
  | Turn done | "{title} finished" | "{n} files changed" or last tool summary | "Open" | In-app, system (background only) |
  | Turn failed | "{title} stopped with an error" | the error reason | "Open", "Restart" | In-app, system |
  | Usage limit | "{account} reached its limit" | "{Engine} · resets {time}" | "Choose account" | In-app, system |
  | Routine result | "{routine} is ready for review" | "Routine · {teammate}" | "Open" | In-app, system |
  | Routine failed / skipped | "{routine} didn't run" | reason ("Tandem was closed", "Routines are turned off for this account") | "Open routine" | In-app |
  | Task | "{teammate} finished {task}" / "{task} is ready for your review" | "Task #{id}" | "Open task" | In-app, system |
  | Lane cleanup | "{n} finished worktrees can be cleaned up" | "{workspace}" | "Review…" | In-app |
  | Teammate mail | "{from} sent {to} a message" | first line | "Open chat" | In-app |
  | Plugin | "{plugin} needs you to reconnect" | account label | "Reconnect" | In-app, system |
  | Credits (a hosted build, signed in) | "Credits are running low" / "You're out of credits" | "{n} left · resets {date}" | — | In-app |
  | Update | "Tandem {version} is ready" | "Restart when you're ready" | "Restart" | In-app |

- **System notifications** (tauri notification plugin): title = item title, body = source line only (no message text unless "Include message preview" is on). Clicking one focuses the window and performs the jump. Windows needs the AppUserModelID registered on the Start Menu shortcut first.
- **Acceptance:** clicking a system notification for a pane in another mode lands focus on that pane; approving from the popover resolves the ApprovalCard in the transcript; snooze suppresses system notifications and sound but not in-app items.

#### UI-41 · Notification settings · DB
Rendered as Settings › Notifications (UI-102); defined here.
```
Notifications
  System notifications                               [●━ on]
  Only when I can't see the agent                    [●━ on]
  Include message preview                            [━○ off]
  Bounce Dock icon / Flash taskbar (background only) [●━ on]
  Sound                               [Chime ▾] [▶]  ──●── 60%
  Quiet hours                 [━○ off]  22:00 → 08:00

  WHAT TO NOTIFY           In app   System   Sound
  Needs approval / question  ☑        ☑        ☑
  Turn finished              ☑        ☑        ☐
  Turn failed                ☑        ☑        ☐
  Usage limits               ☑        ☑        ☐
  Routines                   ☑        ☑        ☐
  Tasks and teammate mail    ☑        ☐        ☐
  Plugins, credits, account  ☑        ☐        ☐
```
- "Only when I can't see the agent": system notifications fire when Tandem isn't focused or the source isn't visible (F-DB-03). Off sends them for every event.
- Sounds (Tandem's own, bundled, synthesized short tones): "Chime", "Tick", "Pluck", "None". ▶ previews without blocking.
- The "In app" column for "Needs approval / question" is locked on (disabled checkbox, tooltip "Approvals always appear in Tandem").
- If the OS has denied notifications, a Banner: "Your system is blocking Tandem's notifications. [Open system settings]".
- Quiet hours behave like Snooze during the window.

---

### 4.11 Settings

#### UI-100 · Settings shell · SH
```
┌ ‹ Back (Esc) ──────────┬─────────────────────────────────────────────────────────┐
│ [⌕ Filter settings    ]│ Dictate                                                 │
│ General                │ How your speech becomes text.                           │
│ Appearance             │ ─────────────────────────────────────────────────────── │
│ Notifications          │ ENGINE                                                  │
│ Terminal               │ Transcribe with      [ Local | Cloud ◈ ]                │
│ Engines                │ Model                Parakeet v3 · 670 MB  [Manage…]    │
│ Accounts               │ ...                                                     │
│ Teammates              │                                                         │
│ Dictate                │                                                         │
│ Conductor              │                                                         │
│ Plugins                │                                                         │
│ Remote                 │                                                         │
│ Privacy & Telemetry    │                                                         │
│ Shortcuts              │                                                         │
│ Account                │                                                         │
│ Updates          •     │                                                         │
│ Advanced               │                                                         │
└────────────────────────┴─────────────────────────────────────────────────────────┘
```
- `Mod+,`, the account menu or the credits pill open Settings. The left rail becomes the nav (rail width, `role="navigation"`, "Settings pages"); the mode switcher hides. Esc (outside a field) closes and restores the previous mode and focus. Settings reopens on the last page.
- Content: max-width 720, PageHeader (title + one-line subtitle), groups with 2xs caps labels, one SettingsRow per control.
- **Filter:** narrows nav items to pages with matching labels/descriptions/keywords and highlights matched rows (accent-soft background for 2s). "No settings match "{q}"".
- **Saving:** every control applies immediately. When the effect isn't visible, a 1s "Saved" hint appears beside the control. Multi-field forms (SSH host, formatting instructions, persona) have explicit "Save" / "Cancel". Failed writes revert the control and toast "Couldn't save that setting. Try again." with "Retry".
- **Hosted rows** (features that need Vibezeo's servers): in a build without accounts, the Privacy rows for them (Name threads with AI, Allow Auto…, Share dictionary hints, Find work's ranking, dictionary sync) are absent, and Settings › Conductor shows only "Comes with Vibezeo accounts" with its sentence (D-2913). Nothing shows a plan badge or an upgrade link (D-869, D-2903).
- **Page ↔ PRODUCT_SPEC F-SH-04 mapping:** Voice is split into Dictate and Conductor; Agents is Teammates; Account is Account; About content lives in Updates (version, notes) and Advanced (licences, logs). Updates is also reachable from General.
- **Acceptance:** typing "scroll" shows Terminal and highlights "Scrollback lines"; every key round-trips a relaunch; Esc restores focus to the element that opened Settings.

Each page below lists its controls as **Label · control · default · notes**. Labels are final copy.

#### UI-101 · General · SH
- "Launch at login" · Switch · off · starts hidden in the tray when background routines or Dictate are on.
- "Keep running in the tray when the window closes" · Switch · on if Dictate or routines are on.
- "Open links in Tandem" · Switch · on · terminal and chat links open in the sidebar Browser.
- "Auto-save edited files" · Switch · on.
- "Confirm before pasting multiple lines into a shell" · Switch · on.
- "Resume agents automatically after restart" · Switch · off · one at a time, newest first.
- "Language" · Select · English · only English in v1; the catalog is ready.
- "Run setup again" · Button · opens UI-04.

#### UI-102 · Appearance · SH
- "Theme" · three preview cards "Dark" / "Light" / "Follow system" · Dark.
- "Terminal theme" · Select "Follow app" / "Night" / "Day" / "High contrast" · Follow app.
- "Interface text size" · SegmentedControl "Small" / "Default" / "Large" (12/13/14px base).
- "Terminal font" · Combobox (monospace fonts found) · JetBrains Mono; "Terminal font size" · number 11–18 · 13.
- "Density" · "Compact" / "Comfortable" (28 vs 32px rows).
- "Reduce motion" · Select "Follow system" / "On" / "Off" · Follow system.
- "Show status previews on Dashboard cards" · Switch · on.
- Preview strip: a mini pane with PaneChrome, a chat bubble and a terminal sample, live.

#### UI-103 · Notifications · DB
See UI-41.

#### UI-104 · Terminal · CD
- "Default shell" · Select · login shell (mac/Linux); Windows lists "PowerShell 7", "Windows PowerShell", "Command Prompt", then "WSL: {distro}" rows.
- "Cursor" · "Bar" / "Block" / "Underline" · Bar; "Blinking cursor" · off.
- "Scrollback lines" · number 1,000–100,000 · 10,000.
- "Copy on select" · off. "Option key acts as Meta" (mac) · on.
- "Shell integration" · per-shell Switches (bash, zsh, fish, PowerShell) · on.
- "Shortcut priority when a terminal is focused" (Windows/Linux) · "Terminal first" / "Tandem first" · Terminal first (§5).
- "GPU rendering" · "Automatic" / "Off" · Automatic (off forces lite mode for all panes).

#### UI-105 · Engines · EN
Embeds the engine list from UI-05 (row states, Install, Sign in, Rescan as "Check again"). Per engine, an expandable row: "Path" (read-only with "Reveal"), "Version", "Extra launch arguments" (monospace Input, validated as a shell-word list), "Newline key in the TUI" (read-only from the registry). This page is specified in §4.4 as UI-33 (with the install flow UI-34); it is listed here only for nav order.

#### UI-106 · Accounts · AC
Specified in §4.4 as UI-35 (usage page UI-37). Summary: accounts per engine with nickname, provider-home path, usage meters and "Default" radio; "Add account…", "Rename", "Sign out", "Remove…" (danger confirm). There's no limit on accounts per engine (D-2900).

#### UI-107 · Teammates · AG
- "Default access for new chats and threads" · Select "Read only" / "Edits" / "Full access" · Edits (Full access asks the §2.23 confirm once).
- "Default engine for new teammates" · engine + account select.
- "Allow teammates to write skills" · on.
- "Learn after each turn" (update NOTES.md using the teammate's own engine) · on.
- "Pause all teammate mail" · off · a global stop switch; while on, a Banner shows on the Agent rail.
- "Routine runs use" · "Read only" / "Edits" · Read only · unattended runs never inherit Full access (copy under the control: "Routines never run with Full access.").

#### UI-108 · Dictate · DI
Defined in UI-62.

#### UI-109 · Conductor · CN
Defined in UI-67.

#### UI-110 · Plugins · PL
Specified in §4.8 (catalog UI-90, connect UI-91, detail UI-92, approval policy UI-93). This page adds:
- "Ask before any write action" · on · locked on for spend actions.
- "Gateway port" · read-only "127.0.0.1:{port}" with "Copy MCP config".
- "Disconnect all…" · danger Button.

#### UI-111 · Remote · RM
- **SSH hosts:** the UI-14 list (compact), "+ Add host".
- **WSL** (Windows only): "Detect distributions" list with "Use for plain terminals" radio, and "Run agents inside WSL" Switch per distro.
- **Phone** (pairing over the local network needs no account; the hosted relay comes with Vibezeo accounts): "Pair a phone" Button → QRCode dialog ("Scan with the Tandem app on your phone. The code expires in 2 minutes."); paired devices list with last seen and "Revoke"; grants per device: "Manage sessions", "Send terminal input", "Write files", "Git actions" (all off by default).
- "Reset remote access…" · danger · revokes every pairing.

#### UI-112 · Privacy & Telemetry · TL
All off by default. A Callout at the top: "Tandem sends nothing off this device unless you turn it on here. Prompts, file contents and tokens are never included."
- "Share anonymous usage statistics" · off · helper: "Feature usage counts and performance timings. Turning it off deletes the local analytics ID."
- "Send crash reports" · off · helper: "Stack traces and app version. No prompts, file contents or terminal output."
- "Allow Auto to send terminal output to Tandem's servers" · off · helper: "Auto needs recent terminal output to decide the next prompt. Off means Auto is unavailable."
- "Share dictionary hints with cloud transcription" · off.
- **"What leaves this machine"** (F-TL-07): a live table, one row per hosted feature (Cloud dictation, Enhance, Conductor, Find work, Auto, Usage statistics, Crash reports) with columns "Status" (On/Off), "What it sends" ("Audio of each take", "Transcript text", "Last 200 terminal lines, redacted"…) and "Where" ("Tandem API → {provider}"). Rows update live when a feature is toggled anywhere.
- "Export my data…" (zip of settings, memory, skills, tasks, routines as JSON; no keychain secrets) · "Delete local data…" (danger, typed "delete"; a checkbox "Also remove engine sign-ins", off) · "Delete my account…" (opens the web portal; deletion completes within 30 days).
- Team members whose org sends audit metadata see a Callout: "Your organization receives a record of Full access use and plugin write approvals (no content)."
- Onboarding asks about both telemetry toggles once, both off (F-TL-01).
- **Org policy:** any control forced by a Team admin (telemetry, max access, Auto, Conductor, phone) is disabled with the reason "Set by your organization".

#### UI-113 · Shortcuts · SH
```
[⌕ Search shortcuts]                       [Reset all to defaults]
GLOBAL (work outside Tandem)
  Dictate (hold)                 [ Fn ]                [Record…] ↺
  Conductor call                 [ ⇧⌘Space ]           [Record…] ↺
APP
  Command palette                ⌘K                    [Record…] ↺
  ...
```
- One table per §5 group; each row has a HotkeyRecorder. Conflicts show inline: "Already used by {action}. [Swap] [Cancel]". OS-reserved combos are refused: "Your system uses this shortcut."
- APP rows (F-SH-09 P2) list every registry command: its shortcut as Kbd ("None" when it has none), [Record…] and, once changed, ↺ "Reset to default".
  - Recording: the button reads "Press keys…" and takes the next combo. Esc or leaving the button cancels, and Backspace sets "None".
  - A combo without Ctrl, Alt or ⌘ (F-keys aside) is refused: "Hold Ctrl or Alt with the key." (macOS: "Hold ⌘, ⌃ or ⌥ with the key.").
  - On Windows and Linux under "Terminal first", a row whose shortcut a terminal keeps says "In a terminal, this key goes to the shell."
  - "Reset all to defaults" asks first ("Reset every shortcut?").
  - The search box matches action names and keys. With no match: "No shortcuts match "{query}"."
- Global rows show a status chip: "Active", "Needs permission" (→ UI-134), "Unavailable on Wayland — use the system shortcut settings" (with the `tandem://dictate/toggle` command to copy), "Paused".
- "Keyboard shortcuts" link: opens the cheatsheet, a dialog with every shortcut in effect by §5 group and a search box; `Mod+/` also opens it (D-805: a dialog, not a printable page, until printing is checked on every platform).

#### UI-114 · Account · BE
```
Signed in as ana@example.com                    [Sign out]
Credits     ◈ 8,420 of 10,000 this month · resets 12 Oct
            Monthly 6,420 · Purchased 2,000
            [Refresh]
```
- Shows who's signed in, "Managed by {org}" for an organization, and the credits hosted features use (CreditMeter, KeyValue), with "Refresh". No plan, status, trial or billing (D-869, D-2904). A build without accounts says "Everything on this computer is free and runs without an account. Hosted features, like Conductor and cloud Dictate, come with Vibezeo accounts."
- Offline-mode users see "Using Tandem offline · {n} days left before sign-in is needed" with "Sign in".

#### UI-115 · Updates · TL
- "Version" · "Tandem {version} ({build})" + "Release notes ↗".
- "Check for updates automatically" · on. "Channel" · "Stable" / "Beta" · Stable.
- "Check now" · Button → "Checking…" → "You're up to date" / "Downloading {v}… 42%" (ProgressBar) / "{v} is ready. [Restart to update]" / error per UI-130.
- Linux deb: the controls are replaced by "Updates are managed by your package manager. [Download page ↗]".

#### UI-116 · Advanced & Diagnostics · TL
- "Copy diagnostics" · Button → toast "Diagnostics copied. They contain no prompts, file contents or tokens." (bundle per F-SH-11).
- "Open logs folder" · "Log level" · "Normal" / "Verbose (24h)" · Normal.
- "Show the in-app performance HUD" · off (fps, PTY throughput, WebGL pool "6 of 8").
- "Data folder" · read-only path + "Reveal" (respects `TANDEM_HOME`).
- "Reset window layout" · "Clear staging files" (shows size) · "Clear cached models…" (links to UI-63).
- "Stop orphaned agent processes on launch" · "Ask" / "Always" / "Never" · Ask.
- "Open-source licences" · opens the generated notices list.
- Dev builds only: "Feature flags" table, "Mock backend" switch, "Open /__gallery".

---

### 4.12 Dictate

Hold a key, speak, release: the text lands where the cursor was, in Tandem or any other app. Local transcription is the default and free. Cloud is opt-in and hosted: it comes with Vibezeo accounts. There is never a silent fallback between the two (F-DI-05).

#### UI-60 · Dictate widget (HUD) · DI
```
Listening      ( ◌ )  Listening                         0:04   [×]
Hands-free     ( ◌ )  Hands-free · press Fn to stop     0:12   [×]
Transcribing   ( ◌ )  Transcribing…
Enhancing      ( ◌ )  Enhancing…                        ◈ 3
Inserted       ✓  Inserted · 42 words        [Enhance ◈3] [Polish ◈3]
Copied         ⧉  Copied — press ⌘V to paste
Reconnecting   ( ◌ )  Reconnecting…
Error          ▲  Your microphone isn't delivering audio     [Settings]
```
- A separate transparent always-on-top window holding a pill (radius full, surface-3, shadow-3), 280px wide (360 for errors), 40px tall, with a 28px WaveformRing on the left. It follows the theme.
- **Position:** bottom-centre of the display under the pointer, 48px above the Dock/taskbar; draggable. Settings: "Follow cursor across displays" (not on Wayland) and "Lock position". In-app composer dictation doesn't use the widget: the composer's 🎙 button shows the ring inline.
- **Visibility** (setting): "Always" (idle shows a small logo capsule) / "While recording" (default; hides 1s after insertion) / "Hidden". **Style:** "Logo and text" / "Logo" / "Mono".
- **States:**

  | State | Ring | Text | Duration |
  |---|---|---|---|
  | Idle (Always only) | idle | "Tandem" capsule | — |
  | Listening (hold) | listening | "Listening" + timer | while held |
  | Hands-free | listening + 🔒 glyph | "Hands-free · press {key} to stop" | until pressed |
  | Transcribing | thinking | "Transcribing…" | until result |
  | Enhancing / Polishing | thinking | "Enhancing…" / "Polishing…" + credit cost | until result |
  | Inserted | ✓ icon | "Inserted · {n} words" | 1s, then hides (While recording) |
  | Copied (fallback) | ⧉ icon | "Copied — press {Mod}V to paste" | 4s |
  | Reconnecting | thinking | "Reconnecting…" | cloud only |
  | Error | error | per table below; full diagnostic in the tooltip | until dismissed or 6s |

- **Trigger modes** (F-DI-01):
  - **Hold** (default): hold the key to record; release stops and transcribes. A tap shorter than 150ms is ignored. A stop is accepted only after a real key-up.
  - **Hands-free latch:** double-tap the hold key; tap once to stop.
  - **Toggle** (optional binding): press to start, press again to stop. Modifier-only combos are rejected with "Toggle needs a key that isn't only modifiers."
  - Esc while recording, or ×, discards the take: no transcription call, nothing inserted.
  - A take is capped at 10 minutes; at 9:30 the timer turns amber and reads "30s left". 2s of silence yields "No speech detected" and no insertion.
- **Insertion (fail-closed, F-DI-06):** the target app and field are captured when recording starts.
  - Verified target → paste, then restore the previous clipboard after 500ms (macOS, Linux). Windows doesn't restore it, so clipboard-history apps can't capture it twice; the setting's helper says so.
  - Terminal targets get control characters stripped and never an Enter.
  - Target lost, changed, or paste failed → the text stays on the clipboard and the Copied state shows.
  - A late transcript from an older take, or a cancelled take, never pastes anywhere. Tandem's own windows are never external targets.
- **Quick actions after a take** (hosted; come with Vibezeo accounts): "Enhance" (rewrite as a clear prompt for a coding agent) and "Polish" (clean grammar and filler). If the target still has focus, the result replaces the inserted text (undo + paste); otherwise it is copied. The first use shows a one-time disclosure (UI-62).
- **Errors:**

  | Condition | Text | Action |
  |---|---|---|
  | No microphone | "No microphone found" | "Settings" |
  | Permission denied | "Tandem can't use the microphone" | "Allow access" (→ UI-134) |
  | Mic silent, recovery failed | "Your microphone isn't delivering audio" | "Settings" |
  | Switched to backup mic | "Microphone stopped responding — switched to backup" | — |
  | Nothing heard | "No speech detected" | — |
  | Secure input (mac) | "Secure input is on (e.g., a password field) — dictation paused" | — |
  | No local model | "Download a model to dictate locally" | "Download" (→ UI-63) |
  | Cloud with no account that carries it | "Cloud transcription comes with Vibezeo accounts" | "Use local for this take" |
  | Out of credits (402) | "Out of credits for cloud transcription" | "Use local for this take" |
  | Cloud daily cap | "You've reached today's cloud limit — switch to Local" | "Use local for this take" |
  | Offline, cloud engine | "You're offline" | "Use local for this take" |
  | Hotkey failed | "Keyboard hook failed — Dictate unavailable" | "Fix…" (→ UI-113) |
  | Elevated target (Windows) | "{app} is running as administrator, so Tandem can't type into it. Copied instead." | — |

- **Sounds:** soft start and stop chimes synthesized in Rust (setting "Recording sounds", on), so they play with the widget hidden.
- **Screen reader:** the ring label announces "Listening", "Transcribing", "Inserted 42 words" (polite); errors are assertive.
- **Acceptance:** switching to another window mid-take ends in Copied, never a paste into the new window; a 100ms tap never records; Esc discards without touching the clipboard; with visibility "While recording" the widget hides 1s after insertion.

#### UI-68 · In-app composer dictation · DI, TH, AG
- The 🎙 button in every Composer and the command bar: click toggles hands-free, press-and-hold records while held. Holding the global Dictate key (Fn / Ctrl+Win / Ctrl+Alt) while the composer has focus does the same, as does ⌥Space on macOS (§5; `Alt+Space` is not bound on Windows/Linux).
- While recording, the button becomes a 28px WaveformRing and the placeholder reads "Listening…". Text is inserted **at the caret, directly**: no clipboard, no paste.
- Same engine, model, dictionary and formatting as global Dictate. While a Conductor call owns the mic, the button is disabled with the tooltip "Conductor is listening".

#### UI-62 · Dictate settings (Settings › Dictate) · DI
```
┌ Ready to dictate · Hold Fn · Local · Parakeet v3 ────────────── [Try it] ┐
│ 63,410 words · 5h 48m · 2,190 takes · 164 wpm      [History & stats…]  │
└─────────────────────────────────────────────────────────────────────────┘
ENGINE
  Transcribe with      [ Local | Cloud ]
  Model                Parakeet v3 · 670 MB · Active        [Manage models…]
  Language             [ Auto-detect ▾ ]
MICROPHONE
  Microphone           [ System default (MacBook Pro Microphone) ▾ ]  ▁▃▅▂
  Backup microphone    [ None ▾ ]
  Mic boost            [ Off | 2× | 4× | 6× | 8× ]
SHORTCUT
  Hold to talk         [ Fn ]  ● Active                          [Change]
  Toggle (optional)    [ Not set ]                               [Set…]
  Double-tap to lock hands-free                                  [●━]
INSERTION
  Where text goes      [ Into the focused app ▾ ]
  Pause other audio while you talk                               [━○]
  Recording sounds                                               [●━]
WIDGET
  Show widget          [ While recording ▾ ]  Style [ Logo and text ▾ ]
  Follow cursor across displays [●━]     Lock position [━○]
ENHANCE
  Show Enhance and Polish after each take                        [●━]
  Enhance every take automatically                               [━○]
  Formatting instructions  [ e.g. "Use lowercase in Slack." ] 0/2,000  [Per app…]
DICTIONARY · HISTORY
  Dictionary (14 rules)   [Open…]      Keep history [ Forever ▾ ]  [Open…]
```
- **Home card** (top): "Ready to dictate" with the key and an engine chip ("Local · Parakeet v3" / "Cloud · Auto-detect"), the four stats, and "Try it". Until the shortcut is verified it reads "Finish Dictate setup" with "Set up" (runs the F-DI-11 steps).
- **Transcribe with:** Local helper "Free and private. Works offline."; Cloud helper "More languages. Audio is sent to Tandem's transcription service and uses about 8 credits a minute (daily cap 120 minutes)." The first switch to Cloud opens a consent Dialog: title "Send audio to the cloud?", body "While Cloud is on, each take's audio goes to Tandem's transcription service and isn't kept after transcription. Your history stays on this device.", actions "Cancel" / "Use cloud". With no account that carries it, the segment stays on Local with "Cloud transcription comes with Vibezeo accounts".
- **Language:** "Auto-detect" first, then the current engine's languages; unsupported ones are disabled with "Not supported by {model}". Cloud adds "Share dictionary hints with cloud" (off).
- **Microphone:** DeviceSelect with a live meter. Loopback and virtual devices are never auto-selected, and a Bluetooth headset mic is never auto-selected when another mic exists (helper: "Keeps headphones out of low-quality call mode."). Changing mics mid-take: "Switches after this take."
- **Shortcut:** HotkeyRecorder; defaults macOS "Fn" (hint "Fn not working? Use Control+Option"), Windows "Ctrl+Win", Linux "Ctrl+Alt". Mouse 4 and 5 are allowed. Status chip: "Active" / "Needs Input Monitoring [Grant]" / "Unavailable on this system" / "Paused" / "Starting…" / "Not running". Wayland without the portal shows a Callout: "Bind this command to a shortcut in your system settings: `xdg-open tandem://dictate/toggle`" with Copy.
- **Where text goes:** "Into the focused app" / "Copy to clipboard".
- **Enhance disclosure** (one time, on first use and as a Callout here): "Enhance and Polish send the transcript text (not audio) to Tandem's AI service, even when you transcribe locally. 3 credits each." Any failure inserts the raw transcript and toasts "Enhance didn't work, so your exact words were used." Input over 5,000 characters is refused before sending: "That's too long to enhance (5,000 characters max)."
- **Formatting instructions** apply to every take; "Per app…" opens a list of app-profile overrides.

#### UI-63 · Model manager · DI
Sheet md "Dictation models".
```
   Model                          Size      Languages    Status
◉  Parakeet v3   RECOMMENDED      670 MB    25 European  Active                        [Remove]
○  Whisper Turbo                  575 MB    ~100         [Download]
○  Whisper Base (English)         145 MB    English      Downloaded                    [Remove]
○  Distil-Whisper (English)       1.5 GB    English      ▰▰▰▱▱ 58% · 9 MB/s · 1 min  [Pause] [×]
──────────────────────────────────────────────────────────────────────
Using 815 MB · 84 GB free                              [Remove unused models…]
```
- The catalog is served by remote config (F-BE-09) with name, size, SHA-256, languages and platforms; the sizes above are the v1 defaults from F-DI-04. Intel Macs show Parakeet as "Not available on this Mac" and mark Whisper Turbo recommended.
- **Row states:** "Download" · "Queued" · downloading (ProgressBar, speed, time left, "Pause" / "Resume", × cancel) · "Verifying files…" · "Loading into memory…" · "Downloaded" · "Active" · "Download failed: the file didn't match. [Retry]" (the file is deleted).
- Distil-Whisper on Windows and Linux shows the Badge "CPU transcription" (warning).
- Selecting a model applies after the current take ("Switches after this take."); if loading fails the old model stays active: "Couldn't load {model}. Still using {old}." If the active model goes missing, the best downloaded one is used: toast "Using {model} because {missing} isn't downloaded."
- Downloads resume across restarts and honour server back-off. On a metered connection: "You're on a metered connection. Download 670 MB anyway?"
- "Remove unused models…" confirm: "Remove {n} models you aren't using? Frees {size}."
- Licence attributions (Parakeet CC-BY-4.0) appear in Settings › Advanced › Open-source licences.

#### UI-64 · Dictionary · DI
Sheet lg "Dictionary". Tabs (underline): "Words" · "Corrections" · "Review ({n})" · and a "Deleted" view under ⋯.
- **Words:** preferred spellings plus the ways they get misheard ("Tauri" ← "tory", "tawri"). Enabled Switch per row.
- **Corrections:** rules applied after transcription. Fields: "When I say" (one or more phrases), "Write", "Match case" (off), "Protect links and code" (on; skips URLs and backticks), "Spacing" ("Exact" / "Flexible"), "Language" (All or one), "Apply in" ("Everywhere" or app identifiers, one per line), "Enabled".
- **Review:** suggestions mined locally from history (inconsistent spellings, frequent unknown terms, acronyms, proper nouns); evidence expires after 90 days. Actions "Remember" / "Ignore"; only Remember creates a rule.
- **Tools:** search, sort ("A–Z" / "Recently changed"), multi-select delete, "Import…" / "Export…" (JSON or CSV with a preview), a test box "Try your dictionary on sample text" (live before/after, with language and app selectors), and "Deleted" (restorable for 30 days).
- **Sync** (hosted, optional; comes with Vibezeo accounts): "Sync across devices" Switch, "Last synced {time}", "Sync now", and a conflict review list. Transcript evidence never syncs.
- Protected spellings survive Enhance and Polish; if a rewrite changes one, the corrected un-enhanced text is used.
- Empty: "No words yet", body "Add names, tools and jargon so Dictate spells them your way.", action "Add word".

#### UI-65 · History & stats · DI
Sheet lg "History & stats". Stored only on this device.
```
 Words 63,410   Speaking time 5h 48m   Takes 2,190   Avg pace 164 wpm   Time saved ~20h   [Share card…]
 [⌕ Search history]                                            [Export CSV] [Clear history…]
 Today 10:42 · Local · Parakeet v3 · 38 words · 14s
   "Refactor the auth guard so it reads the session from the cookie…"   [Copy] [Correct…] [⋯]
```
- "Time saved" compares speaking time with typing the same words at 40 wpm (tooltip).
- Row actions: "Copy", "Correct…" (edit into "My correction", with a checkbox "Remember this correction" that creates a Correction rule), ⋯ "Add to dictionary", "Delete". Full-text search with paging.
- "Export CSV" writes `timestamp,text,word_count,duration_seconds,model`. "Clear history…" confirm (UI-135) keeps the stats.
- "Share card…" renders a 1080×810 PNG of totals (no transcript text) with "Copy image" / "Save PNG".
- Empty: "Nothing dictated yet", body "Hold {key} and speak. Your takes appear here."

---

### 4.13 Conductor

Conductor operates Tandem by voice through a closed tool catalog that mirrors the command registry (F-CN-02): navigation, awareness, panes and agents, threads, teammates, tasks, find work, and a closed list of settings. It types into agents, never straight into a shell; it leaves panes that are waiting on an approval alone and says so; secrets are never spoken.

#### UI-61 · Conductor HUD and ring · CN
```
┌──────────────────────────────────────────────────────────────────┐
│ ( ◎ )  LISTENING                                ◈ 4:12 · ~26 min │
│        "open the auth pane and ask it to run the tests"          │
│  [✓ Code mode] [✓ Focused Fix auth guard] [⟳ Prompting…] [+2]     │
│  ┌ ◆ Run `pnpm test` in Fix auth guard?  Confirm on screen      ┐ │
│  │                                        [No] [Yes, run it]     │ │
│  └───────────────────────────────────────────────────────────────┘ │
│  [🎙 Mute]  [≡ Call log]                                  [× End] │
└──────────────────────────────────────────────────────────────────┘
```
- **Title-bar button:** the 28px WaveformRing (UI-01 item 8). Optional **floating ring** (setting): a 56px ring you can drag anywhere; its position persists; hovering reveals "Mute", "Call log" and "End".
- **HUD card:** 520px, surface-3, radius lg, shadow-3, `--z-hud`, top-centre over the current surface, 12px below the title bar. "–" collapses it to a 36px strip (ring, state, newest chip).
- **Contents:** a 56px ring; the state label (2xs caps); the last utterance in quotes (text-md, 2 lines max); action chips; at most one question row; "Mute"/"Unmute", "Call log", "× End".
- **States:**

  | State | Ring (§2.30) | Label | Notes |
  |---|---|---|---|
  | Idle | idle, title bar only | — | HUD hidden; rendering paused |
  | Connecting | thinking (sweeping) | "CONNECTING" | Up to 3s of early speech is buffered and sent once connected |
  | Listening | listening | "LISTENING" | Full duplex; talking over Conductor interrupts it |
  | Thinking | thinking | "THINKING" | The doer is planning |
  | Acting | thinking | "WORKING ON IT" | The running chip shows a spinner |
  | Speaking | speaking | "SPEAKING" | Caption in fg-muted ("Show captions", on) |
  | Muted | idle + mic-off glyph | "MUTED" | "Conductor can still talk and work." |
  | Error | error | "SOMETHING WENT WRONG" | Reason and action (below) |

- **Action chips:** one per executed tool. Icon by group, short label middle-truncated to 40 chars, then ✓ (done), ✗ (failed; tooltip reason), a spinner (running) or ◆ (waiting on you). The newest 3 show; older fold into "+N" (hover lists them, click opens the call log). A prompt delivered to an agent shows its one-line paraphrase ("Prompting Fix auth guard: run the unit tests and report failures"); saying "send it as I said" skips refinement.
- **Question row:** appears only when Conductor asks something. Buttons are "No" and "Yes, {verb}" ("Yes, run it", "Yes, close 3 agents"), and saying "yes"/"no" works too, bound to that exact request id.
  - **Confirm on screen** (click or keyboard only; voice can't approve): shell commands, forced worktree removal, plugin spend or destructive approvals, enabling Full access. The row shows "Confirm on screen".
  - Voice or screen: closing panes, executing tasks, creating a teammate, other plugin writes.
  - If the target changes first, the row expires: "Expired — {pane} changed."
- **Errors:** "Couldn't connect to Conductor. [Try again]" · "You're offline. Conductor needs a connection." · "Your microphone isn't delivering audio. [Switch mic]" · "This call reached its 30-minute limit. [Start a new call]" · "Out of credits." (UI-122; nothing to buy, no action, D-2914) · "Sign in to use Conductor. [Sign in]". Startup errors always show; nothing pops up after hang-up.
- **Find work:** "Where's the agent doing the login tests?" navigates and flashes the match (UI-42 flash) and says what it was last doing. Two close matches become two chips: "Did you mean: [auth tests] [login page]". The palette's "Find work…" runs the same search.
- **Ending:** hang-up, sign-out, an unrecoverable mic, the 30-minute cap or 0 credits. Audio stops at once, pending deliveries are cancelled and queued actions never run. A 3s receipt replaces the HUD: "Call ended · 4:12 · 520 credits · 6 actions [Call log]".
- **Acceptance:** a voice "yes" never confirms a shell command; hang-up during a pending prompt delivers nothing; a waiting pane never receives Conductor input; after 5 actions the HUD shows 3 chips and "+2"; a hidden window stops the ring's animation loop.

**Starting a call, push-to-talk, wake phrase**
- **Tap** the ring (title bar or floating) or press `Mod+Shift+Space`: start or end a call.
- **Hold** the ring ≥300ms (or hold `Mod+Shift+Space`): push-to-talk. The mic is live only while held; the call stays connected and ends after 2 minutes without speech ("Call ended after 2 quiet minutes").
- **"Hey Conductor"** (opt-in, off): detected on device; no audio leaves the machine until it fires. On detection a soft tone plays, the HUD opens in Connecting, and Conductor answers aloud when ready. While wake listening is on, the title-bar ring shows a 6px accent dot and the tray menu reads "Listening for "Hey Conductor"". It pauses while Dictate or a call owns the mic, while media plays on a Bluetooth output, and while "Pause while watching videos" is on. If the detector misses its accuracy bar, the build hides the option.
- **No account:** the ring is hidden; the palette's call command shows the HUD's error "Conductor comes with Vibezeo accounts." (D-2903).
- **Credits indicator:** the HUD's top-right shows "◈ {elapsed}" and "~{n} min" left at the current rate (tooltip: "About 120 credits a minute. {balance} left."). Under 5 minutes it turns amber: "About 5 min of credits left". Two minutes before the cap: "2 minutes left in this call".

#### UI-66 · Call log · CN
Sheet md "Call log", subtitle "{date} · {duration} · {credits} credits".
- A Timeline of turns, "You" and "{name}" (text-md), interleaved with action rows (chip icon, label, result, time). Failures show the reason; confirmations show how they were given ("Confirmed on screen", "Confirmed by voice").
- A "Previous calls" select (kept locally for 7 days by default). Footer: "Copy log", "Delete this call".
- Empty: "No calls yet", body "Tap the ring or press {shortcut} to start one."

#### UI-67 · Conductor settings (Settings › Conductor) · CN
- "Use Conductor" · Switch · on with an account that carries Conductor · otherwise disabled with "Comes with Vibezeo accounts".
- **Persona:** "Name" (Input, "Conductor"; the wake phrase stays "Hey Conductor") · "Voice" (the voice provider's voices, each with ▶ preview) · "Tone" "Neutral" / "Warm" / "Brief" · "How much it says" "Short" / "Normal" / "Detailed" · "Speech speed" 5-step slider (also "talk faster/slower" during a call) · "Show captions" (on).
- **"Tell it about you":** Textarea (1,000 chars), placeholder "What you work on and how you like to work."
- **Memory:** "Remember things between calls" (off) · "After each call" "Ask me about new memories" / "Save automatically" / "Don't reflect" · "Open memory file" (opens `memory.md` in the Files editor) · "Forget everything…" (UI-135). Credential-shaped text is refused: "That looks like a secret, so Conductor won't remember it."
- **Listening:** "Call shortcut" HotkeyRecorder (`Mod+Shift+Space`) · "Floating ring" (off) · "Wake phrase "Hey Conductor"" (off) with "Sensitivity" "Low" / "Medium" / "High" and "Pause while watching videos" (on) · the microphone is shared with Dictate ("Change microphone").
- **Safety** (read-only text): "Conductor asks before it closes agents, runs tasks or changes plugins. Shell commands, worktree removal, spending and Full access need a click on screen."
- **Call logs:** "Keep call logs" "Don't keep" / "7 days" / "30 days" (7 days).
- Callout: "During a call, your voice goes to Tandem's voice service and its provider. Nothing is recorded after you hang up."

---

### 4.14 Free, and hosted features

Vibezeo is free, with no paid plans (D-869). The app has no plans sheet, prices, trials, checkout or billing, and nothing in it names Free, Pro or Team as a tier. With no account every feature that runs on the computer is unlimited (D-2900); the hosted ones (cloud Dictate, Enhance, Conductor, Auto, dictionary sync, the phone relay, Create with AI, Find work's hosted ranking) need Vibezeo's servers and come with Vibezeo accounts. Copy is factual and never shames; every gate names what still works. The client gates for UX only; the server decides (entitlement token).

#### UI-120 · Upgrade sheet · removed
Removed with the paid plans (D-869, D-2904). Nothing opens it: the account menu, Settings › Account, the palette and the gates no longer offer "See plans" or "Upgrade".

#### UI-121 · Checkout handoff · removed
Removed with the paid plans (D-869, D-2904): the app opens no checkout (`billing_open` has no `upgrade` target).

#### UI-122 · Limits and hosted features · BL
- **Local limits:** none with no account. A signed token can still carry a limit (none does today); the core then refuses the action with its reason ("This account runs up to {n} live panes", "… is turned off for this account"), shown as the action's error. There are no gate cards (DESIGN §9.5 retires `limit.*`, `launch.limitBody`, `thread.engineGate.*`, `plugins.freeLimit`).
- **Hosted features** with no account: the control is disabled or absent, labelled "Comes with Vibezeo accounts", word for word, followed by "It uses Vibezeo's servers and switches on when Vibezeo accounts open." (DESIGN §9.5); where there's no room (a segment, a switch, a menu row) the two are the tooltip. The one action, where needed: "Tell me when accounts open" (a prefilled email to contact@vibezeo.com). Conductor's call command shows the same words as the HUD's error; Auto's pane-menu entry is absent. Never Pro, premium, paid, upgrade, plan, trial, credits, a price or a date (a vitest renders every Settings page of the download build to check, D-2911).
- **No plan UI** (DESIGN §5.5): no plan footer in the rail, no plan badge, no limit cards or limit lines. Settings › Advanced shows one quiet line under the version: "Vibezeo is free. If it helps you, tell a friend."
- **Disabled by an organization's policy:** "Turned off by your organization" / "An admin controls this setting." (no upgrade offered).
- **Credits low** (below 10% of the monthly grant): the title-bar pill turns amber; one in-app notification "Credits are running low · {n} left · resets {date}"; no dialog.
- **Credits exhausted** (a `402 CREDITS_EXHAUSTED` or reserve failure): Dialog sm, title "You're out of credits", body "Cloud dictation, Enhance, Conductor and Auto pause until {reset date}. Coding with your own AI accounts keeps working.", actions "Not now" / "Use local dictation" (when Dictate caused it). Shown at most once per day; later hits use the inline error of the feature (UI-60, UI-61).
- **Auto** stops at the end of its current turn with a pane Banner "Auto paused: out of credits".

#### UI-123 · Trial, payment and lapse notices · removed
Removed with the paid plans (D-869): there are no trials, payments or lapses. Limits that turn routines off still pause them, not delete them (F-RT-04).

#### UI-124 · Manage billing · removed
Removed with the paid plans (D-869, D-2904). Settings › Account (UI-114) shows who's signed in, an organization's name and the credits hosted features use, with "Refresh".

### 4.15 System dialogs

#### UI-130 · Update flow · TL
| Stage | Surface | Copy | Actions |
|---|---|---|---|
| Checking / downloading | Settings › Updates and the status bar ("Updating… 42%") only; no pill | "Downloading {version}…" | "Pause" |
| Ready | Title-bar pill (accent) + status bar "Update ready" + one notification | "Restart to update" | click → dialog below |
| Failed | Pill in error tone | "Update failed — Retry" (tooltip names the stage: download, verify or install) | "Retry", "Details" |
| Can't verify | Pill error + Settings | "Update can't be verified, so it wasn't installed." | "Details" |

**Restart dialog** (Dialog md): title "Restart to update to {version}?", body "These are still running. Agents stop and can be resumed after the restart; terminals reopen in the same folders." followed by a list (StatusDot, title, workspace) of live agent panes and threads, then "What's new ↗". Actions "Later" / "Restart now" (primary). With nothing running the list is omitted and the body is "Tandem will restart in a few seconds.". "Later" keeps the pill; the update installs on the next quit.

#### UI-131 · Unsupported version overlay · TL
Full-window overlay (surface-3 card on `overlay` scrim, `--z-max`), not dismissable: title "This version of Tandem is no longer supported", body "Update to keep using your account features. Terminals keep working for {hours} more hours.", actions "Quit" / "Update now" (primary, shows progress inline). During the 72h grace the card is a dismissable top Banner with the same copy; after it, the modal overlay returns on every focus.

#### UI-132 · Crash restore and orphaned processes · SH, TL
- The crash variant of UI-02 (defined there) is the restore surface.
- **Orphans** (after the restore splash, Dialog sm): title "{n} agent processes from your last session are still running", body "They were started by Tandem before it closed. Stopping them ends their work; their conversations are saved.", list (engine glyph, command, folder, started time), actions "Keep them" / "Stop them" (primary). Setting in UI-116 can make this automatic.
- **Paused agent card** (in each restored agent pane): EngineGlyph, title, "Paused after restart", actions "Resume" (primary) / "Start fresh"; "Launch configuration unavailable" disables Resume.

#### UI-133 · Connectivity and entitlement banners · SH, BE, BL
Banners sit under the title bar (full width, 32px, dismissable per session except where noted). Only one shows at a time, by priority: unsupported > entitlement expired > offline > grace.

| Condition | Tone | Copy | Action |
|---|---|---|---|
| Offline, token valid | info | "You're offline. Local features keep working; your account stays confirmed until {time}." ({time} = token expiry + 24h grace; network errors alone never downgrade.) | — |
| Offline, grace over (24h past the token's expiry) | warning, not dismissable | "We couldn't confirm your account, so hosted features are off until Vibezeo reconnects. Everything on this computer keeps working." | "Retry" |
| Offline-mode user, day 11–14 | warning | "Sign in within {n} days to keep using Tandem." | "Sign in" |
| Server unreachable, online | warning | "Can't reach Tandem's servers. Hosted features are paused." | "Status page ↗" |
| Back online | success toast | "Back online" | — |

The title-bar 2px warning line and status-bar "Offline" (§3.2, §3.6) accompany every offline row.

#### UI-134 · Permission prompts · DI, CN, SH
Tandem always shows its own explainer (Dialog sm) before triggering an OS prompt, and a recovery state if the OS says no.

| Permission | When asked | Explainer title / body | Denied state |
|---|---|---|---|
| Microphone | First Dictate or Conductor use | "Allow microphone access" / "Dictate and Conductor listen only while you hold the key or are on a call." | "Tandem can't use the microphone. [Open system settings]" |
| Accessibility (mac) | Enabling global Dictate | "Let Tandem type for you" / "Accessibility lets Tandem put dictated text into other apps." | "Dictated text will be copied instead of typed. [Open system settings]" |
| Input Monitoring (mac) | Binding a global hold key | "Let Tandem hear your shortcut" / "Input Monitoring lets Tandem notice your dictation key in other apps. It doesn't record what you type." | "Your shortcut only works inside Tandem. [Open system settings]" (+ "Quit & reopen" when the OS requires it) |
| Global shortcuts (Wayland) | Binding a global key | "Allow a global shortcut" / "Your desktop will ask you to approve Tandem's shortcut." | Wayland Callout from UI-62 |
| Text insertion tools (Linux) | First global insert without them | "One more tool is needed" / "Install {tool} so Tandem can type into other apps:" + CodeBlock with apt / dnf / pacman lines | Falls back to Copied |
| Microphone blocked (Windows privacy) | Capture fails with access denied | "Windows is blocking the microphone" | "[Open microphone settings]" |
| Notifications | First system notification | OS prompt only (no explainer) | UI-41 banner |

Explainer actions: "Not now" / "Continue". Granting live-updates the relevant status chips without a restart where the OS allows it.

#### UI-135 · Destructive confirmations · all
All use ConfirmDialog. Title is a question naming the object, body says what happens and what doesn't, the confirm label repeats the verb. Danger styling only when something is lost.

| Action | Title | Body | Confirm | Typed? |
|---|---|---|---|---|
| Stop a working agent | "Stop {title}?" | per §2.20 | "Stop and close" | — |
| Discard worktree | "Discard {branch}?" | "Deletes the worktree folder and its uncommitted changes. Commits on the branch stay." | "Discard worktree" (danger) | the branch name |
| Remove workspace | "Remove {name} from Tandem?" | "Nothing on disk is deleted. Add the folder again to bring back its history." | "Remove" | — |
| Delete teammate | "Delete {name}?" | "Deletes its chats, memory and routines. Skills in the library stay." | "Delete teammate" (danger) | the name |
| Delete routine | "Delete {routine}?" | "Past run results stay in their chats." | "Delete" (danger) | — |
| Disconnect plugin | "Disconnect {plugin}?" | "Teammates lose access immediately. The key is removed from your keychain." | "Disconnect" (danger) | — |
| Sign out of an engine account | "Sign out of {account}?" | "Agents using it stop at their next request." | "Sign out" | — |
| Delete local data | "Delete all Tandem data on this device?" | "Removes settings, history, memory and caches. Your project folders and AI accounts aren't touched." | "Delete data" (danger) | "delete" |
| Reset remote access | "Reset remote access?" | "Every paired phone is disconnected and must pair again." | "Reset" (danger) | — |
| Forget Conductor memory | "Forget everything Conductor remembers?" | — | "Forget" (danger) | — |
| Clear Dictate history | "Clear dictation history?" | "Your stats are kept." | "Clear history" (danger) | — |

Default focus is on the cancel button for danger confirms, on the confirm button otherwise. `Mod+↩` confirms only when no typed text is required or it matches.

---

### 4.16 Extras (M17)

ID range UI-140–149. Every screen here uses only the §2 components and tokens and has empty, loading, error and populated states.

#### UI-140 · Checkpoints sidebar tab · EX
A sidebar tab "Checkpoints" (singleton; "+" menu item "Checkpoints"), following the context on screen like Files: the open thread's checkpoints in Thread mode, the active workspace's or lane's in Code mode.
```
┌ Checkpoints · shop ─────────────────────── [Checkpoint now] ┐
│ ● Before a turn · Fix checkout redirect · 2m   3 files differ │
│ ○ Before a prompt · Claude Code · 2 · 14m      5 files differ │
│ ○ Before restore · 1h                             Same as now │
├────────────────────────────────────────────────────────────────┤
│ ☑ src/cart.ts            +4 −12                                 │
│ ☑ src/new-helper.ts      created since                         │
│ ☐ README.md              +1 −1                                  │
│ (DiffView of the selected file: checkpoint → now)              │
│                  [Restore 2 files…]  [Restore all files…]       │
└────────────────────────────────────────────────────────────────┘
```
- Rows are ListRows: label, source (thread title or pane name), age (§6.3), and a Badge "{n} files differ" (none: "Same as now"). A checkpoint taken late (F-EX-01) adds a warning Badge "Late"; a partial one "Partial".
- The diff list shows one row per file with a Checkbox (all checked), the change ("+a −d", "created since", "deleted since") and a DiffView of the selected file, from the checkpoint to now.
- Restore opens the §4.15 destructive confirmation with the F-EX-01 copy; the primary is "Restore files". Success toast "Restored {n} files" with "Undo" (restores the Before restore checkpoint).
- Notes above the list: not a git repository (Callout, info), git missing (EmptyState with no action), off (EmptyState "Checkpoints are off" with "Turn on").
- Settings › Checkpoints (UI-117): Switch "Save checkpoints before agent turns", NumberControl "Keep per folder" (10–1,000), "Keep for (days)" (1–90), "Skip files larger than (MB)" (1–100), KeyValue "Space used", and the danger Button "Delete all checkpoints…".

#### UI-141 · Recall palette · EX
A palette-style picker with the command palette's shell (`Mod+Alt+J`, or "Recall a command…"; D-706): the input's placeholder is "Search commands you've run", a SegmentedControl "This workspace | Everywhere" sits under it.
- Rows: the command in mono (truncated with the full text in a tooltip), then a second line with the folder, ✓ or "✗ {code}", duration, age and pane name, and "×{n}" when it ran more than once.
- Footer Kbd hints: "↩ Insert", "⌘↩ Run" (hidden for a command with `[REDACTED]` or when the focused pane can't run it), "⌘⌫ Delete".
- Empty: "No commands yet" / "Commands you run in Vibezeo terminals show up here." Off: "Command recall is off" [Turn on]. No results: "No commands match "{query}"".
- Settings › Terminal gains a "Command recall" group: Switch "Remember commands I run", NumberControl "Keep for (days)", and "Clear command history…".

#### UI-142 · Ports sidebar tab · EX
A sidebar tab "Ports" (singleton; "+" menu item "Ports").
```
┌ Ports ─────────────────────────────────────────── [Refresh] ┐
│ :5173  node (41822) · dev server · shop       localhost    │
│        [Open]  [Copy URL]  [Stop…]                          │
│ :8080  python3 (41990) · shell · api   ⚠ all networks       │
│        [Open]  [Copy URL]  [Stop…]                          │
└──────────────────────────────────────────────────────────────┘
```
- The port is mono `text-lg`; the pane name is a link button that focuses the pane. "all networks" is a warning Badge with the tooltip "Reachable from your network".
- Stop… opens the destructive confirmation with the F-EX-03 copy ("Stop process"). "Still running" shows inline with "Force stop…".
- Loading: Skeleton rows. Error: "Couldn't list ports." [Retry]. Empty: "No ports open" / "Servers started from your panes show up here."

#### UI-143 · Best of N sheet · EX
A Sheet md 480, title "Best of N", subtitle the workspace name.
- A Textarea "Prompt" ("Describe the change. Each agent gets the same prompt."), 2–3 seat rows (engine Select, "Remove"; "+ Add agent" up to 3), an Input "Check command (optional)" with placeholder "pnpm test", and the live summary "This opens {n} agents in {n} worktrees (tandem/{slug}-1 … -{n}), one per agent."
- A Callout when the workspace has uncommitted changes: "Worktrees start from the last commit; your uncommitted changes aren't in them."
- Actions "Cancel" / "Start {n} agents ⌘↩". Not a repository: the sheet shows only "Best of N needs a git repository".

#### UI-144 · Best of N compare · EX
A Dialog lg, opened after Start and from the palette ("Compare Best of N", the folder's newest running run; D-710).
```
Best of N · Fix login redirect                [Run check] [Discard all…]
┌ (C) Claude Code ──────┐┌ (X) Codex ────────────┐┌ (C) Claude Code ──────┐
│ ● Done · 3 files +40 −12││ ● Working · 2 files   ││ ● Done · 4 files +61 −9 │
│ Passed                 ││ Failed · exit 1 [Out] ││ Running…               │
│ (DiffView)             ││ (DiffView)            ││ (DiffView)             │
│ [Keep this one…]       ││ [Keep this one…]      ││ [Keep this one…]       │
└────────────────────────┘└───────────────────────┘└────────────────────────┘
```
- Columns share the width (1–3); under 960 px they stack. A file filter (Select "All files" or one path) lines up the same file in each column.
- "Output" opens a Dialog with the check's last 200 lines (mono). Keep this one… and Discard all… ask in the native confirm with the F-EX-04 copy (TL-06 rule 5: discarding uncommitted work always asks natively, so there's no second, in-page dialog).
- States: loading (Skeleton columns), a lane that failed to start (Banner in its column), merged ("Merged into {base}. The other worktrees are gone; their branches stay."), discarded.

#### UI-145 · Prompt picker · EX
A palette-style picker with the command palette's shell (`Mod+Alt+P`, "Insert prompt…"; D-706): rows show the title, the description and a Badge "This repository" or "Yours"; each row has "Edit" (opens it in the Files editor). The last row is "New prompt…".
- Choosing a prompt with variables to fill opens a Dialog sm "Fill in the prompt": one Input per variable (label `{{name}}`, prefilled with what Vibezeo found), a read-only preview, and "Cancel" / "Insert ⌘↩". Insert never sends.
- "New prompt…": Dialog sm with "Name" and a SegmentedControl "This repository | Yours", then the file opens in the editor.
- A Callout at the top when `.git/info/exclude` hides `.tandem/` (F-EX-05) with "Share with the repository".

---

## 5. Keyboard shortcut map

`Mod` = ⌘ on macOS, Ctrl on Windows/Linux. All shortcuts come from the command registry (F-SH-09); rebinding happens in Settings › Shortcuts (UI-113). Kbd renders per platform.

**Terminal rule (Windows/Linux).** With a terminal focused and "Shortcut priority" = "Terminal first" (UI-104), Tandem intercepts only: `Ctrl+Shift+…`, the `Ctrl+Alt+…` combos in this table, `Ctrl+K`, `Ctrl+,`, `Ctrl+1..3`, `Ctrl+Tab`/`Ctrl+Shift+Tab` and `F6`. Everything else goes to the PTY. The last column gives the terminal-safe alias where the primary would pass through; "→ PTY" means use the palette (`Ctrl+K`) or the pane menu. macOS terminals never lose `⌃` keys, because app shortcuts use ⌘.

| Action | macOS | Windows / Linux | Win/Linux, terminal focused |
|---|---|---|---|
| **Global (work outside Tandem)** | | | |
| Dictate, hold to talk | Fn (hold; fallback ⌃⌥) | Ctrl+Win (Windows) · Ctrl+Alt (Linux) | same |
| Dictate, lock hands-free / stop | double-tap Fn / tap Fn | double-tap / tap the hold key | same |
| Dictate, toggle (optional binding) | unset | unset | same |
| Cancel dictation | Esc (while recording) | Esc (while recording) | same |
| Conductor: tap = call, hold = push-to-talk | ⇧⌘Space | Ctrl+Shift+Space | same |
| **App and navigation** | | | |
| Command palette | ⌘K | Ctrl+K | same |
| Settings | ⌘, | Ctrl+, | same |
| Agent / Code / Thread mode | ⌘1 / ⌘2 / ⌘3 | Ctrl+1 / 2 / 3 | same |
| Keyboard shortcuts cheatsheet | ⌘/ | Ctrl+/ | Ctrl+Shift+/ |
| Toggle left rail | ⌘\ | Ctrl+\ | Ctrl+Shift+\ |
| Toggle right sidebar | ⇧⌘B | Ctrl+Shift+B | same |
| Sidebar: Files / Dashboard / Tasks tab | ⌥⌘F / ⌥⌘D / ⌥⌘K | Ctrl+Alt+F / D / K | same |
| Dashboard full view | ⌥⇧⌘D | Ctrl+Alt+Shift+D | same |
| Recall a command / insert a prompt (M17) | ⌥⌘J / ⌥⌘P | Ctrl+Alt+J / Ctrl+Alt+P | same |
| Notifications | ⇧⌘N | Ctrl+Shift+N | same |
| Jump to next agent waiting for you | ⇧⌘J | Ctrl+Shift+J | same |
| Search the current mode's list | ⇧⌘F | Ctrl+Shift+F | (this is Find in terminal) |
| Move focus between regions (title bar, rail, main, sidebar) | F6 / ⇧F6 | F6 / Shift+F6 | same |
| Quit | ⌘Q | Ctrl+Q | → PTY |
| **Code mode: tabs and panes** | | | |
| New tab in workspace | ⌘T | Ctrl+T | → PTY |
| Launch agents (launch sheet) | ⇧⌘L | Ctrl+Shift+L | same |
| Split right / split down | ⌘D / ⇧⌘D | Ctrl+D / Ctrl+Shift+D | Ctrl+Shift+E / Ctrl+Shift+D |
| Close pane (asks if working) | ⌘W | Ctrl+W | Ctrl+Shift+W |
| Focus pane left/right/up/down | ⌥⌘←→↑↓ | Ctrl+Alt+Arrow | same |
| Focus previous / next pane | ⌘[ / ⌘] | Ctrl+Shift+[ / ] | same |
| Maximize / restore pane | ⇧⌘↩ | Ctrl+Shift+Enter | same |
| Auto-arrange panes | ⇧⌘T | Ctrl+Shift+T | same |
| Next / previous tab | ⌃Tab / ⌃⇧Tab | Ctrl+Tab / Ctrl+Shift+Tab | same |
| Rename focused pane or rail item | F2 | F2 | → PTY |
| Cancel a pane drag | Esc | Esc | same |
| **Terminal** | | | |
| Find / next / previous | ⌘F / ⌘G / ⇧⌘G | Ctrl+F / Ctrl+G / Ctrl+Shift+G | Ctrl+Shift+F / Enter / Shift+Enter |
| Copy | ⌘C | Ctrl+C with a selection; Ctrl+Shift+C | same |
| Interrupt (SIGINT) | ⌃C | Ctrl+C without a selection | same |
| Paste | ⌘V | Ctrl+V, Ctrl+Shift+V, Shift+Insert | Ctrl+Shift+V, Shift+Insert |
| Newline in an agent TUI (no submit) | ⇧↩ | Shift+Enter | same |
| Jump to bottom | ⌘↓ | Ctrl+Shift+End | same |
| Open link or file:line | ⌘-click | Ctrl+click | same |
| Select while a TUI captures the mouse | ⇧-drag | Shift+drag | same |
| Cancel a queued restore (paused card) | ⌃C | Ctrl+C | same |
| **Thread, Teammate chat and composers** | | | |
| New thread / new chat with current teammate | ⌘N | Ctrl+N | n/a |
| Send / newline | ↩ / ⇧↩ (swap with "Send with Mod+Enter") | Enter / Shift+Enter | n/a |
| Stop the running turn | Esc | Esc | n/a |
| Toggle Plan / Act (in composer) | ⇧⌘P | Ctrl+Shift+P | n/a |
| Model picker / attach (in composer) | ⇧⌘O / ⌘U | Ctrl+Shift+O / Ctrl+U | n/a |
| Focus composer / toggle Changes panel / find in thread | ⌘L / ⌥⌘C / ⌘F | Ctrl+L / Ctrl+Alt+C / Ctrl+F | n/a |
| Previous / next message | ⌥↑ / ⌥↓ | Alt+↑ / Alt+↓ | n/a |
| Hand off thread or pane | ⇧⌘H | Ctrl+Shift+H | same |
| Teammate page tabs (Chats / Memory / Skills / Settings) | ⌥⌘1…4 | Ctrl+Alt+1…4 | n/a |
| Archive thread (asks first) | ⌘⌫ | Ctrl+Backspace | n/a |
| Edit last message | ↑ in an empty composer | ↑ | n/a |
| Mention / slash command | @ / / | @ / / | n/a |
| Dictate into composer (hold; click mic to lock) | the global Dictate key, or ⌥Space | the global Dictate key (Ctrl+Win · Ctrl+Alt); `Alt+Space` is not bound (OS window menu) | n/a |
| Enhance the draft (with a Vibezeo account) | ⌥⌘E | Ctrl+Alt+E | n/a |
| **Approval card (focused)** | | | |
| Focus the oldest pending approval card (thread) | ⇧⌘A | Ctrl+Shift+A | n/a |
| Allow once (primary) / Allow for this thread / Deny / Deny and explain / focus Deny | A or ⌘↩ / ⇧⌘↩ / D / E / Esc | A or Ctrl+Enter / Ctrl+Shift+Enter / D / E / Esc | same |
| **Dashboard and Notifications (focused)** | | | |
| Move / jump to agent | ↑↓←→ / ↩ | Arrows / Enter | same |
| Focus search | / | / | same |
| Clear search, then leave | Esc | Esc | same |
| Close agent (asks if working) | ⌫ | Delete | same |
| Notification: mark read / dismiss | M / ⌫ | M / Delete | same |
| **Conductor (during a call)** | | | |
| Mute / unmute | ⇧⌘M | Ctrl+Shift+M | same |
| Confirm / cancel the pending action | ⌘↩ / Esc | Ctrl+Enter / Esc | same |
| Open call log | ⇧⌘. | Ctrl+Shift+. | same |
| End call | ⇧⌘Space (tap) | Ctrl+Shift+Space (tap) | same |
| **Lists, menus and dialogs** | | | |
| Context menu | ⇧F10 or right-click | Shift+F10, Menu key | same |
| Primary action in a dialog or sheet | ⌘↩ | Ctrl+Enter | same |
| Close dialog, sheet, popover or Settings | Esc | Esc | same |
| Rail: move / expand / collapse / open / delete | ↑↓ / → / ← / ↩ / ⌫ | Arrows / Right / Left / Enter / Delete | same |
| Settings: focus the filter | ⌘F | Ctrl+F | n/a |

**Suppressed** in release builds: F5, Ctrl/⌘+R, Ctrl/⌘+P, Ctrl+F outside terminals and Settings, F3, Ctrl/⌘+S (outside the Files editor), Ctrl+Shift+I / ⌥⌘I. No shortcut fires during IME composition or in text fields unless it is global (palette, Settings, modes, Dictate, Conductor).

**Reconciliations (applied; this table is the canonical map and PRODUCT_SPEC F-SH-09 matches it):**
- Left rail `Mod+\` (not `Mod+B`, which is Scratchpad bold); maximize `Mod+Shift+Enter` (not `Mod+Enter`); Arrange `Mod+Shift+T` (not `Mod+Alt+T`, which GNOME takes for its terminal); launch sheet `Mod+Shift+L`, with `Mod+T` = new tab. Every per-screen mention in this spec uses these.
- `Mod+Shift+N`, `Mod+Shift+J`, `Mod+Alt+K`, `Mod+Alt+Shift+D`, `Mod+Alt+E`, `Mod+Shift+M`, `Mod+Shift+.`, `Mod+Shift+A`, the Win/Linux terminal aliases and the extended intercept list (`Ctrl+Alt+…`, `Ctrl+Tab`, `F6`) are in F-SH-09.
- `Mod+Shift+Enter` means "Allow for this thread" only while an approval card has focus; elsewhere it maximizes the focused pane.
- Plan/Act is `Mod+Shift+P` (not `Shift+Tab`, which must keep moving focus backwards); the model picker is `Mod+Shift+O` (not `Mod+Shift+M`, which is Conductor mute); "New teammate" has no default (not `Mod+Shift+N`, which is Notifications).
- Page-local single-key shortcuts (Tasks page, Dashboard, dialogs) are listed with their screens and only fire when that page has focus and no text field is focused.
- `Alt+Space` is never bound on Windows or Linux (Windows and GNOME/KDE use it for the window menu). Composer dictation there uses the global Dictate hold key (Ctrl+Win / Ctrl+Alt) while a composer has focus; macOS additionally accepts ⌥Space.
- Linux desktops may reserve `Ctrl+Alt+Arrow`; the recorder flags it and suggests `Ctrl+Shift+Alt+Arrow`.

---

## 6. Microcopy style guide

All strings live in `apps/desktop/src/i18n/en.json`. Quoted strings in this spec are final defaults.

### 6.1 Voice and tone
- **Plain, calm, specific.** Say what happened and what to do next. No hype, no jokes in errors, no exclamation marks, no "Oops".
- **Second person, active voice.** "You're out of credits", not "Credits have been exhausted". Tandem refers to itself as "Tandem" only when the distinction matters ("Tandem never sees your token").
- **Name things exactly.** Engine names as their vendors write them ("Claude Code", "Codex"); Tandem's features capitalised as proper names: Dictate, Conductor, Auto. There are no plan names to write (D-869). Generic nouns stay lowercase in sentences: teammate, routine, task, plugin, workspace, pane, thread.
- **Sentence case** for every title, label, button and menu item. Section labels are the only uppercase (2xs caps via CSS; the string itself is sentence case).
- **Reassure with facts, not adjectives.** "Your conversation is saved" instead of "Don't worry".
- **Never blame the user or the vendor.** "Claude Code needs you to sign in", not "Your login is invalid".

### 6.2 Word list
| Use | Avoid | Note |
|---|---|---|
| agent | bot, AI, assistant | Conductor is the only "assistant" |
| sign in / sign out | log in, login (verb) | |
| folder | directory | "path" is fine in technical rows |
| worktree | lane | "lane" is internal only |
| stop (an agent) · close (a pane) · remove (from a list) · delete (data) · discard (changes) | mixing them | The verb tells the user what is lost |
| credits | tokens, points | "tokens" only for engine usage |
| account | subscription, login | an engine sign-in inside a provider home |
| Waiting for you · Working · Done · Idle · Failed · Paused · Interrupted | any other status words | fixed vocabulary everywhere |

### 6.3 Formatting
- Numbers are numerals with separators ("10,000"), tabular in meters. Durations "4:12" or "14m"; relative times "2m", "1h", "yesterday", then dates ("12 Oct"); resets "resets in 1h 12m" or "resets Mon 09:00".
- "…" (one character) ends labels that open another step ("Add folder…", "Hand off to…") and progress text ("Transcribing…").
- An em dash joins a status and its next step only in one-line toasts and pills ("Update failed — Retry").
- Keyboard hints use Kbd, never the word "Mod".
- Interpolated names are never truncated inside a sentence; the containing element truncates.

### 6.4 Patterns
| Pattern | Shape | Example |
|---|---|---|
| Error | What happened. What still works or why. Action button. | "Couldn't reach Tandem. Check your connection or continue offline." [Retry] |
| Error with a limit | Name the limit and when it resets. | "You've reached today's cloud limit — switch to Local" |
| Empty state | Title names the absence; one sentence on how to start; one primary action. | "No agents running" / "Start one in Code, Thread or Agent mode." [Launch agents…] |
| Confirmation | Title is a question naming the object; body says what is lost and what is kept; confirm repeats the verb. | "Discard tandem/fix-auth?" / "Deletes the worktree folder and its uncommitted changes. Commits on the branch stay." [Discard worktree] |
| Stop vs keep | Cancel reads "Keep {x}" when the default is to stop something running. | "Keep running" / "Stop and close" |
| Gate | Name the plan, what's included, and what keeps working. | "Free includes 4 live panes" / "Running panes aren't affected." |
| Disclosure | Before the first send: what leaves, where, and how to turn it off. | "Enhance sends the transcript text (not audio) to Tandem's AI service…" |
| Progress | Participle + ellipsis, with counts when known. | "Restoring your workspace… 6 of 9 sessions" |
| Success | Past tense, only when the result isn't visible. | "Closed 4 idle agents" [Show saved sessions] |
| Spoken (Conductor) | One sentence: what it did, then what needs you. No paths longer than a file name, never secrets. | "Opened Fix auth guard. It's waiting for you to approve a command." |

### 6.5 Rewrites
| Instead of | Write |
|---|---|
| "Error: ENOENT" | "Folder not found: ~/code/old" (+ "Details" with the code) |
| "Are you sure?" | "Close 4 idle agents?" |
| "OK" | "Close 4 agents" |
| "Something went wrong" | "Couldn't load Parakeet v3. Still using Whisper Base." |
| "Upgrade now to unlock the full power!" | "Conductor comes with Vibezeo accounts" |
| "Invalid hotkey" | "Toggle needs a key that isn't only modifiers." |

---

## 7. Accessibility

Target: WCAG 2.2 AA, keyboard-complete, usable at 200% zoom, VoiceOver (mac), NVDA/Narrator (Windows) and Orca (Linux).

### 7.1 Landmarks and focus order
- Landmarks: title bar `banner`; left rail `navigation` ("Teammates" / "Workspaces" / "Threads" / "Settings pages"); main `main`; right sidebar `complementary` ("Sidebar"); status bar `contentinfo`. The first Tab stop is a visually hidden "Skip to main content".
- Tab order follows visual order inside each region (F-SH-01 acceptance 5). `F6` / `Shift+F6` cycle regions; floating HUDs and toasts join the cycle only while visible.
- Pane grid: each pane is a `group` labelled "{title}, {status}". Tab from the header enters the terminal or transcript; `Mod+Alt+Arrow` moves between panes. Terminals use xterm's `screenReaderMode`, switched on automatically when an OS screen reader is detected (and in Settings › Appearance › "Screen reader support in terminals").
- Opening a dialog, sheet or popover moves focus to its first field (or its title if none); closing returns focus to the trigger. Jumps (Dashboard, notifications, Conductor) move focus to the target and announce it.
- Focus is always visible (`--shadow-focus`); never remove it, including in terminals (the focused pane gets the header ring plus the accent top border).

### 7.2 Roles per component
| Component | Role / pattern | Notes |
|---|---|---|
| Mode switcher (SegmentedControl) | `radiogroup` "Work mode" + `radio` | "Work mode, Code, selected, 2 of 3" |
| Tabs | `tablist` / `tab` / `tabpanel` | manual activation for heavy tabs |
| Rail lists, FileTree | `tree` / `treeitem` with `aria-expanded`, `aria-level` | type-ahead |
| PaneGrid gutters | `separator` (focusable) with `aria-valuenow` | arrows resize 16px, Shift+arrows 64px, Enter equalizes |
| PaneChrome | `toolbar` labelled by the pane title | |
| TerminalView | xterm accessibility tree (`textbox` + live output region) | output announcements throttled by xterm |
| Menu / ContextMenu | `menu` / `menuitem` / `menuitemcheckbox` | |
| CommandPalette | `combobox` + `listbox` / `option` in a `dialog` | |
| Dialog / ConfirmDialog | `dialog` / `alertdialog` (destructive), `aria-modal` | |
| Sheet | `dialog` with `aria-modal="false"` when non-blocking | |
| Toast | `status` (polite) / `alert` (errors) | |
| Banner | `status`, or `alert` for errors | |
| StatusDot | `img` with `aria-label` ("Waiting for you") | never colour alone (§1.3) |
| Dashboard | `feed` of `article` cards, each labelled "{title}, {status}, {elapsed}" | group headers are `heading` level 3 |
| Notifications popover | `dialog` containing a `list`; unread conveyed in the label | |
| ApprovalCard | `region` labelled "{engine} needs approval" | A / D shortcuts listed in `aria-keyshortcuts` |
| Composer | `textbox` multiline + `toolbar` | |
| UsageMeter / CreditMeter / ProgressBar | `meter` / `progressbar` with `aria-valuetext` | "62% used, resets in 1h 12m" |
| WaveformRing | `img` + a polite live region for state changes | |
| Conductor HUD | `region` "Conductor call" with a live region for state and chips | question row is a `group` |
| Dictate widget | separate window; state announced through the OS accessibility API and the in-app announcer | |
| HotkeyRecorder | `button` that captures keys; announces the captured combo and conflicts | |
| SettingsRow | label `for` its control; description via `aria-describedby` | |

### 7.3 Contrast (measured on the original §1.2 tokens; every fix below is now applied in §1.2)
| Pair | Ratio | Result | Fix (applied in §1.2) |
|---|---|---|---|
| Dark text / surface-2 | 15.7 | pass | — |
| Dark text-muted / surface-2 | 7.7 | pass | — |
| Dark text-subtle / surface-1, surface-3 | 4.39, 4.06 | **fail** for body text → fixed | `--tn-text-subtle: #7d8ba0` (5.1 on surface-3, 4.7 on surface-4) |
| Dark accent / bg; accent-fg on accent | 10.6; 7.8 | pass | — |
| Dark status colours as text (waiting, working, done, error) | 6.9–11.1 | pass | — |
| Dark idle (#64748b) as text | 3.9 | fail as text, pass as a shape | idle labels use `text-muted` |
| Light text-subtle / white; on bg and surface-4 (slate-50) | 4.76; 4.34 | pass on white, **fail** on slate-50 → fixed (found in the consistency pass) | `--tn-text-subtle: #5f6f84` (5.1 on white, 4.7 on slate-50) |
| Light accent (#0d9488) on white; white on accent | 3.74 | **fail** for text and primary buttons → fixed | `--tn-accent: var(--teal-700)` #0f766e (5.5), hover `teal-800`, press `teal-900` |
| Light waiting / done / working as text | 3.19 / 3.30 / 4.10 | **fail** as text, pass as shapes (≥3:1) → fixed | added `--tn-status-{waiting,done,working}-text`: amber-700 (5.0), green-700 (5.0), sky-700 #0369a1 (5.9); Badges and labels use them |
| Light danger / white | 4.70 | pass | — |

Non-text contrast (borders of inputs, focus ring, status shapes) must be ≥3:1 against adjacent colours; `--tn-border` isn't used as the only boundary of an input (inputs also have the surface-4 fill). Disabled controls are exempt but keep a `disabledReason` tooltip.

### 7.4 Reduced motion
With OS or in-app Reduce motion: no transforms or pulses (§1.8). In this part specifically: the Dashboard flash becomes a static 1s outline; the waiting attention pulse is removed; the stacked bar and counts update without tweening; WaveformRing is static with thickness for level (§2.30); HUD pop-in is a ≤120ms fade; chip spinners become a static "…"; indeterminate progress shows a static striped fill; skeleton shimmer is off; full view opens without a slide.

### 7.5 Screen-reader announcements
- One app-level announcer with a polite and an assertive region, owned by the shell. Components never create their own live regions for agent status.
- **Agent status** (setting in Appearance › Accessibility: "Announce agent status" "Waiting and failures" (default) / "Waiting, done and failures" / "Everything" / "Off"):

  | Event | Region | Text |
  |---|---|---|
  | → Waiting | polite (assertive if it's the only agent) | "{title} is waiting for you: {summary}" |
  | → Failed | assertive | "{title} stopped with an error" |
  | → Done | polite | "{title} finished" |
  | → Working | polite ("Everything" only) | "{title} is working" |
  | Several within 2s | polite | "3 agents finished: {a}, {b} and {c}" / "2 agents are waiting for you" |

- Announcements are queued while the user types in a composer or terminal and flushed after 1s of no input; a newer status for the same agent replaces a queued one (never announce stale status).
- Also announced: mode switches ("Code mode"), jumps ("Focused {title}"), Dictate states ("Listening", "Inserted {n} words"), Conductor states and each finished chip ("Done: switched to Code mode"), toasts per §2.12, and offline/online changes.
- Sounds are never the only signal; every sound has a visual and an announced equivalent. Conductor speech has captions.

### 7.6 Other rules
- Hit targets ≥24×24px (controls are 28/32px); icon-only buttons need `label` (§2.2).
- Every drag action has a keyboard or menu equivalent: pane move ("Move to…", swap via menu), rail reorder ("Move up/down"), file drops ("Attach to composer"), gutter resize (separator keys).
- Time limits: approval cards with `expiresAt` show the remaining time in text, not only as a ring; the Dictate 10-minute cap warns at 9:30.
- Testing: axe-core on every gallery state in vitest; keyboard-only E2E for onboarding, launch, approve, Dashboard jump and Settings; a manual screen-reader checklist per OS in BUILD_LOG.

---

## 8. Screen index

ID ranges: UI-01–19 shell, onboarding and Code mode (ui_part2) · UI-20–37 Thread, engines and accounts · UI-40–49 Dashboard and notifications · UI-50–53 Routines · UI-60–69 Dictate and Conductor · UI-70–94 Teammates, Tasks and Plugins (ui_part3) · UI-100–116 Settings · UI-120–124 Plans · UI-130–135 System dialogs · UI-140–149 Extras (M17) · UI-150–159 Worktree workflow · UI-160–169 Engines (M18). Unused numbers inside a range are reserved for that range.

| ID | Screen | Section | Areas |
|---|---|---|---|
| UI-01 | Title bar | §3.2 | SH, AC, BL, DB, CN |
| UI-02 | Launch / restore splash (incl. crash variant) | §4.1 | SH, TL |
| UI-03 | Sign-in | §4.1 | BE, BL |
| UI-04 | Onboarding wizard | §4.1 | EN, AC, DI, SH |
| UI-05 | Engine setup | §4.1 | EN, AC |
| UI-10 | Code mode: empty workspace | §4.2 | CD, EN |
| UI-11 | Code mode: pane grid | §4.2 | CD, DB, AU |
| UI-12 | Launch sheet | §4.2 | CD, EN, AC |
| UI-13 | Hand off dialog | §4.2 | CD, TH, AC |
| UI-14 | Remote hosts | §4.2 | RM, CD |
| UI-20 | Thread mode layout | §4.3 | TH, SH |
| UI-21 | Thread view: transcript items | §4.3 | TH |
| UI-22 | Thread composer | §4.3 | TH, EN, AC, DI |
| UI-23 | Inline approvals | §4.3 | TH, PL |
| UI-24 | Changes panel | §4.3 | TH, CD |
| UI-25 | Hand off / switch account in a thread | §4.3 | TH, AC |
| UI-33 | Settings › Engines | §4.4 | EN |
| UI-34 | Install and update flow | §4.4 | EN |
| UI-35 | Accounts | §4.4 | AC, EN |
| UI-36 | Usage-limit card and dialog | §4.4 | AC, TH, CD, AG |
| UI-37 | Usage page | §4.4 | AC |
| UI-40 | Notifications popover | §4.10 | DB |
| UI-41 | Notification settings | §4.10 | DB |
| UI-42 | Dashboard (sidebar tab) | §4.9 | DB, CD, TH, AG |
| UI-43 | Dashboard full view | §4.9 | DB |
| UI-50 | Routines list | §4.7 | RT, AG |
| UI-51 | Routine editor | §4.7 | RT, AG, PL |
| UI-52 | Schedule builder (CronInput) | §4.7 | RT |
| UI-53 | Run history | §4.7 | RT |
| UI-60 | Dictate widget (HUD) | §4.12 | DI |
| UI-61 | Conductor HUD and ring | §4.13 | CN |
| UI-62 | Dictate settings | §4.12 | DI, BL |
| UI-63 | Model manager | §4.12 | DI |
| UI-64 | Dictionary | §4.12 | DI |
| UI-65 | History & stats | §4.12 | DI |
| UI-66 | Call log | §4.13 | CN |
| UI-67 | Conductor settings | §4.13 | CN, BL |
| UI-68 | In-app composer dictation | §4.12 | DI, TH, AG |
| UI-70 | Teammate rail | §4.5 | AG |
| UI-71 | Teammate page | §4.5 | AG |
| UI-72 | New teammate sheet | §4.5 | AG, BL |
| UI-73 | Memory tab | §4.5 | AG |
| UI-74 | Skills tab and Skills library | §4.5 | AG |
| UI-75 | Teammate settings: identity, engine, places | §4.5 | AG, AC |
| UI-76 | Teammate settings: permissions, learning, danger zone | §4.5 | AG, PL, RT |
| UI-80 | Tasks page: board and list | §4.6 | TK |
| UI-81 | Task detail | §4.6 | TK |
| UI-82 | Run in worktree dialog | §4.6 | TK, CD, AC |
| UI-83 | Review state | §4.6 | TK, CD |
| UI-84 | Stop and resume | §4.6 | TK |
| UI-90 | Plugin catalog | §4.8 | PL |
| UI-91 | Connect flow | §4.8 | PL |
| UI-92 | Connected plugin detail | §4.8 | PL, AG |
| UI-93 | Approval policy by risk class | §4.8 | PL, AG, RT, TK |
| UI-94 | Plugins in the composer | §4.8 | PL, AG |
| UI-100 | Settings shell | §4.11 | SH |
| UI-101 | Settings › General | §4.11 | SH |
| UI-102 | Settings › Appearance | §4.11 | SH |
| UI-103 | Settings › Notifications (→ UI-41) | §4.11 | DB |
| UI-104 | Settings › Terminal | §4.11 | CD |
| UI-105 | Settings › Engines (→ UI-33) | §4.11 | EN |
| UI-106 | Settings › Accounts (→ UI-35) | §4.11 | AC |
| UI-107 | Settings › Teammates | §4.11 | AG, RT |
| UI-108 | Settings › Dictate (→ UI-62) | §4.11 | DI |
| UI-109 | Settings › Conductor (→ UI-67) | §4.11 | CN |
| UI-110 | Settings › Plugins (→ UI-90–93) | §4.11 | PL |
| UI-111 | Settings › Remote | §4.11 | RM |
| UI-112 | Settings › Privacy & Telemetry | §4.11 | TL |
| UI-113 | Settings › Shortcuts | §4.11 | SH |
| UI-114 | Settings › Account | §4.11 | BE |
| UI-115 | Settings › Updates | §4.11 | TL |
| UI-116 | Settings › Advanced & Diagnostics | §4.11 | TL |
| UI-120 | Upgrade sheet (removed, D-869) | §4.14 | BL |
| UI-121 | Checkout handoff (removed, D-869) | §4.14 | BL, BE |
| UI-122 | Limits and hosted features | §4.14 | BL |
| UI-123 | Trial, payment and lapse notices (removed, D-869) | §4.14 | BL |
| UI-124 | Manage billing (removed, D-869) | §4.14 | BL |
| UI-130 | Update flow | §4.15 | TL |
| UI-131 | Unsupported version overlay | §4.15 | TL |
| UI-132 | Crash restore and orphaned processes | §4.15 | SH, TL |
| UI-133 | Connectivity and entitlement banners | §4.15 | SH, BE, BL |
| UI-134 | Permission prompts | §4.15 | DI, CN, SH |
| UI-135 | Destructive confirmations | §4.15 | all |
| UI-140 | Checkpoints sidebar tab (and Settings › Checkpoints, UI-117) | §4.16 | EX |
| UI-141 | Recall palette | §4.16 | EX |
| UI-142 | Ports sidebar tab | §4.16 | EX |
| UI-143 | Best of N sheet | §4.16 | EX |
| UI-144 | Best of N compare | §4.16 | EX |
| UI-145 | Prompt picker | §4.16 | EX |
| UI-150 | Worktree setup | §4.2 | WT |
| UI-151 | Review worktree setup | §4.2 | WT |
| UI-154 | Compare previews | §4.2 | WT |
| UI-155 | Land worktrees | §4.2 | WT |
| UI-156 | Check pull requests? | §4.2 | WT |
| UI-157 | Pull request checks | §4.2 | WT |
| UI-158 | Create pull requests | §4.2 | CD |
| UI-160 | Instructions | §4.2 | EN |
| UI-161 | Engine versions | §4.4 | EN |
| UI-162 | Session library | §4.3 | TH |
| UI-163 | ACP agents | §4.4 | EN |

### 8.1 Cross-document conflicts found while writing this part
All items below were fixed at the source during the consistency pass (see CONSISTENCY_REPORT.md); they are kept as a record.
1. **Status and ring colours.** Resolved: PRODUCT_SPEC (F-SH-03, F-CD-12, F-DB-01, F-CN-04) now references the §1.2/§1.3/§2.30 token names (working sky, done green, waiting amber, error rose; ring speaking sky, thinking accent highlight, error rose).
2. **Branch prefix.** Resolved: every example uses `tandem/{slug}` (DECISIONS/CLAUDE.md).
3. **Shortcuts.** Resolved: §5 is the canonical map and F-SH-09 matches it (rail, maximize, Arrange, launch sheet, new shortcuts, Win/Linux intercept list, approval keys; `Alt+Space` is not bound on Windows/Linux).
4. **Settings pages.** Resolved: F-SH-04 now names UI-100 as the binding nav and carries the mapping; F-SH-08 points "Run setup again" at General.
5. **Title-bar credits** (a hosted build, signed in) open Settings › Account (UI-114); there is no "Billing" page.
6. **Onboarding model size.** Resolved: UI-04 shows the manifest size (670 MB, F-DI-04 and ARCHITECTURE §12.4 `approx_mb`).
7. **Light accent contrast.** Resolved: §1.2 now uses teal-700 (#0f766e) as the light-theme accent, `#7d8ba0` for dark `text-subtle`, and adds `status-*-text` tokens (§7.3). DECISIONS "Visual identity" records the light accent.
8. **Sidebar and rail sizes.** Resolved to PRODUCT_SPEC: sidebar 470px (320–900, or full-window), rail from a fully folded row's width (246px in Chromium) to 360px (§1.2 layout constants, §3.3, §3.5; the minimum was 180px until D-2701, and grows while jj or Preview rows show, D-2703).
