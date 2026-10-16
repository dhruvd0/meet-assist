---
name: CASE
description: A private meeting copilot whose meetings read as quiet documents on a zinc ground.
colors:
  accent: "oklch(0.55 0.19 255)"
  accent-dark: "oklch(0.68 0.17 250)"
  background: "#ffffff"
  background-dark: "#09090b"
  sidebar: "#f7f7f8"
  sidebar-dark: "#0c0c0e"
  surface: "#f4f4f5"
  surface-dark: "#111113"
  surface-hover: "#ebebed"
  surface-hover-dark: "#18181b"
  text: "#09090b"
  text-dark: "#fafafa"
  text-muted: "#52525b"
  text-muted-dark: "#a1a1aa"
  text-faint: "#71717a"
  text-faint-dark: "#7c7c86"
  line: "#09090b14"
  line-dark: "#ffffff14"
  line-strong: "#09090b26"
  line-strong-dark: "#ffffff24"
  ok: "oklch(0.55 0.15 150)"
  ok-dark: "oklch(0.72 0.16 150)"
  warning: "oklch(0.58 0.15 65)"
  warning-dark: "oklch(0.76 0.15 75)"
  error: "oklch(0.55 0.2 25)"
  error-dark: "oklch(0.7 0.17 25)"
typography:
  display:
    fontFamily: "Newsreader Variable, Iowan Old Style, Georgia, serif"
    fontSize: "32px"
    fontWeight: 400
    lineHeight: 1.25
    letterSpacing: "-0.01em"
  numeral:
    fontFamily: "Newsreader Variable, Iowan Old Style, Georgia, serif"
    fontSize: "34px"
    fontWeight: 400
    lineHeight: 1
    fontFeature: '"tnum"'
  headline:
    fontFamily: "DM Sans Variable, -apple-system, BlinkMacSystemFont, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: "32px"
  title:
    fontFamily: "DM Sans Variable, -apple-system, BlinkMacSystemFont, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.5
  body-doc:
    fontFamily: "DM Sans Variable, -apple-system, BlinkMacSystemFont, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: "28px"
  body:
    fontFamily: "DM Sans Variable, -apple-system, BlinkMacSystemFont, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: "22px"
    fontFeature: '"ss01", "cv11"'
  label:
    fontFamily: "DM Sans Variable, -apple-system, BlinkMacSystemFont, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: "20px"
  mono:
    fontFamily: "JetBrains Mono Variable, ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "11.5px"
    fontWeight: 400
    lineHeight: 1.5
    fontFeature: '"tnum"'
rounded:
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  gutter: "32px"
  section: "40px"
  sidebar: "240px"
  column: "720px"
  titlebar: "48px"
components:
  button-primary:
    backgroundColor: "{colors.text}"
    textColor: "{colors.background}"
    rounded: "{rounded.lg}"
    padding: "0 14px"
    height: "32px"
    typography: "{typography.body}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.lg}"
    padding: "0 14px"
    height: "32px"
  button-secondary-hover:
    backgroundColor: "{colors.surface-hover}"
  button-ghost:
    textColor: "{colors.text-muted}"
    rounded: "{rounded.lg}"
    padding: "0 14px"
    height: "32px"
  button-ghost-hover:
    backgroundColor: "{colors.surface-hover}"
    textColor: "{colors.text}"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.lg}"
    padding: "0 12px"
    height: "36px"
  chip-meta:
    textColor: "{colors.text-muted}"
    rounded: "{rounded.full}"
    padding: "0 12px"
    height: "32px"
  badge-live:
    textColor: "{colors.accent}"
    rounded: "{rounded.full}"
    padding: "0 8px"
    height: "20px"
  nav-item:
    textColor: "{colors.text-muted}"
    rounded: "{rounded.lg}"
    padding: "0 10px"
    height: "32px"
  nav-item-active:
    backgroundColor: "{colors.surface-hover}"
    textColor: "{colors.text}"
  feed-row:
    textColor: "{colors.text}"
    rounded: "{rounded.xl}"
    padding: "10px 12px"
  feed-row-hover:
    backgroundColor: "{colors.surface}"
  nudge-card:
    textColor: "{colors.text}"
    rounded: "{rounded.xl}"
    padding: "12px 14px"
  now-card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.xl}"
    padding: "20px 24px"
  keycap:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-muted}"
    typography: "{typography.mono}"
    rounded: "{rounded.md}"
    padding: "0 6px"
    height: "20px"
  live-dock:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.full}"
    height: "48px"
---

