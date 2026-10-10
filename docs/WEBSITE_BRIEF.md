# vibezeo.com redesign brief

**For:** the engineer rebuilding `apps/web` (Next.js 15.5 static export, Tailwind 4.3, React 19.3, Fumadocs 15.8).
**Basis:** the code on branch `release-0.5.0` (commits `ebbe472` and `3097954`; the 0.5.0 release is still in progress there), its tests, `CLAUDE.md`, `build-kit/DECISIONS.md`, `build-kit/UI_SPEC.md`, `docs/OPERATING.md`, and a study of BridgeMind's site and 11 other sites (captured on 10 October 2026, §3).
**Scope:** research only. Nothing in the repository was changed.

---

## 0. The short version

The site is honest, static and well tested. It also looks like the desktop app's settings screen blown up to page size. The h1 is 30 px and body text is 13 px. Nineteen identical cards make up the longest section. The site is dark-only, the hero screenshot is too small to read, phones get no navigation, and ~100 KB gzip of framework JavaScript runs on pages that need none. A few published facts have also gone stale (§2.3).

The redesign keeps everything that makes the site trustworthy and changes how it looks and reads:

1. **Make the real app the hero.** Under a large headline, the existing live demo (`/demo/app/`) appears at full container width. It starts as a still image and becomes the live app as the visitor scrolls. The demo is drawn 1:1 (1280 px) on large screens, so its text is crisp.
2. **Use marketing-scale type, with the brand kept.** Headings use Inter's optical-size cut (Inter Display), from 40 to 64 px. Body text is 16 px, labels use JetBrains Mono, and the accent stays teal. The teal→sky highlight and the faint grid already used in the OG image and video poster carry over.
3. **Support light and dark themes with CSS only.** The site follows the OS through `prefers-color-scheme`, so it needs no script and no storage. Product media sits on a dark "stage" in both themes.
4. **Give each section its own shape.** The home page becomes 15 sections, each with a different layout: hero, live demo, proof band, engines, mode rows, a "New in 0.5.0" bento generated from `CHANGELOG.md`, a compact capability index, a privacy data-flow diagram, Built with Claude, founder, video, roadmap, pricing, FAQ and a closing CTA.
5. **Generate every number and shortcut from code.** Figures, prices, limits, engine monograms, keyboard shortcuts, file sizes, checksums and release highlights all come from code, so the site can't drift from the build.
6. **Rebuild the other pages from the same parts.** Pricing gets a full comparison table generated from `LIMITS`. Download follows the Zed and Ghostty layout (icon, version, command, checksum). The changelog becomes a release rail. Docs get grouped navigation and the site's legal footer.
7. **Motion is CSS-only and progressive.** It uses scroll-driven reveals, `@starting-style` and cross-document view transitions, and turns off entirely under `prefers-reduced-motion`.
8. **Performance target is Lighthouse 95+.** Fonts are self-hosted through `next/font/local` with metric-matched fallbacks. Images use `<picture>` AVIF/WebP with fixed sizes. Below-the-fold work is lazy. An optional post-export step removes the unused Next runtime from marketing pages.
9. **Fix the honesty bugs first** (Phase 0). The video caption claims a build the video doesn't show, the demo still shows an old version, Pro is said to have unlimited panes but the code enforces 64, and the Linux requirement lists Fedora although only a `.deb` exists.
10. **Keep every test passing.** Only two test edits are proposed, both of which keep the test's intent (§14).

---

## 1. Hard constraints (read first; they override any design idea below)

### 1.1 Every claim is true and checkable in the shipped build
- The shipped build is **Vibezeo 0.5.0 for Linux x86-64, a `.deb`, Free plan, no account, nothing sent to our servers** (`CHANGELOG.md` 0.5.0 summary, `lib/release.ts`).
- **Never show:** invented customers, logos, testimonials, quotes, user, star or download counts, revenue, reviews, benchmarks, ratings, dates or release timelines.
- **Never show features the build lacks:** Conductor, the phone companion, WSL on Windows, macOS or Windows builds, and hosted features as if live. Hosted features are "built, live when accounts open". State plan limits wherever they apply. `docs/OPERATING.md` §9 says the same.
- **Numbers come from code, never typed into JSX.**

  | Fact | Source |
  |---|---|
  | Figures | `PROGRESS` (`lib/progress.ts`) |
  | Prices | `PLANS` (`lib/pricing.ts`) |
  | Limits | `LIMITS` and `PRICES` (`@tandem/protocol/plans`) |
  | Engines | `ENGINES` plus `crates/tandem-engines/registry.json` (`label`, `logo`) |
  | Shortcuts | `@tandem/protocol/shortcuts.json` |
  | Releases | `RELEASES` (`CHANGELOG.md`) |
  | Builds | `DOWNLOADS` plus `public/downloads/SHA256SUMS.txt` and the file's size on disk |

- **Media are real captures, labelled with the build they show.**
  - Anything not filmed from the app is labelled "Illustration".
  - Sped-up footage says so.
  - Simulated agents appear only inside the labelled demo section (`#try`).
- **What can stand in for social proof** (all true today):
  - 2,508 automated tests and 109 end-to-end suites (`PROGRESS`).
  - 11 supported engines.
  - SHA-256 for every download.
  - A public changelog with platform tags.
  - Signed, tamper-refusing updates.
  - "Built with Claude Code" (recorded in the commit history).
  - A named founder with an X link.

### 1.2 Nothing taken from other products
- **No names, branding, copy, code, assets, fonts, illustrations or signature effects from other products.**
  - This covers BridgeMind and anything under `build-kit/reference/`.
  - DECISIONS: Vibezeo is a clean-room product in BridgeMind's category. Never use "BridgeMind", "BridgeVoice", "Bridge", "BridgeSpace" or "Hey Bridge".
  - Other sites are inspiration for layout patterns and engineering techniques only.
- **Do not echo these signature looks:**
  - BridgeMind's vertical light beam behind the hero and its glowing demo frame.
  - Cursor's oil-painting backdrops behind app windows.
  - Raycast's red striped 3D hero.
  - Vercel's prism or triangle and its pixel fonts.
  - Stripe's ribbon gradient.
  - Arc's wavy section edges.
  - Zed's blue blueprint footer and giant outlined wordmark.
  - Warp's bracketed "fig." frames.
  - Their typefaces: Sora, Geist, CursorGothic, Söhne, GT Walsheim, Berkeley Mono, Azeret, IBM Plex Serif.
- **Vibezeo's own identity** (DECISIONS "Visual identity"):
  - Inter plus JetBrains Mono.
  - Teal accent: `#2DD4BF` in dark, `#0F766E` in light.
  - Slate neutrals.
  - Radius 8 px on panels and 6 px on controls.
  - 1 px borders.
  - Status colours: sky for working, amber for waiting, green for done, slate for idle, rose for error.
  - The two-ring mark.
  - The teal→sky highlight, faint grid and soft teal glow from `public/og.png` and the video poster.
- **No vendor logos.** Engines appear as Vibezeo's own letter monograms (`EngineGlyph`, the registry's `logo` letters), with exact names and the existing disclaimer ("Product names belong to their owners…"). Sources: `HUMAN_TODO.md` line 36 and UI_SPEC §1.6.
- **No macOS traffic-light dots on window frames.** The app runs on Linux and draws its own title bar. Fake chrome would misrepresent the platform.
- **The demo label is Vibezeo's own spec text.** "Interactive demo · sample data" is required by F-WB-01 (PRODUCT_SPEC §4.17) and the tests. Keep it exactly, but don't copy BridgeMind's arrangement around its own similar label (label left, Reset and Explore right, glowing frame).
- **Copy is Vibezeo's own.** Reuse existing site copy, which has already been reviewed for honesty, and the UI_SPEC §6 voice. Never paraphrase a competitor's headline.

### 1.3 First-party only, and the pages work without JavaScript
- **No third-party resources of any kind.**
  - No third-party scripts, stylesheets, fonts, images, frames, embeds or analytics.
  - No cookies, `localStorage` or `sessionStorage` on any site page, docs included. `e2e-site/wb01.spec.ts` checks every page.
  - `SITE.analytics` stays `null` (D-132).
- **Script allowlist.** Source: `app/site.test.tsx`, "F-WB-01 AC2".
  - On `/`, exactly `<script src="/demo/embed.js" defer="">`.
  - On `/download/`, exactly `<script src="/download/detect.js" defer="">`.
  - On every other page, none.
  - Menus, accordions, theming and motion use HTML and CSS only: the Popover API, `<details>`, `prefers-color-scheme`, scroll-driven animations.
- **No `<iframe>` in server markup.** Only `embed.js` creates the demo frame.
- **CSP stays as is.** The CSP in `vercel.json` (`default-src 'self'`, `frame-ancestors 'none'` except `/demo/app/`) does not change.
- **Fonts are self-hosted** from the fontsource packages already in the workspace. No Google Fonts or other remote CSS.

### 1.4 Strings, ids and attributes the tests pin
Keep these verbatim, or change the test and record why its intent still holds (§14).

