# Vibezeo design direction

**Owner:** the design lead (`design-lead`). **Decisions:** D-1890 to D-1899, then D-2700 on (BUILD_LOG "Design direction (`design-lead`)").
**Read with:** `build-kit/DECISIONS.md` (Identity, Visual identity, the clean room), `build-kit/UI_SPEC.md` §1 (the app's tokens), `docs/WEBSITE_ELITE.md` (the site's lit system, D-1830 to D-1844) and `docs/BLOG.md` §1.
**Who builds on it:** `app-ui` (the app: tokens, engine marks, pane headers, rail, launch grid, Agent mode's first run, status bar, threads; decisions from D-2600) and `site-motion` (the site: dark mode, motion and animated product scenes; decisions from D-1870).

This document is the direction, not a spec to paste. Where it names a value, the value is the target; the token files in `packages/ui/src/styles/` are the source of truth once a slice lands. Where it says **must**, a design review ranks breaking it as must-fix.

---

## 0. The ask, and where we are

The owner, 20 October: visitors should feel they are visiting "something premium and not something cheap and just another product", and should want to stay, try the product, use it and pay for it. "The features are primary but the look and feel is equally important."

What we have is good engineering wearing a competent but generic coat. Captured from vibezeo.com (0.8.0) and its live demo at 1440×900, 20 October:

| Where | What we see | Why it costs us |
|---|---|---|
| Home, first viewport | The hero fills the whole fold; the product starts below it. The lead names three other products. | Visitors judge in seconds. The best sites show their product in the first viewport. |
| Home, length | 12,984 px: 12 sections, most of them paragraphs plus tick lists, 18 capability tiles, "Read the docs" five times. | Density without hierarchy reads as a spec sheet. Premium sites say less, larger. |
| Accent | Teal is the buttons, every eyebrow, every inline link, every tick, the headline gradient and the demo coach card's border. | When everything is teal, nothing is. The accent stops pointing at the next step. |
| Product imagery | Real captures (good), but the demo opens on an empty thread with a six-step coach card over the rail. | The first look at the product is an empty state and a to-do list. |
| Demo, Agent mode | Two empty states side by side: "No teammates. Add one with +." and "No teammate open / Pick a teammate in the rail." (The installed app creates and opens Lead instead.) | The mode we're proudest of says nothing about what it is for in the one place visitors try it, and offers no button in the main area. |
| App, rail | Workspace rows carry uppercase bordered pills ("WORKING 1", "WAITING ON YOU") that truncate the workspace name to "sh…". | Status is loud and names are lost: UI_SPEC §1.1's "calm surfaces, loud status" turned up past loud. |
| App vs site | The site has a lit system (edges, elevation, blooms, expo-out motion, display type). The app has flat surfaces and single grey shadows. | They feel like two products. The download should feel like the site you just left. |
| Honesty slips (20 October; the upgrade dialog goes with D-869) | The upgrade dialog says "Unlimited panes & threads" (`LIMITS`: Pro is 64 panes, 6 threads) and lists the hosted features (Conductor, cloud Dictate, Enhance, Auto) beside local ones as if all were the same kind of thing, though the hosted ones need accounts that aren't open. The download page says "choose Start"; the button reads "Start using Vibezeo". | Our edge is trust. One untrue line on the way to paying costs more than any visual polish buys. |
| Voice | Honest and exact, often long ("Vibezeo finds the engine CLIs installed on your machine and launches them in panes, each signed in to your own account. If an engine is missing, Vibezeo shows you how to install it."). | Honesty is our edge; length hides it. |

The plan: keep the identity (DECISIONS: Inter, JetBrains Mono, teal, the two rings; the neutrals move from slate to a true neutral, D-1894), make one design system for both surfaces (§4), cut the site's words in half and show the product sooner (§5.1), and give the app the same light, edges and motion so first run feels like a continuation of the site (§5.3).

---

## 1. Brand attributes

Five words, each with what it means on screen and what it never does. Every review checks a change against all five.

| Attribute | On screen | Never |
|---|---|---|
| **Calm** | Neutrals carry 95 % of every view. One accent, used for the next step. Status colour only where something needs you. Motion settles; nothing loops in the corner of your eye except a working agent. | Badges on everything, colour as decoration, a pulsing upgrade button, confetti. |
| **Precise** | A 4 px grid, nested radii, aligned baselines, tabular numbers, 1 px hairlines that line up across panes. Copy names the exact thing ("Waiting on you · Claude Code in checkout"). | Half-pixel borders, off-grid gaps, "something went wrong", rounded numbers that are wrong. |
| **Fast** | Feedback inside 100 ms (Doherty, §2). Hovers 120–180 ms, menus 180 ms, nothing on a critical path over 260 ms. The site paints its h1 under 2 s and runs no framework script. | Spinners for local work, entrance animations that block input, skeletons for things already in memory. |
| **Trustworthy** | Every claim true of the shipped build (D-128), with its version named. Hosted features labelled "Comes with Vibezeo accounts", never priced (§5.5). Data flows drawn, not described. Real captures, labelled with their build. | Mock-ups presented as the product, fake urgency, "trusted by" walls, hidden costs, share or rating nags. |
| **Crafted** (premium) | Light that comes from somewhere: a lit top edge, layered soft shadows, a bloom behind the hero. Display type tracked tight. Empty states that teach. Details nobody asked for (the key cap that travels 1 px). | Stock gradients, default shadows, generic illustrations, emoji in UI chrome, a different look on every screen. |

Order matters when they conflict: **Trustworthy > Fast > Calm > Precise > Crafted**. A beautiful effect that slows the first paint, or a striking claim we can't prove, loses.

---

## 2. Principles from the best design books

Ideas, not quotations. Each ends in a rule we apply.

**Refactoring UI (Wathan, Schoger)**
- *Hierarchy is not size alone:* de-emphasise secondary text with colour and weight before making the primary bigger. → Three text tones only (`fg`, `fg-muted`, `fg-subtle`); labels get muted, values get ink.
- *Start with too much white space, then remove it.* → Site sections keep 80–144 px; the app keeps its 4 px grid and compact density, but empty states and first run get room.
- *Define systems in advance:* a constrained spacing and type scale beats picking values. → §4; no arbitrary Tailwind values in new code.
- *Depth from light:* a light top edge and a dark bottom shadow make a surface sit up; two shadows (tight contact + soft ambient) beat one. → §4.5's e1–e3.
- *Don't use grey on colour; fewer borders* — use spacing, background shifts and shadows to separate. → The rail loses boxed pills (§5.4).

**The Design of Everyday Things (Norman)**
- *Affordances and signifiers:* things that can be pressed look pressable. → Buttons have a lit edge and travel 1 px when pressed; rows show a hover fill; drag handles appear on hover.
- *Feedback:* every action answers at once, and its result is visible. → Send shows the message in the thread immediately; Split shows the pane growing (§4.7).
- *Mapping:* controls sit next to what they change. → Pane actions live in the pane header; a refusal (no folder, an engine not signed in) is explained on the control that hit it, not in a modal elsewhere.
- *Constraints and forgiving errors:* prevent the mistake, make undo cheap. → Destructive actions name their object and offer undo where the core allows it.
- *The gulfs of execution and evaluation:* the user must see what they can do and what happened. → Every agent's state is visible in three places (pane header, rail, Dashboard) with a shape, not colour alone.

**Don't Make Me Think (Krug)**
- *People scan, they don't read; they satisfice.* → Headlines carry the meaning alone; leads are one or two sentences; tick lists are three items at most.
- *Omit needless words; get rid of half the words, then half of what's left.* → §4.8; the home page's copy target is half of today's.
- *Make it obvious what is clickable; one obvious next step per screen.* → One primary button per view.
- *The trunk test:* on any page, can you tell what site this is, where you are and what you can do? → Every page has the header, a clear h1 and one primary action.

**Laws of UX (Yablonski)**
- *Hick's law:* choice time grows with the number of options. → First run offers one recommended engine and one folder, with "others" one click away; the hero has two CTAs, not three.
- *Fitts's law:* targets near and large are faster. → Primary CTAs 48 px tall on the site; the composer's send button stays at the composer's edge; pane close targets at least 24 px.
- *Jakob's law:* people spend most of their time in other products. → Terminals behave like terminals; Ctrl/Cmd+K is the palette; the download page looks like a download page.
- *Doherty threshold:* under 400 ms keeps people in flow; under 100 ms feels instant. → §1 "Fast"; optimistic UI for local actions.
- *Aesthetic–usability effect:* attractive designs are perceived as easier to use and are forgiven more. → Craft is not decoration; it buys patience during first run.
- *Peak–end rule:* people judge an experience by its peak and its end. → Design the first agent finishing (§5.3) and the end of the download (§5.2) as the peaks.
- *Goal-gradient and Zeigarnik effects:* progress shown pulls people forward; unfinished tasks are remembered. → First run shows two steps, not a checklist of six.
- *Von Restorff (isolation) effect:* the one different thing is remembered. → The accent is that one thing (§4.2).

**Practical Typography (Butterick) and The Elements of Typographic Style (Bringhurst)**
- *Body text sets the page:* 15–25 px on screens, line length 45–90 characters, line height 120–145 %. → Site copy 16 px at 1.65 in a 38–44 rem measure; app chat 14/22 at most 72 ch.
- *Bringhurst: choose a scale and stick to it; space in the margins, not between letters; letter-space capitals; use real figures.* → One modular scale per surface (§4.3); caps labels at +0.06 em (app) and +0.12 em (site mono eyebrows); tabular figures for every number that changes.
- *Butterick: avoid all caps for more than a few words; bold or italic, not both; one space; real quotes and apostrophes.* → Status labels move from uppercase pills to sentence-case text (§5.4); curly quotes in copy.
- *Kerning and tracking grow negative with size.* → Display tracking from −0.024 em (30 px) to −0.042 em (76 px), as the site already does.

**Thinking with Type (Lupton)**
- *Typographic contrast (size, weight, colour, case) creates hierarchy with few fonts.* → Two families only (Inter, JetBrains Mono); mono is a voice for facts (versions, paths, keys, figures), never for paragraphs.
- *Text as interface:* labels, captions and microcopy are designed, not filled in. → §4.8 has rules for each.

**Grid Systems in Graphic Design (Müller-Brockmann)**
- *A grid gives order, rhythm and recognisability; every element relates to it.* → The site's 12-column grid in a 1200 px container with 24 px gutters; the app's 4 px base and fixed chrome heights (title bar 40, pane header 30, status bar 22).
- *Objectivity: the grid serves the content and disappears.* → No decorative grid lines on the page (the faint grid stays only in og.png and posters).

**Universal Principles of Design (Lidwell, Holden, Butler)**
- *Signal-to-noise ratio:* maximise the signal, minimise everything else. → Remove borders that a background shift already makes; remove repeated "Read the docs" links.
- *Progressive disclosure:* show the minimum, reveal on demand. → Advanced launch options behind "More options"; the site's capability index collapses to the eight most-used with "Everything else" linking the docs.
- *Consistency (aesthetic, functional, internal, external)* → One token source (§4.1).
- *Figure–ground and closure* → Product frames sit on a quiet ground; nothing competes behind the demo.

**Designing Interfaces (Tidwell, Brewer, Valencia)**
- *Patterns: Instant Gratification, Safe Exploration, Habituation, Prospective Memory, Incremental Construction.* → First run gets the user to a running agent in under a minute (Instant Gratification); the demo resets in one click and changes nothing (Safe Exploration); keyboard paths never change between releases (Habituation); Notes and the review queue hold what you meant to come back to (Prospective Memory).
- *Good defaults and "Input Hints"* → Engine, model and permission default to the user's last choice; empty composers show an example task, not "Type here".

**About Face (Cooper)**
- *Design for intermediates:* most users are perpetual intermediates; beginners become them fast and experts are rare. → Don't build a tutorial; make the interface explain itself, and keep power behind shortcuts and the palette.
- *Goal-directed design and personas:* our people are developers running several agents at once who want to see which one needs them. → The Dashboard and the status shapes are the product's heart; they get the craft budget first.
- *Don't make the user feel stupid; no modal "are you sure" for undoable actions; respect the user's attention (excise).* → Confirm only the irreversible; never interrupt a typing user with a toast that steals focus.

---

## 3. What makes premium products feel premium

Sources: `docs/WEBSITE_ELITE.md` §1 (14 sites read from their HTML, CSS and captures, 15 October), and a fresh pass on 20 October: Stripe (home, pricing), Linear (home, pricing, method), Vercel (home, pricing), Raycast, Warp, Arc, Apple (a laptop page) and BridgeMind, measured with headless Chromium at 1440×900 and 390×844 (computed styles, first-viewport CTAs, section padding, transition timings). Public pages only; principles only, under the clean room. Arc's onboarding notes are from general knowledge, not checked this session.

| Site | Ground | Hero (size / weight / tracking / words) | Accents above the fold | CTAs in first viewport | Real UI in first viewport | Section padding | Motion |
|---|---|---|---|---|---|---|---|
| Stripe | white; pricing #F6F9FC | 48 px / 300 / −0.02 em / 7 (+22 muted) | ~4, all actionable | 2 | no (art) | grid-ruled | 300 ms hovers, 800 ms reveals, ease-out |
| Linear | #08090A | 64 px / 510 / −0.022 em / 8 | 0 | none in the hero | yes, a populated app from y≈490 | 128 px | 100–160 ms, ease-out |
| Vercel | #FAFAFA | 64 px / 400 / −0.06 em / 2 | 0 | 2 | no (mark) | 72–144 px | 100–150 ms |
| Raycast | #07080A | 64 px / 600 / 0 / 4 | 1 (art) | 1 (download) | no (art) | large | 300–400 ms, ease-out |
| Warp | off-white | 54 px mono / 400 / 6 | 2 | 3, uppercase | a figure | 56–64 px | 150–300 ms |
| Arc | cream + blue bands | ~40 px serif / 7 | many | 1 | yes, framed window | — | 150 ms, slow ambient loop |
| Apple | #000 | 64 px / 600 / 5 | 2 | 1 (buy) | yes, the product | 144–216 px | 240–320 ms, ease-out |
| BridgeMind | #0D0E11 | 84 px / 600 / −0.035 em / 4 | 3 | 2, uppercase | yes, interactive demo with sample data | 96 px | 200 ms, 700 ms reveals |

Pricing pages (Linear, Vercel, Raycast, Warp, BridgeMind): three or four plans; free first at $0 with numeric limits; one plan emphasised, often only by filling its CTA (Linear) rather than a loud badge; a comparison table and an FAQ that states the unglamorous rules (Raycast's credit expiry, "free is not a trial").

What they have in common, and what it means for us:

1. **Restraint and a single accent.** Linear and Vercel show no accent at all above the fold; Apple two small spots; Stripe's four are all things you can act on, and its colour otherwise lives in one signature art piece. Grounds are off-black and off-white, never pure (#08090A, #07080A, #FAFAFA, #F6F9FC), except Apple's black behind photography. → §4.2: one accent, three jobs.
2. **Few words, large.** Hero headlines run two to eight words; leads are one or two sentences; CTAs number zero to two (Warp's three uppercase buttons are the one that feels busy), labelled verb + object. Apple puts one short claim per screen and proves it with an image. → §5.1: half the words.
3. **Type does the branding.** One family, display sizes tracked tight (−0.02 to −0.06 em) with a line height near 1, quiet 15–18 px body, secondary text muted by colour rather than by a lighter weight. Display weights range from 300 (Stripe) to 600 (Raycast, Apple): weight is a brand choice, tight tracking and few words are the constant. Nobody shouts in 800. → §4.3.
4. **Rhythm through changing shapes, density inside, air outside.** Sections alternate centred, split, bento, list and full-bleed shapes, 96–144 px apart (Warp's 56–64 px reads as a technical document), separated by hairlines that fade at both ends; inside a block, information is dense and aligned. → §4.4, D-1837.
5. **Depth and light, not decoration.** A lit 1 px top edge, layered shadows (contact plus a wide soft ambient), one low-alpha bloom. No grey drop shadows, no glassmorphism everywhere. → §4.5.
6. **The product is the hero, and it's real.** Linear, Arc, Apple and BridgeMind show the real product in the first viewport, Linear's as a readable, populated workspace even at 390 px. Raycast and Vercel lean on abstract art, which works for known brands and feels-first rather than explains; an unknown product can't afford it. Mock-ups that look better than the app are what cheap products do. → §4.6, §5.1.1.
7. **Motion is choreographed and earns its place.** Interface feedback 100–160 ms (Linear, Vercel), reveals 700–800 ms (Stripe, BridgeMind), always an ease-out; the headline lands first, then the action, then the product. Apple ties motion to scroll so it explains the product. → §4.7.
8. **The voice is confident and specific, and proof sits beside the claim.** Short sentences (4–8 words), "you" as the subject, numbers instead of adjectives; Stripe's live metric, Linear's populated workspace, Warp's live counters and Raycast's exact credit rules each stand next to what they prove. → §4.8.
9. **The path to paying is short and calm** (studied 20 October; since D-869 Vibezeo has no paid plans, so what carries over is the honesty: free stated plainly, the rules stated in full, nobody nagged). As in the pricing table above; trial terms in one line. In the product, Linear and Raycast show a limit where it's met (issue caps, a credit balance in settings), and Warp's docs let you skip sign-up and type straight into a session. Nobody nags on launch. → §5.5.
10. **Craft is consistent end to end.** The download page, the installer, the first window and the docs look like the home page. A premium site that hands you a generic app breaks the promise; that's our biggest gap (§0). → §4.1, one token source.
11. **Weight is earned.** The pages that feel best are not the heaviest; ours paints in under 2 s with no framework script. Keep it: speed is felt as quality (Doherty). → §1 "Fast".

**Signature looks we keep clear of** (in addition to WEBSITE_ELITE §1.3): Stripe's tilted multicolour ribbon art, ruled layout lines and colour-switching run-on hero; Linear's full-bleed fading app shot under a button-less hero, and its serif manifesto page; Vercel's triangle-centred monochrome and very tight geometric sans; Raycast's diagonal light bars and floating nav capsule; Warp's mono display headlines, uppercase tracked buttons and paper-style captions; Arc's cream and electric blue with scalloped edges and rounded serif; Apple's side-lit product on black, gradient tagline and price-and-buy pill; BridgeMind's orb, vertical light beam, glowing demo frame, uppercase pill CTAs and price in the hero. BridgeMind also opens on an interactive multi-pane demo with sample data, as we do: ours must differ clearly in framing (our neutral bezel, no glow on the frame), light (the rings' bloom on the page) and type. Our own: the two rings and their light.

---

## 4. The system (app and site, one brand)

### 4.1 One token source

Today the app and the site both import `packages/ui/src/styles/tokens.css` (UI_SPEC §1.2: palette, the `--tn-*` semantics, the app's type scale, radii, shadows, durations). The site adds `site.css` on top, which holds two things: a copy of the light theme under `prefers-color-scheme` (D-1801), and the site's own lit layer (edges, fills, blooms, elevation, the expo-out curve) in `--site-*` names the app can't see. So the brand's light and depth exist only on the site.

The target (D-1891):

```
packages/ui/src/styles/
  tokens.css   palette + --tn-* semantics + the app's scale     (app and site; UI_SPEC §1.2 plus its §5.4/§7.3 fixes)
  brand.css    the shared brand layer: light, edges, fills,      (app and site)  ← new
               elevation e1–e3, radii card/frame, the expo-out
               curve and reveal timing, the highlight ink
  base.css     app behaviour: reduce motion, contrast, text size (app and site)
  site.css     the site only: the OS-light mirrors of tokens.css (site only)
               and brand.css, page bands, containers, display
               type, marketing utilities, page motion
```

- `brand.css` declares `--tn-edge-*`, `--tn-fill-*`, `--tn-elev-1…3`, `--tn-glow-a/b`, `--tn-ring`, `--tn-ring-lit`, `--tn-spot`, `--tn-ink-a/b` and `--tn-ease-out` for `:root, [data-theme="dark"]` and `[data-theme="light"]`, and maps them into Tailwind (`shadow-e1…e3`, `rounded-card`, `rounded-frame`, `ease-out-expo`).
- `site.css` keeps its `--site-*` names as aliases (`--site-elev-1: var(--tn-elev-1)`), so no site component changes, and mirrors `brand.css`'s light block under `prefers-color-scheme: light` exactly as it already mirrors `tokens.css`; `site.test.ts` fails if either mirror drifts.
- The app imports `brand.css` after `tokens.css`. The tokens are available; nothing in the app changes until a slice uses them.
- One value, one place: a site value that the app also needs moves to `brand.css`; a value only the site uses stays in `site.css`. Raw palette values stay forbidden in components.

Migration, slice by slice (each slice is small enough to merge alone while five feature sessions change the app):

| Slice | Who | What | Done when |
|---|---|---|---|
| T1 | design-lead | Add `brand.css`; move the shared values out of `site.css` into it; alias the `--site-*` names; import it in the app and the site; extend `site.test.ts` to the new mirror. No visual change on either surface. | The gate passes; site screenshots before and after are identical. |
| T2 | app-ui | Raised surfaces in the app take the brand: menus, popovers, dialogs, sheets, toasts and the command palette use `shadow-e2`/`e3` and the lit top edge (`inset 0 1px 0 var(--tn-edge-hi)`) instead of `--shadow-2/3`. Cards in the launch grid and Dashboard use e1. | Dialog, menu and palette screenshots in both themes match §4.5. |
| T3 | app-ui | Status as text, not pills (§5.4): the rail's workspace badges, pane headers and the Dashboard use `StatusDot` shapes plus sentence-case labels. | No uppercase status pills remain; workspace names no longer truncate at 248 px for names up to 16 characters. |
| T4 | app-ui | Motion tokens: app transitions use `--tn-ease-out` for entrances, `--ease-exit` for exits and UI_SPEC's durations; no new durations. | `motion.test.ts` passes; nothing animates under reduce. |
| T5 | site-motion | Accent discipline on the site (§4.2): eyebrows and inline links go neutral; teal stays on the primary CTA, focus and the one highlight. | Accent count above the fold ≤ 3 (§4.2). |
| T6 | site-motion | Dark-first product scenes use the same `frame` and the app's real dark surfaces; the site's stage tokens read from `brand.css`. | Scenes and captures sit in one bezel style. |
| T7 | either | Remove `--shadow-1…3` once no component uses them (grep is empty). | — |
| T8 | site-motion | The site on ink (D-1894): `--site-stage`, `--site-bezel` and `--site-raised` from the ink scale, the light page `--slate-25` → `--ink-25`, and every slate-tinted alpha (`rgb(148 163 184 / …)` lines, rings, grid, glass, stage line) becomes white alpha in dark and ink alpha in light. | No `--slate-*` or `148 163 184` left in `site.css` or site components; contrast tests pass. |

### 4.2 Colour

Keep the palette; change how much of it we use.

- **One accent, three jobs.** Teal (`--tn-accent`: #2DD4BF dark, #0F766E light) is for: the primary action in a view, focus and selection, and the active state of a control (a selected tab, the current mode). It is not for eyebrows, inline links, tick marks, icons in feature lists, or decoration.
  - **Site:** at most three teal elements in any viewport: the primary CTA, the one highlighted phrase in a hero (the `text-ink` gradient, teal → sky), and focus. Eyebrows become `fg-muted` mono caps; inline links are ink with an underline (D-1805 already underlines them); ticks become `fg-subtle` icons.
  - **App:** one accent-filled button per view. Secondary actions are neutral buttons.
- **Status colours mean status, always** (UI_SPEC §1.3): working sky, waiting amber, done green, idle grey, error rose. Never decorative, never the accent's job. Each pairs with its shape (filled dot with a breathing halo, diamond, check, hollow circle, triangle).
- **Neutrals are `ink`, a true neutral near-black** (D-1894, from `app-ui`'s D-2600), for the app and the site alike: `--ink-950` #09090B ground, each surface a step lighter (#0E0E10, #121214, #1C1C20, #232327), borders and lines as white alpha in dark and ink alpha in light, never slate-tinted. Slate's blue cast tinted everything, teal included, toward one cool wash. Every premium dark product we measured grounds on a neutral near-black (#08090A, #07080A, #0D0E11): an industry convention, not anyone's signature. On it teal and the status colours read true, and the product, not the grey, carries the colour. A neutral grey is never what makes us recognisable; the teal, the rings and their light, the type and the craft are. Slate stays in the palette for compatibility; no semantic token uses it. Dark is the brand's home: the app defaults to dark, product imagery is dark in both site themes (D-1801), and the site follows the OS or the visitor's choice.
- **Contrast is AA at least, checked by tests** (`contrast.test.ts`, `site.test.ts`). The pairs we lean on, computed from the tokens:

| Pair | Dark | Light |
|---|---|---|
| `fg` on page | 17.0 : 1 | 16.1 : 1 |
| `fg-muted` on page | 8.0 : 1 | 7.4 : 1 |
| `fg-subtle` on page / surface-2 / surface-4 | 5.9 / 5.6 / 4.6 : 1 | 5.5 (ink-50) / 6.0 (white) : 1 |
| accent as text on page | 10.7 : 1 | 5.0 : 1 (ink-50), 5.5 : 1 (white) |
| `accent-fg` on accent (primary button) | 7.8 : 1 | 5.5 : 1 |
| status text (working / waiting) on surface-2 | 8.7 / 11.2 : 1 | 5.9 / 5.0 : 1 (sky-700 / amber-700, on white) |

  `fg-subtle` is the floor for text; `fg-disabled` is for disabled controls only, never for information. Anything new adds its pair to the relevant test.

### 4.3 Type

Two families, both **SIL Open Font License 1.1** (checked: `apps/web/app/fonts/LICENSE-Inter.txt`, `LICENSE-JetBrainsMono.txt`, and `licenses/extra-attributions.toml`), self-hosted, no reserved font name, so subsetting and instancing (D-1844) are allowed:
- **Inter** (variable, with the optical-size axis): everything. Display sizes draw in its display cut automatically. Site features `cv01 cv09 cv11 ss03` (open digits, straight l); the app may adopt the same in T2 (they don't change metrics).
- **JetBrains Mono** (variable): terminals, code, paths, versions, key caps, figures and eyebrows. Never for paragraphs or buttons.
- No third face. A serif or a "personality" display face would be the first thing to make us look like someone else (WEBSITE_ELITE §1.3).

Two scales on one ratio family, because a desktop tool and a marketing page read at different distances:

| App (UI_SPEC §1.2, unchanged) | Use | Site (D-1832, unchanged) | Use |
|---|---|---|---|
| 10 / 14 (`2xs`) | caps labels | 12 mono caps +0.12 em (`eyebrow`) | eyebrows |
| 12 / 18 (`sm`) | meta, code in chat | 13 / 1.5 (`caption`) | captions, footer |
| **13 / 20 (`base`)** | **UI default** | 15 / 1.6 (`copy-sm`) | card body |
| 14 / 22 (`md`) | chat, composer | **16 / 1.65 (`copy`)** | **body** |
| 16 / 24 (`lg`) | screen and dialog titles | 17 → 21 (`lead`) | leads |
| 20 / 28 (`xl`) | empty-state titles, first run | 22 → 30 (`display-md`) | row titles |
| 24 / 32 (`2xl`) | first-run step titles | 30 → 48 (`display-lg`) | h2 |
| 30 / 38 (`3xl`) | first-run hero only | 36 → 56, 40 → 64, 44 → 76 | h1s, CTA, home h1 |

Rules for both:
- Weights: 400 body, 500 UI labels and buttons in the app, 560–620 site buttons and display, 600 titles. Never 700+ (the trimmed site font stops at 650).
- Tracking tightens with size (−0.01 em at 20 px to −0.042 em at 76 px); caps open up (+0.06 em app, +0.12 em site mono).
- Headings balance (`text-wrap: balance`), body text is `pretty`. Measure 45–75 characters.
- Numbers that change are tabular (`tabular-nums`): counts, timers, meters, sizes and figures.
- Sentence case everywhere: headings, buttons, menu items, labels. Uppercase only for ≤ 3-word mono eyebrows and caps section labels.

### 4.4 Space and grid

- **Base unit 4 px** in both. Spacing values come from the scale (4, 8, 12, 16, 20, 24, 32, 40, 48, 64, 80, 96, 128, 144); no arbitrary values.
- **App:** compact density (32 px controls, 28 px dense), fixed chrome heights (title bar 40, pane header 30, status bar 22), pane gutters 6 px. Empty states and first run step out of density: 24–32 px between groups, 20 px titles.
- **Site:** a 1200 px container (`container-site`) with a 16 → 32 px gutter, 12 columns with 24 px gaps; sections 80 → 144 px apart (`--site-section-y`); a 38–44 rem measure for text; the stage at 1280 px for the product drawn 1:1.
- **Rhythm:** related things sit closer than unrelated ones (label → control 6 px, field → field 16 px, group → group 24 px; site: eyebrow → h2 16 px, h2 → lead 20 px, lead → content 48–64 px).

### 4.5 Radii, elevation, borders and light

Light comes from above. A surface that sits higher catches more of it on its top edge and casts a softer, wider shadow.

| Level | Used for | Radius | Edge | Shadow (dark) |
|---|---|---|---|---|
| 0 · ground | window background, site page | — | — | — |
| 1 · surface | panes, rails, cards, launch grid tiles | 10 px app panels, 12 px site cards | 1 px `border` (app); gradient edge lit top → quiet bottom (site) | e1: 1 px inner highlight + 1 px contact |
| 2 · raised | menus, popovers, selects, toasts, a lifted card | 10 px | lit top edge | e2: highlight + contact + 12/32 −12 ambient |
| 3 · floating | dialogs, sheets, command palette, product frames | 12 px app, 16 px site frames | lit top edge, 1 px dark outer ring | e3: ring + contact + 16/32 + 48/96 −24 ambient |

- **Radii nest:** inner radius = outer radius − padding. Controls 6 px, chips and key caps 4 px, panels, panes, cards and popovers 10 px (a 6 px control inside 4 px of padding), dialogs and sheets 12 px, site cards 12 px, frames 16 px around a 10 px screen (D-1894). No pills except avatars' status dots and toggles (DECISIONS: 6 px controls).
- **Borders are 1 px hairlines.** In dark themes, separate with a border; in light themes, prefer a background shift plus e1. Never two borders side by side (pane border + header border): one of them goes.
- **The light is ours:** the two rings' bloom (teal and sky, `bg-lens`) sits behind heroes and the closing call to action, never on a product frame (D-1834). In the app the light appears only as lit edges and, on first run, the rings' bloom behind the welcome title.
- **Elevation tokens come from `brand.css`** (T1), one set for both surfaces: `--tn-edge-hi` is the top-edge highlight and `--tn-elev-1…3` the shadows. The app's `--shadow-1…3` alias them (`--shadow-2: var(--tn-elev-2)`) until T7 retires them. If a value is too heavy for the app, it changes in `brand.css`, for both surfaces; there is no second set.

### 4.6 Icons, engine marks and product imagery

- **Icons:** lucide (ISC licence), 1.75 stroke; 16 px in controls, 14 px in dense rows and pane headers, 20 px in empty states, 24 px on the site's capability rows. Icons label, they don't decorate: a capability row has an icon; a tick list doesn't need one per line.
- **Engine marks (D-1895):** our own monogram (`EngineGlyph`: C, X, Cu, G, Co, Gk, O, D, A, Ai, K, Ag, Ki) on a small **tinted tile**, always next to the engine's name, so four or more panes can be told apart at a glance. The letterform carries identity; the tint is a quiet second cue.
  - **Tile:** a rounded square, 16 px (14 px in pane headers and dense rows), radius 4 px. The letters are Inter semibold in `fg`, never in the hue.
  - **Tint:** each engine has its own hue at low chroma, as tokens in `brand.css` (`--tn-engine-<id>`, dark and light, so the site's engine list draws the same tiles): dark `oklch(0.34 0.05 h)`, light `oklch(0.93 0.035 h)`. At that chroma no tile competes with the accent or a status colour (§4.2), and `fg` on every tile passes AA (a contrast test lists them all).
  - **Hues** (OKLCH angle), chosen away from each vendor's own brand colour: Claude Code 275 (indigo), Codex 10 (rose), Gemini CLI 85 (ochre), Copilot CLI 120 (olive), Cursor Agent 300 (violet), Grok 235 (steel), OpenCode 55 (clay), Droid 140 (moss), Amp 205 (lagoon), Aider 330 (orchid), Kimi 25 (brick), Antigravity CLI 315 (plum), Kiro CLI 165 (mint); Shell and Terminal stay neutral (`surface-4`). Before shipping, `app-ui` checks each against the vendor's current site and logo; any hue that lands near a vendor's brand colour moves to a free angle, and the table here changes with it.
  - **Where:** wherever `EngineGlyph` stands next to an engine's name: pane headers, the launch grid, the rail's agent rows, thread rail rows, Dashboard rows, the onboarding engines step, engine pickers, and the site's engine list. The status mark always sits beside the tile, never on it, so status stays shape plus status colour and engine stays letters plus tint.
  - **With the accent and status:** the tints sit at chroma ≤ 0.05, under half the accent's (teal-400 is about 0.13) and the status colours', and never fill more than the 16 px tile. A tile is never the only thing that names its engine.
  - **Never:** a vendor logo, shape, colour or gradient (DECISIONS); hue on text, borders or the pane frame; a tile at full saturation. At rest the tile shows at 80 % opacity, and at 100 % on the focused pane.
- **The mark:** the two overlapping rings. It is the only illustration we own. At page scale it becomes the backdrop (`Orbit`); in the app it appears at 16 px in the title bar and at 64–96 px on first run and empty Agent mode.
- **Product imagery is the product.** Real captures from the shipped build at 2×, labelled with their build (`Vibezeo 0.8.0 · Linux`), or the live demo, or a scene animated from the real UI's components and tokens (`site-motion`). Never a mock-up drawn to look better than the app. When a scene simplifies (fewer panes, scripted output), its caption says so.
- **Dark stage:** product media sits on the dark stage in both site themes (D-1801), in the `frame` bezel.

### 4.7 Motion

Motion explains change: where something came from, where it went, what it's doing now. It is slow to arrive and fast to respond.

| Token | Value | Use |
|---|---|---|
| `--dur-instant` | 60 ms | hover colour |
| `--dur-fast` | 120 ms | press, toggle, tooltip |
| `--dur-base` | 180 ms | menus, popovers, tabs, mode switch |
| `--dur-slow` | 260 ms | sheets, dialogs, sidebar |
| `--dur-deliberate` | 400 ms | first-run steps |
| site reveal | 700 ms | a section's content as it scrolls in (site only) |
| `--tn-ease-out` | `cubic-bezier(0.16, 1, 0.3, 1)` | entrances (expo-out); the site's `--ease-out-site` aliases it |
| `--ease-exit` | `cubic-bezier(0.4, 0, 1, 1)` | exits, which are shorter than entrances |
| `--ease-standard` | `cubic-bezier(0.2, 0, 0, 1)` | moves and resizes |

Choreography:
- **Stagger related items by 40–80 ms**, never more than five steps (a list of 20 doesn't cascade for a second).
- **Enter: fade + 4–8 px rise or scale 0.97 → 1.** Exit: fade, 60–70 % of the entrance's time. Nothing slides more than 24 px in the app.
- **Status changes animate colour over 180 ms**; "waiting" gets one 600 ms attention pulse, once (UI_SPEC §1.8). A working agent's breathing halo (a filled dot whose halo fades in and out over 1.6 s, D-2601) is the only perpetual motion in the app; under reduced motion the filled dot stays, still.
- **Site:** reveals by scroll timeline (CSS only), the rings drawing in, the engines' slow orbit and the data-flow packets (D-1842); `site-motion`'s product scenes play once when they enter the viewport and rest on their final frame; they never loop faster than every 8 s and pause off screen.
- **Transform and opacity only** (and a stroke dash); never animate layout, filters on large areas, or anything that delays the largest paint.
- **Reduced motion:** under `prefers-reduced-motion: reduce` or Settings › Appearance › Reduce motion, every transform and pulse stops; opacity changes stay at ≤ 120 ms; scenes show their final frame (tests already enforce this: `motion.test.ts`, the site's reduced-motion test).

### 4.8 Voice and microcopy

We sound like a senior engineer you trust: plain, exact, brief, warm without being chummy.

- **Say what it does, then stop.** One idea per sentence; most sentences under 20 words. Leads are one or two sentences.
- **Lead with the user's goal, name the product rarely.** "Run every agent side by side" over "Vibezeo runs coding agents side by side".
- **Concrete over abstract:** numbers, names, versions. "Waiting on you · Codex in checkout", not "Action required".
- **Honest by construction:** every claim true of the shipped build, with its version where it matters; state what isn't available plainly ("Comes with Vibezeo accounts"). No superlatives we can't measure ("fastest", "best"), no "AI-powered", no "seamless", "effortless", "revolutionary", "unleash", "supercharge".
- **Sentence case; verbs on buttons** ("Start agent", "Open folder", "Download for Linux"); a button says what happens, never "OK", "Submit" or "Click here".
- **Errors:** what happened, why if we know, what to do next, in that order. "Codex isn't signed in. Open a terminal in its pane and run `codex login`." Never blame the user; never "Oops".
- **Empty states:** what this place is for, plus one action that fills it. (§5.3 has the Agent mode rewrite.)
- **Confirmations:** name the object and the consequence. "Close 3 agents in checkout? Their terminals end; their threads stay." Buttons repeat the verb: "Close 3 agents" / "Cancel".
- **Other products:** name engines only where the user needs to recognise them (the engine list, the launch grid, the docs). The hero speaks about the user's agents, not about vendors.
- **Typography in copy:** curly quotes and apostrophes, the en dash for ranges, the ellipsis character for truncation and for commands that open a dialog ("Worktree setup…").

---

## 5. The journey

Five steps, each with what the person needs to feel, what they see today and the concrete changes. Owners: **site** = `site-motion`, **app** = `app-ui`, **copy** = whoever owns the surface, **lead** = design-lead. Every change keeps D-128: the site and the demo show only what the shipped build does.

### 5.1 Landing: the first five seconds, and why someone stays

**They need to feel:** "this is serious, it's for me, and I can see it." In five seconds a visitor answers three questions: what is it, is it for me, is it good. The headline answers the first, the product image the third.

**Today:** a strong h1 ("Your agents. Your machine. Your plan.") and two CTAs fill the fold; the product starts below it; the lead names three vendors; the page is 13,000 px of mostly prose.

**Changes:**
1. **Product in the first viewport (site, must).** At 1440×900 and 1280×800 the top 15–25 % of the live demo frame (or `site-motion`'s first scene) shows above the fold, rising into place under the CTAs. On a phone, the still shows below the CTAs within the first 1.5 screens. Keep the largest paint under 2.0 s: the scene's first frame is the still, already lazy-safe (D-1816, D-1842).
2. **Hero, shorter (copy).** Keep the h1. The lead becomes one sentence about the user's outcome, with the engine count from code and no vendor names: for example "Run every coding agent you use side by side, in real terminals, and see at a glance which one needs you." The four ownership chips stay (they are proof), drawn neutral, not teal.
3. **Two CTAs, one primary (site).** "Download for Linux" (primary) and "Try it in your browser" (secondary). The meta line under them stays small: "Free · .deb and .rpm · 53 MB · Watch it run (1 min)". On a phone, where a desktop download is a dead end, "Try it in your browser" becomes the primary and the second action is "Email me the link" (a `mailto:` with the download URL, no recipient and no script).
4. **Half the words (copy, site).** Target ≤ 6,500 px at 1440 px. The order becomes: hero → live demo → "what it feels like" (three animated scenes: many agents at once, the Dashboard telling you who needs you, review and second opinion) → ownership and data flow → engines → the capability index (eight most-used, then "Everything else" to the docs) → FAQ (four questions) → closing CTA. The Built with Claude band and the stats move to /about/ (they are about us, not the visitor); the founder note stays as one line with a link in the closing band.
5. **Accent discipline (site, must).** ≤ 3 teal elements per viewport (§4.2). Eyebrows go `fg-muted`; "Read the docs" appears once per section group, as a neutral text link.
6. **Scenes, not screenshots (site).** Each mode row shows a short scene built from the real UI (one composited layer, plays once on entry, rests on its final frame, captioned "Scene from Vibezeo 0.8.0, simplified"), with the 2× capture as its poster and reduced-motion state.

**Why someone stays:** they see the thing working (scene), they can touch it (demo), and every line they read is short and checkable.

### 5.2 Trying the demo, downloading and installing

**They need to feel:** "that was easy, and nothing was hidden."

**The demo (app, site):**
1. **Open on something happening, not an empty state (app, must).** The embedded demo opens in Code mode with two agents mid-task and one waiting (the demo's Code mode already holds one agent done, one working and one waiting in another workspace), so the first look shows status shapes, real terminals and the rail. The thread with its first message ready is one click away.
2. **Replace the six-step coach card with one hint at a time (app).** A small popover anchored to the thing it describes ("Codex is waiting on you. Open it to answer."), three hints in total, each dismissed by doing it. It never covers the rail's content.
3. **Agent mode in the demo opens Lead,** as the installed app does, with a sample brief, so the mode explains itself.
4. **The end of the demo is a peak (site).** After the visitor answers the approval and the agent finishes, the demo bar offers "Download for Linux" with the size, once. "Demo limit reached" says what to do next.

**Download (site):**
1. One card per OS: the detected OS first, the version, the size, the packages as buttons, then Install and Verify side by side (as today). Windows and macOS say "Coming soon" with "Tell me when", never a disabled button that looks broken.
2. **After clicking Download,** the page scrolls to "Running in four steps", whose step 2 must quote the app's real button ("Start using Vibezeo"; copy, must).
3. Checksums stay: no other site we studied publishes them, and they say "we have nothing to hide".

**Install:** the first launch must open on the welcome screen in under 3 s (the splash holds at most 3 s today).

### 5.3 The app's first run: onboarding and the first agent

**They need to feel:** "it already knows my setup, and I got an agent running in a minute." Peak: the first agent's status turning from Working to Done.

**Today:** a welcome screen ("Welcome to Vibezeo", a Free plan callout (which goes with D-869), "Start using Vibezeo"), then a five-step wizard (engines, accounts, Dictate, privacy, workspace), then Code mode on an empty tab with the launch grid.

**Changes:**
1. **Welcome (app).** The two rings at 96 px on the brand bloom (`bg-lens` light, from `brand.css`), "Welcome to Vibezeo" at `3xl`, one line ("Run your coding agents side by side, on this computer."), the primary "Start using Vibezeo", and under it one quiet fact line: "Free · no account · nothing sent to Vibezeo" (not "nothing leaves this computer": the engines talk to their vendors). The rings draw in once (400 ms, off under reduce).
2. **Three steps, not five (app).** Engines (what we found, with sign-in inline per engine) → Your first folder → Done. Dictate moves to a dismissible tip after the first agent finishes; privacy switches (both off) move to a single line on the Done step with a link to Settings. Hick's law and the goal gradient: fewer, clearer steps. The stepper shows three dots.
3. **Engines step (app).** Found engines first, each a row with its `EngineGlyph`, name, version and a status: "Signed in", or "Sign in" (opens a terminal in that engine's provider home). Missing engines collapse under "10 more you can install". One engine is preselected as the default: the first signed-in one.
4. **First folder → first agent in one move (app).** The folder step ends with "Start Claude Code in shop" (the default engine and the chosen folder), which opens Code mode with that agent already launching. The launch grid stays for every later empty tab.
5. **Launch grid (app).** Tiles become e1 surfaces with the glyph at 20 px, the name, and a one-line state ("Signed in", "Not signed in"), the default engine first and marked "Default". The heading reads "Start an agent in shop"; the body drops its em dash: "It runs in a real terminal in this folder."
6. **The first finish is celebrated quietly (app).** When the first agent finishes, the pane header's status animates to Done, and a single toast: "Claude Code finished in shop. Review its changes" with an action. No confetti. The peak–end rule says this moment decides how first run is remembered.

### 5.4 Daily use

**They need to feel:** calm control. Many agents, one glance.

1. **Status as shapes and words, not pills (app, must; slice T3).** Rail rows: the workspace name gets the space; status shows as `StatusDot` shapes with a count in `tabular-nums` ("◆ 1" amber for waiting, a pulse for working) and a sentence-case tooltip. Uppercase bordered pills go.
2. **Pane headers (app).** 30 px, one hairline (not a border plus a header border), the focused pane marked by its header's ink and a 2 px accent line at the top, the engine glyph at 14 px, the title in `fg`, meta in `fg-subtle`. Actions appear on hover or focus; close stays visible.
3. **Raised surfaces take the brand (app; slice T2).** Menus, the palette, dialogs and sheets get the lit top edge and e2/e3 so they visibly float over terminals.
4. **The Dashboard is the heart (app).** It earns the craft budget: agents grouped by state (Waiting on you first), each row with glyph, name, workspace, elapsed time in `tabular-nums` and one action. Empty state: "No agents running · Start one from Code, Thread or Agent mode."
5. **Threads (app).** Transcript at 14/22 in a 72-character measure, tool calls collapsed to one line with their result, approvals as cards with the verb on the button ("Allow once", "Allow for this thread", "Deny").
6. **Status bar (app).** Quiet by default: branch and version. Each other item appears only while it is non-zero, and leaves when it isn't (the coordinating session's call, which keeps this intent): agents waiting ("1 waiting on you", amber text, first and clickable), agents working (a count with the working dot), live ports and previews, and the usage window when an account nears its limit. Never more than four items at once; waiting always wins a place.
7. **Keyboard first, always discoverable.** Every command in the palette shows its shortcut as a `Kbd`; tooltips on icon buttons show the shortcut after the label.

### 5.5 The moments that make someone stay, tell a friend and come back

Vibezeo is free, with no paid plans (D-869, the owner, 24 October). Nothing in the product asks for money, so the journey's last step is no longer a purchase. It is three moments that turn a download into a habit, and a habit into a recommendation. They replace the paying moments of earlier drafts (D-1896).

**They need to feel:** "it already does what I wanted, and it never once tried to sell me something."

**Stay: the first agent working (first run, §5.3).**
1. From launch to an agent running in a real terminal in under a minute: welcome → engines → folder → "Start {engine} in {folder}".
2. The peak is the first finish: the pane header turns to Done and one quiet toast says "{engine} finished in {workspace}" with "Review changes". No confetti, no rating prompt, no "share this".
3. The Dictate tip comes after that finish, once, never before it.

**Tell a friend: the first worktree landed.**
1. The moment people describe to others is running several agents on one task and merging the best: New worktree… or Best of N, each agent in its own lane, the review queue showing every diff, Approve, merged. That flow gets the craft budget after the Dashboard.
2. When the first worktree lands, the confirmation names what happened plainly ("Merged tandem/fix-login into main · 3 files"), with Undo where the core allows it.
3. We earn the recommendation; we never ask for it inside the work. No share prompts, referral codes, star-us banners or "rate Vibezeo" dialogs, and no tracking of who told whom (rule 4 of DECISIONS: nothing leaves the machine by default). One quiet line sits under the version in Settings (beside "Vibezeo {version}"): "Vibezeo is free. If it helps you, tell a friend."

**Come back: exactly where they left off.**
1. Reopen to the same workspaces, tabs and panes (crash-safe restore), agents paused and ready to resume, nothing re-run on its own.
2. The Dashboard and the status bar say first who needs you ("1 waiting on you").
3. After an update, one line in the status bar for one session: "Updated to {version} · What's new", opening the existing What's new sheet only when clicked, instead of on launch.

**Rules (must):**
1. **No plan UI anywhere.** No plan names, "Free" badges, plan footers, upgrade buttons, "See plans", limit dialogs, trials, prices or "Pro" on any surface of the app or the site, once the release that lifts the limits ships. Until then, the shipped build's limits are stated plainly where they apply, with no upgrade path ("Vibezeo 0.9.0 runs 4 panes at once; the next release lifts this").
2. **Hosted features carry one phrase, and never a price.** Features that need Vibezeo's servers (Conductor, cloud Dictate, Enhance and Polish, the Auto supervisor, dictionary sync, the phone relay) are labelled **"Comes with Vibezeo accounts"**, word for word, in sentence case, as the 0.9.0 docs already say (D-1896). It is always followed, where there's room, by the sentence that keeps it honest about today: "It uses Vibezeo's servers and switches on when Vibezeo accounts open." Where there isn't room (a badge, a menu row), the tooltip carries that sentence. Where an action is needed: **"Tell me when accounts open"**, a prefilled email to the contact address. Never "Pro", "premium", "paid", "upgrade", "credits", a price or a date.
3. **Free is said once, plainly, where it answers a question:** the download page, the FAQ and the welcome screen ("Free · no account · nothing sent to Vibezeo"). Not as a badge, and never as "free for now".
4. **No pressure mechanics, ever:** no countdowns, nag screens, prompts on launch, or interruptions while someone types.

---

## 6. Design QA: how a READY is reviewed

When the coordinating session says an implementer sent READY, the design lead:
1. Checks out the branch and builds what changed: `pnpm --filter desktop build:demo` (the demo runs the real UI) and/or `pnpm --filter web build:site`, served locally.
2. Captures before (the branch's merge base) and after, in Chromium (`/opt/pw-browsers/chromium`): 1440×900 and 390×844, dark and light, motion on and reduced. Scenes are also captured at their first and final frames. `scripts/agent/design-shots.mjs` does the stills: every page at both sizes in the OS's dark and light and with Light or Dark forced, reporting the resolved theme, any sideways scroll and the teal elements in the first viewport (more than three fails, §4.2). Implementers run it before sending READY.
3. Reviews against this document, and sends the coordinating session one list, each item ranked:
   - **Must-fix:** breaks a **must** here; a claim untrue of the shipped build; contrast below AA; anything that moves under reduced motion; a raw colour or arbitrary value where a token exists; a regression in the site's largest paint or CLS budgets; a missing empty, loading or error state; a clean-room problem.
   - **Polish:** a better value, spacing, timing or word that would raise the craft, which may land in a later slice.

The checklist per review:
- Attributes (§1): calm, precise, fast, trustworthy, crafted; in that order when they conflict.
- Accent count per viewport (§4.2); status colours only for status, each with its shape.
- Type from the scales (§4.3), sentence case, tabular numbers where values change.
- Spacing on the 4 px scale; radii nest; one hairline where there were two (§4.4, §4.5).
- Elevation from `brand.css` tokens; raised surfaces lit at the top.
- Motion durations and curves from §4.7; transform and opacity only; reduced motion honoured.
- Copy against §4.8, and every product claim against the shipped build (D-128).
- The journey change it implements, against §5's wording.

---

## 7. Who does what, in order

The implementers take slices in this order; each slice is one READY. The words for every changed string are in §9: paste them, adjusting only to fit.

**`app-ui`** (D-2600 on):
1. T2: brand elevation on raised surfaces (§4.5), after T1 lands.
2. T3: status as shapes and words in the rail, pane headers and Dashboard (§5.4.1–2).
3. Plan UI out (D-869, done by the `free-for-everyone` session): no upgrade dialog, plan footer, limit prompts or Pro badges remain; hosted features say "Comes with Vibezeo accounts" (§5.5).
4. First run: the welcome, three steps, first folder → first agent (§5.3).
5. Launch grid and Agent mode's first run (§5.3.5, and Lead opening in the demo, §5.2).
6. The demo opening in Code mode with one hint at a time (§5.2).
7. T4: motion tokens; status bar; threads (§5.4).

**`site-motion`** (D-1870 on):
1. T5: accent discipline (§4.2) and dark mode on the shared tokens.
2. Product in the first viewport (§5.1.1) and the hero copy (§5.1.2–3).
3. The three scenes (§5.1.4, §5.1.6, §4.7's scene rules), with posters and reduced-motion frames.
4. The home page at half its length (§5.1.4); Built with Claude and the stats to /about/.
5. Download (§5.2), including the step that quotes the app's button. The pricing page is gone (D-869).
6. T6: scenes and captures in one bezel.

**design-lead:** T1 (the shared token layer), then design QA on each READY (§6), and this document kept current.

## 8. Clean room

We study other products for principles and record them in our own words. We never use another product's names, logos, artwork, copy, code, fonts or signature effects (DECISIONS "Identity"; CLAUDE.md "Never"); WEBSITE_ELITE §1.3 and §3 here list the looks we keep clear of. Our own signatures: the two rings and their teal-and-sky light, monogram engine marks, mono facts, and checksums on the download page.

---

## 9. Copy deck

Exact words for every string §5 changes, checked on 20 October against 0.8.0's code: `LIMITS` (`packages/protocol/src/plans.ts`), `apps/desktop/src/i18n/en.json`, `apps/web/lib/*.ts`. `{braces}` are values from code, never typed in. When code changes a fact, the string follows the code. Implementers may adjust a word to fit; they may not add a claim.

### 9.1 Site: home (`apps/web/lib/home.ts`, `components/site/home/*`) (site-motion)

| Where | Today | Use |
|---|---|---|
| Hero h1 (`HERO.title`) | Your agents. / Your machine. / Your plan. | unchanged |
| Hero lead (`Hero.tsx`, from `ENGINES`) | Vibezeo runs Claude Code, Codex, Gemini CLI and 10 more coding agents side by side in real terminals. One Dashboard shows what each one is doing, and which one needs you. | Run the coding agents you already use side by side, in real terminals, and see at a glance which one needs you. |
| Under the lead, small, linking `#engines` | — | Works with {ENGINES.length} agent CLIs |
| Ownership chips (`OWNERSHIP`) | four facts | unchanged; drawn neutral (§4.2) |
| Meta line | Free for Linux. · .deb · .rpm · {size} · Watch it run (1 min) | Free · .deb and .rpm · {size} · Watch it run (1 min) |
| Phone CTAs (below 640 px) | Download for Linux / Try it in your browser | Try it in your browser (primary) / Email me the link (`mailto:?subject=Vibezeo for Linux&body=https://vibezeo.com/download/`) |
| Scene 1 h2 / lead | — | Many agents, one screen / Each agent runs in a real terminal over your own folder. Split, arrange and keep going. |
| Scene 2 h2 / lead | — | See who needs you / Working, waiting, done or failed: every pane, the rail and the Dashboard show it, from each engine's own signals. |
| Scene 3 h2 / lead | — | Review before you merge / Finished work waits in one queue with its diff. Ask another engine for a second opinion before you approve. |
| Scene caption (each) | — | Scene from Vibezeo {RELEASE.version}, simplified. |
| Ownership lead | Engines sign in by themselves, in their own folders, and Vibezeo never reads, copies or proxies their tokens. | Engines sign in by themselves, in their own folders. Vibezeo never reads, copies or proxies their tokens. |
| Engines lead | Vibezeo finds the engine CLIs installed on your machine and launches them in panes, each signed in to your own account. If an engine is missing, Vibezeo shows you how to install it. | Vibezeo finds the agent CLIs on your machine and starts each one signed in to your own account. Missing ones come with install steps. |
| Capabilities h2 | And everything else that works today | Built in, working today |
| Capabilities (eight, then a link) | 18 tiles | Dictate; Worktree lanes and handoff; Previews per worktree; Files, git and a browser; SSH workspaces; Find work by what it's about; Plugins with approvals; Crash-safe restore. Link: "Everything else in the docs" |
| FAQ | six questions | four: "Can I use Vibezeo today?", "Do I need a new AI subscription?", "Does Vibezeo see my API keys or subscription tokens?", "Is Vibezeo free?" (as 0.9.0 ships it: "Yes. Vibezeo is free, with no paid plans. Your agents run on the AI subscriptions you already have." While a shipped build still has limits, the download page says so, as 0.9.0's does: "The next release removes these limits."). "How does Vibezeo use Claude?" moves to /about/ with the Built with Claude band; "Which platforms?" is answered by the meta line and /download/. |
| Closing band lead | Download Vibezeo for Linux, then tell us what works and what doesn't at contact@vibezeo.com. | Free for Linux, no account. Tell us what works and what doesn't: {SITE.contactEmail}. |
| Closing band, one line | — | Built by {founder}. More about Vibezeo → (/about/) |

### 9.2 Site: download (site-motion; the pricing page is gone, D-869)

| Where | Today | Use |
|---|---|---|
| Download step 2 title / body | Start / Open the app and choose Start on the welcome screen. It runs on your computer, no account needed. (0.9.0) | Start / Open the app and choose Start using Vibezeo on the welcome screen. It runs on your computer, no account needed. (The button's real label, `signin.earlyAccess.start`; site-motion's branch already reads it from `en.json`.) |
| Download step 3 body | The launch grid lists the agents it found: pick one to start it in a real terminal, or Terminal for a shell. | Setup ends by asking for a project folder. Then pick an agent from the launch grid to start it in a real terminal, or Terminal for a shell. |
| Windows and macOS rows | Coming soon | Coming soon · Tell me when (a prefilled `mailto:` to {SITE.contactEmail}) |

### 9.3 App: welcome and first run (`en.json`) (app-ui)

| Key | Today | Use |
|---|---|---|
| `signin.title` | Welcome to Vibezeo | unchanged |
| `signin.subtitle` | Run your coding agents side by side. | Run your coding agents side by side, on this computer. |
| `signin.earlyAccess.start` | Start using Vibezeo | unchanged (the site quotes it) |
| `signin.earlyAccess.note` | Free plan: 4 panes, 3 workspaces | Free · no account · nothing sent to Vibezeo |
| `signin.earlyAccess.title` / `.body` (callout) | Free plan / Vibezeo runs on this computer… | drop the callout; the note carries it. |
| Steps | engines, accounts, dictate, privacy, workspace | engines (with sign-in), folder, done |
| `onboarding.engines.title` / `.body` | Set up your coding agents / Vibezeo runs the agents you already use… | Your coding agents / Vibezeo found these on your computer. Each signs in by itself; Vibezeo never sees its token. |
| Engine row states | Installed {version} / Not installed / Not signed in | {version} · Signed in / {version} · Sign in (button) / Not installed |
| Missing engines, collapsed | — | {n} more you can install |
| `onboarding.workspace.title` / `.body` | Open your first workspace / Pick a project folder. Agents work on it right where it is. | Pick a project folder / Agents work on it right where it is. |
| Done step title / body | — | You're set / Usage statistics and crash reports stay off. You can turn them on in Settings › Privacy. |
| Done step primary | — | Start {engine} in {folder} |
| Done step secondary | — | Open {folder} without an agent |
| Dictate tip, after the first agent finishes (dismissible) | — | Hold {dictateKey} in any app and speak: Dictate types where your cursor is, recognized on this computer. |

### 9.4 App: launch grid, empty states, first finish, demo (app-ui)

| Key | Today | Use |
|---|---|---|
| `code.startIn` | Start in {name} | Start an agent in {name} |
| `code.startBody` | Launch a coding agent — it runs in a real terminal in this folder. | It runs in a real terminal in this folder. |
| Tile meta (new) | — | Default / Signed in / Not signed in |
| `agent.main.emptyTitle` / `.emptyBody` | No teammate open / Pick a teammate in the rail. | Pick a teammate, or make one / A teammate is an agent with a name, a brief and notes it keeps between chats. |
| Agent mode main action (new) | — | New teammate |
| `agent.rail.empty` | No teammates. Add one with +. | No teammates yet. |
| `dashboard.emptyTitle` / `.emptyBody` | No agents running / Start one in Code, Thread or Agent mode. | No agents running / Start one from Code, Thread or Agent mode. Agents that need you show up here. |
| First finish toast (new) | — | {engine} finished in {workspace} · Review changes (when it changed files; otherwise · Open) |
| `demo.hint.*`, `demo.try.*` (six-step card) | Your first message is ready… / Try this… | three hints, one at a time: (1) on the waiting agent: "{engine} is waiting on you. Open it to see why." (2) on the thread's composer: "Press Enter to send this task. The agent is simulated and runs in your browser." (3) on Changes: "Open Changes to see what it edited." |
| Demo end (new, once, in the demo bar) | — | That's the loop. Download for Linux · {size} |

### 9.5 Hosted features, and the retired plan strings (D-869, D-1896)

Vibezeo has no paid plans. The plan and limit strings of the earlier deck are retired; the `free-for-everyone` session removes them from the app with the gates. What stays is one honest phrase for the features that need Vibezeo's servers.

| Where | Use, word for word |
|---|---|
| Label on a hosted feature (Conductor, cloud Dictate, Enhance and Polish, the Auto supervisor, dictionary sync, the phone relay), in the app, the docs and the site | Comes with Vibezeo accounts |
| The sentence that follows it (or its tooltip, where there's no room) | It uses Vibezeo's servers and switches on when Vibezeo accounts open. |
| The action, where one is needed | Tell me when accounts open (a prefilled `mailto:` to {SITE.contactEmail}) |
| Never, anywhere | Pro, premium, paid, upgrade, plan, trial, credits, a price, a date, "free for now" |

Retired keys and constants (don't translate, restyle or reuse them): `upgrade.*`, `limit.*`, `launch.limitBody`, `plan.footer.*`, `thread.grid.freeTitle`, `thread.grid.freeBody`, `thread.grid.plans`, `thread.engineGate.*`, `agent.new.limit`, `agent.new.upgrade`, `upgrade.gate.teammates`, `plugins.freeLimit`, the `*.upgrade` and `routines.upsell.*` buttons, `signin.earlyAccess.title` and `.body`, and on the site `PLANS`, `PLAN_GROUPS` and the pricing copy. Until the release that lifts the limits ships, a limit the shipped build still enforces is stated as a fact of that version, with no way to pay past it: "Vibezeo {version} runs {n} panes at once. The next release lifts this."
