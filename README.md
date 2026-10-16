# CASE

Your private meeting companion.

CASE listens to both sides of a call, transcribes locally, remembers your project, and passes you a short nudge when a decision is unresolved, a dependency blocks progress, or the conversation reopens parked scope. It never joins the call or speaks into it.

## What it does

- Captures microphone and system audio, with local speech recognition.
- Keeps meetings, transcripts, notes, and project sources on your machine.
- Uses one configured LLM provider for nudges, meeting filing, summaries, and image digests.
- Carries decisions, open questions, contracts, and parked topics across live checks.
- Records why a nudge was shown or suppressed.
- Starts and ends a meeting through one shortcut, the tray menu, or the CLI.

Read [the vision](docs/VISION.md), [architecture](docs/ARCHITECTURE.md), and [design system](DESIGN.md).

## Development

Install Rust (stable) and Bun, then:

```sh
bun install
bun run tauri dev
```

On macOS, if a native dependency needs the older CMake compatibility policy:

```sh
CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev
```

For a local macOS test application (no updater signing key required):

```sh
bun run build:test:macos
```

For release bundles, use `bun run tauri build` with the updater signing key configured.

See [BUILD.md](BUILD.md) for platform dependencies and model setup. Set `CASE_OPENROUTER_KEY` at build time only if distributing a build with a default provider key. Users can configure their own provider and key in Settings.

## Application identity

- App name: **CASE**
- Executable: `case`
- Application identifier: `com.case.app`
- Current log: `case.log`

| Platform | Application data                              |
| -------- | --------------------------------------------- |
| macOS    | `~/Library/Application Support/com.case.app/` |
| Windows  | `%APPDATA%\com.case.app\`                     |
| Linux    | `~/.local/share/com.case.app/`                |

Use Settings to open the active data and log directories. Portable Windows installs use `Data/` beside the executable.

## Existing installations

On first launch, CASE copies the prior meeting application's data into its new directory if CASE does not already have data. Meetings and settings are preserved, models and recordings are copied, and previous logs are retained under `previous-logs/`. SQLite databases are imported using a consistent backup, including committed WAL data. Original files remain in place. An interrupted import resumes on the next launch.

CASE has a new operating-system identity. Grant microphone, system-audio, and optional accessibility permissions to **CASE** when prompted. Quit the previous app before testing to avoid competing global shortcuts or capture sessions.

## Keyboard shortcuts

The configured shortcut toggles a meeting. macOS and Linux use the system shortcut implementation by default; the native keyboard implementation may require Accessibility permission on macOS. If shortcuts stop working while a password field is active, exit that secure-input field and retry.

```sh
command case --toggle-meeting
command case --start-hidden
command case --no-tray
command case --debug
command case --help
```

Unix integrations can also send SIGUSR2 to the `case` process.

## Updates and support

[Releases](https://github.com/dhruvd0/meet-assist/releases) · [Report a bug](https://github.com/dhruvd0/meet-assist/issues)

The updater and portable installer fallback point to this repository. Self-updating can be disabled at runtime with `CASE_DISABLE_UPDATER=1` without modifying saved preferences.

## Verification

```sh
bun run lint
bun run format:check
bunx tsc --noEmit
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

## License

MIT. Original copyright and third-party license notices are preserved in [LICENSE](LICENSE) and dependency distributions.