# Design System: CASE

## Overview

**Creative North Star: "The Quiet Notebook"**

A meeting is a document, not a record. CASE sets every meeting as a centered page with a serif title, read top to bottom: agenda, nudges, notes, and the transcript last. The shell around it is a near-silent zinc ground: a slightly tinted sidebar, a white (or near-black) canvas, hairlines drawn in text-alpha rather than gray, and almost nothing raised off the page. It is a reading surface between calls, not a dashboard.

Density is calm and editorial. One 720px column, generous 40px breaks between document sections, and 14px UI type with 15px reading type. Color is nearly absent: the one blue accent marks only what is alive or yours (a running meeting, focus, selection, your lines in the transcript, an undismissed nudge). Actions are carried by an inverted neutral button, so the accent never has to compete with a call to action.

The live meeting is the one thing allowed to float. The live dock, a pill at the foot of the canvas with a pulsing level meter, timer, pause, and end, sits above every page while a meeting runs; it is the system's signature and its only persistent elevated object.

**Key Characteristics:**

- Zinc neutrals in matched light and dark pairs; both themes are first-class and follow the OS unless forced.
- One blue accent, reserved for live state, focus, selection, and "you".
- Three voices of type: Newsreader for page titles, DM Sans for everything read or clicked, JetBrains Mono for times, durations, and keys.
- Flat by default; depth comes from tonal steps (canvas, sidebar, surface, surface-hover) and alpha hairlines.
- Rounded but not soft: 12px controls, 16px containers, full pills for chips, meta, and the dock.

## Colors

A cool zinc ground with a single blue signal; every color is defined once as a light/dark pair and the active set is swapped by media query or `data-theme`.

### Primary

- **Signal Blue** (accent / accent-dark): the live dot and its pulse ring, the dock's level meter, the "Live" badge, the focus outline (70% mix) and text selection (30% mix), the "You" speaker label in transcripts, undismissed nudge cards (25% border, 6% fill), toggle-on state, and the brand mark. Dark mode lifts it to a lighter, slightly greener blue so it holds against #09090b.

### Neutral

- **Canvas** (background / background-dark): the main reading surface and the inverted text color of primary buttons.
- **Sidebar Zinc** (sidebar / sidebar-dark): the 240px navigation rail, one half-step off the canvas.
- **Surface** (surface / surface-dark): cards, inputs, settings groups, hover fill on feed rows and nav items, inline title editing.
- **Surface Hover** (surface-hover / surface-hover-dark): hover on surfaces, the active nav item, secondary badges.
- **Ink** (text / text-dark): titles, body, and the fill of the primary button.
- **Muted Ink** (text-muted / text-muted-dark): secondary UI text, "Them" transcript lines, ghost buttons, meta chips.
- **Faint Ink** (text-faint / text-faint-dark): timestamps, subtitles, section descriptions, placeholders, inactive icons, the "#" heading mark. Dark is lifted to #7c7c86 from t3's #71717a so small faint text clears contrast on #09090b and #111113.
- **Hairline** (line / line-dark): default borders and dividers, an 8% alpha of ink rather than a gray.
- **Strong Hairline** (line-strong / line-strong-dark): hovered borders, keycaps, floating-layer borders, toggle track at rest.

### Status

- **OK, Warning, Error** (ok / warning / error, each with a dark pair): only in alerts (8% fill, 20% border), the danger button (10% fill, 25% border), destructive hover on trash icons, and the success badge. Never decorative.

### Named Rules

**The One Signal Rule.** Blue means alive, focused, selected, or yours. If an element is none of those, it is neutral; ordinary actions use the inverted ink button, not the accent.

**The Alpha Hairline Rule.** Borders are ink at low alpha (8% / 15% light, 8% / 14% dark), never a fixed gray, so they sit correctly on every tonal step.

**The Paired Token Rule.** A themed color is added as a `--light-color-*` / `--dark-color-*` pair in `src/styles/theme.css` and wired through all three active blocks; no color is written as a literal in components.

## Typography

**Display Font:** Newsreader Variable (with Iowan Old Style, Georgia)
**Body Font:** DM Sans Variable (with -apple-system, system-ui), stylistic sets ss01 and cv11 on
**Label/Mono Font:** JetBrains Mono Variable (with ui-monospace, SFMono-Regular, Menlo)

**Character:** A bookish serif names each page; a clean geometric sans does all the work; a mono with tabular figures keeps every time and shortcut lined up. All three are bundled; nothing loads from a CDN.

### Hierarchy

