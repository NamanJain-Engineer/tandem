# vibezeo.com: the elite pass

**Why:** on 15 October the owner said the site looked basic: "we're selling a product so we need to make it look like a premium thing". This pass raises its craft to the level of the best developer-tool sites. It changes how the site is built and lit, not what it claims.
**Builds on:** `docs/WEBSITE_BRIEF.md` (branch `docs-briefs`). Its §1 hard constraints still hold, with `contact@vibezeo.com` and 0.7.2 as the shipped build. Its §3 study (10 October) is the base this one extends.
**Decisions:** D-1830 to D-1849 (BUILD_LOG).

---

## 1. What we studied (15 October)

Fourteen sites, read from their HTML and CSS and from 1440 px headless Chromium captures:
- BridgeMind (the category peer), Linear and Vercel;
- Raycast, Warp, Zed and Cursor;
- Resend, Supabase, Ghostty, Tailwind CSS and Factory.

Windsurf now redirects to another product, so it was dropped. These notes record patterns and techniques in our own words. No copy, asset, font or signature effect is taken from any of them.

### 1.1 Measured facts

| Site | Display type | Page ground | Edges and depth | Product framing | Motion |
|---|---|---|---|---|---|
| BridgeMind | 84 px / 600, −0.035 em; 56–72 px h2 | near-black `#0d0e11`, deeper bands | hairline `#2a2d34` plus a 7 % inset ring; window shadow with a −40 px spread | the live app in a 12–16 px frame; demos fade with a vertical mask | 700 ms reveals on a strong ease-out |
| Linear | 64 px / 510, −0.022 em; 48 px h2 | `#08090a`, monochrome | 5–8 % white hairlines; a 4-layer inset button recipe | full-width UI that bleeds off the bottom and fades | 100–160 ms hovers, 700 ms reveals |
| Vercel | 64 px / 400, −0.06 em | follows the OS (`#fafafa` light) | borders drawn as 1 px shadow rings | UI inside a bordered 12-column bento | 100–150 ms |
| Raycast | 64 px / 600; small 20 px two-tone h2s | `#07080a`, grain on its hero | layered "physical key" shadows; nested screen frames (12 px inside 19 px) | screens fade into the page with masks | 200–400 ms on an expo-out |
| Warp | 54 px semi-mono, −2 px | off-white with a dot grid | almost flat | a canvas inside ruled frames | 150 ms colour, 500 ms reveals |
| Zed | serif headings | warm paper with grain | keycap buttons (a darker 2 px bottom) | the app 1:1, square, on the grid | 150 ms, 700 ms reveals |
| Cursor | 26 px hero (product first), 52 px pricing | warm cream | two-layer ambient window shadow | windows overlapping on large art panels | 140–150 ms, 700 ms reveals |
| Resend | display serif up to 96 px | near-black, noise texture | glass buttons with a stacked glow on hover | code cards, 16 px radius | scroll-driven cues, `@starting-style` entrances |
| Supabase | 48 px / normal | themed, surfaces derived in OKLCH | flat hairline cards | line-art bento cells | honours reduced motion everywhere |
| Ghostty | 35 px, no negative tracking | `#0f0f11` | almost none | a real CSS terminal window | none but the hero |
| Tailwind CSS | up to 96 px, tighter tracking | white / gray-950 | outlines at 5–10 % | editor beside result | `@starting-style` |
| Factory | uppercase centred h1 | paper `#eee`, dark blocks | flat, sharp corners | a dark rounded video stage | character reveals |

### 1.2 What actually makes them feel premium

1. **Light, not colour.** The best dark sites are nearly monochrome. A single low-alpha bloom (6–18 %) or a grain layer gives the page depth. The accent is rationed to the primary action.
2. **Tight display type, quiet body.** Display tracking tightens as size grows (−0.02 to −0.04 em at 48–84 px), with a line height near 1. Body copy is 15–18 px with `text-wrap: pretty`, and eyebrows are mono caps at +0.1 em.
3. **Edges are lit.** Cards and controls carry a 1 px inner highlight on the top edge and a darker ring outside. The border is often a gradient: brighter at the top, almost gone at the bottom.
4. **Shadows are deep and soft.** Floating windows use a big negative-spread shadow (`0 40px 120px -40px`) layered over a tight contact shadow. Nothing uses a single grey box-shadow.
5. **The product is the hero, large and crisp.** Real UI is shown near 1:1, at 2×, in a nested frame (an outer rim around an inner screen). The bottom often melts into the page with a mask.
6. **Rhythm comes from changing shapes.** Sections alternate between a centred header, a split header (title left, lead right), a bento, a list and a full-bleed band. Generous padding (96–144 px) and hairlines that fade at both ends separate them.
7. **Motion is slow to arrive and fast to respond.** Reveals take 500–700 ms on an expo-out curve. Hovers take 100–200 ms. Everything stops under reduced motion.
8. **The header is glass.** A 64–73 px bar, transparent or blurred, gains a hairline only once the page scrolls.
9. **Pricing and download pages are calm and complete.** Plans sit side by side with one emphasised, and a sticky comparison table follows. Download pages show the icon, the version, the per-OS rows and a copyable command. None of the 14 sites publishes checksums; ours do.
10. **Weight is earned.** The heaviest pages (1.2 MB of JavaScript, dozens of trackers) aren't the ones that feel best. Every effect here is CSS.

