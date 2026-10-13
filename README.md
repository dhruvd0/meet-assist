# Meet Assist

**A private meeting copilot that listens to your call and nudges only you when the conversation needs it.**

Meet Assist runs on your desktop. It is being built to transcribe both sides of a call locally, watch for loops, stuck moments, missing facts, and untried ideas, and show you a quiet card with a beep. It never joins the call and never speaks into it.

It is a fork of [Handy](https://github.com/cjpais/Handy), the open source offline dictation app. Handy's dictation still works here. Read the [vision](docs/VISION.md) for why this exists and the [architecture](docs/ARCHITECTURE.md) for how it is built.

## Status

Early. The meeting subsystem is specified and being built in order:

1. Docs and vision (done)
2. Projects and sources: brief, local files and folders, links, images, full-text search
3. Meetings as persisted threads under projects, microphone-only transcript
4. Observer with context tools, intervention card, beep
5. System audio capture (macOS first)
6. Summaries, image digests, continue flow, onboarding

Until step 3 lands, this app behaves like Handy with a new name.

## How it works

1. **Make a project.** Write a short brief and add sources: notes, local files and folders (a checked-out repo works), links, images. Everything is indexed locally for search.
2. **Start a meeting** in that project. Button or global shortcut. A meeting is a thread; you can pause, resume, end, and continue it on another day.
3. **It listens.** Your microphone and, where supported, system audio for the remote side (macOS first; Windows/Linux not yet verified). Each utterance is transcribed locally and tagged as you or them.
4. **It watches.** Every ~20 seconds the model you chose gets the project brief, the last meeting's summary, and the recent transcript, with one question: should I speak up? It can search and read your sources on its own before answering.
5. **It nudges.** If yes, you hear a beep and see a floating card: looping, stuck, quick fact, or idea. Dismiss it or open the meeting.
6. **It remembers.** Ending a session writes a summary. Continuing a meeting carries the summary forward.

Everything local stays local. The language model is your choice: Anthropic, OpenAI, OpenRouter, or a local Ollama / LM Studio endpoint through the same provider settings Handy uses for post-processing.

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

Meet Assist uses its own bundle identifier (`com.meetassist.app`), so it gets a fresh app-data directory and macOS treats it as a new app. Handy's settings, history, and downloaded models are not picked up automatically, and you will be asked to grant Microphone and Accessibility permissions again.

To reuse what Handy already has, copy `models/`, `settings_store.json`, and `history.db` from Handy's directory into Meet Assist's:

| OS      | Handy                                           | Meet Assist                                         |
| ------- | ----------------------------------------------- | --------------------------------------------------- |
| macOS   | `~/Library/Application Support/com.pais.handy/` | `~/Library/Application Support/com.meetassist.app/` |
| Windows | `%APPDATA%\com.pais.handy\`                     | `%APPDATA%\com.meetassist.app\`                     |
| Linux   | `~/.local/share/com.pais.handy/`                | `~/.local/share/com.meetassist.app/`                |

Quit or uninstall Handy before using Meet Assist, or change one app's shortcut. Both apps register the same default shortcut, so one key press would record and paste twice.

**Updater:** the self-update check is inherited from Handy and is not yet pointed at Meet Assist releases. Until it is, turn off update checks in Settings or launch with `HANDY_DISABLE_UPDATER=1`.

## Dictation (inherited from Handy)

Press a shortcut, speak, release, and your words are pasted into the active app. Silence is trimmed with Silero VAD; transcription uses Whisper-family or Parakeet models with GPU acceleration where available. Works on macOS, Windows, and Linux.

### Debug mode

- **macOS**: `Cmd+Shift+D`
- **Windows/Linux**: `Ctrl+Shift+D`

### CLI parameters

Flags work on all platforms. Remote-control flags are sent to the running instance; startup flags change how this launch behaves.

```bash
handy --toggle-transcription    # Toggle recording on/off
handy --toggle-post-process     # Toggle recording with post-processing on/off
handy --cancel                  # Cancel the current operation
handy --start-hidden            # Start without showing the main window
handy --no-tray                 # Start without the system tray icon
handy --debug                   # Enable debug mode with verbose logging
handy --help                    # Show all available flags
```

> **macOS tip:** When installed as an app bundle, invoke the binary directly:
>
> ```bash
> "/Applications/Meet Assist.app/Contents/MacOS/<binary>" --toggle-transcription   # list the folder to find the binary name
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

**Text Input Tools:**

For reliable text input on Linux, install the appropriate tool for your display server:

| Display Server | Recommended Tool | Install Command                                    |
| -------------- | ---------------- | -------------------------------------------------- |
| X11            | `xdotool`        | `sudo apt install xdotool`                         |
| Wayland        | `wtype`          | `sudo apt install wtype`                           |
| Both           | `dotool`         | `sudo apt install dotool` (requires `input` group) |

- **X11**: Install `xdotool` for both direct typing and clipboard paste shortcuts
- **Ubuntu 26.04**: Has Wayland display server by default. `wtype` does not work, you need to install `ydotool` and configure systemd as described [here](https://github.com/cjpais/Handy/pull/557#issuecomment-3781249267).
- **Wayland**: Install `wtype` (preferred) or `dotool` for text input to work correctly
- **dotool setup**: Requires adding your user to the `input` group: `sudo usermod -aG input $USER` (then log out and back in)

Without these tools, Handy falls back to enigo which may have limited compatibility, especially on Wayland.

**Wayland Support (Linux):**

- Limited support for Wayland display server
- Requires [`wtype`](https://github.com/atx/wtype) or [`dotool`](https://sr.ht/~geb/dotool/) for text input to work correctly (see [Linux Notes](#linux-notes) below for installation)

**Other Notes:**

- **Runtime library dependency (`libgtk-layer-shell.so.0`)**:
  - Handy links `gtk-layer-shell` on Linux. If startup fails with `error while loading shared libraries: libgtk-layer-shell.so.0`, install the runtime package for your distro:

    | Distro        | Package to install    | Example command                        |
    | ------------- | --------------------- | -------------------------------------- |
    | Ubuntu/Debian | `libgtk-layer-shell0` | `sudo apt install libgtk-layer-shell0` |
    | Fedora/RHEL   | `gtk-layer-shell`     | `sudo dnf install gtk-layer-shell`     |
    | Arch Linux    | `gtk-layer-shell`     | `sudo pacman -S gtk-layer-shell`       |

  - For building from source on Ubuntu/Debian, you may also need `libgtk-layer-shell-dev`.

- The recording overlay is disabled by default on Linux (`Overlay Position: None`) because certain compositors treat it as the active window. When the overlay is visible it can steal focus, which prevents Handy from pasting back into the application that triggered transcription. If you enable the overlay anyway, be aware that clipboard-based pasting might fail or end up in the wrong window.
- Handy disables the WebKit DMA-BUF renderer by default on Linux, but preserves explicit `WEBKIT_DISABLE_DMABUF_RENDERER` values. For rendering workarounds, see [Troubleshooting → Linux Startup Crashes or Instability](#linux-startup-crashes-or-instability).
- If Handy fails to start reliably on Linux, see [Troubleshooting → Linux Startup Crashes or Instability](#linux-startup-crashes-or-instability).
- **Global keyboard shortcuts (Wayland):** On Wayland, system-level shortcuts must be configured through your desktop environment or window manager. Use the [CLI flags](#cli-parameters) as the command for your custom shortcut.

  **GNOME:**
  1. Open **Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts**
  2. Click the **+** button to add a new shortcut
  3. Set the **Name** to `Toggle Handy Transcription`
  4. Set the **Command** to `handy --toggle-transcription`
  5. Click **Set Shortcut** and press your desired key combination (e.g., `Super+O`)

  **KDE Plasma:**
  1. Open **System Settings > Shortcuts > Custom Shortcuts**
  2. Click **Edit > New > Global Shortcut > Command/URL**
  3. Name it `Toggle Handy Transcription`
  4. In the **Trigger** tab, set your desired key combination
  5. In the **Action** tab, set the command to `handy --toggle-transcription`

  **Sway / i3:**

  Add to your config file (`~/.config/sway/config` or `~/.config/i3/config`):

  ```ini
  bindsym $mod+o exec handy --toggle-transcription
  ```

  **Hyprland:**

  Add to your config file (`~/.config/hypr/hyprland.conf`):

  ```ini
  bind = $mainMod, O, exec, handy --toggle-transcription
  ```

- You can also trigger Handy externally via Unix signals or the CLI flags, which lets Wayland window managers or other hotkey daemons keep ownership of keybindings:

  | Action                                    | Trigger                                                  |
  | ----------------------------------------- | -------------------------------------------------------- |
  | Toggle transcription                      | `pkill -USR2 -n handy` or `handy --toggle-transcription` |
  | Toggle transcription with post-processing | `handy --toggle-post-process`                            |

  Example Sway config:

  ```ini
  bindsym $mod+o exec pkill -USR2 -n handy
  bindsym $mod+p exec handy --toggle-post-process
  ```

  `pkill` here simply delivers the signal—it does not terminate the process.

  > **Behavior change:** older releases also accepted `SIGUSR1` for toggling transcription with post-processing. WebKitGTK — the webview engine embedded in Handy on Linux — uses SIGUSR1 internally to coordinate JavaScript garbage collection, so listening for it caused phantom recordings and interrupted dictations every few minutes ([#1660](https://github.com/cjpais/Handy/issues/1660)). Handy no longer listens for SIGUSR1 on Linux; the post-processing toggle is still available via `handy --toggle-post-process`. **Remove any `pkill -USR1` bindings**: the signal is now delivered straight to WebKit's internal handler and can crash the app.

**Overlay & Pasting Issues (Linux):**

- The recording overlay window can interfere with pasting transcribed text into target applications on Linux (X11)
- **Solution:** Open **Settings > Advanced** and set **"Overlay Position"** to **"None"** to disable the overlay
- Enable **"Audio Feedback"** (also in Advanced) if you still want audible confirmation of recording state
- Users who upgrade from older versions or import settings from other platforms may need to manually apply this change

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

### Previous Clipboard Content Is Pasted Instead of the Transcription

If the transcription is correct in **History** but Handy inserts text you copied earlier, see [issue #502](https://github.com/cjpais/Handy/issues/502). With the standard clipboard paste method, Handy restores your previous clipboard after a fixed delay. Under load, the receiving application may read the clipboard only after that restoration.

1. Open Handy's settings window and press `Cmd+Shift+D` (macOS) or `Ctrl+Shift+D` (Windows/Linux) to reveal **Debug**.
2. On **macOS and Windows**, try **Reliable Paste (Beta)** in Debug with a clipboard paste method selected. It uses clipboard read notifications to delay restoration instead of relying on the standard fixed delay. Test it in the application where the problem occurs; it is still experimental.
3. If Reliable Paste is disabled or unavailable, increase **Paste Delay (After)** in Debug and test again. This controls the wait before restoring your previous clipboard. **Paste Delay (Before)** controls the wait before sending the paste keystroke and addresses a different part of the operation. These delay settings apply to the standard paste path, not Reliable Paste.

If the problem persists, add your Handy version, operating system, receiving application, paste method, Reliable Paste setting, and before/after delays to the existing issue. Redact private dictated text before sharing logs.

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

**1. Install (or reinstall) `gtk-layer-shell`**

Handy uses `gtk-layer-shell` for its recording overlay and links against it at runtime. A missing or broken installation is the most common cause of startup failures and can manifest as a crash or a hang well before any window is shown. Make sure the runtime package is installed for your distro:

| Distro        | Package to install    | Example command                        |
| ------------- | --------------------- | -------------------------------------- |
| Ubuntu/Debian | `libgtk-layer-shell0` | `sudo apt install libgtk-layer-shell0` |
| Fedora/RHEL   | `gtk-layer-shell`     | `sudo dnf install gtk-layer-shell`     |
| Arch Linux    | `gtk-layer-shell`     | `sudo pacman -S gtk-layer-shell`       |

If it is already installed and you still see startup problems, try reinstalling it (e.g. `sudo pacman -S gtk-layer-shell` again) in case the library files were corrupted by a partial upgrade.

**2. Disable the GTK layer shell overlay (`HANDY_NO_GTK_LAYER_SHELL`)**

If installing the library does not help, you can skip `gtk-layer-shell` initialization entirely as a workaround. On some compositors (notably KDE Plasma under Wayland) it has been reported to interact poorly with the recording overlay. With this variable set, the overlay falls back to a regular always-on-top window:

```bash
HANDY_NO_GTK_LAYER_SHELL=1 handy
```

**3. Configure WebKit DMA-BUF rendering (`WEBKIT_DISABLE_DMABUF_RENDERER`)**

Handy disables the WebKitGTK DMA-BUF renderer by default on Linux because it can cause blank windows or crashes on some GPU/driver combinations. Explicit environment-variable values are preserved, including in the Nix package. To explicitly keep the renderer disabled:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 handy
```

Some users on WebKitGTK 2.54 report a flickering or partially drawn recording overlay with the renderer disabled ([#2166](https://github.com/cjpais/Handy/issues/2166)). If affected, try opting out of the default workaround:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=0 handy
```

This allows WebKit to use the renderer when supported, but can reintroduce the GPU/driver-specific crashes described above. Remove the override or use `=1` if that happens.

Fully quit Handy, including its tray instance, before testing either value. Running a second instance with `--toggle-transcription` does not change the running process's environment.

**Making a workaround permanent**

Set the variable in the environment of the launcher that actually starts Handy. Shell profile exports may not apply to desktop-menu or autostart launches. If you launch Handy from a `.desktop` file, you can prefix the `Exec=` line, e.g.:

```ini
Exec=env HANDY_NO_GTK_LAYER_SHELL=1 handy
```

Handy's built-in autostart rewrites its generated desktop entry, so editing that entry is not a durable workaround. For autostart, use an appropriate session/service environment, or disable Handy's built-in autostart and manage your own startup entry with the required variable.

If a workaround helps you, please [open an issue](https://github.com/cjpais/Handy/issues) describing your distro, desktop environment, and session type — that information helps us narrow down the underlying bug.

### Empty recording overlay on Hyprland / Omarchy

If the recording overlay is empty or bordered, fully quit Handy and launch a
[native installation](BUILD.md#linux-install-from-source) with Wayland enabled:

```bash
env -u HANDY_NO_GTK_LAYER_SHELL GDK_BACKEND=wayland handy
```

If this works, apply `GDK_BACKEND=wayland` only to Handy's launcher. This
workaround does not work with the 0.9.6 AppImage, which forces X11.

### Vulkan Overlays and Capture Tools on Windows (`HANDY_KEEP_VULKAN_IMPLICIT_LAYERS`)

On Windows, Handy asks the Vulkan loader to skip implicit layers to avoid crashes caused by overlay and capture hooks ([#2049](https://github.com/cjpais/Handy/issues/2049)). GPU acceleration remains enabled; this does not change system-wide settings.

To opt out for GPU selection or debugging tools, fully quit Handy (including the tray icon), then run both commands in the same PowerShell window:

```powershell
$env:HANDY_KEEP_VULKAN_IMPLICIT_LAYERS = "1"
& "$env:ProgramFiles\Handy\handy.exe"
```

Adjust the executable path if needed. This override only applies to apps launched from that PowerShell session, not the Start menu. Handy also preserves any existing `VK_LOADER_LAYERS_DISABLE` value.

### How to Contribute

See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports and fixes against the inherited dictation pipeline are welcome; meeting-subsystem work follows the [design spec](docs/superpowers/specs/2026-10-13-meeting-assistant-design.md).

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