- **Display** (400, 32px, 1.25, -0.01em): page titles only: Home, a space, a meeting, Settings. Editable in place on meeting and space pages. Also the wordmark at 17px.
- **Numeral** (400, 34px, 1, tabular): the day-of-month figure in the Now card's date block, beside the month (ink) and weekday (faint) at 12px.
- **Headline** (600, 15px): document section headings (Agenda, Nudges, Notes, Transcript, Brief), with a faint "#" hung in the start gutter so the heading text aligns with the body.
- **Title** (600, 14px): sub-headings inside notes and summaries; settings group titles at 14px/500.
- **Body Doc** (400, 15px, 28px): reading text in notes, summaries, agenda, and briefs; transcript and nudge text at 14.5px/24px.
- **Body** (400, 14px, 22px): base UI text; nav items at 13.5px.
- **Label** (500 or 400, 13px down to 11px): group headings ("Today"), speaker labels, subtitles, descriptions, badges (11px/500).
- **Mono** (400, 11-12px, tabular): times in the feed and transcript, elapsed timer, date chip, version, keycaps.

### Named Rules

**The Serif Is a Title Rule.** Newsreader appears only as a page title, the day numeral, and the wordmark. Never in body, buttons, or section headings.

**The Numbers Line Up Rule.** Every time, duration, count, and shortcut is mono or carries `tabular-nums`.

**The Sentence Case Rule.** Labels and section names are sentence case at normal tracking; the system has no uppercase tracked eyebrows.

## Layout

A fixed two-pane shell: a 240px sidebar (wordmark in a 48px drag region, Home, Spaces with an add button, Settings and model/version status pinned at the foot) and a main pane with its own 48px drag bar (which carries a pill "Back" button when a meeting is open) above a scrolling page. Every page is a single centered column, max 720px with 32px side padding, entering with the page-in settle. Document sections are separated by 40px; Home feed day groups by 28px. Pages reserve 128px of bottom padding so the floating live dock never covers the last line. The window minimum (760px) keeps the column and sidebar side by side; there is no collapsed layout.

### Named Rules

**The One Column Rule.** Content lives in the centered 720px column. No multi-column dashboards, no settings-panel grids in the main pane.

## Elevation & Depth

Flat by default. Depth is tonal: canvas, sidebar, surface, surface-hover, each a half-step apart, separated by alpha hairlines. Shadows are reserved for objects that genuinely float above the page: the live dock, toasts, tooltips, and select menus. In-page cards (Now card, nudges, settings groups, alerts) carry a hairline and a fill, never a shadow.

### Shadow Vocabulary

- **Float** (`box-shadow: 0 16px 40px -12px rgb(0 0 0 / 0.45)`): toasts; the live dock adds a contact shadow `0 2px 6px -2px rgb(0 0 0 / 0.2)` and a 95% surface with backdrop blur.
- **Popover** (`box-shadow: 0 12px 32px -8px rgb(0 0 0 / 0.35)`): tooltips and select menus, over a line-strong border and a 12px radius.

### Named Rules

**The Only-Floaters-Cast Rule.** A shadow means "this is above the page". If it scrolls with the document, it is flat.

## Shapes

Rounded, controlled geometry. Controls (buttons, inputs, nav items, icon tiles, icon buttons) are 12px; containers (feed rows, nudge cards, settings groups, alerts, toasts, the Now card) are 16px; keycaps and inline code are 8px or less. Pills (full radius) are reserved for meta chips, badges, the Back button, the live dock and its buttons, the toggle, and the live dot. Accents of form: a 3px rounded vertical bar marks the Now card's state (accent when running), and 5px round bullets in faint ink mark list items in notes.

## Components

### Buttons

Quiet and decisive: the main action is ink on canvas, everything else recedes.

- **Shape:** gently rounded (12px); heights 28 / 32 / 40px with 10 / 14 / 16px side padding; 500 weight; 6px icon gap.
- **Primary:** inverted neutral, ink fill with canvas text; hover drops to 85% ink. Used for Start meeting, End meeting (in the dock as a pill), and toast actions.
- **Secondary:** surface fill, hairline border; hover to surface-hover and strong hairline.
- **Ghost:** muted text, no border; hover to ink on surface-hover.
- **Danger:** error text on 10% error fill with 25% border; **danger-ghost** is faint until hover reveals error.
- **States:** 150ms color transitions; disabled at 40% opacity; focus is the global accent outline (2px, 70% mix, 2px offset).

### Chips and Badges