### 1.3 Signature looks we must not echo
The brief's §1.2 list stands. This study adds:
- **BridgeMind:** the vertical light beam; the glowing frame round the demo; the white-to-blue-to-cream headline gradient; the gradient footer rule; uppercase pill buttons; its three-mode triad layout.
- **Linear:** isometric "FIG" line art; its pill primary button.
- **Vercel:** −0.06 em Geist and its bento at 6 px; the feature search in the pricing table.
- **Raycast:** the diagonal light-band hero and the floating nav island; the 3D keyboard.
- **Warp:** ruled figure frames and the violet dot band; uppercase mono buttons.
- **Zed:** the blueprint grid with diamond nodes and the blue footer watermark.
- **Cursor:** painted landscapes behind windows; the cream palette.
- **Resend:** the serif display with a fading fill; the diagonal beam; the rotating conic border.
- **Supabase:** the green second headline line; its line-art bento.
- **Tailwind CSS:** hatched gutters and rotated eyebrows.
- **Factory:** the uppercase centred h1 and the inset dark footer card.

---

## 2. Vibezeo's direction

Keep the identity (DECISIONS "Visual identity"): Inter and JetBrains Mono, teal (`#2DD4BF` dark, `#0F766E` light), slate neutrals, the two-ring mark and the teal→sky highlight. Make it feel lit, deep and exact.

### 2.1 The signature: the rings' light (D-1834)
The mark is two overlapping rings. At page scale they become our backdrop: two hairline circles, an echo pair wider and fainter, and the light gathered in the lens where they overlap, teal on one side and sky on the other.
- It is drawn as inline SVG plus two radial gradients, so it costs no request.
- It sits behind the hero, the page heroes and the closing call to action, and slowly turns (120 s a revolution) in the closing band.
- The light is on the page, never on a frame: product frames stay neutral. That keeps clear of the glowing-frame and light-beam looks.
- No film grain. A soft-light grain layer over the lit bands cost the home page about 80 ms of simulated largest paint (2.05 s against the 2.0 s budget), so it was dropped (D-1840). The blooms are wide and low-contrast enough not to band.

### 2.2 Tokens (`packages/ui/src/styles/site.css`)
- **Light (D-1830):** `--site-glow-a/b` (teal and sky blooms), `--site-ring` and `--site-ring-lit`.
- **Edges and fills (D-1830):** `--site-edge-top/bottom` for a gradient border, `--site-edge-hi` for a 1 px inner highlight, and `--site-fill-top/bottom` for light at the top of a card.
- **Elevation (D-1830):** `--site-elev-1` for cards, `-2` for a lifted card and `-3` for product frames. Each is layered, and each has a light-theme value.
- **Lines:** `--site-line` and `--site-line-strong`; the `horizon` hairline fades at both ends.
- **Radii (D-1833):** the app's 6 px controls stay. Cards are 12 px, frames 16 px and the screen inside a frame 10 px, so the radii nest.
- **Type (D-1832):** see §2.3.
- **Motion (D-1831):** an expo-out curve (`--ease-out-site`); 700 ms reveals with a 6 px blur; 180 ms hovers. A test fails if anything animates outside `prefers-reduced-motion: no-preference`.
- **Themes:** every light, edge and depth token the dark theme sets has a light value, and a test keeps the lists equal. Contrast tests cover the new button gradient stops.

### 2.3 Type (D-1832)

| Token | Size | Tracking | Use |
|---|---|---|---|
| `display-3xl` | 44 → 76 px | −0.042 em | home h1 |
| `display-2xl` | 40 → 64 px | −0.038 em | closing CTA |
| `display-xl` | 36 → 56 px | −0.036 em | page h1s |
| `display-lg` | 30 → 48 px | −0.032 em | section h2s |
| `display-md` | 22 → 30 px | −0.024 em | row titles |
| `lead` | 17 → 21 px | −0.006 em | leads |
| `stat` | 40 → 64 px mono | −0.04 em | figures |

