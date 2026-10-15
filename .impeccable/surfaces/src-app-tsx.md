---
version: 1
slug: "src-app-tsx"
primary_target: "src/App.tsx"
related_targets: ["src/components/Sidebar.tsx"]
---

# Surface: Meet Assist main window

Mode: Operate. Scope: app shell (sidebar, home, space, meeting document, settings), onboarding, intervention card.

Audience/job: person between back-to-back calls; scan recent meetings by day, open one, read notes; manage spaces (projects); start/end a meeting.
Constraints: frontend only; reuse stores and commands; i18n for every string; light and dark themes; no Ask bar.
Pinned by user: t3.codes tokens; Granola experience; DM Sans + serif titles (Newsreader) + JetBrains Mono.

## Direction contract

THESIS: A meeting is a document, not a record. The shell refuses the settings-panel arrangement (grouped boxes in a narrow column) and sets meetings like Granola notes: a calm feed by day, a centered page per meeting.

OWN-WORLD: t3 zinc ground (#09090b canvas, #0c0c0e sidebar, #111113 surfaces, white-alpha hairlines), one blue accent oklch(.68 .17 250) for live state, focus, selection, and "you" in transcripts. DM Sans UI, Newsreader titles, JetBrains Mono for times and shortcuts. Radii 8/12/16. Pill chips and inverted neutral primary buttons.

STORY: Open the app, see what is live and what happened today; open a meeting and read decisions first, transcript last; start the next one with one key.

FIRST VIEWPORT: Left sidebar 240px (wordmark, Home, Spaces list, Settings at foot). Main: centered 720px column, serif "Home" heading; a "Now" card (live meeting with pulsing accent, timer, end) or a start card with the shortcut in mono keycaps; below, meetings grouped Today / Yesterday / date: icon tile, title, space name, right-aligned mono time.

FORM: pinned by brief (no concept-seed roll): Granola-pattern notes app in t3 tokens. Signature move: the live dock, a floating pill at the bottom of the canvas with an accent level-pulse, elapsed time, pause and end, present on every page while a meeting runs.

FINISH: unreviewed and unverified until the finish reviewer returns ship.
