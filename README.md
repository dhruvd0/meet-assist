# Meet Assist

**A private meeting copilot that listens to your call and nudges only you when the conversation needs it.**

Meet Assist runs on your desktop. It is being built to transcribe both sides of a call locally, watch for loops, stuck moments, missing facts, and untried ideas, and show you a quiet card with a beep. It never joins the call and never speaks into it.

It is a fork of [Handy](https://github.com/cjpais/Handy), the open source offline dictation app. Meet Assist is meeting-only: the dictation features (paste, post-processing, history) were removed. Read the [vision](docs/VISION.md) for why this exists and the [architecture](docs/ARCHITECTURE.md) for how it is built.

## Status

Early. The meeting subsystem is built in this order (all six steps are in):

1. Docs and vision (done)
2. Projects and sources: brief, local files and folders, links, images, full-text search
3. Meetings as persisted threads under projects, microphone-only transcript
4. Observer with context tools, intervention card, beep
5. System audio capture (macOS verified; Windows and Linux built, unverified)
6. Summaries, image digests, continue flow, onboarding

## How it works

1. **Make a project.** Write a short brief and add sources: notes, local files and folders (a checked-out repo works), links, images. Everything is indexed locally for search.
2. **Start a meeting** in that project. Button or global shortcut. A meeting is a thread; you can pause, resume, end, and continue it on another day.
3. **It listens.** Your microphone and, where supported, system audio for the remote side (macOS first; Windows/Linux not yet verified). It can be turned off in Advanced settings. On macOS the first meeting asks for the "Audio capture" permission; if it is denied, or the default output is also an input (a headset in call mode), a banner says system audio is unavailable and the meeting continues with your microphone only. Each utterance is transcribed locally and tagged as you or them.
4. **It watches.** Every ~20 seconds the model you chose gets the project brief, the last meeting's summary, and the recent transcript, with one question: should I speak up? It can search and read your sources on its own before answering.
5. **It nudges.** If yes, you hear a beep and see a floating card: looping, stuck, quick fact, or idea. Dismiss it or open the meeting.
6. **It remembers.** Within seconds of pressing End, the meeting shows a session summary and an updated rolling summary. Continuing a meeting carries the summary forward. Image sources get a text description from a vision-capable model when one is configured.

Everything local stays local. The language model is your choice: Anthropic, OpenAI, OpenRouter, or a local Ollama / LM Studio endpoint through one provider setting on the Assistant page.

First run includes an optional "assistant" onboarding step (after the model step) to enter and test an LLM provider, base URL, API key, and model. Skip it and set it up later in settings.

## Quick start

### Install

Releases are not yet published for Meet Assist. Build from source (see below).

### Build from source

Prerequisites: [Rust](https://rustup.rs/) stable, [Bun](https://bun.sh/).

```bash
bun install
mkdir -p src-tauri/resources/models
curl -L -o src-tauri/resources/models/silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
bun run tauri dev
# macOS cmake error? prefix with CMAKE_POLICY_VERSION_MINIMUM=3.5
```

Platform-specific details are in [BUILD.md](BUILD.md).

### Migrating from Handy

Meet Assist uses its own bundle identifier (`com.meetassist.app`), so it gets a fresh app-data directory and macOS treats it as a new app. Handy's settings and downloaded models are not picked up automatically, and you will be asked to grant Microphone permission again.

To reuse what Handy already has, copy `models/` and `settings_store.json` from Handy's directory into Meet Assist's:

| OS      | Handy                                           | Meet Assist                                         |
| ------- | ----------------------------------------------- | --------------------------------------------------- |
| macOS   | `~/Library/Application Support/com.pais.handy/` | `~/Library/Application Support/com.meetassist.app/` |
| Windows | `%APPDATA%\com.pais.handy\`                     | `%APPDATA%\com.meetassist.app\`                     |
| Linux   | `~/.local/share/com.pais.handy/`                | `~/.local/share/com.meetassist.app/`                |

Quit or uninstall Handy before using Meet Assist, or change one app's shortcut. Both apps register the same default shortcut, so one key press would trigger both.

**Updater:** the self-update check is inherited from Handy and is not yet pointed at Meet Assist releases. Until it is, turn off update checks in Settings or launch with `HANDY_DISABLE_UPDATER=1`.

### Debug mode

- **macOS**: `Cmd+Shift+D`
- **Windows/Linux**: `Ctrl+Shift+D`

### CLI parameters

Flags work on all platforms. Remote-control flags are sent to the running instance; startup flags change how this launch behaves.

```bash
handy --toggle-meeting          # Start a meeting, or end the running one
handy --start-hidden            # Start without showing the main window
handy --no-tray                 # Start without the system tray icon
handy --debug                   # Enable debug mode with verbose logging
handy --help                    # Show all available flags
```

> **macOS tip:** When installed as an app bundle, invoke the binary directly:
>
> ```bash
> "/Applications/Meet Assist.app/Contents/MacOS/<binary>" --toggle-meeting   # list the folder to find the binary name
> ```

## Known Issues & Current Limitations

This project is actively being developed and has some [known issues](https://github.com/cjpais/Handy/issues). We believe in transparency about the current state:

### Bluetooth Headset Microphones (macOS)

Using a Bluetooth headset microphone on macOS may temporarily reduce playback quality or volume while recording because Bluetooth switches to bidirectional audio. Keep your headphones as the output device and select your Mac's built-in or an external microphone in Handy to avoid this.

### fn and Globe Key Shortcuts (macOS)

Shortcuts that include the `fn` (Globe) key **only work on Apple keyboards** — your Mac's built-in keyboard or an Apple external keyboard. They will never trigger on a third-party keyboard, even while it is connected to the same Mac.

This is a hardware limitation rather than a Handy bug. `fn` is not part of the standard USB HID keyboard specification: Apple reports it through a vendor-specific usage that macOS honors only from Apple devices, while third-party keyboards handle their `Fn` key entirely in firmware and send nothing to the computer. There is no event for Handy to listen for.

If you switch between a MacBook keyboard and an external one, pick a shortcut built from standard modifiers (`ctrl`, `option`, `shift`, `command`) or a regular key instead.

### Linux Notes

**Other Notes:**

- Handy disables the WebKit DMA-BUF renderer by default on Linux, but preserves explicit `WEBKIT_DISABLE_DMABUF_RENDERER` values. For rendering workarounds, see [Troubleshooting → Linux Startup Crashes or Instability](#linux-startup-crashes-or-instability).
- If Handy fails to start reliably on Linux, see [Troubleshooting → Linux Startup Crashes or Instability](#linux-startup-crashes-or-instability).
- **Global keyboard shortcuts (Wayland):** On Wayland, system-level shortcuts must be configured through your desktop environment or window manager. Use the [CLI flags](#cli-parameters) as the command for your custom shortcut.

  **GNOME:**
  1. Open **Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts**
  2. Click the **+** button to add a new shortcut
  3. Set the **Name** to `Toggle Meet Assist Meeting`
  4. Set the **Command** to `handy --toggle-meeting`
  5. Click **Set Shortcut** and press your desired key combination (e.g., `Super+O`)

  **KDE Plasma:**
  1. Open **System Settings > Shortcuts > Custom Shortcuts**
  2. Click **Edit > New > Global Shortcut > Command/URL**
  3. Name it `Toggle Meet Assist Meeting`
  4. In the **Trigger** tab, set your desired key combination
  5. In the **Action** tab, set the command to `handy --toggle-meeting`

  **Sway / i3:**

  Add to your config file (`~/.config/sway/config` or `~/.config/i3/config`):

  ```ini
  bindsym $mod+o exec handy --toggle-meeting
  ```

  **Hyprland:**

  Add to your config file (`~/.config/hypr/hyprland.conf`):

  ```ini
  bind = $mainMod, O, exec, handy --toggle-meeting
  ```

- You can also trigger Handy externally via Unix signals or the CLI flags, which lets Wayland window managers or other hotkey daemons keep ownership of keybindings:

  | Action         | Trigger                                            |
  | -------------- | -------------------------------------------------- |
  | Toggle meeting | `pkill -USR2 -n handy` or `handy --toggle-meeting` |

  Example Sway config:

  ```ini
  bindsym $mod+o exec pkill -USR2 -n handy
  ```

  `pkill` here simply delivers the signal—it does not terminate the process.

  > **Note:** SIGUSR2 toggles a meeting. SIGUSR1 is unhandled: WebKitGTK uses it internally for JavaScript garbage collection, so **remove any `pkill -USR1` bindings**; the signal is delivered straight to WebKit and can crash the app.

## Verify Release Signatures

Handy release artifacts are signed with Tauri's updater signature format. The public key is stored in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) under `plugins.updater.pubkey`.

To verify a release manually, set `ARTIFACT` to the filename you downloaded, save the `pubkey` value from `src-tauri/tauri.conf.json` to `handy.pub.b64`, then decode the public key and matching `.sig` file from base64 and verify the artifact with `minisign`:

```bash
# Replace with the file you downloaded
ARTIFACT="Handy_0.8.1_amd64.AppImage"

python3 - "$ARTIFACT" <<'PY'
import base64, pathlib, sys

artifact = sys.argv[1]

pub = pathlib.Path("handy.pub.b64").read_text().strip()
pathlib.Path("handy.pub").write_bytes(base64.b64decode(pub))

sig = pathlib.Path(f"{artifact}.sig").read_text().strip()
pathlib.Path(f"{artifact}.minisig").write_bytes(base64.b64decode(sig))
PY

minisign -Vm "$ARTIFACT" \
  -p handy.pub \
  -x "$ARTIFACT.minisig"
```

On success, `minisign` prints:

```text
Signature and comment signature verified
```

Do not use `gpg` for these `.sig` files.

## Troubleshooting

### Manual Model Installation (For Proxy Users or Network Restrictions)

If you're behind a proxy, firewall, or in a restricted network environment where Handy cannot download models automatically, you can manually download and install them. The URLs are publicly accessible from any browser.

#### Step 1: Find Your App Data Directory

1. Open Handy settings
2. Navigate to the **About** section
3. Copy the "App Data Directory" path shown there, or use the shortcuts:
   - **macOS**: `Cmd+Shift+D` to open debug menu
   - **Windows/Linux**: `Ctrl+Shift+D` to open debug menu

The typical paths are:

- **macOS**: `~/Library/Application Support/com.meetassist.app/`
- **Windows**: `C:\Users\{username}\AppData\Roaming\com.meetassist.app\`
- **Linux**: `~/.local/share/com.meetassist.app/`

#### Step 2: Create Models Directory

Inside your app data directory, create a `models` folder if it doesn't already exist:

```bash
# macOS
mkdir -p ~/Library/Application\ Support/com.meetassist.app/models

# Linux
mkdir -p ~/.local/share/com.meetassist.app/models

# Windows (PowerShell)
New-Item -ItemType Directory -Force -Path "$env:APPDATA\com.meetassist.app\models"
```

#### Step 3: Download Model Files

Download the models you want from below

**Whisper Models (single .bin files):**

- Small (487 MB): `https://blob.handy.computer/ggml-small.bin`
- Medium (492 MB): `https://blob.handy.computer/whisper-medium-q4_1.bin`
- Turbo (1600 MB): `https://blob.handy.computer/ggml-large-v3-turbo.bin`
- Large (1100 MB): `https://blob.handy.computer/ggml-large-v3-q5_0.bin`

**Parakeet Unified EN 0.6B (single `.gguf` file, recommended):**

- Q8_0 (731 MB): `https://huggingface.co/handy-computer/parakeet-unified-en-0.6b-gguf/resolve/main/parakeet-unified-en-0.6b-Q8_0.gguf`

#### Step 4: Install Models

**For Whisper Models (.bin files):**

Simply place the `.bin` file directly into the `models` directory:

```
{app_data_dir}/models/
├── ggml-small.bin
├── whisper-medium-q4_1.bin
├── ggml-large-v3-turbo.bin
└── ggml-large-v3-q5_0.bin
```

**For GGUF Models (.gguf files):**

Place the `.gguf` file directly into the `models` directory, exactly like the Whisper `.bin` files above. Handy also picks up models already present in the shared Hugging Face cache (`~/.cache/huggingface/hub`), so a copy downloaded by another tool works without being moved.

**Important Notes:**

- Do not rename the `.bin` or `.gguf` files—use the exact filenames from the download URLs
- After placing the files, restart Handy to detect the new models

#### Step 5: Verify Installation

1. Restart Handy
2. Open Settings → Models
3. Your manually installed models should now appear as "Downloaded"
4. Select the model you want to use and test transcription

### Custom Whisper Models

Handy can auto-discover custom Whisper GGML models placed in the `models` directory. This is useful for users who want to use fine-tuned or community models not included in the default model list.

**How to use:**

1. Obtain a Whisper model in GGML `.bin` format (e.g., from [Hugging Face](https://huggingface.co/models?search=whisper%20ggml))
2. Place the `.bin` file in your `models` directory (see paths above)
3. Restart Handy to discover the new model
4. The model will appear in the "Custom Models" section of the Models settings page

**Important:**

- Community models are user-provided and may not receive troubleshooting assistance
- The model must be a valid Whisper GGML format (`.bin` file)
- Model name is derived from the filename (e.g., `my-custom-model.bin` → "My Custom Model")

### Linux Startup Crashes or Instability

If Handy fails to start reliably on Linux — for example, it crashes shortly after launch, never shows its window, or reports a Wayland protocol error — try the steps below in order.

**1. Configure WebKit DMA-BUF rendering (`WEBKIT_DISABLE_DMABUF_RENDERER`)**

Handy disables the WebKitGTK DMA-BUF renderer by default on Linux because it can cause blank windows or crashes on some GPU/driver combinations. Explicit environment-variable values are preserved, including in the Nix package. To explicitly keep the renderer disabled:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 handy
```

Some users on WebKitGTK 2.54 report flickering or partial drawing with the renderer disabled. If affected, try opting out:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=0 handy
```

This allows WebKit to use the renderer when supported, but can reintroduce the GPU/driver-specific crashes described above. Remove the override or use `=1` if that happens.

Fully quit Handy, including its tray instance, before testing either value. Running a second instance with `--toggle-meeting` does not change the running process's environment.

**Making a workaround permanent**

Set the variable in the environment of the launcher that actually starts Handy. Shell profile exports may not apply to desktop-menu or autostart launches. If you launch Handy from a `.desktop` file, you can prefix the `Exec=` line, e.g.:

```ini
Exec=env WEBKIT_DISABLE_DMABUF_RENDERER=1 handy
```

Handy's built-in autostart rewrites its generated desktop entry, so editing that entry is not a durable workaround. For autostart, use an appropriate session/service environment, or disable Handy's built-in autostart and manage your own startup entry with the required variable.

If a workaround helps you, please [open an issue](https://github.com/cjpais/Handy/issues) describing your distro, desktop environment, and session type — that information helps us narrow down the underlying bug.

### Vulkan Overlays and Capture Tools on Windows (`HANDY_KEEP_VULKAN_IMPLICIT_LAYERS`)

On Windows, Handy asks the Vulkan loader to skip implicit layers to avoid crashes caused by overlay and capture hooks ([#2049](https://github.com/cjpais/Handy/issues/2049)). GPU acceleration remains enabled; this does not change system-wide settings.

To opt out for GPU selection or debugging tools, fully quit Handy (including the tray icon), then run both commands in the same PowerShell window:

```powershell
$env:HANDY_KEEP_VULKAN_IMPLICIT_LAYERS = "1"
& "$env:ProgramFiles\Handy\handy.exe"
```

Adjust the executable path if needed. This override only applies to apps launched from that PowerShell session, not the Start menu. Handy also preserves any existing `VK_LOADER_LAYERS_DISABLE` value.

### How to Contribute

See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports and fixes for the transcription engine, audio capture, and models are welcome; meeting-subsystem work follows the [design spec](docs/superpowers/specs/2026-10-13-meeting-assistant-design.md).

## Related Projects

- [Handy](https://github.com/cjpais/Handy), the upstream dictation app this fork is built on.

## License

MIT License - see [LICENSE](LICENSE) file for details.

Upstream Handy is open-source software, but the Handy name, logo, icon, and brand assets are not. Meet Assist is an independent fork, not endorsed by or affiliated with Handy. It uses its own name and identifier; the app icon still needs to be replaced before any public release.

## Acknowledgments

- [Handy](https://github.com/cjpais/Handy) by cjpais and contributors, the dictation app Meet Assist is forked from
- **Whisper** by OpenAI for the speech recognition model
- **ggml** for an amazing cross-platform tensor library
- **Silero** for great lightweight VAD
- **Tauri** team for the excellent Rust-based app framework
- **Community contributors** helping make Handy better