- Inter's optical sizes draw the display cut automatically. The site turns on Inter's open digits and straight-legged l (`cv01 cv09 cv11 ss03`).
- Headings use `text-sheen`: full ink at the cap height, slightly softer at the baseline. It stays monochrome.
- Eyebrows are mono caps at +0.12 em after a short lit rule.

### 2.4 Components
- **Buttons (D-1835):** `siteButton()` in `packages/ui`.
  - The primary has the accent as a gradient, a lit top edge, a dark ring and a soft accent glow.
  - The secondary is glass with a hairline.
  - Both keep 6 px corners. No pills.
- **Header (D-1836):** a glass bar with links centred, and a hairline that appears on scroll. The phone menu is a sheet that drops from the top (Popover API, `@starting-style`).
- **Footer (D-1836):** a lit horizon, the bloom, the brand with the current release, four columns and the owner line.
- **Sections (D-1837):** three header shapes (left, centre, split title/lead), an optional horizon divider, and 80–144 px padding.
- **Surfaces (D-1830):** `surface`, `surface-raised` and `surface-hover` (lift plus light gathering at the top), and the `Card` component.
- **Product frames (D-1834):** `ProductShot` puts every capture in a 6 px bezel with a lit edge and the e3 shadow. Each one names the build it shows (`Vibezeo 0.7.2 · Linux`) and offers a 2× source. The live demo sits in the same bezel, which carries no transform or filter so the demo's full-screen mode still works.
- **CTA band (D-1837):** the mark on a small lit tile at the centre of the turning rings.
- **Keycaps:** a lit cap with 3 px of travel.

### 2.5 Pages
Every marketing page uses the frame, the hero and the sections above: home, pricing, download, changelog (index and versions), about, security, privacy, terms, demo and 404. The docs (Fumadocs) take the same tokens, header feel, code frames and footer (§3). The transparency page (`app/transparency/page.tsx`) belongs to the account portal and isn't part of the static export, so it keeps the portal's look.

### 2.6 Product imagery (D-1838)
Captures are the real app, taken at 2× and served as AVIF and WebP with a 1× fallback. Each is labelled with the build it shows. Stale captures (`code-mode` and `launch-grid` showed 0.1.0; the demo still showed 0.5.0) are retaken from 0.7.2.

## 3. Docs
The docs keep Fumadocs for their content (MDX, the page tree and the table of contents) but no longer use its client layout (D-1819, D-1839).
- **Frame:** `DocsShell` renders them as static pages inside the site's own header and footer. A sticky sidebar groups pages under mono labels and lights the active one. "On this page" sits on wide screens, and lit previous/next cards close each page. Phones get the sidebar from a popover.
- **Prose:** Fumadocs' typography plugin, mapped to the site's tokens, with display-size titles, hairline h2s, key caps and lit tables.
- **Script:** none (D-1843). Links are plain, each at its slash URL, and moving between pages uses the site's cross-document view transition.

## 3a. Phase 2: ownership first, and motion with a purpose (D-1841, D-1842)
After the first pass, the owner widened the brief: restructure freely, animate where it helps, and lead with neutrality and ownership.
- **Hero:** "Your agents. Your machine. Your plan."
  - The lead counts the engines from code.
  - Four facts follow, each true of 0.7.2: agent CLIs run unmodified; on your own subscriptions; runs on your computer; no account, nothing sent to us.
  - No other product is named.
- **Home order:** hero, live demo, how data moves (moved up), engines, modes, everything else, Built with Claude, proof figures, founder, pricing, FAQ, the ask. The release tiles and the roadmap moved to /changelog/, and the video to /demo/. Their four headline features joined the capability index.
- **Motion (CSS only, transforms and a stroke dash, all off under reduced motion):**
  - **Hero rings:** the two rings draw themselves in as the page opens.
  - **Engine orbit:** the 11 engines' monograms orbit them once every three minutes, as one composited layer.
  - **Data flow:** packets travel the lines that really carry data: to and from the engine vendors, and to services you connect. The line to Vibezeo's servers stays still.
- **Performance:** a phone gets an 800 px demo still (18 KB instead of 36 KB), since it loads before the largest paint. The hero's lead paints at once rather than fading in, because it is the largest paint now that the h1 is three short lines.

## 4. Budgets kept
- No new requests above the fold.
- No new scripts.
- The CSP is unchanged.
- Lighthouse CI on its five URLs, CLS ≤ 0.02 (numbers in BUILD_LOG).