- **Meta chip:** a 32px pill with a hairline border and muted text, used for the meeting date (mono 12px) and the space picker on a meeting page.
- **Badge:** 20px pill, 11px/500; accent at 15% for Live and Recommended, ok at 15% for success, surface-hover for neutral.

### Cards / Containers

- **Corner Style:** 16px.
- **Background:** surface with a hairline border (Now card, settings groups, info alerts); status alerts tint with 8% of their status color.
- **Shadow Strategy:** none (see Elevation).
- **Internal Padding:** 20px x 24px for the Now card (date block, a 3px state bar in accent when running or strong hairline when idle, then content); 12px x 14px for alerts and nudges; settings groups divide rows with hairlines.

### Inputs / Fields

- **Style:** surface fill, hairline border, 12px radius, 36px tall (32px compact), faint placeholder.
- **Focus:** border shifts to 70% accent; no glow. Selects mirror this with a 1px accent ring.
- **Document fields:** agenda and brief textareas are borderless, transparent, 15px/28px, gaining a surface fill only on hover and focus, so writing happens on the page itself.

### Navigation

- **Sidebar items:** 32px rows, 12px radius, 13.5px muted text with faint 16px line icons; hover to ink on surface; active is ink on surface-hover. A space with a running meeting shows a pulsing accent dot at its end.
- **Group heading:** "Spaces" in 12px/500 faint sentence case, with a 24px add button.
- **Settings tabs:** 36px text tabs on a hairline; the active tab gets ink text and a 2px rounded ink underline.

### Meeting Feed

Rows grouped under 13px/500 faint day headings (Today, Yesterday, a date). Each row: a 36px icon tile (hairline, surface, faint document icon; accent border, tint, and pulsing dot when live), a 14.5px title over a 13px faint subtitle (space name), and a right-aligned mono time that yields to a delete button on hover. Rows fill with surface on hover.

### Transcript

A three-column grid (52px speaker, text, hover-revealed mono time). "You" is the accent speaker label with ink text; "Them" is faint label with muted text. Sessions are separated by faint 12px labels.

### Live Dock (signature)

A 48px floating pill centered 20px above the bottom of the canvas on every page while a meeting runs: an accent-tinted circle with a four-bar level meter that rises and falls (0.9s, out of phase), the meeting title over "Recording" and a mono elapsed time, a hairline divider, a pause/resume icon button, and an ink "End meeting" pill. 95% surface, strong hairline, backdrop blur, Float shadow.

### Intervention Card

A separate small window: canvas background, 12px padding, 16px radius, hairline border; an accent kind icon with a 12px muted label, 14px snug text, and accent text-link actions aligned end.

## Do's and Don'ts

### Do:

- **Do** reserve the accent for live state, focus, selection, "you", and undismissed nudges.
- **Do** use the inverted ink button for the one main action on a surface.
- **Do** set page titles in Newsreader 32px and everything else in DM Sans; put times and keys in JetBrains Mono with tabular figures.
- **Do** keep content in the centered 720px column with 40px between document sections and 128px bottom padding under the dock.
- **Do** draw borders with the alpha hairline tokens and separate layers with tonal steps before reaching for a shadow.
- **Do** add new colors as light/dark pairs in `src/styles/theme.css` and use them through the Tailwind token names.
- **Do** ease entrances with `cubic-bezier(0.16, 1, 0.3, 1)` and turn the live pulse, level meter, and page-in off under reduced motion.

### Don't:

- **Don't** use the accent as a button fill or for decoration; a blue that does not mean "alive or yours" dilutes the signal.
- **Don't** put shadows on in-page cards; only the dock, toasts, tooltips, and menus float.
- **Don't** use the serif for body, buttons, or section headings.
- **Don't** add uppercase tracked eyebrows or kickers above titles; headings are sentence case.
- **Don't** write color literals in components; pink is not part of the approved CASE palette.
- **Don't** arrange the main pane as grouped boxes in a narrow settings column; meetings and spaces are documents.

## CASE identity

CASE is uppercase and is not an acronym. The shared mark in `src/components/icons/Wordmark.tsx` is two curved strokes with an offset dot. `src/assets/brand/case-mark.svg` is the standalone vector master; `case-app.svg` supplies the blue app icon. Native tray variants live in `src-tauri/resources/brand/`. The mark inherits `currentColor` in the interface, using the existing `text-accent` token from `src/styles/theme.css`; packaging uses the approved blue icon artwork. Keep the existing Newsreader wordmark and titles, DM Sans interface, and JetBrains Mono utility type.
