# Contributing to Meet Assist

Meet Assist is a private meeting copilot forked from [Handy](https://github.com/cjpais/Handy). Thanks for helping.

## Where things stand

Read [docs/VISION.md](docs/VISION.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The meeting subsystem is built from the spec in `docs/superpowers/specs/` through plans in `docs/superpowers/plans/`. If you want to change the direction, propose a spec edit first.

Dictation is inherited from Handy and kept working. Bug fixes there are welcome; consider sending them upstream too.

## Setup

- [Rust](https://rustup.rs/) stable
- [Bun](https://bun.sh/)
- Platform build tools, see [BUILD.md](BUILD.md)

```bash
git clone git@github.com:dhruvd0/meet-assist.git
cd meet-assist
bun install
mkdir -p src-tauri/resources/models
curl -L -o src-tauri/resources/models/silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
bun run tauri dev
# macOS cmake error? prefix with CMAKE_POLICY_VERSION_MINIMUM=3.5
```

## Workflow

1. Branch: `feature/<name>` or `fix/<name>`.
2. Follow existing patterns. Reuse `TranscriptionManager`, `CaptureVad`, `llm_client`, `audio_feedback`, and `components/ui`. Keep the current design language.
3. Before committing: `bun run lint`, `bun run format`, `cargo clippy` in `src-tauri`.
4. Conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`. Say why.
5. Open a PR using [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md). Link the spec or plan task in place of community feedback. Disclose AI assistance: whether, which tools, how much.

## Code style

**Rust:** `cargo fmt`, `cargo clippy`, explicit error handling, doc comments on public APIs, no `unwrap` in production paths.

**TypeScript/React:** strict types, no `any`, functional components, Tailwind, all user-facing strings through i18next (`src/i18n/locales/en/translation.json`).

## Bugs

Open an issue with the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md): app version, OS, CPU/GPU, steps, expected, actual, debug-mode output (`Cmd/Ctrl+Shift+D`).

## Translations

See [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md).

## License

MIT, see [LICENSE](LICENSE). Contributions are licensed the same way.