| Page | Must contain |
|---|---|
| `/` | Content: `SITE.tagline`; `Released · ${RELEASE.version} for Linux`; each of the 11 `ENGINES` as a whole text node (`>Claude Code<`); `mailto:hello@vibezeo.com`. |
| | Section ids: `id="demo"`, `claude`, `features`, `engines`, `roadmap`, `pricing`, `team`, `faq`. |
| | Links: `href="/download/"`, `href="/demo/"`, `href="/changelog/"`, `href="/pricing/"`, `href="/about/"`; the text "Download for Linux". |
| | Images: `src="/screens/code-mode.png"`, `src="/screens/launch-grid.png"`; `<img … src="/demo/still.jpg" …>`. |
| | Demo section: `data-demo-embed=""`; "Interactive demo · sample data"; "This is Vibezeo itself, running in your browser with sample data. Click around."; "agents are simulated"; "Explore full screen"; "Tap to explore". |
| | Video: `<video … controls>` (never `muted`) with `poster="/screens/product-poster.jpg"`; sources `/screens/product.webm` and `/screens/product.mp4`; the strings "2.3× speed", "illustration" and "Watch it run (1 min)". |
| | Figures: `PROGRESS.automatedTests` formatted (`2,508`); `milestones in the ${RELEASE.version} release`. |
| | Built with Claude: "Built with Claude", "--resume", "Built, live when accounts open", "go live when accounts open". Never "not built yet" or "approve or deny tool requests right on the card". |
| | Plan sentence: "Free threads run Claude Code and Codex; Cursor Agent, Grok and OpenCode threads need Pro." |
| | Principle sentence: "The goal is that Linux, macOS and Windows get the same features on the same day". |
| | Founder: `<a href="https://x.com/jus_eng_njainam" …>Naman Jain</a>` (href first, text exact) and "Founder, Vibezeo". |
| `/download/` | Ids and links: `id="linux"` and no `id="macos"` or `id="windows"`; a link to each available asset and none to unavailable ones; `href="/downloads/SHA256SUMS.txt"`; `id="getting-started"`. |
| | Checksums: `data-sha256="<file>"` plus the hash. |
| | Copy: "isn't code-signed, so the checksum is how you confirm"; "contact our servers". |
| | Script: the `detect.js` tag and `data-os-note=""`. |
| | The link name "Download for Linux (.deb)" (Playwright). |
| `/pricing/` | `id="free"`, `pro`, `team`; "download Vibezeo", "start with", "Free is available now", "$290 a year"; `data-credit-price="<key>"` per credit row; "switch on with a Pro account (the download, on the Free plan, never uses them)". |
| `/changelog/` | `id="m14"` … `id="m0"`; links to every version page, newest first. Version pages: `<h1`, `<h2`, "Added", rendered Markdown (`<strong>Files.</strong>`, `<code>.gitignore</code>`), platform words, never `[lin]`. |
| `/demo/` | `href="/demo/app/"`, "simulated", "nothing you type leaves your browser". |
| Every page in `PAGES` | Exactly one `<h1`; `href="/privacy/"`, `/terms/`, `/security/`, `/about/`; `hello@vibezeo.com`; a canonical URL. |
| Home, Download, Pricing | Never: `/early.access/i`, "at/before/with launch", "(for\|on) macOS, Windows and Linux", ".dmg", ".msi", "are next", "Coming soon", "on sale yet", "Planned", "isn't code-signed yet", "testing early builds". |
| All pages | Never the word "Tandem" (the code's working name), never `github.com/NamanJain-Engineer/tandem`, never a year. |

### 1.5 Test gotchas that will bite a redesign
- **The "no year" regex scans the whole markup, attributes included.** It is `\b(19|20)\d{2}\b`.
  - These fail: `width="1920"`, `srcset="… 2048w"`, a class like `duration-2000` or `max-w-[1920px]`, and an unformatted figure like `2013`.
  - Instead, use 1440 or 2880 widths, write `2s`, and format numbers with `toLocaleString("en-US")`.
- **"sample data" may appear only inside the `#try` section.**
  - The test cuts `<section id="try" … </section>` with a lazy regex.
  - Keep `id="try"` as the section's first attribute.
  - Never nest another `<section>` inside it.
  - Label illustrations elsewhere "Illustration", never "sample data".
- **Engine names must be whole text nodes.** `<li><span aria-hidden>CC</span>Claude Code</li>` passes; `Claude Code CLI` does not.
- **One h1 per page.** The mobile menu, the footer and cards use no `h1`.
- **The demo must stay below the fold in the first e2e test.**
  - `e2e-site/demo-embed.spec.ts` AC1 opens `/` at 1280×400 and expects no `/demo/app/` request within 1.5 s.
  - The loader's `rootMargin` is 200 px, so the top of `[data-demo-embed]` must sit at least ~620 px from the top of the page at that size.
  - With the hero in §6.2 (≈680 px tall at 1280 px wide) plus the demo section's own label row, the slot starts at about 740 px. Keep that margin, and re-run the spec after any hero change.
- **`embed.js` must keep its exact markers.** It must contain `IntersectionObserver` and the literal `"/demo/app/?embed=1"`. It must not contain `http(s)://`, `document.cookie`, `localStorage`, `sessionStorage`, `sendBeacon` or `fetch(`.
- **`renderToStaticMarkup` doesn't insert `<!-- -->` between text nodes, but the live HTML does.** OPERATING §7 strips them before grepping. Keep "Released · {version} for Linux" in one element.

---

## 2. The current site

### 2.1 How it is built
- **Static export.** `SITE_EXPORT=1 next build` only exports `*.site.tsx` and `*.site.ts` pages (`output: "export"`, `trailingSlash: true`) to `apps/web/out`. Vercel serves it with the headers in `vercel.json`. The account portal (`*.tsx` without `.site`) stays out of the export.
- **Marketing pages:**
  - `/` (`app/page.site.tsx`, 13 sections), `/download/`, `/demo/` (which opens `/demo/app/`, the desktop UI built against a simulated core by `pnpm --filter desktop build:demo`), `/pricing/`, `/changelog/` and `/changelog/[version]/`, `/about/`, `/privacy/`, `/terms/`, `/security/`, and the 404.
- **Docs.** `/docs/[[...slug]]` uses Fumadocs with 16 MDX pages and its own layout, without `SiteFrame`.
- **Machine-readable routes:** `/llms.txt`, `/llms-full.txt`, `/openapi.json`, `/api/v1/site-info.json`, `/sitemap.xml`, `/robots.txt`, `/.well-known/security.txt`.
- **Frame.**
  - `components/site/SiteFrame.tsx` holds the header (brand, 7 links and a Download button) and the footer (copyright, a legal nav and the email).
  - `ProsePage` wraps the legal pages.
  - `InlineMarkdown` renders changelog bullets.
- **Data.**

  | File | Holds |
  |---|---|
  | `lib/site.ts` | `SITE`, `ENGINES`, `SITE_PAGES`, the mailto link, `securityTxt` |
  | `lib/progress.ts` | `PROGRESS` and `MILESTONES` |
  | `lib/release.ts` | `RELEASE` and `DOWNLOADS` |
  | `lib/pricing.ts` | `PLANS` and `CREDIT_ROWS` |
  | `lib/changelog.ts` | `RELEASES`, parsed from `CHANGELOG.md` |
  | `lib/manifest.ts` | `SHA256SUMS.txt` |
  | `lib/docs/tables.ts` | Generated tables from the registry, shortcuts, settings and `LIMITS` |
  | `lib/discovery.ts` | llms.txt, site-info and OpenAPI |

- **Styling.**
  - `app/globals.css` imports `@tandem/ui/tokens.css` (the app's Tailwind 4 theme, from UI_SPEC §1.2) and `@tandem/ui/fonts.css` (Inter Variable and JetBrains Mono Variable through fontsource).
  - `app/docs/docs.css` maps Fumadocs' colours to the same tokens.
  - `app/layout.site.tsx` hard-codes `<html data-theme="dark">`.
- **Media.**
  - `public/screens/code-mode.png` (1440×900) and `launch-grid.png` (544×570).
  - `product.mp4` and `product.webm` (64 s, 13 MB and 11 MB) with a poster.
  - `public/demo/still.jpg` (1280×800).
- **Scripts.**
  - `public/demo/embed.js` frames the demo when it's near, scales it, opens it full screen with Esc to close, and uses "Tap to explore" under 1024 px.
  - `public/download/detect.js` writes the OS note.
- **Strengths to keep:**
  - Rigorous honesty.
  - Generated facts.
  - A real interactive demo.
  - Machine-readable discovery.
  - Strict privacy and CSP.
  - Tests that encode all of this.

### 2.2 Weaknesses: look and structure
Measured in Chromium against `apps/web/out` at 1440×900.

1. **The type is app-sized.**
   - The h1 is 30 px/600 and h2s are 24 px.
   - Body text is 13 px: `text-base` comes from the app's tokens, which are built for a 13 px desktop UI.
   - Card text is 13 px too.
   - The sites studied use 48–84 px display type and 16 px body text (§3.1).
2. **The header and content are misaligned.** The header uses `max-w-5xl` (1024 px) and the content `max-w-6xl` (1152 px), so the logo starts 64 px to the right of the content edge.
3. **The hero is cramped.**
   - Three CTAs wrap, leaving "Watch it run (1 min)" alone on a second row.
   - The 1440×900 capture is squeezed into a ~580 px column, which makes the four terminals unreadable.
   - The badge is 10 px uppercase.
4. **The rhythm is monotone.**
   - 11 sections repeat the same eyebrow, h2, `border-t` and card grid.
   - "Built for running many agents at once" is a wall of 19 equal cards, so flagship modes weigh the same as "Signed updates".
5. **The site is dark-only.** DECISIONS asks for "Dark and light themes from day one", and a full light token set already exists.
6. **Some glyphs fall back to another font.** "→", "✓", "⌘" and "⇧" are not in any Inter fontsource subset (checked against the unicode ranges in `opsz.css`), so they render in a system font. Every CTA arrow and pricing tick looks off.
7. **Phones get no header navigation.** The nav is `hidden sm:flex` with no menu. Under 640 px only "Download" shows, and Pricing, Docs and Changelog are reachable only from the footer.
8. **The docs feel detached.**
   - Fumadocs' default skin has no site header or brand mark.
   - The sidebar is a flat list of 16 items with no groups.
   - There is no search.
   - **Docs pages have no legal links**, unlike every other page.
   - `<kbd>` is styled like inline code.
9. **The changelog is a plain list.** It is a list of links plus a milestone list. Version pages have no next or previous navigation and no version metadata, and the bold lead phrases in `CHANGELOG.md` aren't used.
10. **Pricing is thin.**
    - The Team card is nearly empty ("Everything in Pro").
    - There is no comparison table, so the enforced limits appear only in the docs.
    - The trial and money-back lines are visually buried.
11. **The download page is minimal.**
    - It is one 448 px card with a four-line hash.
    - It shows no file size, no install command block and no app icon.
    - The requirements read as fine print.

### 2.3 Honesty and currency bugs (fix in Phase 0, before the redesign)
| # | Finding | Evidence | Fix |
|---|---|---|---|
| H1 | The video caption says "Every shot of the app is a screen recording of the Linux **0.5.0** build", interpolating `RELEASE.version`. | The video was rendered on 7 Oct for 0.1.0 (commit `b8c4cac`). Its poster reads "Released · 0.1.0 for Linux" and the footage shows the M5 build. | Add `lib/media.ts` with `VIDEO.recordedWith = "0.1.0"` and use it in the caption. Or re-record with 0.5.0 (`media/product-video`). Owner chooses (§15). |
| H2 | `public/demo/still.jpg` shows "v0.3.0" and "**Online**" in the status bar. Every phone visitor sees this image. | Open the file. D-816 removed "Online"; the demo now reads 0.5.0. | Re-capture from the 0.5.0 demo (§7.1) and add it to the OPERATING §6 checklist. |
| H3 | Pricing says Pro has "Unlimited panes and workspaces". | `LIMITS.pro.panes = 64` in `packages/protocol/src/plans.ts`, and the docs table shows 64. | Write "Up to 64 live panes · unlimited workspaces", or change `LIMITS` (owner). Add a consistency test. |
| H4 | The Linux requirement lists "Fedora 38+". | Only a `.deb` with apt steps is offered (`lib/release.ts`). | Drop Fedora, or ship an AppImage (owner). |
| H5 | "Split with Mod+D (right)" appears on `/download/` and in the docs. | On Linux with a terminal focused, split right is **Ctrl+Shift+E** (`shortcuts.json` `pcTerminal`). Mod+D only works outside terminals. | Render keys from `shortcuts.json` (`Keys` component, §6.12). |
| H6 | Privacy says "has no Vibezeo accounts and **don't** contact our servers". | `app/privacy/page.site.tsx` | Change to "doesn't". |

### 2.4 Weaknesses: performance
- **Framework JavaScript the pages don't need.**
  - Every marketing page ships Next's runtime: ~100 KB gzip on `/` (chunks `b7f2936b…` 54 KB, `43-…` 46 KB, `main-app`, `webpack`).
  - It also ships an inline RSC payload: 51 KB of the 84 KB home HTML.
  - The pages have no client components, and hydration once wiped the demo frame (D-128).
- **Below-the-fold images are preloaded.** React 19 preloads every non-lazy `<img>`, including `launch-grid.png` far below the fold.
- **Fonts are found late.** They're discovered through CSS `@font-face`, never preloaded, and have no metric-matched fallback, so swapping fonts can shift layout.
- **The video loads early.** `<video preload="metadata">` starts fetching a 13 MB file on load.
- **The demo loads right after page load on desktop.** At 1440×900 the stage is within 200 px of the first viewport, so the 2 MB demo (600 KB gzip) loads right after `load` on every desktop visit. That is acceptable for the centrepiece, but it's a reason to keep the stage just below the fold.

---

## 3. Inspiration study

**Method.**
- Headless Chromium through Playwright, at 1440×900 in the dark colour scheme. Vercel, Linear and Zed were also captured in the light scheme.
- 12–14 viewport screenshots per page, plus computed styles for the h1, h2, body and CTAs, the loaded fonts, script origins and CSS features.
- WebFetch was blocked by bot protection on bridgemind.ai; Chromium loaded it.
- Captured 10 October 2026. These sites change often.

### 3.1 Measured facts (home pages)
| Site | h1 size / weight / tracking | Display font | Page bg (dark) | Header | Primary CTA | Scripts (3rd-party origins) |
|---|---|---|---|---|---|---|
| BridgeMind | 84 px / 600 / −0.035 em, gradient text | Sora | `#0D0E11` | 64 px fixed | 44 px white pill | 31 (Google Tag Manager) |
| Linear | 64 px / 510 / −0.022 em | Inter Variable | `#08090A` | 73 px fixed | small header pill; no hero button, a "New: … →" link instead | 17 |
| Vercel | 64 px / 400 / −0.06 em | Geist | `#000` (follows OS: `#FAFAFA` light) | 64 px sticky | 40 px pill | 108 (1.2 MB JS) |
| Raycast | 64 px / 600 | Inter | `#07080A` | floating pill | 36 px, r 8 | 61 (4) |
| Warp | 54 px / 400 / −2 px | Azeret SemiMono | light | 64 px | 34 px, r 4 | 84 (21) |
| Zed | serif headings, dense | IBM Plex Serif | `#111216` | 28 px sticky | 34 px, r 4 | 62 |
| Cursor | 26 px / 400 (product-first) | CursorGothic | `#14120B` | 52 px fixed | 43 px pill | 174 (10) |
| Stripe | 48 px / 300, two-tone | Söhne | light | 76 px | 48 px, r 4 | 85 |
| Framer | 54 px / 500 / −0.04 em | GT Walsheim | `#000` | — | ~30 px, r 6–8 | 45 (11) |
| **Vibezeo today** | **30 px / 600 / −0.01 em** | Inter Variable | `#060A12` | 65 px static | 38 px, r 6 | 33 (Next runtime) |

Takeaways:
- Display type is 48–84 px, body is always 16 px, and tracking runs from −0.02 to −0.04 em at display sizes.
- Every peer pairs a sans with a mono for labels.
- Only Vercel follows the OS theme on its marketing page.
- Most peers carry heavy third-party tracking. Vibezeo's zero is a feature to keep and to mention.

### 3.2 What each site does best, and what to take from it

**BridgeMind** (bridgemind.ai; the category peer)
- **Hero.**
  - A left-aligned 2-line, 84 px headline.
  - A one-line, 18 px muted subhead.
  - The Pro price stated right above the CTAs.
  - Two pill CTAs.
  - A platform plus bring-your-own-subscription note under the buttons.
- **How the product is shown.**
  - The real app runs live with sample data **directly under the hero**, in its own window chrome, with Reset and Explore controls.
  - The caption says it's live and clickable.
  - Smaller live fragments (an accounts diagram, a voice widget) each carry an honest line about what is simulated ("no microphone access…").
- **Page flow.**
  1. Three mode cards (Agent, Code, Thread).
  2. A mode strip of UI thumbnails.
  3. Split sections: a big headline that fades in on scroll on one side, live UI and two sub-points on the other.
  4. Voice, community and testimonials.
  5. A 3-tier pricing table with a monthly/annual toggle, a highlighted middle plan, a money-back line, and a "Bring your own AI / Runs on" box.
  6. A two-column FAQ: headline and contact links left, accordion right.
  7. A huge closing CTA.
  8. Download cards per OS.
  9. A four-column footer.
- **Tech.**
  - Fixed 64 px header.
  - Inter 16/24 body.
  - Google Tag Manager, so not a privacy model.
- **Take:**
  - The live demo as the centrepiece under the hero (we already have it; give it the space).
  - An honest caption on every simulated element.
  - Price transparency near the CTA.
  - The two-column FAQ.
  - Per-OS download cards with a requirement line.
- **Avoid:**
  - Community, revenue and testimonial blocks (we have none, and we never invent any).
  - The beam and glow effects, and pill buttons.
  - Sora, and every word or section title of theirs.

**Linear**
- **Hero.**
  - A 64 px, weight 510 headline at line-height 1.0 and −0.022 em.
  - One 15 px muted subhead.
  - A "New: <feature> →" link right-aligned on the subhead's baseline.
  - A large real UI in slight perspective that fades into the page.
- **Statements and figures.** Two-tone paragraphs (a white lead clause followed by a muted continuation) and mono "FIG 0.1" labels on line illustrations.
- **Product-area sections.**
  - A 48 px two-line title on the left, the description on the right, then "Learn more →".
  - Full-width UI.
  - A "Features" list of small links.
- **Changelog on the home page.** A horizontal rail of 4 entries, each with a dot marker, mono date, title, a 2-line excerpt and "View all".
- **Pricing.**
  - Plan columns divided by hairlines instead of boxed cards, with CTAs aligned along one row.
  - A full comparison table with a sticky plan header and grouped rows.
- **Changelog page.** A sticky date column with a dot on a vertical rail, a content column, and collapsible "Fixes" and "Improvements" groups.
- **Take:**
  - Two-tone statements.
  - The "What's new" link in the hero.
  - Title-left, description-right section headers.
  - A home "Latest release" block.
  - The sticky comparison table.
  - The changelog rail (without dates; see §1.4).

**Vercel**
- Very tight 64 px display type and 56 px h2s.
- Monochrome, with hairline grid lines framing a 12-column layout.
- A "Recently shipped" bento: one tall tile and two stacked, one of them a terminal transcript.
- A centred closing CTA and a dense 6-column footer.
- Follows the OS theme through CSS.
- **Take:**
  - System-following light and dark.
  - A "new in this release" bento with a terminal tile.
  - Hairline structure.
- **Avoid:** its JS weight (1.2 MB).

**Raycast**
- **Hero.**
  - A centred hero.
  - One "Download for Mac" button with a **mono requirement line under it** and alternative install links ("Install via Homebrew | Download V1").
- **Header and storytelling.**
  - A floating, rounded, blurred header.
  - Product storytelling through an interactive dock that swaps UIs.
  - 20 px two-tone section heads.
  - **Oversized keycap illustrations** (⌥ + ⌘ + L) for shortcuts.
  - A sentence that lights up phrase by phrase on scroll.
- **Pricing.** A monthly/yearly toggle, a raised middle card, and "Compare all features ↓" leading to a full table.
- **Changelog.** A version chip and date column, and bold lead phrases on bullets.
- **Take:**
  - A CTA with a requirement line and an alternative method.
  - Keycaps generated from `shortcuts.json`.
  - Bold-lead bullets (`CHANGELOG.md` already writes them).

**Warp**
- A monospace display headline and a technical-document look: numbered features, `# SECTION` labels, and tabs that switch code samples.
- A sticky numbered in-page index replaces the header after scrolling.
- **Take:** mono eyebrows and numbered steps.
- **Avoid:** bracketed figure frames (their signature) and the 21 third-party script origins.

**Zed**
- **Home.**
  - An editorial 1024 px column framed by side hairlines on a faint grid.
  - Real screenshots throughout.
  - A **compact four-column capability index** (a title and one line each) instead of large cards.
  - A "From the team" letter.
  - A search hint in the header.
- **Download page.**
  - App icon, big version number, date and "View changelog →".
  - Per-OS rows with an architecture selector and requirement line.
  - A Linux command block with copy.
- **Docs.** A grouped, collapsible sidebar, an "On this page" TOC and a copy-page button.
- **Take:**
  - The capability index.
  - The download-page anatomy (minus the date).
  - A founder note.

**Cursor**
- **Hero.** Product-first: a small headline above a very large app frame, and OS-detected download pills.
- **Feature rows.** Two-tone 22 px title and body on the left, a framed UI on the right.
- **Install command.** Shown with a copy button.
- **Home changelog.** Cards with a date and title.
- **Closing CTA.** "Try … now." at 72 px with the detected-OS download.
- **Download page.** Per-surface cards, an OS-by-architecture matrix and old-version accordions. The page carries 174 scripts and 10 tracker origins.
- **Take:**
  - Product-first hero proportions.
  - Feature rows with framed media.
  - A closing CTA.
- **Avoid:** the painted backdrops and the trackers.

**Stripe** (light reference)
- A two-tone 48 px/300 headline.
- Vertical hairline rails framing the column.
- A bento of product cards.
- A stat row of big real numbers with small labels.
- **CSS scroll-driven animations (`animation-timeline`) and `<picture>` on every image** (40 pictures).
- **Take:** scroll-driven reveals in CSS only, `<picture>` with modern formats, and a stat band.

**Framer**
- A Linear-like hero with a "New: … →" link.
- Media-first feature cards with a footer text and one link each.
- A "full platform" bento, including a Core Web Vitals tile.
- **Take:** one link per card and media-first cards.

**Arc**
- Playful and brand-heavy.
- A **dedicated privacy section** with a link to details.
- Persistent OS download buttons.
- **Take:** a privacy and trust section, Vibezeo's strongest differentiator.

**Ghostty**
- A one-screen home: a terminal window with an animated ASCII scene, one sentence and two buttons.
- The download page has a centred icon, "Version x · Release Notes" and OS cards.
- **Take:** restraint, and the centred download header.

**Tailwind CSS docs**
- Mono uppercase group labels in the sidebar.
- An eyebrow above the H1.
- **Numbered steps (01, 02…) beside "Terminal" code panels.**
- **Take:** the Getting-started layout and the group labels.

### 3.3 Pattern synthesis
| Pattern | Seen at | For Vibezeo |
|---|---|---|
| Real product, live, right under the hero | BridgeMind, Linear, Cursor, Framer | **Adopt.** The existing demo at full width (§7.1). |
| Big display headline, short subhead, 2 CTAs, a meta line | All | **Adopt.** Tagline as h1 (§6.2). |
| "New: … →" announcement link | Linear, Framer, Vercel | **Adopt.** The "Released · 0.5.0 for Linux" pill links to the notes. |
| Two-tone statement paragraph | Linear, Stripe, Raycast, Cursor | **Adopt** in the trust and pricing intros. |
| Bento of recent work | Vercel, Framer, Stripe | **Adopt**, generated from `CHANGELOG.md` (§6.8). |
| Compact capability index | Zed | **Adopt**, replacing the 19-card wall. |
| Keycap illustrations | Raycast | **Adopt** with real bindings (`shortcuts.json`). |
| Stat band of real numbers | Stripe | **Adopt** with `PROGRESS`. |
| Customer logos, testimonials, community counts | Most | **Never.** We have none. |
| Sticky comparison table under plan columns | Linear, Raycast | **Adopt**, from `LIMITS`. |
| Changelog rail | Linear, Raycast, Cursor | **Adopt**, with versions and no dates. |
| CTA plus requirement line plus alternative method | Raycast, Zed, Ghostty | **Adopt.** Size, OS versions, checksum. |
| CSS scroll-driven animation, view transitions | Stripe, Linear | **Adopt** as progressive enhancement. |
| System-following theme | Vercel | **Adopt.** No toggle (storage is forbidden). |
| Pill buttons, glows, painted or 3D backdrops | BridgeMind, Vercel, Cursor, Raycast | **Avoid.** Use the app's 6 px controls and Vibezeo's own grid and glow motif. |

---

## 4. Information architecture

### 4.1 Global navigation
- **Desktop header** (64 px, sticky, aligned to the content container):
  - Mark plus "Vibezeo" on the left.
  - **Features** (`/#features`), **Demo** (`/demo/`), **Pricing**, **Docs**, **Changelog**, **About**.
  - A primary **Download** button on the right.
  - Mark the current page with `aria-current="page"`.
  - The "Claude" item becomes a home section and a footer link; its place in the header is an owner decision (§15).
- **Mobile header** (< 1024 px):
  - Mark, then Download (compact), then a **Menu** button.
  - The button opens a Popover API panel with the same links (§6.1). No JavaScript.
- **Footer** (every page, docs included). Four columns:

  | Column | Links |
  |---|---|
  | **Product** | Download, Live demo, Pricing, Changelog, Built with Claude (`/#claude`) |
  | **Docs** | Getting started, Code mode, Thread mode, Agent mode, Keyboard shortcuts, Troubleshooting |
  | **Company** | About, the founder on X, `hello@vibezeo.com` |
  | **Legal** | Privacy, Terms, Security |

  The bottom row reads "© Vibezeo · Built by Naman Jain." (**no year**), followed by "Product names belong to their owners. Vibezeo isn't affiliated with or endorsed by any engine vendor." and small links to `llms.txt` and the site-info JSON.

### 4.2 Pages (no new pages needed)
| Page | Purpose | Key change |
|---|---|---|
| `/` | Persuade and let people try | §4.3 |
| `/download/` | Get the build safely | Icon, version, size, command, checksum, steps (§6.17) |
| `/demo/` | Explain and launch the full-page demo | Same parts. A large still with a "Launch the demo" CTA and the "What to try" steps as numbered cards. |
| `/pricing/` | Plans and what credits pay for | Plan columns plus a generated comparison (§6.15, §6.16) |
| `/changelog/` and `/changelog/x.y.z/` | What shipped | Release rail, milestones grid, version page with prev/next (§6.18) |
| `/docs/*` | How it works | Fumadocs skin, groups, site footer (§6.19) |
| `/about/`, `/privacy/`, `/terms/`, `/security/` | Trust and legal | The new `ProsePage` typography: 44 rem measure, prose styles; an optional on-page TOC for Privacy |
| 404 | Recovery | Same frame, with links to home, docs and download |

### 4.3 Home page: section order
| # | Section | id | Layout | Content source | Notes |
|---|---|---|---|---|---|
| 1 | Hero | — | Text block on a grid and glow background | `SITE.tagline`, `RELEASE`, existing lead copy | §6.2. h1 = tagline. |
| 2 | Live demo | `try` | Full-width stage (max 1280 px) | `DEMO_STAGE`, `embed.js` | §6.3. Label, h2 "Try Vibezeo right here", caption, "Explore full screen". |
| 3 | Proof band | — (`aria-label="Progress"`) | 4 big numerals | `PROGRESS` | "Built so far. See the changelog →" |
| 4 | Engines | `engines` | Monogram chips plus a launch-grid screenshot | `ENGINES`, registry `logo` | Disclaimer line kept |
| 5 | Modes | `features` | Alternating rows: Code (full width), Thread, Agent, Dashboard | FEATURES copy, `code-mode.png`, `Keys`, `StatusDot` | Includes the pinned plan sentence |
| 6 | New in 0.5.0 | `new` | Bento of the release's Added entries | `RELEASES[0]` | Each tile links to its docs page |
| 7 | Everything else | `capabilities` | Dense 3-column index | FEATURES (title + first sentence) | Replaces the 19-card wall |
| 8 | Your machine, your accounts | `principles` | Two-tone statement, data-flow diagram, 4 principles | PRINCIPLES, privacy facts | Pinned "The goal is…" sentence |
| 9 | Built with Claude | `claude` | 3 cards with status tags | CLAUDE | Pinned strings |
| 10 | Founder | `team` | A note card with name, role and link | `SITE.founder` | Pinned link format |
| 11 | See it run | `demo` | Video in a stage | `lib/media.ts` | Honest caption (H1), `preload="none"` |
| 12 | Roadmap | `roadmap` | A "Next" track, numbered | ROADMAP | Labelled as not shipped |
| 13 | Pricing | `pricing` | 3 plan cards (compact) | `PLANS` | "Compare the plans →" |
| 14 | FAQ | `faq` | 2 columns: title and contact left, `<details>` right | FAQ | `<details name="faq">` |
| 15 | Final CTA | — | Centred band | `SITE`, `EARLY_ACCESS_HREF` | Download plus "tell us what works" mailto |

**Why this order.** Promise (1), then proof you can touch (2) and proof in numbers (3). Next come compatibility with what you already pay for (4), what it does (5–7) and why you can trust it (8–10). Motion (11), future (12) and cost (13) follow, then doubts (14) and the ask (15).

### 4.4 Section order on the other pages
- **Download:**
  1. Header: icon, "Download Vibezeo" h1, "Released · 0.5.0" and Release notes.
  2. OS note (`detect.js`).
  3. Linux panel: button, size, requirements, install command, SHA-256 and verify command.
  4. Unsigned-build note (pinned).
  5. `#getting-started`: 4 numbered steps with keycaps.
  6. "What this release includes" (plan limits).
  7. "Previous versions": links to older release notes only; the old builds aren't served (OPERATING §6).
  8. Footer.
- **Pricing:**
  1. Hero (two-tone statement, "Free is available now…").
  2. Plan columns (`#free`, `#pro`, `#team`).
  3. "Compare plans" table (generated).
  4. "What credits pay for" (`data-credit-price` rows).
  5. Billing FAQ, built only from facts in `pricing.ts` and `plans.ts`.
  6. A CTA to download Free.
- **Changelog:**
  1. Hero (existing copy).
  2. Release rail (newest first).
  3. Milestones grid (`#m14` … `#m0`).
  4. Footer.
- **Version page:**
  1. A sticky meta column: version, platforms, "Latest" with a Download button when current, prev and next links.
  2. The notes: summary, then Added cards, then Changed, Fixed and Security lists.

---

## 5. Visual system

### 5.1 Principles
- **Calm surfaces, loud status.** This scales UI_SPEC §1.1 up to page size.
  - Neutrals carry the page.
  - Teal marks actions and focus.
  - The teal→sky gradient appears only on the hero's highlighted phrase and the mark's glow.
  - Status colours appear only where they mean status.
- **Product media sits on a dark stage** in both themes, because the app's captures and the demo are dark.
- **Prefer hairlines to boxes.** Use 1 px `--tn-border` dividers; fill a card only when it groups content.
- **Display type uses Inter's optical-size axis** ("Inter Display" at large sizes, automatic with `font-optical-sizing: auto`). JetBrains Mono is for eyebrows, versions, numbers, keycaps, commands and hashes.
- **Keep the app's geometry:** 6 px controls, 8 px cards, 12 px media frames, rounded-full only for chips and pills. This also sets Vibezeo apart from the pill-button sites.

### 5.2 Light and dark without JavaScript
- **Follow the OS.** Remove `data-theme="dark"` from `<html>` in `app/layout.site.tsx`.
  - `tokens.css` already makes dark the `:root` default.
  - A new `@media (prefers-color-scheme: light)` block maps the light semantic tokens.
  - That block mirrors `[data-theme="light"]` in `tokens.css`. `tokens.css` stays verbatim (UI_SPEC §1.2, `base.css` note).
  - A unit test keeps the two blocks identical (§14).
- **Add colour-scheme metadata:** `export const viewport = { colorScheme: "dark light", themeColor: [{ media: "(prefers-color-scheme: dark)", color: "#060a12" }, { media: "(prefers-color-scheme: light)", color: "#f8fafc" }] }`. The second value is `--site-page` in light, `#F8FAFC`.
- **No theme toggle.** A toggle needs a script and storage, both forbidden. Document the decision as a D-n in `BUILD_LOG.md`.
- **Docs follow automatically**, because `docs.css` maps Fumadocs' colours to `--tn-*`. If fenced code blocks are added, configure Shiki dual themes and switch them with the same media query, since there is no `.dark` class.
- **Forced colours:** gradient text falls back to `CanvasText`, and card and button boundaries are real borders, not shadows.

### 5.3 Design tokens
Create `packages/ui/src/styles/site.css` and export it as `@tandem/ui/site.css`, so the site still uses "only `packages/ui` tokens" (CLAUDE.md). Import it in `apps/web/app/globals.css` after `tokens.css`, together with `@tandem/ui/base.css` (keyframes, reduced motion, contrast handling).

```css
/* @tandem/ui/site.css: the public site's layer over tokens.css (which stays UI_SPEC §1.2 verbatim). */

/* 1. Light theme by OS preference. A copy of [data-theme="light"] from tokens.css;
      site.test.ts fails if the two blocks differ. */
@media (prefers-color-scheme: light) {
  :root:not([data-theme="dark"]) {
    color-scheme: light;
    --tn-bg: var(--slate-50);
    --tn-surface-1: var(--slate-25);
    --tn-surface-2: var(--slate-0);
    /* …every remaining --tn-* declaration from the [data-theme="light"] block, unchanged… */
  }
}

/* 2. Site semantics (built only on the palette and --tn-* tokens). */
:root {
  --site-page:        var(--tn-bg);                 /* #060A12 */
  --site-raised:      var(--tn-surface-1);          /* bands, alt sections */
  --site-card:        var(--tn-surface-2);
  --site-stage:       var(--slate-900);             /* product media backdrop, dark in both themes */
  --site-stage-line:  rgb(148 163 184 / 0.18);
  --site-grid:        rgb(148 163 184 / 0.06);      /* the faint grid of og.png and the poster */
  --site-glow:        rgb(45 212 191 / 0.13);       /* the soft teal glow of the poster */
  --site-ink-a:       var(--teal-400);              /* highlight gradient start */
  --site-ink-b:       var(--sky-400);               /* highlight gradient end */
  --site-header:      color-mix(in oklab, var(--tn-bg) 88%, transparent);
  --site-gutter:      clamp(1rem, 0.6rem + 1.8vw, 2rem);     /* 16 → 32 px */
  --site-section-y:   clamp(4.5rem, 3rem + 5vw, 8rem);       /* 72 → 128 px */
  --site-header-h:    4rem;
}
@media (prefers-color-scheme: light) {
  :root:not([data-theme="dark"]) {
    --site-page:   var(--slate-25);                 /* #F8FAFC, a touch whiter than the app's slate-50 */
    --site-grid:   rgb(15 23 42 / 0.05);
    --site-glow:   rgb(15 118 110 / 0.09);
    --site-ink-a:  var(--teal-700);
    --site-ink-b:  var(--sky-700);
  }
}

@theme inline {
  --color-page: var(--site-page);
  --color-raised: var(--site-raised);
  --color-card: var(--site-card);
  --color-stage: var(--site-stage);
  --color-stage-line: var(--site-stage-line);
}

@theme {
  /* Display and marketing type. Names don't collide with the app's text-xs…3xl. */
  --text-display-2xl: clamp(2.5rem, 1.5rem + 4vw, 4rem);          /* 40 → 64 px: h1 */
  --text-display-2xl--line-height: 1.04;
  --text-display-2xl--letter-spacing: -0.03em;
  --text-display-2xl--font-weight: 600;
  --text-display-xl: clamp(2.125rem, 1.45rem + 2.6vw, 3.25rem);   /* 34 → 52 px: closing CTA, pricing h1 */
  --text-display-xl--line-height: 1.06;
  --text-display-xl--letter-spacing: -0.028em;
  --text-display-xl--font-weight: 600;
  --text-display-lg: clamp(1.75rem, 1.35rem + 1.6vw, 2.5rem);     /* 28 → 40 px: section h2 */
  --text-display-lg--line-height: 1.1;
  --text-display-lg--letter-spacing: -0.024em;
  --text-display-lg--font-weight: 600;
  --text-display-md: clamp(1.375rem, 1.2rem + 0.7vw, 1.75rem);    /* 22 → 28 px: row titles, docs h1 */
  --text-display-md--line-height: 1.2;
  --text-display-md--letter-spacing: -0.018em;
  --text-display-md--font-weight: 600;
  --text-title: 1.125rem;   --text-title--line-height: 1.45;   --text-title--font-weight: 600;  /* 18 px: card titles */
  --text-lead:  clamp(1.0625rem, 1rem + 0.3vw, 1.25rem);           /* 17 → 20 px */
  --text-lead--line-height: 1.6;
  --text-copy:  1rem;       --text-copy--line-height: 1.65;      /* 16 px body */
  --text-copy-sm: 0.9375rem; --text-copy-sm--line-height: 1.6;   /* 15 px card body */
  --text-caption: 0.8125rem; --text-caption--line-height: 1.5;   /* 13 px captions, footer */
  --text-eyebrow: 0.75rem;  --text-eyebrow--line-height: 1rem;   /* 12 px mono caps */
  --text-eyebrow--letter-spacing: 0.08em;
  --text-stat:  clamp(2rem, 1.6rem + 1.6vw, 3rem);                 /* 32 → 48 px, tabular figures */
  --text-stat--line-height: 1;
  --text-stat--letter-spacing: -0.02em;

  /* Layout */
  --container-site: 75rem;     /* 1200 px: header, sections, footer */
  --container-stage: 80rem;    /* 1280 px: the demo drawn 1:1 */
  --container-narrow: 56rem;   /* 896 px: centred bands, the FAQ */
  --container-prose: 44rem;    /* 704 px: legal and changelog text */
  --container-measure: 38rem;  /* 608 px: leads */

  /* Depth */
  --shadow-stage: 0 0 0 1px var(--site-stage-line), 0 28px 80px -28px rgb(var(--tn-shadow-color) / 0.7);
  --shadow-card-hover: 0 8px 24px -12px rgb(var(--tn-shadow-color) / 0.45);
}

@utility text-ink {             /* the hero highlight: teal → sky, as in og.png */
  background-image: linear-gradient(90deg, var(--site-ink-a), var(--site-ink-b));
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}
@utility eyebrow {              /* "— CODE MODE", the video's label style */
  font-family: var(--font-mono);
  font-size: var(--text-eyebrow);
  line-height: var(--text-eyebrow--line-height);
  letter-spacing: var(--text-eyebrow--letter-spacing);
  text-transform: uppercase;
  color: var(--tn-accent);
}
@utility bg-site-grid {          /* hero and CTA backdrop: grid plus glow, fading out */
  background-image:
    radial-gradient(60% 55% at 28% 0%, var(--site-glow), transparent 70%),
    linear-gradient(var(--site-grid) 1px, transparent 1px),
    linear-gradient(90deg, var(--site-grid) 1px, transparent 1px);
  background-size: auto, 96px 96px, 96px 96px;
  mask-image: linear-gradient(to bottom, black 55%, transparent);
}
@media (forced-colors: active) {
  .text-ink { background: none; color: CanvasText; }
}
```

Font variables: see §5.8. With `next/font`, override `--font-sans` and `--font-mono` in `globals.css` (`@theme inline { --font-sans: var(--font-inter), ui-sans-serif, system-ui, sans-serif; --font-mono: var(--font-jbm), ui-monospace, "SF Mono", Menlo, Consolas, monospace; }`) and set `body { font-family: var(--font-sans); font-size: var(--text-copy); line-height: var(--text-copy--line-height); }`. The `SiteFrame` root changes from `bg-bg` to `bg-page text-fg`.

### 5.4 Type scale (usage)
| Token | Size | Use |
|---|---|---|
| `text-display-2xl` | 40 → 64 px, 600, −0.03 em, 1.04 | The home h1 (tagline); `text-wrap: balance`; `max-w-[17em]` |
| `text-display-xl` | 34 → 52 px | h1 on Pricing, Download and Changelog; closing CTA |
| `text-display-lg` | 28 → 40 px | Section h2 |
| `text-display-md` | 22 → 28 px | Mode row titles, docs h1, version-page h2 |
| `text-title` | 18 px, 600 | Card, tile and FAQ question titles |
| `text-lead` | 17 → 20 px, 1.6 | Hero and section leads; colour `text-fg-muted` |
| `text-copy` | 16 px, 1.65 | Body |
| `text-copy-sm` | 15 px | Card body, table cells |
| `text-caption` | 13 px | Figure captions, footer, meta lines |
| `eyebrow` | 12 px mono caps, 0.08 em, accent | Section labels; prefix with a 16 px rule like the video's "— LAUNCH" |
| `text-stat` | 32 → 48 px, `tabular-nums` | Proof band, prices |
| `font-mono text-sm` | 12 px | Versions, hashes, commands (the app's own token) |

Rules:
- Headings use `text-fg`. Leads and body use `text-fg-muted`. Never use `text-fg-subtle` for copy longer than a caption.
- Sentence case everywhere (UI_SPEC §6.1). Only eyebrows are uppercase, through CSS.
- Numerals: `font-variant-numeric: tabular-nums` in stats, prices and tables.

### 5.5 Colour (both themes; measured WCAG contrast)
| Role | Dark | Ratio (on `--site-page`) | Light | Ratio (on `--site-page`) |
|---|---|---|---|---|
| Page | `#060A12` (`--tn-bg`) | — | `#F8FAFC` (slate-25) | — |
| Text | `#E6EDF5` | 16.8:1 | `#0F172A` | 17.1:1 |
| Muted (leads, body) | `#9AA8BB` | 8.2:1 | `#475569` | 7.2:1 |
| Subtle (captions only) | `#7D8BA0` | 5.7:1 | `#5F6F84` | 4.9:1 |
| Accent text and links | `#2DD4BF` | 10.6:1 | `#0F766E` | 5.2:1 |
| Highlight gradient | teal-400 → sky-400 | ≥ 9.3:1 | teal-700 → sky-700 | ≥ 5.2:1 |
| Primary button | `#042F2C` on `#2DD4BF` | 7.8:1 | `#FFFFFF` on `#0F766E` | 5.5:1 |
| Focus ring | teal-400 at 55% | 3.8:1 (non-text, ≥ 3:1) | teal-700 at 85% | 3.9:1 |
| Stage caption | `#9AA8BB` on `#0A0F1A` | 7.9:1 | (the stage stays dark) | 7.9:1 |
| Status text (badges) | sky, amber and green 400 | ≥ 9.2:1 | sky-700, amber-700, green-700 | ≥ 4.6:1 |

All pairs pass AA. Ratios were computed with the same formula as `packages/ui/src/styles/contrast.test.ts`. Extend that test to the site pairs (§14).

### 5.6 Layout, grid and spacing
- **Containers.** `mx-auto max-w-site px-(--site-gutter)` for header, sections and footer. **One width everywhere** fixes the current misalignment.
  - The demo stage uses `max-w-stage` (1280 px), so on viewports ≥ 1344 px it renders at scale 1.
- **Grid.** 12 columns.
  - `gap-6` (24 px) from `lg`, `gap-4` (16 px) below.
  - Feature rows use 5/7 columns (text and media), alternating.
  - Bento: 6 columns.
  - Index: 3 columns at `lg`, 2 at `sm`, 1 below.
- **Breakpoints** are Tailwind's defaults: 640, 768, 1024, 1280. 1024 is also `embed.js`'s phone and desktop boundary.
- **Vertical rhythm** (4 px base, `--spacing: 4px`):
  - Section padding `py-(--site-section-y)`.
  - Section header to content: 48 px.
  - Between mode rows: 96 px.
  - Card padding: 24 px (32 px for bento hero tiles).
  - Eyebrow to h2: 16 px. h2 to lead: 16 px. Lead to actions: 32 px.
- **Section separation:** alternate `bg-page` and `bg-raised`, or use a hairline `border-t border-divider`. Never both, and never a hairline between two bands of different colour.

### 5.7 Radii, borders, shadows, layers
- **Radii:** controls `rounded-sm` (6 px), cards `rounded-md` (8 px), media and stage `rounded-lg` (12 px), chips and pills `rounded-full`. These are all existing tokens.
- **Borders:** 1 px `border-border`; hover uses `border-border-strong`.
- **Shadows:** `shadow-stage` on media only. Cards are flat. Hover lifts with `shadow-card-hover` plus a border colour change (not a transform).
- **Layers:** header `z-50`. The demo's full-screen stage is `z-index: 1000` (set inline by `embed.js`), so the header must stay below it. The mobile menu popover sits in the top layer by definition.

### 5.8 Fonts (self-hosted, preloaded, no layout shift)
- Use `next/font/local` in `apps/web/app/fonts.ts` instead of the site importing `@tandem/ui/fonts.css`.
  - Add `@fontsource-variable/inter` and `@fontsource-variable/jetbrains-mono` to `apps/web/package.json` at the versions `packages/ui` uses.
  - `next/font` emits fonts to `/_next/static/media` (served immutable), adds `<link rel="preload">`, and generates a **metric-matched fallback face** (`adjustFontFallback`), so swapping fonts doesn't shift layout.

```ts
// apps/web/app/fonts.ts
import localFont from "next/font/local";

/** Inter with the optical-size axis: 14–32 opsz, 100–900 wght (latin, 73 KB). */
export const inter = localFont({
  src: "../node_modules/@fontsource-variable/inter/files/inter-latin-opsz-normal.woff2",
  variable: "--font-inter",
  weight: "100 900",
  display: "swap",
  preload: true,
  adjustFontFallback: "Arial",
});

/** JetBrains Mono for labels, versions, keycaps and commands (latin, 40 KB). */
export const mono = localFont({
  src: "../node_modules/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-latin-wght-normal.woff2",
  variable: "--font-jbm",
  weight: "100 800",
  display: "swap",
  preload: false, // not in the first viewport's critical text
});
```
- In `layout.site.tsx`, use `<html lang="en" className={`${inter.variable} ${mono.variable}`}>`.
- **Glyph rule.** The latin subset doesn't contain → ✓ ⌘ ⇧ ↗. Use lucide icons instead (§5.9). In `<kbd>`, write "Ctrl", "Shift" and "Alt": the only build is Linux, and `Kbd` already renders "Ctrl" on the server.

### 5.9 Icons and marks
- **lucide-react** (already used by `packages/ui`; add it to `apps/web`). Stroke 1.75. 16 px inline, 20 px in tiles, 14 px in dense rows, always `aria-hidden` next to text.
  - Arrows: `ArrowRight`, `ArrowUpRight` for external links.
  - Others: `Check`, `Download`, `Play`, `Copy`, `Terminal`, `Mic`, `Plug`, `ListTodo`, `Server`, `Activity`, `ShieldCheck`, `KeyRound`, `HardDrive`, `Menu`, `X`.
  - Rendered as server components, they ship no JS.
- **Mark.** Keep the two-ring SVG (move it to `components/site/Mark.tsx`). In the hero and CTA bands it may carry the poster's soft glow (`filter: drop-shadow(0 0 24px var(--site-glow))`) — Vibezeo's own treatment.
- **Engines.** `EngineGlyph` from `packages/ui` with the registry's `logo` letters (C, X, Cu, G, Co, Gk, O, D, A, Ai, K). Never a vendor logo.

---

## 6. Component inventory
All components are server components in `apps/web/components/site/` built from `@tandem/ui` primitives and the tokens above. None ships JS. The only scripts remain `embed.js` and `detect.js`.

### 6.1 `SiteHeader`
- **Layout.** A sticky 64 px bar with `bg-(--site-header)` and a hairline that appears once scrolled.
  - Optional, with CSS `animation-timeline: scroll()`: animate `border-color` from transparent to `--tn-border` over the first 64 px. It is static where unsupported.
- **Desktop.** Brand, the nav list (`text-copy-sm text-fg-muted hover:text-fg`, 40 px hit targets), then `Download` (primary, `md` size).
- **Mobile (< 1024 px).** Brand, a compact Download button, and a Menu button using the Popover API:
  ```tsx
  <button type="button" popoverTarget="site-menu" aria-label="Menu" className="lg:hidden …"><Menu size={20} aria-hidden /></button>
  <div id="site-menu" popover="auto" className="m-0 ml-auto h-dvh w-[min(20rem,85vw)] border-l border-border bg-card p-6 …">
    <nav aria-label="Site menu"> …the same links, 48 px rows… </nav>
  </div>
  ```
  - Light-dismiss and Esc come for free.
  - Browsers without the Popover API still have every link in the footer.
- **Current page:** `aria-current="page"` plus `text-fg`. Pass `current` from each page to `SiteFrame`.
- `view-transition-name: site-header`, so the header holds still across page transitions.

### 6.2 `Hero` (home)
- **Structure, top to bottom:**
  1. `AnnouncementPill`: a link to `/changelog/${RELEASE.version}/` containing "Released · 0.5.0 for Linux" (one text run, pinned), a divider, "What's new" and `ArrowRight`. Chip style: `rounded-full border border-accent/40 bg-accent-soft px-3 h-8 text-caption`, with the version in mono.
  2. `h1` in `text-display-2xl text-balance max-w-[17em]`: `SITE.tagline`, with "coding agents" wrapped in `<span class="text-ink">` (the brand treatment from `og.png` and the poster). This needs the one-line test tweak in §14. Without it, render the tagline plain and give the whole h1 a subtle `fg → fg-muted` vertical gradient.
  3. Lead in `text-lead text-fg-muted max-w-measure`: the existing paragraph ("Vibezeo runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side…").
  4. Actions:
     - Primary: `Download for Linux` with the `Download` icon (→ `/download/`).
     - Secondary: `Try it in your browser` (→ `/demo/`).
     - Both are 44 px (`lg`) link-buttons from `buttonClass()` (§6.20).
  5. Meta row (`text-caption text-fg-muted`): "Free for Linux. No account needed: it runs on your computer." (existing). Then a dot, then a text link with the `Play` icon, "Watch it run (1 min)" (→ `#demo`). Optionally add "· {size} .deb" from the manifest (§6.17).
- **Background.** `bg-site-grid` on the section, plus a faint "pane split" figure: hairlines at the positions of a 2×2 split with the app's 6 px gutters, at 4% opacity, masked radially. It's drawn from the product's own layout, which makes it a Vibezeo motif.
- **Alignment.** Left-aligned on desktop; the text column takes 8 of 12 columns. The demo stage follows directly (§6.3).
- **Spacing.** `pt-24 pb-16` from `lg`, `pt-16 pb-12` below. This keeps the demo slot at about 740 px at 1280×400 (§1.5).
- **Motion.** None on the h1 (it's the LCP candidate). Lead, actions and meta may enter with `@starting-style` (§8).

### 6.3 `DemoStage` (the `#try` section)
- **Header row** (above the stage, `max-w-stage`):
  - Left: the "Interactive demo · sample data" `Badge`, then the h2 "Try Vibezeo right here".
  - Right, from `lg`: "Explore full screen" (`data-demo-open`, href `/demo/app/`, `Maximize2` icon).
- **Stage.** Keep the existing inner-HTML block (`DEMO_STAGE`) and its data attributes exactly, with these changes:
  - Wrapper: `relative w-full max-w-stage mx-auto` with `style={{ aspectRatio: "1280 / 800" }}`. Keep `data-demo-embed=""` and the `style` attribute so there's no shift.
  - `[data-demo-stage]`: `rounded-lg bg-stage shadow-stage overflow-hidden` (replacing `shadow-3`).
  - The still becomes a `<picture>` with AVIF and WebP sources. The `<img src="/demo/still.jpg" data-demo-still …>` fallback stays with `width="1280" height="800"`. Because the stage now sits high on the page, use `loading="eager" fetchpriority="high"` (replacing `loading="lazy"`).
  - "Tap to explore" (under 1024 px) stays an accent chip on a 40% scrim. White on the scrim alone would be 3.3:1, so the text must stay inside the solid chip.
- **Caption under the stage** (`text-caption text-fg-muted`, two lines max): "This is Vibezeo itself, running in your browser with sample data. Click around. Its agents are simulated: nothing runs and nothing you type leaves your browser." (pinned strings).
- **Rules.**
  - The section is `<section id="try" aria-labelledby="try-title">`, with `id` first and **no nested `<section>`**.
  - `<script src="/demo/embed.js" defer />` stays inside it.
- **States** (CLAUDE.md asks for empty, loading, error and populated states). These all exist through `embed.js`; style them, don't change the loader:

  | State | What the visitor sees |
  |---|---|
  | No JS | The still, plus the "Explore full screen" link to `/demo/app/` |
  | Still (before near) | The still image |
  | Loading | The still stays until the frame's `load` event, so there is no blank flash |
  | Live | `data-demo-state="live"` |
  | Full screen | `data-demo-full`, with the bar showing "Close (Esc)" |
  | Frame failed | The still remains, with the link to `/demo/app/` |

### 6.4 `StatsBand` ("Progress")
- **Layout.** Four cells in a `bg-raised` band with hairline separators. Each cell has a big numeral (`text-stat font-mono tabular-nums text-fg`) over a label (`text-caption text-fg-muted`).
  - Keep `<dl>` semantics with `order` swapped, as now.
- **Figures** come from `PROGRESS` and are formatted with `toLocaleString("en-US")`. The label `milestones in the ${RELEASE.version} release` is pinned.
- **Trailing line:** "Built so far. See the changelog →".
- **Optional:** count-up animation is **not** recommended. It needs JS, and a CSS `@property` counter is fragile and changes the text that screen readers read. Show final numbers.

### 6.5 `EngineList` (`#engines`)
- **Layout.** A 7/5 split: text and chips on the left, the `launch-grid.png` shot on the right in a `ProductShot`.
- **Chips.** `<li class="inline-flex items-center gap-2 h-9 px-3 rounded-full border border-border bg-card">` holding `<EngineGlyph letters={logo} size={20} /> {label}`. The label must be the element's last, whole text node.
- **Data.** `ENGINES` (pinned), with letters looked up in `registry.json` by label. Add a test that every `ENGINES` entry has a registry entry.
- **Copy.** Keep the existing paragraph and the disclaimer.

### 6.6 `ProductShot`
Replaces `Screenshot`. A real capture in a frame, with a caption.

```tsx
<figure className="flex flex-col gap-3">
  <div className="overflow-hidden rounded-lg bg-stage shadow-stage">
    <picture>
      <source type="image/avif" srcSet="/screens/code-mode.avif 1440w, /screens/code-mode@2x.avif 2880w" sizes="(min-width: 1280px) 1200px, 100vw" />
      <source type="image/webp" srcSet="/screens/code-mode.webp 1440w, /screens/code-mode@2x.webp 2880w" sizes="(min-width: 1280px) 1200px, 100vw" />
      <img src="/screens/code-mode.png" width={1440} height={900} loading="lazy" decoding="async" alt="…" className="block h-auto w-full" />
    </picture>
  </div>
  <figcaption className="text-caption text-fg-muted">…which build, what's on screen…</figcaption>
</figure>
```
- Every shot passes `width` and `height`. Below the fold it's `loading="lazy"`, which also stops React's automatic preload.
- **Never use 19xx or 20xx numbers in `srcset`, `sizes` or `width`** (the year test).
- Keep the full-size link (`<a href=… title="Open full size">`) only on the Code mode shot.

### 6.7 `ModeRow` (`#features`)
- **Section header.** Eyebrow "Working today", then the h2 "Built for running many agents at once" (both existing).
- **Rows.** Four rows, each with an eyebrow (mode name), an h3 (`text-display-md`), a body (`text-copy text-fg-muted`, at most 3 sentences), 3 facts with `Check` icons, and a "Read the docs →" link.

| Row | Media | Copy source | Notes |
|---|---|---|---|
| **Code mode**: real terminals, side by side | `code-mode.png` at **full container width** above the text. The four real terminals become legible. | FEATURES "Real terminals, side by side" | `Keys` strip: split right (`pane.split_right`, terminal context Ctrl+Shift+E), split down (Ctrl+Shift+D), palette (Ctrl+K), modes (Ctrl+1/2/3), all from `shortcuts.json` |
| **Thread mode**: agents as conversations | Until a real capture exists (§7.2), a "Try it in the demo above" card linking `#try`. The demo opens on Thread mode. | FEATURES "Thread mode" and "Switch engines mid-thread" | **Must contain the pinned plan sentence.** Pro shows up to 6 threads side by side. |
| **Agent mode**: teammates that remember | A real capture of the roster and a teammate brief, once taken (§7.2); text-only before that | FEATURES "Teammates" | "Free includes one teammate besides Lead." |
| **One Dashboard**: every agent, and which ones need you | A status legend built from `StatusDot` (Working, Waiting for you, Done, Idle), captioned "The status shapes Vibezeo uses on every pane and in the Dashboard". Plus the demo link. | FEATURES "One Dashboard", "Live status for every agent" | A real Dashboard capture replaces the legend later |

- **Layout.** Code mode is full width. Thread, Agent and Dashboard alternate 5/7 and 7/5. On mobile they stack, text first.

### 6.8 `ReleaseBento` (`#new`)
- **Header.** Eyebrow "New in {RELEASE.version}", the h2 "What's new in this release", and a "Release notes →" link to `/changelog/{version}/`.
- **Data, generated by construction.** `RELEASES[0].entries.filter(e => e.category === "Added")`.
  - Title: the leading `**…**` phrase without its period ("Dictate", "Plugins", "Tasks", "SSH workspaces", "Diagnostics", "Tray icon").
  - Body: the first sentence after it, cut at the first ". " with a test that it's a prefix of the CHANGELOG text.
  - Platform chip: from `platforms` (`PLATFORM_LABEL`).
  - Icon and docs link: from a small map in `lib/home.ts`, for example Dictate → `Mic`, `/docs/dictate/`; Plugins → `Plug`, `/docs/plugins/`; Tasks → `ListTodo`, `/docs/tasks/`; SSH workspaces → `Server`, `/docs/remote/`; Diagnostics → `Activity`, `/docs/troubleshooting/`.
  - A test makes sure every Added title has a map entry, so a new release can't render a tile without a link.
- **Grid** (6 columns at `lg`):
  - Row 1: the first entry spans 4 columns (with a real crop when available, §7.2), the second spans 2.
  - Row 2: three tiles of 2 columns each.
  - Any remainder: compact 2-column tiles.
  - Below `lg`: 1–2 columns.
- **Tile.** `rounded-md border border-border bg-card p-6`: icon tile (40 px, `bg-accent-soft text-accent rounded-md`), `text-title`, `text-copy-sm text-fg-muted`, then the link. On hover, `border-border-strong` and `shadow-card-hover`.
- **Terminal tile** (optional, Vercel pattern): a tile showing the real install command from `DOWNLOADS`, `sudo apt install ./Vibezeo_0.5.0_amd64.deb`, in `CommandLine` (§6.13).

### 6.9 `FeatureIndex` (`#capabilities`)
- **Header.** Eyebrow "Everything in the app", h2 "And everything else that works today".
- **Content.** A dense list (`grid lg:grid-cols-3 gap-x-8 gap-y-6`) of FEATURES not covered by the mode rows and the bento: worktree lanes and handoff, files, git and a browser, Find work, several accounts per engine, notifications, crash-safe restore, signed updates, private by default, your accounts untouched.
- **Each item.** A 20 px icon, a `text-copy font-semibold text-fg` title, and the **first sentence** of the existing body in `text-copy-sm text-fg-muted`. The full copy moves to the docs (link per item).
- This replaces the 19-card wall and halves the section's height.

### 6.10 `TrustFlow` (`#principles`)
- **Statement** (two-tone `text-display-md`):
  > Nothing leaves your machine unless you opt in. **Engines sign in by themselves, in their own folders, and Vibezeo never reads, copies or proxies their tokens.**

  Built from the existing principles and FAQ copy.
- **Diagram.** An inline SVG with `role="img"` and an `aria-label` summarising it. Pure markup, tokens via `currentColor`, no images. Caption: "How data moves in Vibezeo {RELEASE.version}". Links: Privacy (`/privacy/`) and "Privacy and data flows" (`/docs/privacy/`).
  - "Your computer" box containing:
    - "Vibezeo": workspaces, layouts, history, notes and tasks, stored on your computer.
    - "OS keychain": secrets and plugin credentials.
    - "Engine CLIs": each signed in by itself, in its own folder.
  - Arrows:
    - Engine CLIs ↔ "Engine vendors", directly, under their terms.
    - Vibezeo → "Vibezeo servers", labelled "**nothing in this build**".
    - A dashed optional arrow to "Services you connect (plugins): writes ask you first".
  - Facts come from `/privacy/`, `docs/privacy.mdx` and the 0.5.0 CHANGELOG summary.
  - **Don't show provider-home paths.** They contain the code name.
- **Principles.** The existing 4 PRINCIPLES in a 2×2 grid with a left accent rule (as now, restyled). The pinned "The goal is…" sentence stays verbatim.

### 6.11 `ClaudeBand` (`#claude`) and `FounderNote` (`#team`)
- **ClaudeBand.**
  - Eyebrow "Built with Claude", h2 "Made for Claude Code, open to every agent", lead (existing).
  - 3 cards from CLAUDE with a tag badge: "Shipped" (`Badge variant="done"` with a `Check` icon), "How we build" (`variant="neutral"`), "Built, live when accounts open" (`variant="waiting"`).
  - All strings pinned; keep `--resume` in `<code>`.
- **FounderNote.**
  - A single card (`max-w-narrow`) styled as a note.
  - Name link with the exact format `<a href={SITE.founder.link} …>{SITE.founder.name}</a>`, "Founder, Vibezeo", the bio, and the existing sentence.
  - "More about Vibezeo →" links to `/about/`.
  - No photo unless the owner supplies one.

### 6.12 `Keys` and `Keycap`
- `Keys({ id, context })` reads `@tandem/protocol/shortcuts.json` by command id and renders the **Linux** binding: `pcTerminal` when `context="terminal"` and it exists, otherwise `pc`.
- It renders through `@tandem/ui` `Kbd` with key arrays (`["Mod","Shift","E"]` becomes Ctrl, Shift, E caps). `Mod` maps to "Ctrl" on the server.
- **Large keycap variant** for the Code mode row (Raycast pattern): 40–48 px caps in mono with `border-border-strong`, `bg-(--tn-kbd-bg)` and a 2 px bottom border for depth, joined by a `Plus` icon.
- A test asserts the hero, download and docs keys come from the JSON (§14).

### 6.13 `CommandLine`
- A single-line command in `font-mono text-sm`: a `rounded-md border border-border bg-stage` box with a `Terminal` icon and the label "Terminal".
- **No-JS copy:** `user-select: all` on the `<code>`, so one click selects the whole command; the hint reads "Click to select".
- No clipboard button: it would need a script.
- Used for the install and verify commands (Download, the bento, docs Getting started).

### 6.14 `VideoSection` (`#demo`)
- **Header.** Eyebrow "See it run", h2 "Vibezeo in 1 minute".
- **Video.** In a stage frame with `controls playsInline preload="none"`, the poster, and the webm then mp4 sources. **Never `muted`, never `autoplay`** (it has sound; pinned).
- **Caption from `lib/media.ts`:** "Every shot of the app is a screen recording of the Linux {VIDEO.recordedWith} build. The terminal part plays at 2.3× speed (marked on screen), and the live-status scene is an illustration. 0:08 …" (H1).

### 6.15 `PlanCards` (home teaser and `/pricing/`)
- **Columns.** Three columns divided by hairlines (Linear style), not boxed cards. Pro gets a 1 px `border-accent` outline and a "Most capable" label only if the owner wants one; otherwise no highlight.
- **Each column, top to bottom:**
  1. Plan name (`text-title`), summary (`text-copy-sm text-fg-muted`).
  2. Price (`text-stat font-mono`) plus cadence. Show monthly and yearly as text ("$29 a month, or $290 a year"); no toggle.
  3. Trial line.
  4. Availability:
     - Free: "Available now: download Vibezeo for Linux".
     - Pro and Team: "Starts with Vibezeo accounts".
  5. CTA:
     - Free: **Download**.
     - Pro and Team: a disabled-looking secondary "Not on sale yet" is **forbidden** by the tests ("on sale yet"). Instead show text: "Starts with Vibezeo accounts" plus a mailto "Tell me when" (prefilled subject, as `EARLY_ACCESS_HREF` does).
  6. Feature list with `Check` icons from `PLANS` (fix H3 first).
- **Ids** `free`, `pro`, `team` on the `<li>` (pinned).

### 6.16 `PlanComparison` (`/pricing/`)
- **Table.** `<table>` with `<caption class="sr-only">`, `<th scope="col">` plan headers that are sticky (`position: sticky; top: var(--site-header-h)`), and `<th scope="row">` row labels. Rows are grouped by `<tbody>` with a heading row.
- **Rows are generated** from `LIMITS` through a `PLAN_ROWS` list in `lib/pricing.ts`, extending `LIMIT_ROWS` in `lib/docs/tables.ts`. Share one list.

  | Group | Rows |
  |---|---|
  | **Workspace** | Live terminal panes (4 / 64 / No limit), Workspaces (3 / No limit / No limit), Threads side by side (1 / 6 / 6), Thread mode engines (Claude Code, Codex / All / All) |
  | **Accounts and teammates** | Accounts per engine (1 / No limit / No limit), Teammates besides Lead (1 / No limit / No limit), Plugin connections (1 / No limit / No limit) |
  | **Switch on with accounts** | Routines, Tasks run in their own lanes, Cloud dictation, Enhance and Polish, monthly credits (10,000 / 10,000 per seat, pooled), credit packs |

  Each row in the last group carries the note "Built into the app; switches on with Vibezeo accounts".
- **Only features the build has.** Each row maps to a feature ticked in `docs/REMAINING.md`, as `CREDIT_ROWS` already does. Exclude Conductor, Auto, phone and admin policy until they ship.
- **Cells.** Text values, or `Check` and `Minus` icons with `sr-only` "Included" and "Not included".
- **Small screens.** The table scrolls horizontally inside its own container (`overflow-x-auto`, with a fade edge). The page itself never scrolls horizontally.

### 6.17 `DownloadPanel` (`/download/`)
- **Header** (Ghostty and Zed pattern, centred on mobile, left on desktop):
  - The app icon (`/icon.png`, 96 px, `width`/`height` set).
  - `h1` "Download Vibezeo".
  - A mono line: "Released · {version}" · "Release notes" link.
- **OS note.** The `detect.js` paragraph stays as inner HTML, styled as an info row with an icon.
- **Linux panel** (`id="linux"`, `max-w-narrow`, `rounded-lg border bg-card p-8`):
  - The button: `primary.label` ("Download for Linux (.deb)") with the `Download` icon.
  - Facts as a 2-column `<dl>`: Requirements (`d.requirement`, fixed per H4), File (`Vibezeo_0.5.0_amd64.deb`), Size (from `statSync`, in decimal MB with one decimal, for example "16.2 MB"; add `bytes` to `ReleaseAsset` in `lib/manifest.ts`).
  - SHA-256: mono, `break-all`, selectable, with `data-sha256` (pinned).
  - `CommandLine`s, from `d.steps`: `sudo apt install ./Vibezeo_0.5.0_amd64.deb` and `sha256sum Vibezeo_0.5.0_amd64.deb`.
  - The unsigned note (pinned copy) and the `SHA256SUMS` link (pinned href).
- **`#getting-started`.** Four numbered steps (Tailwind docs pattern: mono `01`–`04` in bordered boxes), with `Keys` replacing the hard-coded "Mod+D" text (H5), then "What this release includes" (LIMITS copy).
- **"Previous versions".** Links to the older release-note pages only.

### 6.18 `ReleaseRail` and `ReleaseNotes` (`/changelog/`)
- **Rail item** (`<article>`), with a 3/9 grid at `lg`:
  - Left: a sticky column with the version (`font-mono text-display-md`), platform chips and a "Latest" badge on the first item.
  - Right: the summary's first paragraph, the release's Added titles as small inline chips, and "Release notes →".
  - A 1 px vertical rail with a dot per release (accent for latest, `--tn-border-strong` otherwise).
  - **No dates** (no-year rule; `CHANGELOG.md` has none).
- **Milestones.** A 2-column grid of compact cards, `<li id="m14">` (pinned ids), with the mono id, title and items. These are all open; don't collapse them.
- **Version page:**
  - Sticky meta column: version, platforms, "Latest" plus a Download button when `version === RELEASE.version`, and prev/next links in `RELEASES` order.
  - Content: the summary, then `Added` entries as cards rendered **through `InlineMarkdown`**, which keeps `<strong>Files.</strong>` (pinned). The first `<strong>` is styled as the card title with `[&>strong:first-child]:block [&>strong:first-child]:text-title`.
  - Then `Changed`, `Fixed` and `Security` as lists, with platform words in `text-caption`.

### 6.19 Docs skin (Fumadocs)
- **Layout and header.** Keep `DocsLayout` and `DocsPage`.
  - `nav.title`: `<Mark /> Vibezeo <span class="text-fg-muted">Docs</span>`.
  - `links`: Home, Download (emphasised), Pricing, Changelog.
- **Grouped sidebar.** Use `content/docs/meta.json` separators in the same order as `DOCS_PAGES`. Reorder `DOCS_PAGES` to match.

  | Group | Pages |
  |---|---|
  | Get started | index |
  | Modes | code-mode, thread-mode, agent-mode |
  | Features | dictate, tasks, routines, plugins, remote, conductor |
  | Plans and privacy | accounts-and-limits, privacy |
  | Reference | keyboard-shortcuts, settings, engines, troubleshooting |

- **Typography** (in `docs.css`, using the site tokens):
  - Map `--color-fd-background` to `var(--site-page)`, so docs and marketing pages share one background in both themes.
  - h1 `text-display-md`; description `text-lead`; body `text-copy` at 1.7.
  - h2 with a hairline above.
  - `<kbd>` styled like `@tandem/ui` `Kbd` (border, `--tn-kbd-bg`, mono 12 px), not like code.
  - Tables use `text-copy-sm` with zebra-free hairlines.
- **Getting started.** Numbered steps with `CommandLine` for install and verify. Write them in MDX with a small `Steps` component in `components/docs/`.
- **Footer.** Add `components/docs/DocsFooter.tsx` after `<DocsBody>`: the legal links, About and `hello@vibezeo.com`. Docs then meet the "legal links and contact on every page" rule (new test, §14).
- **No "Edit on GitHub"** (no repository links).
- **Search stays disabled** unless a same-origin static index is proven to leave storage empty (`wb01.spec`). Optional later.

### 6.20 Primitives
- **`buttonClass(variant, size)`.** Export it from `packages/ui/src/components/Button.tsx` so link-buttons (`<a>`) share the app's button styles.
  - Add a `xl` size (44 px, `px-5`, `text-copy`, `gap-2`) for marketing CTAs.
  - Variants: `primary`, `secondary` (`border-border-strong`, transparent), `ghost`.
  - Radius stays `rounded-sm` (6 px).
- **`Section`.** Props: `id`, `eyebrow`, `title`, `lead?`, `tone: "page" | "raised"`, `align: "left" | "center"`, `width: "site" | "narrow"`.
  - Renders `<section id aria-labelledby>` and the header block. The h2 gets `id={`${id}-title`}`.
  - **Never wrap `#try` in it.**
- **`Faq`.** `<details name="faq">` items in a 2-column layout.
  - Left: h2 "Questions" plus "Not answered here? Write to hello@vibezeo.com".
  - Summary: `text-title` with a `Plus` icon that rotates 45° (transition under no-preference). The answer is `text-copy text-fg-muted`.
  - The first item is `open`.
- **`CtaBand`.** Centred, `bg-site-grid`, the mark with its glow, the h2 "Try it on your own projects" (`text-display-xl`), the line with the feedback mailto (pinned `EARLY_ACCESS_HREF`), and the primary Download button.
- **`SiteFooter` and `ProsePage`:** see §4.1 and §4.2.

---

## 7. Presenting the product

### 7.1 The interactive demo
- **Placement.**
  - It sits directly under the hero, as FEATURES_TO_BUILD says ("Right under the hero, inside a Vibezeo window frame").
  - It is drawn 1:1 at 1280 px when the viewport allows, which avoids blurry scaled text, and scaled down to the container otherwise (`embed.js` `fit()`).
  - At 1440×900 its top ≈40% peeks into the first viewport and invites scrolling. At 1280×400 it starts below the fold (§1.5).
- **The app is its own frame.** The demo renders the app's title bar, so add no fake chrome: just the 12 px radius, the stage background and `shadow-stage`.
- **Keep `embed.js` behaviour unchanged**: proximity load after `load` (or 2 s), full screen on the same frame, Esc from page or frame, Tap to explore under 1024 px.
  - Optional, kept-intent change: if Lighthouse desktop shows the demo's script competing with the page, load at `threshold: 0.15` without `rootMargin`. AC1 still holds, and the literal markers stay.
- **The still.**
  - It **must show the current release.** Today it shows v0.3.0 and "Online" (H2).
  - Capture it from `/demo/app/?embed=1` at 1280×800 (DPR 2 for a 2560×1600 master) after the first-run hint appears, with a Playwright script (`scripts/agent/demo-still.mjs`, modelled on `demo-walkthrough.mjs`).
  - Export `still.avif`, `still.webp` (q≈60, both ≈40–70 KB at 1280 px) and `still.jpg` (the pinned fallback).
  - Add the capture to OPERATING §6 step 5 next to the demo checks.
  - Update the alt text to what the still really shows.
- **Content stays honest** (D-128): the demo shows only what the released download does, on the Free plan, and says its agents are simulated (its bar and the caption).

### 7.2 Real product screenshots
- **Extend `apps/desktop/e2e/screens/site.screens.ts`.** It already drives the real Tauri build over a fresh clone of the repository and has `shotOf()` for element crops.
  - For 2× captures (2880×1800), try `GDK_SCALE=2` (WebKitGTK's HiDPI scale) and check the output size.
  - Never use 1920- or 2048-wide outputs, because of the year test.

| File (`public/screens/`) | Needs a signed-in engine? | What | Used in |
|---|---|---|---|
| `code-mode` (exists; re-take at 2×) | No | 4 terminals: git history, htop, vim, tests | Mode row 1 (pinned `.png` src) |
| `launch-grid` (exists; re-take) | No | Engines found plus Install buttons | Engines (pinned) |
| `files-git` | No | Files tab plus git panel over the clone | Capability index or bento |
| `tasks` | No | Tasks board with a few tasks created in the UI | Bento "Tasks" |
| `plugins` | No | Settings › Plugins catalog and risk classes | Bento "Plugins" |
| `dictate` | No | Settings › Dictate: model list, history | Bento "Dictate" |
| `ssh-host` | No (uses the `sshd` fixture, OPERATING §5) | The host key fingerprint prompt or a remote folder | Bento "SSH workspaces" |
| `palette` | No | Ctrl+K command palette with Find work | Keys row |
| `agent-mode` | Roster: no. Chat: yes | Lead plus a teammate brief | Mode row 3 |
| `thread-mode` | **Yes**: a real Claude Code or Codex sign-in on a sample repo | An approval inline plus the Changes panel | Mode row 2 |
| `dashboard` | **Yes** (live agents) | Working, Waiting for you and Done | Mode row 4 |

- **Human task.** Rows marked "Yes" need the owner's machine with a signed-in engine. Add them to `HUMAN_TODO.md` and **show no screenshot until they exist**: the row links to the live demo instead. Never use the fake engine's output ("You said: …") or demo footage outside `#try`.
- **Formats.** AVIF plus WebP (generate with `sharp` in `apps/web/scripts/optimize-screens.mjs`; add `sharp` as a devDependency), plus the PNG fallback.
  - Budget: ≤ 120 KB AVIF per full-width shot, ≤ 50 KB per crop.
- **Capture hygiene:**
  - Nothing that shows "tandem" (the alias already filters git history).
  - No years (the alias already filters subjects), no personal data, no tokens.
  - Free plan; the current version in the status bar.
- **Theme.** Captures are dark and shown on the dark stage in both site themes, consistent with the demo. Light captures are optional later, delivered as `<source media="(prefers-color-scheme: light)">`.
- **Alt and caption rules.**
  - Alt text describes what is on screen.
  - The caption adds context and the build ("Vibezeo 0.5.0 on Linux: …").
  - Both come from a `MEDIA` manifest in `lib/media.ts`, with `recordedWith` per asset. A test checks that captions use it, not `RELEASE.version`.

### 7.3 The video
- **Phase 0 fix.** Choose one:
  - (a) Caption with `recordedWith: "0.1.0"`, and re-render the poster without a version or with the right one.
  - (b) Re-record `public/clips/app.mp4` with the 0.5.0 AppImage (`media/product-video/capture/record.sh`), re-render, and keep the honesty rules in its README.
- **Delivery:** `preload="none"`; the poster gets `width`/`height` through the `video` attributes; sources stay webm then mp4.
- **Placement:** section 11, linked from the hero's "Watch it run (1 min)".

### 7.4 Illustrations
- **Allowed** only when labelled ("Illustration" or "Diagram") and built from Vibezeo's own components or shapes: the status legend, the data-flow diagram, keycaps, the pane-split motif.
- **Never** a fake app screen with made-up data outside `#try`.

---

## 8. Motion
- **Principles.**
  - Motion explains a state or reveals the product; it never decorates the reading path.
  - CSS only. No animation libraries, no scroll-jacking, no parallax, no autoplaying media.
  - Durations and easings come from the tokens: `--dur-fast` 120 ms, `--dur-base` 180 ms, `--dur-slow` 260 ms, `--dur-deliberate` 400 ms, `--ease-enter`, `--ease-standard`.
- **Scroll reveals** (Stripe technique; progressive). Content is visible by default where unsupported, so there is never a flash of hidden content:
  ```css
  @keyframes site-reveal { from { opacity: 0.001; translate: 0 1rem; } to { opacity: 1; translate: 0 0; } }
  @media (prefers-reduced-motion: no-preference) {
    @supports (animation-timeline: view()) {
      .reveal { animation: site-reveal linear both; animation-timeline: view(); animation-range: entry 0% entry 35%; }
    }
  }
  ```
  - Apply to section headers, bento tiles (staggered with `animation-range` offsets), media frames and the diagram.
  - Keep it to about 25 elements on the home page.
  - **Never on the h1, the demo still or the first viewport.**
- **First-paint entrance.** For lead, actions and meta only:
  ```css
  @media (prefers-reduced-motion: no-preference) {
    .enter { transition: opacity var(--dur-deliberate) var(--ease-enter), translate var(--dur-deliberate) var(--ease-enter); }
    @starting-style { .enter { opacity: 0; translate: 0 0.5rem; } }
  }
  ```
  Stagger with `transition-delay: 60ms` steps. Never on the LCP element.
- **Page transitions.** These are plain links, so they're full navigations:
  ```css
  @media (prefers-reduced-motion: no-preference) {
    @view-transition { navigation: auto; }
    ::view-transition-group(root) { animation-duration: var(--dur-base); animation-timing-function: var(--ease-standard); }
  }
  ```
  The header carries `view-transition-name: site-header`. Supported in Chromium 126+ and Safari 18.2+; a no-op elsewhere.
- **Micro-interactions:**
  - Buttons: background colour over `--dur-instant`.
  - Cards: border colour and shadow over `--dur-fast`.
  - Link arrows nudge `translate: 2px 0` on hover or focus.
  - FAQ plus icon rotates 45°.
  - Status dots use the app's own pulses (`.tn-dot`, `.tn-attention` from `base.css`).
- **The demo's still-to-live swap** is instant today. Optionally cross-fade the frame in with `[data-demo-state="live"] [data-demo-frame] { animation: tn-fade-in var(--dur-slow) var(--ease-enter); }`. This is CSS only, so `embed.js` stays as is.
- **Reduced motion.**
  - `base.css` (to be imported) already forces every animation and transition to 0.01 ms under `prefers-reduced-motion: reduce`.
  - All rules above live inside `no-preference`.
  - The video never autoplays.
  - Test it with Playwright's `reducedMotion: "reduce"` (§14).

---

## 9. Accessibility (WCAG 2.2 AA in both themes)
- **Contrast:** §5.5 lists every pair. Extend `contrast.test.ts` to the site tokens in both themes, including text on `--site-stage` and the gradient endpoints.
- **Structure.**
  - Skip link (keep it).
  - `header > nav[aria-label="Site"]`, `main#main`, `footer > nav[aria-label="Footer"]`.
  - One `h1` per page, then an `h2` per section and `h3` in cards, never skipping levels.
  - Each `section` has `aria-labelledby` pointing at its h2.
- **Focus.**
  - `:focus-visible` uses the token ring (`--shadow-focus`, ≥ 3.8:1).
  - Never remove outlines without the ring.
  - The ring must show on dark stage surfaces too: use `--tn-accent-ring` with a 2 px `--site-stage` gap.
- **Targets:** ≥ 24×24 px (2.5.8). Nav links and buttons are 40–48 px tall; footer links get 32 px line boxes.
- **Media.**
  - Real screenshots get descriptive `alt`.
  - Decorative SVG and background motifs are `aria-hidden`.
  - The diagram is `role="img"` with a full `aria-label`; its text also appears in the caption.
  - The video is music-only with on-screen text, and the figcaption lists the scenes. If narration is ever added, add a WebVTT captions track.
- **Status meaning.** Status is never shown by colour alone: `StatusDot` pairs a shape with a label (UI_SPEC §1.3).
- **Keyboard.**
  - Every interactive element is native: `a`, `button`, `details`, popover.
  - The demo frame has a `title`, and full screen returns focus to the opener (`embed.js` does this).
  - Arrow keys never scroll-jack.
- **Zoom and reflow.**
  - Type uses `rem` plus `vw` clamps, so 200% zoom scales.
  - No horizontal scroll at 320 px. Tables scroll inside their own container.
  - Test at 320, 360, 768, 1024, 1440 and 1920 px.
- **Forced colours and contrast preference.** Real borders on cards and controls; gradient text falls back to `CanvasText` (§5.3).
  - Optionally, map `@media (prefers-contrast: more)` to the `[data-contrast="more"]` values from `base.css`.
- **Language and readability.** `lang="en"`, the UI_SPEC §6 voice, sentence case, and no text inside images except real captures.
- **Automated check.** In `e2e-site`, inject `axe-core` (already a dev dependency of `packages/ui`) from disk with `page.addScriptTag({ path })`. Run it on every `SITE_PAGES` and docs page under `colorScheme: "dark"` and `"light"`, failing on serious or critical issues. Lighthouse accessibility should be 100.

---

## 10. Performance budget and techniques

### 10.1 Budgets
Assert these in `lighthouserc.json` and the site e2e tests.

| Metric (Lighthouse mobile, median of 3, served compressed) | Budget |
|---|---|
| Performance / Accessibility / Best practices / SEO on `/`, `/pricing/`, `/download/`, `/changelog/`, `/docs/` | **≥ 95 / ≥ 95 (target 100) / ≥ 95 / ≥ 95** |
| CLS | ≤ 0.02 (target 0) |
| LCP | ≤ 2.0 s |
| TBT | ≤ 100 ms |
| FCP | ≤ 1.4 s |
| HTML (gzip), home | ≤ 40 KB (≤ 55 KB if CSS is inlined) |
| CSS (gzip), marketing pages | ≤ 20 KB |
| Framework JS (gzip), marketing pages | 0 KB if the runtime is stripped (§10.3); otherwise no more than today (~100 KB) |
| Fonts | 1 preloaded file (Inter latin opsz, 73 KB); JetBrains Mono (40 KB) not preloaded |
| First-viewport images (home) | Demo still AVIF ≤ 70 KB, `fetchpriority="high"` (desktop); none on mobile beyond the still |
| Whole home page before scrolling, excluding the demo app and video | ≤ 350 KB |

### 10.2 Techniques (all first-party)
1. **Fonts:** `next/font/local` with preload and metric-matched fallback (`adjustFontFallback`). Only the latin subsets; `display: swap`.
2. **Images.**
   - `<picture>` with AVIF, WebP and fallback; fixed `width`/`height` or `aspect-ratio` on every media box.
   - `loading="lazy"` below the fold. React then doesn't add preloads, which fixes the current preload of `launch-grid.png`.
   - `fetchpriority="high"` only on the demo still.
3. **No layout shift.**
   - The demo slot keeps `aspect-ratio: 1280 / 800`.
   - The OS note on `/download/` reserves its line box (`min-h-6`) even while hidden, or sits where its insertion moves nothing.
   - Fonts use fallback metrics.
   - No content is injected above existing content.
4. **Video:** `preload="none"`.
5. **The demo** loads only near the viewport and only on ≥ 1024 px screens (unchanged), and never during a mobile Lighthouse run.
6. **CSS.**
   - Tailwind 4 already purges unused classes.
   - Try `experimental.inlineCss: true` in the `siteExport` config to remove the render-blocking stylesheet request. Measure FCP and LCP and keep it only if mobile improves.
   - The docs CSS (61 KB) stays on docs pages only.
7. **Rendering.** `content-visibility: auto; contain-intrinsic-size: auto 900px` on sections below the FAQ fold. Measure CLS when scrolling and drop it if it moves anything.
8. **Caching** (`vercel.json`, added after the existing rules so the header-order test still holds):
   - `/screens/(.*)` and `/demo/still(.*)`: `public, max-age=86400, stale-while-revalidate=604800`.
   - Or use content-hashed file names with `immutable`.
   - `/_next/static` is already immutable.
9. **No third parties:** zero analytics, tag managers, cookie banners or chat widgets. This is one reason the site can be fast.
10. **Static motion:** CSS only; nothing on the main thread.

### 10.3 Optional: remove the unused Next runtime from marketing pages
- **Why.** The App Router always ships its runtime (~100 KB gzip) and an inline RSC payload (≈50% of each HTML file), even for pages without client components. Vibezeo's marketing pages use plain `<a>` links and no client components.
- **How.** Add `apps/web/scripts/strip-runtime.mjs` and run it at the end of `build:site`, over `out/**/*.html` **excluding `out/docs/**` and `out/demo/app/**`**. It removes:
  - `<script src="/_next/static/chunks/…">`,
  - `<link rel="preload" as="script" href="/_next/static/chunks/…">`,
  - inline `self.__next_f.push(…)` scripts.

  It keeps `/demo/embed.js` and `/download/detect.js`, then **fails the build** if any other `<script` remains in those files. That is the AC2 rule at the HTML level.
- **Tests.** Add an e2e assertion that marketing pages request no `/_next/static/chunks/*.js`.
- **Decision.** Record a D-n.
- **Risks.**
  - Any future client component on a marketing page would silently stop working. Add a lint or test that `components/site/**` has no `"use client"`.
  - A Next upgrade could change the markup, but then the post-check fails loudly instead of shipping broken pages.
  - The `dangerouslySetInnerHTML` workarounds (D-128, D-132) become unnecessary but harmless; keep them.
- **Do it only if** §10.1 budgets aren't met after Phases 1–3. It's the largest single win available.

### 10.4 Lighthouse CI config
```json
{
  "ci": {
    "collect": {
      "staticDistDir": "./out",
      "url": ["http://localhost/", "http://localhost/pricing/", "http://localhost/download/", "http://localhost/changelog/", "http://localhost/docs/"],
      "numberOfRuns": 3,
      "settings": { "chromeFlags": "--headless=new --no-sandbox" }
    },
    "assert": {
      "assertions": {
        "categories:performance": ["error", { "minScore": 0.95, "aggregationMethod": "median-run" }],
        "categories:accessibility": ["error", { "minScore": 0.95, "aggregationMethod": "median-run" }],
        "categories:best-practices": ["error", { "minScore": 0.95, "aggregationMethod": "median-run" }],
        "categories:seo": ["error", { "minScore": 0.95, "aggregationMethod": "median-run" }],
        "cumulative-layout-shift": ["error", { "maxNumericValue": 0.02, "aggregationMethod": "median-run" }],
        "largest-contentful-paint": ["warn", { "maxNumericValue": 2000, "aggregationMethod": "median-run" }],
        "total-blocking-time": ["warn", { "maxNumericValue": 100, "aggregationMethod": "median-run" }]
      }
    },
    "upload": { "target": "filesystem", "outputDir": "./.lighthouseci/reports" }
  }
}
```
Run it through OPERATING §7 (`CHROME_PATH=/opt/pw-browsers/chromium pnpm --filter web lhci`), served compressed. Raising the thresholds from 0.90 to 0.95 tightens F-WB-01 AC1, so the test's intent is kept and strengthened.

---

## 11. Copy and claims
- **Voice** (UI_SPEC §6): plain, calm, specific. Second person. No hype, no exclamation marks. Engine names exactly as vendors write them. "agent", not "bot" or "AI". "sign in". Numerals with separators.
- **Reuse before you write.** The headings in §4.3 reuse existing site copy, which has already passed honesty review. New copy is limited to:
  - Mode row titles, for example "Code mode: real terminals, side by side" and "One Dashboard: every agent, and which ones need you".
  - The bento header and the capability index header.
  - The trust statement and the diagram labels.
  - "What's new" in the announcement pill.

  Treat all of it as drafts for the owner.
- **Claims ledger.** Every claim on the site maps to a source and, where possible, a test:

| Claim | Source of truth | Guarded by |
|---|---|---|
| Version, platform, file name, SHA-256, size | `lib/release.ts`, `SHA256SUMS.txt`, `statSync` | `site.test`, `discovery.test`, new size test |
| Tests, suites, milestones, engines | `PROGRESS` (from gate output and spec files) | `site.test` |
| Prices, trials, seats, credits | `PLANS`, `PRICES`, `CREDIT_ROWS`, remote config | `wb01.test`, `discovery.test` |
| Plan limits | `LIMITS` | New `PlanComparison` test, H3 consistency test |
| "New in x.y.z" | `RELEASES[0]` | New bento test (prefix of CHANGELOG text) |
| Shortcuts | `shortcuts.json` | New keys test |
| Engines and monograms | `ENGINES`, `registry.json` | `site.test`, new registry test |
| Screenshot and video build | `lib/media.ts` `recordedWith` | New media test |
| "Nothing sent to our servers" | 0.5.0 CHANGELOG summary, `TANDEM_HOSTED=0` | Existing download-page copy test |
| Built with Claude statements | Existing copy (commit history) | `site.test` |

- **Forbidden on Home, Download and Pricing** (tests): "early access", "launch", "Coming soon", "are next", "Planned", "on sale yet", ".dmg", ".msi", "(for|on) macOS, Windows and Linux". Elsewhere, the CHANGELOG text may say "macOS and Windows are next".

---

## 12. Phased implementation plan
Land this after `release-0.5.0` merges into `product-m0-m5`, or rebase onto it: that branch is editing `page.site.tsx` and `progress.ts` right now. Work on its own branch and record each decision as a D-n in `BUILD_LOG.md`. Run the gate (`pnpm gate`), `pnpm --filter web exec vitest run`, `build:site`, `test:site` and `lhci` before each merge (OPERATING §7). Deploy only with the owner's go-ahead.

**Phase 0: honesty fixes** (≈½ day; can ship on its own)
- H1: add `lib/media.ts` and correct the video caption (or re-record). Re-render the poster without "0.1.0".
- H2: re-capture `still.jpg` from the 0.5.0 demo and add `scripts/agent/demo-still.mjs`.
- H3: Pro panes copy or `LIMITS` (owner). Add a test that `PLANS` copy can't say "Unlimited panes" while `LIMITS.pro.panes !== null`.
- H4: fix the Fedora requirement. H5: replace hard-coded shortcut copy with `Keys`. H6: privacy grammar.
- Add the mobile menu (Popover API). It's a navigation hole, fixed independently of the redesign.
- **Done when:** all tests pass, plus the new media, limits and keys tests.

**Phase 1: foundation** (≈1–2 days)
- Add `packages/ui/src/styles/site.css` and its export, the light-theme mirror, and the drift and contrast tests.
- In `globals.css`, import `site.css` and `base.css`; switch fonts to `next/font/local`.
- Drop `data-theme="dark"`; add the `viewport` colour scheme and theme colours.
- Add `buttonClass()` and the `xl` size; add `lucide-react` to `apps/web`.
- Build `SiteHeader` (desktop plus popover), `SiteFooter` (4 columns, no year), `Section`, `ProductShot`, `Keys`, `CommandLine`, `Mark`.
- Unify containers. Add the view-transition and reduced-motion CSS.
- **Done when:** every existing page renders in both themes with no test changes. axe is clean in both schemes, there's no horizontal scroll at 320 px, and Lighthouse doesn't regress.

**Phase 2: home page** (≈2–3 days)
- Move the data arrays from `page.site.tsx` to `lib/home.ts` (FEATURES, CLAUDE, ROADMAP, PRINCIPLES, FAQ, plus the bento icon and link map).
- Build the 15 sections (§4.3, §6.2–§6.15) and the demo stage in its new place.
- Add `ReleaseBento` from `RELEASES[0]`, `FeatureIndex`, the `TrustFlow` SVG, `Faq` with `<details>`, and `CtaBand`.
- Apply the tagline test tweak (§14). Re-run `demo-embed.spec` (AC1 position).
- **Done when:** all pinned strings are present, the e2e and demo specs pass, and Lighthouse mobile is ≥ 95 or §10.3 is queued.

**Phase 3: Pricing, Download, Changelog, Demo, About, prose pages, 404** (≈2 days)
- `PlanCards` and `PlanComparison` (generated rows, REMAINING-ticked).
- `DownloadPanel` (size from the manifest, commands, steps with `Keys`).
- `ReleaseRail`, milestones grid, and the version page with prev/next.
- Restyle `/demo/` and the `ProsePage` typography.
- **Done when:** the pinned strings and the wb01 Playwright tests pass, and the comparison table equals `LIMITS`.

**Phase 4: docs** (≈1 day)
- Header title with the mark; grouped `meta.json` and the reordered `DOCS_PAGES` (with the test tweak).
- `docs.css` typography and `kbd`; the `Steps` and `CommandLine` MDX components; `DocsFooter` with the legal links.
- **Done when:** the docs tests pass, plus the new legal-links test, and `wb01.spec` storage and origin checks still pass.

**Phase 5: media refresh** (partly human; can run in parallel from Phase 1)
- Extend `site.screens.ts` with the shots in §7.2 at 2×, and add `optimize-screens.mjs` (sharp) for AVIF and WebP.
- Add `HUMAN_TODO.md` entries for the signed-in Thread, Agent chat and Dashboard captures, and optionally the video re-record.
- **Done when:** every image has AVIF, WebP and fallback, fits the size budget, and has a `MEDIA` entry with `recordedWith`.

**Phase 6: performance hardening** (≈1 day)
- Apply the new `lighthouserc.json` and measure.
- If needed: `experimental.inlineCss`, `content-visibility`, then the runtime strip (§10.3) with its build check and e2e test.
- Add caching headers.
- **Done when:** the §10.1 budgets hold on all five URLs, three runs each.

**Phase 7: QA and release**
- Run the full gate, `test:site`, `lhci`, and `scripts/agent/demo-walkthrough.mjs`.
- Check by hand in both themes at 320, 390, 768, 1024, 1440 and 1920 px, with reduced motion, keyboard only, Orca or VoiceOver smoke tests, and JavaScript disabled.
- Update the OPERATING §6 checklist (still, poster, screenshots, video `recordedWith`) and the `BUILD_LOG.md` decisions.
- Deploy only with the owner's go-ahead (OPERATING §2 and §7).

---

## 13. Files to change

**`packages/ui`**
- `src/styles/site.css` (new): site tokens, the OS light-theme mirror, type scale, containers and utilities (§5.3).
- `src/styles/site.test.ts` (new), or extend `contrast.test.ts`: light-block drift and site contrast pairs.
- `package.json`: `"./site.css": "./src/styles/site.css"` export.
- `src/components/Button.tsx`: export `buttonClass(variant, size)` and add the `xl` size (with a unit test).

**`apps/web`**
- `package.json`: add `lucide-react`, `@fontsource-variable/inter`, `@fontsource-variable/jetbrains-mono` (same versions as `packages/ui`), and `sharp` (dev). Optionally chain `node scripts/strip-runtime.mjs` onto `build:site`.
- `app/globals.css`: import `site.css` and `base.css`; font variable overrides; body typography; drop `fonts.css`.
- `app/fonts.ts` (new): `next/font/local`.
- `app/layout.site.tsx`: no `data-theme`; font classes; `viewport` export (colour scheme and theme colours).
- `components/site/SiteFrame.tsx`: compose `SiteHeader` and `SiteFooter`; `current` prop; keep the `SiteFrame` and `ProsePage` exports.
- `components/site/` (new):
  - Frame and primitives: `SiteHeader.tsx`, `SiteFooter.tsx`, `Mark.tsx`, `Section.tsx`, `Keys.tsx`, `CommandLine.tsx`.
  - Home sections: `Hero.tsx`, `DemoStage.tsx` (moves `DEMO_STAGE`), `ProductShot.tsx`, `StatsBand.tsx`, `EngineList.tsx`, `ModeRow.tsx`, `ReleaseBento.tsx`, `FeatureIndex.tsx`, `TrustFlow.tsx`, `ClaudeBand.tsx`, `FounderNote.tsx`, `VideoSection.tsx`, `Roadmap.tsx`, `Faq.tsx`, `CtaBand.tsx`.
  - Other pages: `PlanCards.tsx`, `PlanComparison.tsx`, `DownloadPanel.tsx`, `ReleaseRail.tsx`.
- `app/page.site.tsx`: compose the sections. Data moves to `lib/home.ts` (new).
- `app/pricing/page.site.tsx`, `app/download/page.site.tsx`, `app/changelog/page.site.tsx`, `app/changelog/[version]/page.site.tsx`, `app/demo/page.site.tsx`, `app/about/page.site.tsx`, `app/not-found.site.tsx`; `app/privacy/page.site.tsx` (H6).
- `app/docs/layout.site.tsx`, `app/docs/[[...slug]]/page.site.tsx` (add `DocsFooter`), `app/docs/docs.css`.
- `components/docs/DocsFooter.tsx` and `components/docs/Steps.tsx` (new).
- `content/docs/meta.json` (groups) and `lib/docs/pages.ts` (order). `content/docs/index.mdx` (steps, commands, generated keys). Other MDX files only where shortcut copy is hard-coded (H5).
- `lib/media.ts` (new, H1). `lib/manifest.ts` (`bytes`). `lib/release.ts` (H4). `lib/pricing.ts` (H3, `PLAN_ROWS`). `lib/docs/tables.ts` (share `PLAN_ROWS`/`LIMIT_ROWS`).
- `public/demo/still.{jpg,webp,avif}` (re-captured). `public/screens/*` (new and re-taken, with AVIF and WebP). `public/screens/product-poster.jpg` (re-rendered).
- `next.config.ts`: optional `experimental.inlineCss` in the export config.
- `lighthouserc.json` (§10.4). `vercel.json`: cache rules appended after the existing ones.
- `scripts/optimize-screens.mjs` (new); `scripts/strip-runtime.mjs` (new, optional).
- `e2e-site/site-quality.spec.ts` (new): axe in both schemes, no horizontal scroll at 320 px, CLS via `PerformanceObserver`, reduced motion, no runtime chunks (if stripped), docs legal links.
- `app/site.test.tsx`, `app/docs/docs.test.tsx`: §14.

**Elsewhere**
- `apps/desktop/e2e/screens/site.screens.ts`: the new captures (§7.2).
- `scripts/agent/demo-still.mjs` (new).
- `media/product-video/*`: only if re-recording (H1b).
- `docs/OPERATING.md` §6 and §7: still, poster, screenshot and media steps; the new LHCI URLs.
- `HUMAN_TODO.md`: captures with a signed-in engine; optional video re-record; owner decisions in §15.
- `BUILD_LOG.md`: D-n entries (light theme by OS, fonts through `next/font`, demo stage placement, runtime strip if adopted, docs groups).

---

## 14. Tests: what changes, what's new

**Existing tests that change** (two edits, intent kept):
1. **`site.test.tsx`, "F-WB-01 home shows the tagline…".** Compare the tagline against text content: `expect(h.replace(/<[^>]+>/g, "")).toContain(SITE.tagline)`.
   - **Why the intent holds:** the test checks that the tagline is visibly on the home page. Wrapping "coding agents" in a highlight `<span>` keeps it there, word for word.
   - Every other assertion in the file stays as is.
   - Skip this change if the owner prefers the plain h1.
2. **`docs.test.tsx`, "the sidebar lists the pages in DOCS_PAGES order".** Ignore Fumadocs separators: `meta.pages.filter((p) => !p.startsWith("---"))`.
   - **Why the intent holds:** the sidebar still lists exactly `DOCS_PAGES`, in the same order. Group labels aren't pages.

**No other existing test changes** if the pinned strings in §1.4 are kept. If the demo loader's trigger changes (§7.1, optional), `demo-embed.spec.ts` stays unchanged and must still pass.

**New tests:**

| Test | File | What it checks |
|---|---|---|
| F-WB-01 site light theme mirrors the app's light tokens | `packages/ui/src/styles/site.test.ts` | Every `--tn-*` in the `site.css` media block equals `[data-theme="light"]` in `tokens.css`, and vice versa |
| F-WB-01 site colour pairs meet WCAG AA in both themes | same | The §5.5 pairs, including the gradient endpoints and the stage |
| F-WB-01 the video and screenshots name the build they show (H1) | `app/site.test.tsx` | The caption contains `VIDEO.recordedWith`; no media caption interpolates `RELEASE.version` unless `recordedWith === RELEASE.version` |
| F-WB-01 plan copy matches enforced limits (H3) | `app/wb01.test.tsx` | "Unlimited" panes or workspaces appear only where `LIMITS` is `null` |
| F-WB-01 the comparison table is `LIMITS` | `app/wb01.test.tsx` | Each `<tr data-limit=…>` cell equals the generated value; every row's feature is ticked in REMAINING |
| F-WB-01 "New in" tiles come from CHANGELOG | `app/site.test.tsx` | Tile titles equal the bold lead phrases of `RELEASES[0]` Added; bodies are prefixes of the bullet text; every title has a docs link |
| F-WB-01 shortcuts on the site come from the registry (H5) | `app/site.test.tsx` | `Keys` output on home, download and docs index equals `shortcuts.json` (pc and pcTerminal) |
| F-WB-01 engine monograms come from the registry | `app/site.test.tsx` | Every `ENGINES` label exists in `registry.json`, and the glyph letters equal `logo` |
| F-WB-01 download shows the build's size | `app/discovery.test.tsx` | The size shown equals `statSync` of the served file |
| F-WB-02 every docs page has the legal links and contact | `app/docs/docs.test.tsx` | `DocsFooter` markup contains `/privacy/`, `/terms/`, `/security/`, `/about/`, `hello@vibezeo.com` |
| F-WB-01 no fallback glyphs in site markup | `app/site.test.tsx` | No "→", "✓", "⌘" outside `<kbd>` (icons instead) |
| Site quality (e2e) | `e2e-site/site-quality.spec.ts` | axe (serious or critical = 0) per page per scheme; `scrollWidth <= innerWidth` at 320 px; CLS < 0.02; the reduced-motion snapshot has no running animations; (if stripped) no `/_next/static/chunks/` requests on marketing pages |
| (If stripped) the export has only allowlisted scripts | `scripts/strip-runtime.mjs` build check | Fails the build otherwise |

---

## 15. Open questions for the owner

1. **Video.** Label it as the 0.1.0 recording (fast), or re-record it with 0.5.0? The caption must change either way.
2. **Pro panes.** Change the copy to "up to 64" or change `LIMITS.pro.panes` to unlimited?
3. **Fedora.** Drop it from the requirement, or add an AppImage download?
4. **Theme.** Should the site follow the OS (recommended), or stay dark-only for brand reasons? A toggle isn't possible without storage.
5. **Header.** Keep "Claude" as a header item (today's nav) or move it to the footer and the home section?
6. **Hero highlight.** Use the teal→sky "coding agents" highlight (one-line test tweak) or a plain h1?
7. **Signed-in captures.** Can you capture Thread mode, Agent chat and the Dashboard with a signed-in engine on a sample repository? These need your machine and accounts.
8. **Runtime strip.** Is stripping Next's runtime from marketing pages acceptable if needed for Lighthouse 95+ (§10.3)?
9. **Founder.** Supply a photo and a longer founder note, or keep the text card?

---

## Appendix A: inspection notes per captured page

**Home pages**

| Page | Notes |
|---|---|
| bridgemind.ai | 13,284 px tall, 15 sections, 6 FAQ `<details>`, 2 canvases, 1 iframe (live demo). Fonts: Inter plus Sora 600. Container queries and `prefers-reduced-motion` present. |
| linear.app | 9,960 px tall. Inter Variable plus Berkeley Mono. View transitions in CSS. Footer has 5 columns. |
| vercel.com | 5,333 px tall. Geist and Geist Mono. `@starting-style` in CSS. Follows the OS colour scheme. 1.2 MB JS. |
| raycast.com | 15,983 px tall. Inter, Geist Mono, JetBrains Mono. View transitions. Keycap and dock storytelling. |
| warp.dev | 9,395 px tall. Light. Azeret SemiMono display. 21 third-party script origins. |
| zed.dev | Inner scrolling container. Plex Serif plus a custom mono. Grid-framed column. |
| cursor.com | 8,173 px tall. CursorGothic, Berkeley Mono, EB Garamond. 174 scripts, 10 tracker origins. |
| stripe.com | 14,803 px tall. Söhne. `animation-timeline` present. 40 `<picture>` elements. |
| framer.com | 10,741 px tall. GT Walsheim. 4 videos. 11 third-party origins. |
| arc.net | 6,125 px tall. Marlin. 4 videos. Dedicated privacy section. |
| ghostty.org | One screen. Pretendard plus JetBrains Mono. Animated ASCII terminal. |

The pricing, changelog, download and docs pages captured (Linear, Raycast, Zed, Cursor, Ghostty, Tailwind CSS) are summarised in each site's entry in §3.2.

## Appendix B: sources
- BridgeMind's official site, identified by web search (third-party listings at everydev.ai and hunted.space point to it): https://www.bridgemind.ai/
- Linear: https://linear.app/ · https://linear.app/pricing · https://linear.app/changelog
- Vercel: https://vercel.com/
- Raycast: https://www.raycast.com/ · https://www.raycast.com/pricing · https://www.raycast.com/changelog
- Warp: https://www.warp.dev/
- Zed: https://zed.dev/ · https://zed.dev/pricing · https://zed.dev/download · https://zed.dev/docs/ · https://zed.dev/releases/stable
- Cursor: https://cursor.com/ · https://cursor.com/changelog · https://cursor.com/download
- Stripe: https://stripe.com/
- Framer: https://www.framer.com/
- Arc: https://arc.net/
- Ghostty: https://ghostty.org/ · https://ghostty.org/download
- Tailwind CSS docs: https://tailwindcss.com/docs/installation/using-vite
- Repository sources: `apps/web/**`, `packages/ui/src/styles/*`, `packages/protocol/src/plans.ts`, `packages/protocol/shortcuts.json`, `crates/tandem-engines/registry.json`, `CHANGELOG.md`, `CLAUDE.md`, `build-kit/DECISIONS.md`, `build-kit/UI_SPEC.md`, `build-kit/PRODUCT_SPEC.md` §4.17, `docs/OPERATING.md`, `docs/FEATURES_TO_BUILD.md` ("The interactive demo on the website"), `BUILD_LOG.md` (D-126, D-128, D-132), `media/product-video/README.md`.
