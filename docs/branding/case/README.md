# CASE identity

The approved identity uses **CASE** with the blue curved-strokes-and-dot mark.
CASE is uppercase and is not an acronym. The three numbered PNG boards in this
directory are earlier explorations, not the installed identity.

- Interface: `src/components/icons/Wordmark.tsx`, inheriting the existing accent token.
- Standalone vector: `src/assets/brand/case-mark.svg`.
- App-icon master: `src/assets/brand/case-app.svg`.
- Generated native icons: `src-tauri/icons/`.
- Tray masters: `src-tauri/resources/brand/`, with generated PNGs in `resources/`.
- Typography and token ownership: project-root `DESIGN.md` and `src/styles/theme.css`.

Regenerate native app icons with:

```sh
bunx tauri icon src/assets/brand/case-app.svg -o src-tauri/icons
```

The installed application is `CASE.app` on macOS, its executable is `case`,
its identifier is `com.case.app`, and new logs are written to `case.log`.
Legacy application directories and previous logs are preserved during import.
Third-party dependency identifiers, model repositories, and license notices
retain their original, accurate identities.
